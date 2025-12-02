//! Storage module for persistent data management.
//!
//! This module provides persistence capabilities for the zkUSD protocol:
//! - CDP state storage
//! - Token balance tracking
//! - Protocol configuration persistence
//! - Transaction history
//!
//! ## Backends
//!
//! - **InMemoryStore**: Fast, ephemeral storage for testing
//! - **FileStore**: JSON file-based persistence for development
//! - **BinaryStore**: Compact binary format
//! - **RocksStore**: Production-grade persistence using RocksDB
//!
//! ## Usage
//!
//! ```rust,ignore
//! use zkusd::storage::{StorageBackend, TypedStore};
//!
//! // For testing
//! use zkusd::storage::InMemoryStore;
//! let store = TypedStore::new(InMemoryStore::new());
//!
//! // For production (requires rocksdb-storage feature)
//! #[cfg(feature = "rocksdb-storage")]
//! {
//!     use zkusd::storage::rocks::{RocksStore, RocksConfig};
//!     let store = TypedStore::new(RocksStore::open_default("/path/to/db").unwrap());
//! }
//! ```

pub mod backend;
pub mod rocks;
pub mod state;

pub use backend::*;
pub use rocks::{RocksConfig, BatchOperation, column_families};
#[cfg(feature = "rocksdb-storage")]
pub use rocks::RocksStore;
pub use state::*;
