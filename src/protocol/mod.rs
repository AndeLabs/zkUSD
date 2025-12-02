//! Protocol module - Core state machine and orchestration.
//!
//! This module provides the central state machine that orchestrates
//! all zkUSD protocol operations atomically and safely.

pub mod events;
pub mod operations;
pub mod state_machine;

pub use events::*;
pub use operations::*;
pub use state_machine::*;
