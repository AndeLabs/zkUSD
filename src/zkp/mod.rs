//! Zero-Knowledge Proof module for zkUSD protocol.
//!
//! This module provides the ZK proof infrastructure for proving state transitions
//! on BitcoinOS. It supports multiple zkVM backends (SP1, RISC Zero, etc.) through
//! an abstract interface.

pub mod circuits;
pub mod inputs;
pub mod prover;
pub mod verifier;

pub use circuits::*;
pub use inputs::*;
pub use prover::*;
pub use verifier::*;
