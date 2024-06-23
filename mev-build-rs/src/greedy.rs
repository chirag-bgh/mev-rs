use reth::primitives::TransactionSignedEcRecovered;
// use reth_primitives::Address;
use reth_transaction_pool::{PoolTransaction, ValidPoolTransaction};

pub trait Helper {
    fn profit(&self, base_fee: u64, gas_used: u64) -> u128;

    /// CutoffPriceFromOrder returns the cutoff price for a given order based on the cutoff percent.
    ///
    /// For example, if the cutoff percent is 90, the cutoff price will be 90% of the order price,
    /// rounded down to the nearest integer.
    fn cut_off_price(&self, cutt_off_percent: u128) -> u128;

    /// returns true if the order price is greater than or equal to the minPrice.
    fn is_order_in_price_range(&self, min_price: u128) -> bool;
}

impl Helper for TransactionSignedEcRecovered {
    fn profit(&self, base_fee: u64, gas_used: u64) -> u128 {
        self.effective_tip_per_gas(Some(base_fee)).unwrap()
    }

    /// CutoffPriceFromOrder returns the cutoff price for a given order based on the cutoff percent.
    ///
    /// For example, if the cutoff percent is 90, the cutoff price will be 90% of the order price,
    /// rounded down to the nearest integer.
    fn cut_off_price(&self, cutt_off_percent: u128) -> u128 {
        self.max_fee_per_gas() * cutt_off_percent / 100
    }

    /// returns true if the order price is greater than or equal to the minPrice.
    fn is_order_in_price_range(&self, min_price: u128) -> bool {
        self.max_fee_per_gas() >= min_price
    }
}

impl<T> Helper for ValidPoolTransaction<T>
where
    T: PoolTransaction,
{
    fn profit(&self, base_fee: u64, gas_used: u64) -> u128 {
        self.effective_tip_per_gas(base_fee).unwrap()
    }

    /// CutoffPriceFromOrder returns the cutoff price for a given order based on the cutoff percent.
    ///
    /// For example, if the cutoff percent is 90, the cutoff price will be 90% of the order price,
    /// rounded down to the nearest integer.
    fn cut_off_price(&self, cutt_off_percent: u128) -> u128 {
        self.max_fee_per_gas() * cutt_off_percent / 100
    }

    /// returns true if the order price is greater than or equal to the minPrice.
    fn is_order_in_price_range(&self, min_price: u128) -> bool {
        self.max_fee_per_gas() >= min_price
    }
}
