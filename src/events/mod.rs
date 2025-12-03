//! Event Indexing System for zkUSD protocol.
//!
//! This module provides production-ready event storage, indexing, and querying:
//! - Persistent append-only event log with RocksDB backend
//! - Multiple indexes for efficient queries (by type, CDP, block, time)
//! - Real-time subscription system for event notifications
//! - Batch operations for high-throughput scenarios
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │                     Event Indexer                             │
//! ├──────────────────────────────────────────────────────────────┤
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
//! │  │   Storage   │  │   Indexes   │  │    Subscriptions    │  │
//! │  │  (RocksDB)  │  │  (in-mem)   │  │    (broadcast)      │  │
//! │  └─────────────┘  └─────────────┘  └─────────────────────┘  │
//! └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use zkusd::events::{EventIndexer, EventQuery, EventFilter};
//!
//! // Create indexer with RocksDB backend
//! let indexer = EventIndexer::with_path("/var/lib/zkusd/events")?;
//!
//! // Store events from a block
//! indexer.store_events(block_height, events)?;
//!
//! // Query events
//! let query = EventQuery::new()
//!     .event_type("CDPOpened")
//!     .from_block(1000)
//!     .limit(100);
//! let events = indexer.query(query)?;
//!
//! // Subscribe to events
//! let mut rx = indexer.subscribe();
//! while let Some(event) = rx.recv().await {
//!     println!("New event: {:?}", event);
//! }
//! ```

pub mod indexer;
pub mod storage;

pub use indexer::*;
pub use storage::*;

// Re-export core event types from protocol module
pub use crate::protocol::events::{
    CDPClosedEvent, CDPLiquidatedEvent, CDPOpenedEvent, CollateralDepositedEvent,
    CollateralWithdrawnEvent, ConfigChangedEvent, DebtMintedEvent, DebtRepaidEvent,
    EventLog, GainsClaimedEvent, LiquidationAbsorbedEvent, LiquidationMode,
    PriceUpdatedEvent, ProtocolEvent, RecoveryModeEvent, RedemptionEvent,
    StabilityDepositEvent, StabilityWithdrawEvent, TokenTransferEvent,
};
