//! # zkUSD Protocol
//!
//! A decentralized stablecoin backed 100% by Bitcoin, built natively on BitcoinOS
//! using zkBTC as collateral and Charms as the token standard.
//!
//! ## Architecture
//!
//! The protocol consists of several core modules:
//!
//! - **Core**: Fundamental types, configuration, and CDP engine
//! - **Oracle**: Price feed aggregation with ZK verification
//! - **Liquidation**: Liquidation engine and stability pool
//! - **Spells**: Bitcoin transaction spells for protocol operations
//!
//! ## Design Principles
//!
//! - **Professional**: Production-grade code with comprehensive testing
//! - **Robust**: Fail-safe mechanisms and invariant checking
//! - **Scalable**: Modular architecture supporting future extensions
//! - **Modular**: Clean separation of concerns
//! - **Fluent**: Intuitive API design
//!
//! ## Example
//!
//! ```rust,ignore
//! use zkusd::prelude::*;
//!
//! // Create a new CDP
//! let cdp = CDP::new(owner_pubkey, collateral_amount);
//!
//! // Mint zkUSD against collateral
//! let mint_result = cdp.mint(amount, current_price)?;
//! ```

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    rust_2018_idioms,
    trivial_casts,
    unused_lifetimes,
    unused_qualifications
)]

pub mod btc;
pub mod charms;
pub mod cli;
pub mod core;
pub mod error;
pub mod events;
pub mod governance;
pub mod liquidation;
pub mod monitoring;
pub mod oracle;
pub mod protocol;
pub mod rpc;
pub mod spells;
pub mod storage;
pub mod utils;
pub mod zkp;

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::core::{
        cdp::{CDP, CDPId, CDPState, CDPStatus},
        config::{ProtocolConfig, ProtocolParams},
        token::{ZkUSD, TokenAmount},
        vault::{Vault, VaultState},
    };
    pub use crate::error::{Error, Result};
    pub use crate::liquidation::{
        engine::LiquidationEngine,
        stability_pool::StabilityPool,
    };
    pub use crate::oracle::{
        price_feed::{PriceFeed, PriceData},
        aggregator::PriceAggregator,
    };
    pub use crate::utils::{
        math::FixedPoint,
        crypto::{PublicKey, Signature, Hash},
    };
}

/// Protocol version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Protocol name
pub const PROTOCOL_NAME: &str = "zkUSD";
