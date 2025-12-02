//! Storage module for persistent data management.
//!
//! This module provides persistence capabilities for the zkUSD protocol:
//! - CDP state storage
//! - Token balance tracking
//! - Protocol configuration persistence
//! - Transaction history

pub mod backend;
pub mod state;

pub use backend::*;
pub use state::*;
