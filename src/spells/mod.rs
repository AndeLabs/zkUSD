//! Spells module for zkUSD protocol.
//!
//! Spells are the on-chain operations that modify protocol state.
//! Each spell is validated using ZK proofs before execution.

pub mod cdp_spells;
pub mod redemption;
pub mod types;

pub use cdp_spells::*;
pub use redemption::*;
pub use types::*;
