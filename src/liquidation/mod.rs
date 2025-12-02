//! Liquidation module for zkUSD protocol.
//!
//! This module handles liquidations and the stability pool:
//! - Liquidation engine for undercollateralized CDPs
//! - Stability pool for absorbing liquidations
//! - Redistribution mechanism for excess debt

pub mod engine;
pub mod stability_pool;

pub use engine::*;
pub use stability_pool::*;
