//! Oracle module for price feeds.
//!
//! This module provides price feed functionality:
//! - Multi-source price aggregation
//! - Price validation and sanity checks
//! - HTTP-based exchange price fetching
//! - Background price update service
//! - ZK proof generation for prices
//!
//! ## Usage
//!
//! ```rust,ignore
//! // For async oracle service (requires async-oracle feature)
//! #[cfg(feature = "async-oracle")]
//! {
//!     use zkusd::oracle::service::{OracleService, OracleConfig};
//!
//!     let service = OracleService::new(OracleConfig::default()).await?;
//!     service.start();
//!
//!     let price = service.current_price().await;
//! }
//! ```

pub mod aggregator;
pub mod fetchers;
pub mod price_feed;
pub mod service;
pub mod sources;

pub use aggregator::*;
pub use fetchers::*;
pub use price_feed::*;
pub use service::{OracleConfig, OracleState, PriceUpdate, OracleStatistics};
#[cfg(feature = "async-oracle")]
pub use service::OracleService;
pub use sources::*;
