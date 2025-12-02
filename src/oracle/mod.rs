//! Oracle module for price feeds.
//!
//! This module provides price feed functionality:
//! - Multi-source price aggregation
//! - Price validation and sanity checks
//! - HTTP-based exchange price fetching
//! - ZK proof generation for prices

pub mod aggregator;
pub mod fetchers;
pub mod price_feed;
pub mod sources;

pub use aggregator::*;
pub use fetchers::*;
pub use price_feed::*;
pub use sources::*;
