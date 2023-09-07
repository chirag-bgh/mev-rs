use async_trait::async_trait;
use beacon_api_client::mainnet::Client;
use ethereum_consensus::{
    builder::ValidatorRegistration,
    clock::get_current_unix_time_in_secs,
    crypto::SecretKey,
    primitives::{BlsPublicKey, Hash32, Root, Slot, U256},
    state_transition::Context,
};
use mev_build_rs::NullBuilder;
use mev_rs::{
    signing::sign_builder_message,
    types::{
        bellatrix, capella, BidRequest, ExecutionPayload, ExecutionPayloadHeader,
        SignedBlindedBeaconBlock, SignedBuilderBid, SignedValidatorRegistration,
    },
    BlindedBlockProvider, Error, ValidatorRegistry,
};
use parking_lot::Mutex;
use std::{collections::HashMap, ops::Deref, sync::Arc};

// `PROPOSAL_TOLERANCE_DELAY` controls how aggresively the relay drops "old" execution payloads
// once they have been fetched from builders -- currently in response to an incoming request from a
// proposer. Setting this to anything other than `0` incentivizes late proposals and setting it to
// `1` allows for latency at the slot boundary while still enabling a successful proposal.
// TODO likely drop this feature...
const PROPOSAL_TOLERANCE_DELAY: Slot = 1;

fn validate_bid_request(
    bid_request: &BidRequest,
    context: &Context,
    validator_registry: &ValidatorRegistry,
) -> Result<(), Error> {
    // TDOD: take this as input
    let fork = "Bellatrix";
    // Check if the slot is timely
    if !is_slot_timely(&bid_request.slot, &fork, &context) {
        return Err(Error::InvalidSlot)
    }

    // Check if the parent_hash is on a chain tip
    if !is_parent_hash_on_chain_tip(&bid_request.parent_hash, &context) {
        return Err(Error::InvalidParentHash)
    }

    // Check if public_key is one of the possible proposers
    if !is_valid_proposer(&bid_request.public_key, &validator_registry) {
        return Err(Error::ValidatorNotRegistered(bid_request.public_key.clone()))
    }

    Ok(())
}

fn is_slot_timely(slot: &Slot, fork: &str, context: &Context) -> bool {
    let current_slot = match fork {
        "Bellatrix" => 32 + context.bellatrix_fork_epoch * context.slots_per_epoch,
        "Capella" => 32 + context.capella_fork_epoch * context.slots_per_epoch,
        _ => unimplemented!(),
    };
    slot + PROPOSAL_TOLERANCE_DELAY >= current_slot
}

fn is_parent_hash_on_chain_tip(parent_hash: &Hash32, context: &Context) -> bool {
    let chain_tip = &context.terminal_block_hash;
    parent_hash == chain_tip
}

fn is_valid_proposer(public_key: &BlsPublicKey, validator_registry: &ValidatorRegistry) -> bool {
    // check if validator is in the current validator set\
    let validator_index = validator_registry.get_validator_index(public_key);
    validator_index.is_some()
}

fn validate_execution_payload(
    execution_payload: &ExecutionPayload,
    _value: &U256,
    preferences: &ValidatorRegistration,
) -> Result<(), Error> {
    // TODO validations

    // TODO allow for "adjustment cap" per the protocol rules
    // towards the proposer's preference
    if execution_payload.gas_limit() != preferences.gas_limit {
        return Err(Error::InvalidGasLimit)
    }

    // verify payload is valid

    // verify payload sends `value` to proposer

    Ok(())
}

fn validate_signed_block(
    signed_block: &mut SignedBlindedBeaconBlock,
    public_key: &BlsPublicKey,
    local_payload: &ExecutionPayload,
    genesis_validators_root: &Root,
    context: &Context,
) -> Result<(), Error> {
    let local_block_hash = local_payload.block_hash();
    let block_hash = signed_block.block_hash();
    if block_hash != local_block_hash {
        return Err(Error::UnknownBlock)
    }

    // OPTIONAL:
    // -- verify w/ consensus?
    // verify slot is timely
    // verify proposer_index is correct
    // verify parent_root matches

    signed_block.verify_signature(public_key, *genesis_validators_root, context).map_err(From::from)
}

#[derive(Clone)]
pub struct Relay(Arc<Inner>);

