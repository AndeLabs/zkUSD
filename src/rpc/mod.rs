//! RPC Server Components for zkUSD.
//!
//! This module provides production-grade RPC server components including:
//! - Rate limiting and DDoS protection
//! - Request validation
//! - Health monitoring
//!
//! # Features
//!
//! - `rpc-server`: Enables axum middleware (requires async runtime)

pub mod rate_limiter;

#[cfg(feature = "rpc-server")]
pub mod middleware;

pub use rate_limiter::*;

#[cfg(feature = "rpc-server")]
pub use middleware::*;
