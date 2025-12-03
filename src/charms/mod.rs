//! Charms SDK integration for zkUSD.
//!
//! This module provides integration with the Charms token standard on BitcoinOS.
//! Charms is the primary token standard for assets on BitcoinOS, and zkUSD
//! implements this interface to be compatible with the ecosystem.
//!
//! ## Components
//!
//! - **adapter**: Bridge between zkUSD core and Charms SDK
//! - **executor**: BitcoinOS spell executor with ZK proofs
//! - **metadata**: Token metadata registry
//! - **spells**: Charm spell definitions and builders
//! - **token**: Charms-compatible token interface

pub mod adapter;
pub mod executor;
pub mod metadata;
pub mod spells;
pub mod token;

pub use adapter::*;
pub use executor::*;
pub use metadata::*;
pub use spells::*;
pub use token::*;