impl Deref for Relay {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct Inner {
    secret_key: SecretKey,
    public_key: BlsPublicKey,
    genesis_validators_root: Root,
    builder: NullBuilder,
    validator_registry: ValidatorRegistry,
    context: Arc<Context>,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    execution_payloads: HashMap<BidRequest, ExecutionPayload>,
}

impl Relay {
    pub fn new(
        genesis_validators_root: Root,
        beacon_node: Client,
        secret_key: SecretKey,
        context: Arc<Context>,
    ) -> Self {
        let public_key = secret_key.public_key();
        let validator_registry = ValidatorRegistry::new(beacon_node);
        let inner = Inner {
            secret_key,
            public_key,
            genesis_validators_root,
            builder: NullBuilder,
            validator_registry,
            context,
            state: Default::default(),
        };
        Self(Arc::new(inner))
    }

    async fn load_full_validator_set(&self) {
        if let Err(err) = self.validator_registry.load().await {
            tracing::error!("could not load validator set from provided beacon node; please check config and restart: {err}");
        }
    }

    pub async fn initialize(&self) {
        self.load_full_validator_set().await;
    }

    pub async fn on_slot(&self, slot: Slot, next_epoch: bool) {
        if next_epoch {
            // TODO grab validators more efficiently
            self.load_full_validator_set().await;
        }
        let mut state = self.state.lock();
        state
            .execution_payloads
            .retain(|bid_request, _| bid_request.slot + PROPOSAL_TOLERANCE_DELAY >= slot);
    }
}

#[async_trait]
impl BlindedBlockProvider for Relay {
    async fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), Error> {
        let current_time = get_current_unix_time_in_secs();
        self.validator_registry.validate_registrations(
            registrations,
            current_time,
            &self.context,
        )?;
        Ok(())
    }

    async fn fetch_best_bid(&self, bid_request: &BidRequest) -> Result<SignedBuilderBid, Error> {
        validate_bid_request(bid_request, &self.context, &self.validator_registry)?;

        let public_key = &bid_request.public_key;
        let preferences = self
            .validator_registry
            .get_preferences(public_key)
            .ok_or_else(|| Error::MissingPreferences(public_key.clone()))?;
        let (mut payload, value) = self.builder.get_payload_with_value(
            bid_request,
            &preferences.fee_recipient,
            preferences.gas_limit,
            &self.context,
        )?;

        let header = {
            let mut state = self.state.lock();

            validate_execution_payload(&payload, &value, &preferences)?;

            let header = ExecutionPayloadHeader::try_from(&mut payload)?;

            state.execution_payloads.insert(bid_request.clone(), payload);
            header
        };

        match header {
            ExecutionPayloadHeader::Bellatrix(header) => {
                let mut bid =
                    bellatrix::BuilderBid { header, value, public_key: self.public_key.clone() };
                let signature = sign_builder_message(&mut bid, &self.secret_key, &self.context)?;

                let signed_bid = bellatrix::SignedBuilderBid { message: bid, signature };
                Ok(SignedBuilderBid::Bellatrix(signed_bid))
            }
            ExecutionPayloadHeader::Capella(header) => {
                let mut bid =
                    capella::BuilderBid { header, value, public_key: self.public_key.clone() };
                let signature = sign_builder_message(&mut bid, &self.secret_key, &self.context)?;

                let signed_bid = capella::SignedBuilderBid { message: bid, signature };
                Ok(SignedBuilderBid::Capella(signed_bid))
            }
            ExecutionPayloadHeader::Deneb(_header) => unimplemented!(),
        }
    }

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error> {
        let slot = signed_block.slot();
        let parent_hash = signed_block.parent_hash().clone();
        let proposer_index = signed_block.proposer_index();
        let public_key =
            self.validator_registry.get_public_key(proposer_index).map_err(Error::from)?;
        let bid_request = BidRequest { slot, parent_hash, public_key };

        let payload = {
            let mut state = self.state.lock();
            state.execution_payloads.remove(&bid_request).ok_or(Error::UnknownBid)?
        };

        validate_signed_block(
            signed_block,
            &bid_request.public_key,
            &payload,
            &self.genesis_validators_root,
            &self.context,
        )?;

        Ok(payload)
    }
}
