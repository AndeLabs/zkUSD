//! Zero-Knowledge Proof module for zkUSD protocol.
//!
//! This module provides the ZK proof infrastructure for proving state transitions
//! on BitcoinOS. It supports multiple zkVM backends (SP1, RISC Zero, etc.) through
//! an abstract interface.
//!
//! ## Backends
//!
//! - **Native**: For testing, executes circuits without ZK
//! - **SP1**: Production-grade zkVM from Succinct Labs
//!
//! ## Usage
//!
//! ```rust,ignore
//! use zkusd::zkp::{ProverManager, ProverBackend};
//!
//! // Use native prover for testing
//! let mut manager = ProverManager::new(ProverBackend::Native);
//!
//! // Or use SP1 for production (requires sp1-prover feature)
//! #[cfg(feature = "sp1-prover")]
//! let mut manager = ProverManager::new(ProverBackend::SP1);
//! ```

pub mod circuits;
pub mod inputs;
pub mod prover;
pub mod sp1_prover;
pub mod verifier;

pub use circuits::*;
pub use inputs::*;
pub use prover::*;
pub use sp1_prover::{SP1Prover, SP1ProverConfig, SP1Verifier, ElfRegistry};
pub use verifier::*;
