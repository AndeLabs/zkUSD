//! Bitcoin integration module.
//!
//! This module provides real Bitcoin transaction building and signing
//! for the zkUSD protocol operations.

pub mod tx_builder;
pub mod utxo;
pub mod scripts;

pub use tx_builder::*;
pub use utxo::*;
pub use scripts::*;
