//! RocksDB storage backend for production persistence.
//!
//! This module provides a high-performance, persistent storage backend using RocksDB.
//! It's designed for production use with features like:
//! - Column families for data separation
//! - Atomic batch writes
//! - Snapshots for consistent reads
//! - Compaction and compression
//!
//! ## Usage
//!
//! ```rust,ignore
//! use zkusd::storage::rocks::{RocksStore, RocksConfig};
//!
//! let config = RocksConfig::default();
//! let store = RocksStore::open("/path/to/db", config)?;
//!
//! store.set(b"key", b"value")?;
//! let value = store.get(b"key")?;
//! ```

#[cfg(feature = "rocksdb-storage")]
use rocksdb::{
    ColumnFamily, ColumnFamilyDescriptor, DB, DBWithThreadMode, IteratorMode,
    Options, SingleThreaded, WriteBatch, WriteOptions,
};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::storage::backend::{StorageBackend, StorageKey, StorageValue};

// ═══════════════════════════════════════════════════════════════════════════════
// CONFIGURATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Configuration for RocksDB storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RocksConfig {
    /// Create database if it doesn't exist
    pub create_if_missing: bool,
    /// Maximum number of open files
    pub max_open_files: i32,
    /// Write buffer size in bytes
    pub write_buffer_size: usize,
    /// Maximum number of write buffers
    pub max_write_buffer_number: i32,
    /// Target file size for compaction
    pub target_file_size_base: u64,
    /// Enable compression
    pub enable_compression: bool,
    /// Enable statistics
    pub enable_statistics: bool,
    /// Block cache size in bytes
    pub block_cache_size: usize,
    /// Enable bloom filters
    pub enable_bloom_filters: bool,
}

impl Default for RocksConfig {
    fn default() -> Self {
        Self {
            create_if_missing: true,
            max_open_files: 256,
            write_buffer_size: 64 * 1024 * 1024, // 64 MB
            max_write_buffer_number: 3,
            target_file_size_base: 64 * 1024 * 1024, // 64 MB
            enable_compression: true,
            enable_statistics: false,
            block_cache_size: 128 * 1024 * 1024, // 128 MB
            enable_bloom_filters: true,
        }
    }
}

impl RocksConfig {
    /// Create a configuration optimized for SSDs
    pub fn for_ssd() -> Self {
        Self {
            max_open_files: 1000,
            write_buffer_size: 128 * 1024 * 1024, // 128 MB
            max_write_buffer_number: 4,
            target_file_size_base: 128 * 1024 * 1024, // 128 MB
            block_cache_size: 256 * 1024 * 1024, // 256 MB
            ..Default::default()
        }
    }

