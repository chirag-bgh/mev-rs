use crate::reth_builder::build::BuildIdentifier;
use ethereum_consensus::{primitives::Slot, Error as ConsensusError};
use reth_interfaces::RethError;
use reth_payload_builder::error::PayloadBuilderError;
use reth_primitives::B256;
use revm::primitives::EVMError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("not aware of any proposals for slot {0}")]
    NoProposals(Slot),
    #[error("missing a build request with identifier {0}")]
    MissingBuild(BuildIdentifier),
    #[error("missing parent block {0}")]
    MissingParentBlock(B256),
    #[error("payload requested but build {0} has not produced one yet")]
    PayloadNotPrepared(BuildIdentifier),
    #[error("{0}")]
    Consensus(#[from] ConsensusError),
    #[error(transparent)]
    Reth(#[from] RethError),
    #[error(transparent)]
    RethPayloadBuilder(#[from] PayloadBuilderError),
    #[error("evm execution error: {0:?}")]
    Execution(#[from] EVMError<RethError>),
    #[error("{0}")]
    Internal(&'static str),
}