    /// Create a configuration for low memory environments
    pub fn low_memory() -> Self {
        Self {
            max_open_files: 64,
            write_buffer_size: 16 * 1024 * 1024, // 16 MB
            max_write_buffer_number: 2,
            target_file_size_base: 32 * 1024 * 1024, // 32 MB
            block_cache_size: 32 * 1024 * 1024, // 32 MB
            ..Default::default()
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// COLUMN FAMILIES
// ═══════════════════════════════════════════════════════════════════════════════

/// Column family names for data organization
pub mod column_families {
    /// Default column family (required by RocksDB)
    pub const DEFAULT: &str = "default";
    /// CDP data
    pub const CDP: &str = "cdp";
    /// Token balances
    pub const BALANCES: &str = "balances";
    /// Protocol configuration
    pub const CONFIG: &str = "config";
    /// Price data
    pub const PRICES: &str = "prices";
    /// Stability pool
    pub const STABILITY_POOL: &str = "stability_pool";
    /// Transaction history
    pub const TRANSACTIONS: &str = "transactions";
    /// Merkle tree nodes
    pub const MERKLE: &str = "merkle";

    /// Get all column family names
    pub fn all() -> Vec<&'static str> {
        vec![DEFAULT, CDP, BALANCES, CONFIG, PRICES, STABILITY_POOL, TRANSACTIONS, MERKLE]
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ROCKSDB STORE
// ═══════════════════════════════════════════════════════════════════════════════

/// RocksDB storage backend
#[cfg(feature = "rocksdb-storage")]
pub struct RocksStore {
    /// Database handle
    db: Arc<DB>,
    /// Database path
    path: PathBuf,
    /// Configuration
    config: RocksConfig,
}

#[cfg(feature = "rocksdb-storage")]
impl RocksStore {
    /// Open a RocksDB database
    pub fn open<P: AsRef<Path>>(path: P, config: RocksConfig) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Create options
        let mut opts = Options::default();
        opts.create_if_missing(config.create_if_missing);
        opts.create_missing_column_families(true);
        opts.set_max_open_files(config.max_open_files);
        opts.set_write_buffer_size(config.write_buffer_size);
        opts.set_max_write_buffer_number(config.max_write_buffer_number);
        opts.set_target_file_size_base(config.target_file_size_base);

        if config.enable_compression {
            opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        }

        // Create column family descriptors
        let cf_names = column_families::all();
        let cf_descriptors: Vec<ColumnFamilyDescriptor> = cf_names
            .iter()
            .map(|name| {
                let mut cf_opts = Options::default();
                if config.enable_bloom_filters {
                    let mut block_opts = rocksdb::BlockBasedOptions::default();
                    block_opts.set_bloom_filter(10.0, false);
                    cf_opts.set_block_based_table_factory(&block_opts);
                }
                ColumnFamilyDescriptor::new(*name, cf_opts)
            })
            .collect();

        // Open database
        let db = DB::open_cf_descriptors(&opts, &path, cf_descriptors).map_err(|e| {
            Error::Internal(format!("Failed to open RocksDB: {}", e))
        })?;

        Ok(Self {
            db: Arc::new(db),
            path,
            config,
        })
    }

    /// Open with default configuration
    pub fn open_default<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open(path, RocksConfig::default())
    }

    /// Get a column family handle
    fn cf_handle(&self, cf_name: &str) -> Result<&ColumnFamily> {
        self.db.cf_handle(cf_name).ok_or_else(|| {
            Error::Internal(format!("Column family '{}' not found", cf_name))
        })
    }

    /// Get value from specific column family
    pub fn get_cf(&self, cf_name: &str, key: &[u8]) -> Result<Option<StorageValue>> {
        let cf = self.cf_handle(cf_name)?;
        self.db.get_cf(cf, key).map_err(|e| {
            Error::Internal(format!("RocksDB get error: {}", e))
        })
    }

    /// Set value in specific column family
    pub fn set_cf(&self, cf_name: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let cf = self.cf_handle(cf_name)?;
        self.db.put_cf(cf, key, value).map_err(|e| {
            Error::Internal(format!("RocksDB put error: {}", e))
        })
    }

    /// Delete from specific column family
    pub fn delete_cf(&self, cf_name: &str, key: &[u8]) -> Result<bool> {
        let cf = self.cf_handle(cf_name)?;

        // Check if key exists first
        let exists = self.db.get_cf(cf, key).map_err(|e| {
            Error::Internal(format!("RocksDB get error: {}", e))
        })?.is_some();

        if exists {
            self.db.delete_cf(cf, key).map_err(|e| {
                Error::Internal(format!("RocksDB delete error: {}", e))
            })?;
        }

        Ok(exists)
    }

    /// List keys with prefix in specific column family
    pub fn list_prefix_cf(&self, cf_name: &str, prefix: &[u8]) -> Result<Vec<StorageKey>> {
        let cf = self.cf_handle(cf_name)?;

        let iter = self.db.iterator_cf(cf, IteratorMode::From(prefix, rocksdb::Direction::Forward));

        let mut keys = Vec::new();
        for item in iter {
            let (key, _) = item.map_err(|e| {
                Error::Internal(format!("RocksDB iterator error: {}", e))
            })?;

            if !key.starts_with(prefix) {
                break;
            }

            keys.push(key.to_vec());
        }

        Ok(keys)
    }

    /// Execute a batch write
    pub fn write_batch(&self, operations: Vec<BatchOperation>) -> Result<()> {
        let mut batch = WriteBatch::default();

        for op in operations {
            match op {
                BatchOperation::Put { cf, key, value } => {
                    let cf_handle = self.cf_handle(&cf)?;
                    batch.put_cf(cf_handle, &key, &value);
                }
                BatchOperation::Delete { cf, key } => {
                    let cf_handle = self.cf_handle(&cf)?;
                    batch.delete_cf(cf_handle, &key);
                }
            }
        }

        self.db.write(batch).map_err(|e| {
            Error::Internal(format!("RocksDB batch write error: {}", e))
        })
    }

    /// Get database statistics
    pub fn statistics(&self) -> Option<String> {
        // RocksDB statistics are available through properties
        self.db.property_value("rocksdb.stats").ok().flatten()
    }

    /// Compact the database
    pub fn compact(&self) -> Result<()> {
        for cf_name in column_families::all() {
            if let Ok(cf) = self.cf_handle(cf_name) {
                self.db.compact_range_cf(cf, None::<&[u8]>, None::<&[u8]>);
            }
        }
        Ok(())
    }

    /// Get database path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get approximate database size in bytes
    pub fn approximate_size(&self) -> u64 {
        let mut total = 0u64;

        for cf_name in column_families::all() {
            if let Ok(cf) = self.cf_handle(cf_name) {
                if let Ok(Some(size_str)) = self.db.property_value_cf(cf, "rocksdb.estimate-live-data-size") {
                    if let Ok(size) = size_str.parse::<u64>() {
                        total += size;
                    }
                }
            }
        }

        total
    }
}

#[cfg(feature = "rocksdb-storage")]
impl StorageBackend for RocksStore {
    fn get(&self, key: &[u8]) -> Result<Option<StorageValue>> {
        self.db.get(key).map_err(|e| {
            Error::Internal(format!("RocksDB get error: {}", e))
        })
    }

    fn set(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.db.put(key, value).map_err(|e| {
            Error::Internal(format!("RocksDB put error: {}", e))
        })
    }

    fn delete(&self, key: &[u8]) -> Result<bool> {
        let exists = self.db.get(key).map_err(|e| {
            Error::Internal(format!("RocksDB get error: {}", e))
        })?.is_some();

        if exists {
            self.db.delete(key).map_err(|e| {
                Error::Internal(format!("RocksDB delete error: {}", e))
            })?;
        }

        Ok(exists)
    }

    fn exists(&self, key: &[u8]) -> Result<bool> {
        self.db.get(key).map(|v| v.is_some()).map_err(|e| {
            Error::Internal(format!("RocksDB get error: {}", e))
        })
    }

    fn list_prefix(&self, prefix: &[u8]) -> Result<Vec<StorageKey>> {
        let iter = self.db.iterator(IteratorMode::From(prefix, rocksdb::Direction::Forward));

        let mut keys = Vec::new();
        for item in iter {
            let (key, _) = item.map_err(|e| {
                Error::Internal(format!("RocksDB iterator error: {}", e))
            })?;

            if !key.starts_with(prefix) {
                break;
            }

            keys.push(key.to_vec());
        }

        Ok(keys)
    }

    fn flush(&self) -> Result<()> {
        self.db.flush().map_err(|e| {
            Error::Internal(format!("RocksDB flush error: {}", e))
        })
    }

    fn keys(&self) -> Result<Vec<StorageKey>> {
        let iter = self.db.iterator(IteratorMode::Start);

        let mut keys = Vec::new();
        for item in iter {
            let (key, _) = item.map_err(|e| {
                Error::Internal(format!("RocksDB iterator error: {}", e))
            })?;
            keys.push(key.to_vec());
        }

        Ok(keys)
    }

    fn clear(&self) -> Result<()> {
        // Clear by iterating and deleting all keys
        let keys = self.keys()?;
        for key in keys {
            self.db.delete(&key).map_err(|e| {
                Error::Internal(format!("RocksDB delete error: {}", e))
            })?;
        }
        Ok(())
    }
}

/// Batch operation for atomic writes
#[derive(Debug, Clone)]
pub enum BatchOperation {
    /// Put a key-value pair
    Put {
        /// Column family name
        cf: String,
        /// Key
        key: Vec<u8>,
        /// Value
        value: Vec<u8>,
    },
    /// Delete a key
    Delete {
        /// Column family name
        cf: String,
        /// Key
        key: Vec<u8>,
    },
}

impl BatchOperation {
    /// Create a put operation
    pub fn put(cf: impl Into<String>, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Self::Put {
            cf: cf.into(),
            key: key.into(),
            value: value.into(),
        }
    }

    /// Create a delete operation
    pub fn delete(cf: impl Into<String>, key: impl Into<Vec<u8>>) -> Self {
        Self::Delete {
            cf: cf.into(),
            key: key.into(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// STUB IMPLEMENTATION (WHEN FEATURE DISABLED)
// ═══════════════════════════════════════════════════════════════════════════════

/// Stub implementation when RocksDB feature is disabled
#[cfg(not(feature = "rocksdb-storage"))]
pub struct RocksStore;

#[cfg(not(feature = "rocksdb-storage"))]
impl RocksStore {
    /// Open (stub)
    pub fn open<P: AsRef<Path>>(_path: P, _config: RocksConfig) -> Result<Self> {
        Err(Error::Internal(
            "RocksDB feature not enabled. Rebuild with --features rocksdb-storage".into(),
        ))
    }

    /// Open with default config (stub)
    pub fn open_default<P: AsRef<Path>>(_path: P) -> Result<Self> {
        Err(Error::Internal(
            "RocksDB feature not enabled. Rebuild with --features rocksdb-storage".into(),
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = RocksConfig::default();
        assert!(config.create_if_missing);
        assert!(config.enable_compression);
    }

    #[test]
    fn test_config_ssd() {
        let config = RocksConfig::for_ssd();
        assert!(config.write_buffer_size > RocksConfig::default().write_buffer_size);
    }

    #[test]
    fn test_config_low_memory() {
        let config = RocksConfig::low_memory();
        assert!(config.write_buffer_size < RocksConfig::default().write_buffer_size);
    }

    #[test]
    fn test_batch_operation() {
        let put = BatchOperation::put("default", b"key", b"value");
        assert!(matches!(put, BatchOperation::Put { .. }));

        let delete = BatchOperation::delete("default", b"key");
        assert!(matches!(delete, BatchOperation::Delete { .. }));
    }

    #[test]
    fn test_column_families() {
        let cfs = column_families::all();
        assert!(cfs.contains(&column_families::DEFAULT));
        assert!(cfs.contains(&column_families::CDP));
        assert!(cfs.contains(&column_families::BALANCES));
    }

    #[cfg(feature = "rocksdb-storage")]
    #[test]
    fn test_rocks_store_basic() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = RocksStore::open_default(temp_dir.path()).unwrap();

        // Test basic operations
        store.set(b"key1", b"value1").unwrap();
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        // Test exists
        assert!(store.exists(b"key1").unwrap());
        assert!(!store.exists(b"nonexistent").unwrap());

        // Test delete
        assert!(store.delete(b"key1").unwrap());
        assert!(!store.exists(b"key1").unwrap());
    }

    #[cfg(feature = "rocksdb-storage")]
    #[test]
    fn test_rocks_store_prefix() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = RocksStore::open_default(temp_dir.path()).unwrap();

        store.set(b"cdp:1", b"data1").unwrap();
        store.set(b"cdp:2", b"data2").unwrap();
        store.set(b"bal:user1", b"100").unwrap();

        let cdp_keys = store.list_prefix(b"cdp:").unwrap();
        assert_eq!(cdp_keys.len(), 2);

        let bal_keys = store.list_prefix(b"bal:").unwrap();
        assert_eq!(bal_keys.len(), 1);
    }

    #[cfg(feature = "rocksdb-storage")]
    #[test]
    fn test_rocks_store_batch() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = RocksStore::open_default(temp_dir.path()).unwrap();

        let ops = vec![
            BatchOperation::put(column_families::DEFAULT, b"key1", b"value1"),
            BatchOperation::put(column_families::DEFAULT, b"key2", b"value2"),
        ];

        store.write_batch(ops).unwrap();

        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(store.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }
}
