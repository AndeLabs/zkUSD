//! Storage backend implementations.
//!
//! This module provides different storage backends:
//! - InMemoryStore: Fast, ephemeral storage for testing
//! - FileStore: JSON file-based persistent storage
//! - BinaryStore: Compact binary format for production

use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::error::{Error, Result};

// ═══════════════════════════════════════════════════════════════════════════════
// STORAGE TRAIT
// ═══════════════════════════════════════════════════════════════════════════════

/// Key type for storage operations
pub type StorageKey = Vec<u8>;

/// Value type for storage operations
pub type StorageValue = Vec<u8>;

/// Trait for storage backends
pub trait StorageBackend: Send + Sync {
    /// Get a value by key
    fn get(&self, key: &[u8]) -> Result<Option<StorageValue>>;

    /// Set a value for a key
    fn set(&self, key: &[u8], value: &[u8]) -> Result<()>;

    /// Delete a key
    fn delete(&self, key: &[u8]) -> Result<bool>;

    /// Check if a key exists
    fn exists(&self, key: &[u8]) -> Result<bool>;

    /// List all keys with a given prefix
    fn list_prefix(&self, prefix: &[u8]) -> Result<Vec<StorageKey>>;

    /// Flush any pending writes to persistent storage
    fn flush(&self) -> Result<()>;

    /// Get all keys
    fn keys(&self) -> Result<Vec<StorageKey>>;

    /// Clear all data
    fn clear(&self) -> Result<()>;
}

// ═══════════════════════════════════════════════════════════════════════════════
// IN-MEMORY STORE
// ═══════════════════════════════════════════════════════════════════════════════

/// In-memory storage backend (for testing and ephemeral use)
#[derive(Debug, Default)]
pub struct InMemoryStore {
    data: RwLock<HashMap<Vec<u8>, Vec<u8>>>,
}

impl InMemoryStore {
    /// Create a new in-memory store
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }

    /// Get number of entries
    pub fn len(&self) -> usize {
        self.data.read().unwrap().len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.data.read().unwrap().is_empty()
    }
}

impl StorageBackend for InMemoryStore {
    fn get(&self, key: &[u8]) -> Result<Option<StorageValue>> {
        let data = self.data.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        Ok(data.get(key).cloned())
    }

    fn set(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut data = self.data.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        data.insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<bool> {
        let mut data = self.data.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        Ok(data.remove(key).is_some())
    }

    fn exists(&self, key: &[u8]) -> Result<bool> {
        let data = self.data.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        Ok(data.contains_key(key))
    }

    fn list_prefix(&self, prefix: &[u8]) -> Result<Vec<StorageKey>> {
        let data = self.data.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        Ok(data
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }

    fn flush(&self) -> Result<()> {
        // In-memory store doesn't need flushing
        Ok(())
    }

    fn keys(&self) -> Result<Vec<StorageKey>> {
        let data = self.data.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        Ok(data.keys().cloned().collect())
    }

    fn clear(&self) -> Result<()> {
        let mut data = self.data.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        data.clear();
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// FILE-BASED STORE
// ═══════════════════════════════════════════════════════════════════════════════

/// File-based storage backend using JSON
#[derive(Debug)]
pub struct FileStore {
    /// Base directory for storage
    base_path: PathBuf,
    /// In-memory cache
    cache: RwLock<HashMap<Vec<u8>, Vec<u8>>>,
    /// Whether cache is dirty and needs flushing
    dirty: RwLock<bool>,
}

impl FileStore {
    /// Create a new file store at the given path
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let base_path = path.as_ref().to_path_buf();

        // Create directory if it doesn't exist
        if !base_path.exists() {
            fs::create_dir_all(&base_path).map_err(|e| {
                Error::Internal(format!("Failed to create storage directory: {}", e))
            })?;
        }

        let store = Self {
            base_path,
            cache: RwLock::new(HashMap::new()),
            dirty: RwLock::new(false),
        };

        // Load existing data
        store.load_from_disk()?;

        Ok(store)
    }

    /// Get the path for a specific data file
    fn data_file_path(&self) -> PathBuf {
        self.base_path.join("data.json")
    }

    /// Load data from disk
    fn load_from_disk(&self) -> Result<()> {
        let path = self.data_file_path();

        if !path.exists() {
            return Ok(());
        }

        let file = File::open(&path).map_err(|e| {
            Error::Internal(format!("Failed to open data file: {}", e))
        })?;

        let reader = BufReader::new(file);

        // Read as JSON with hex-encoded keys and values
        let data: HashMap<String, String> = serde_json::from_reader(reader).map_err(|e| {
            Error::Internal(format!("Failed to parse data file: {}", e))
        })?;

        let mut cache = self.cache.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;

        for (key_hex, value_hex) in data {
            let key = hex::decode(&key_hex).map_err(|e| {
                Error::Internal(format!("Invalid key in storage: {}", e))
            })?;
            let value = hex::decode(&value_hex).map_err(|e| {
                Error::Internal(format!("Invalid value in storage: {}", e))
            })?;
            cache.insert(key, value);
        }

        Ok(())
    }

    /// Save data to disk
    fn save_to_disk(&self) -> Result<()> {
        let cache = self.cache.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;

        // Convert to hex-encoded format for JSON storage
        let data: HashMap<String, String> = cache
            .iter()
            .map(|(k, v)| (hex::encode(k), hex::encode(v)))
            .collect();

        let path = self.data_file_path();
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| Error::Internal(format!("Failed to open data file for writing: {}", e)))?;

        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &data).map_err(|e| {
            Error::Internal(format!("Failed to write data file: {}", e))
        })?;

        let mut dirty = self.dirty.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        *dirty = false;

        Ok(())
    }
}

impl StorageBackend for FileStore {
    fn get(&self, key: &[u8]) -> Result<Option<StorageValue>> {
        let cache = self.cache.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        Ok(cache.get(key).cloned())
    }

    fn set(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut cache = self.cache.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        cache.insert(key.to_vec(), value.to_vec());

        let mut dirty = self.dirty.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        *dirty = true;

        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<bool> {
        let mut cache = self.cache.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        let existed = cache.remove(key).is_some();

        if existed {
            let mut dirty = self.dirty.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
            *dirty = true;
        }

        Ok(existed)
    }

    fn exists(&self, key: &[u8]) -> Result<bool> {
        let cache = self.cache.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        Ok(cache.contains_key(key))
    }

    fn list_prefix(&self, prefix: &[u8]) -> Result<Vec<StorageKey>> {
        let cache = self.cache.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        Ok(cache
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }

    fn flush(&self) -> Result<()> {
        let dirty = *self.dirty.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        if dirty {
            self.save_to_disk()?;
        }
        Ok(())
    }

    fn keys(&self) -> Result<Vec<StorageKey>> {
        let cache = self.cache.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        Ok(cache.keys().cloned().collect())
    }

    fn clear(&self) -> Result<()> {
        let mut cache = self.cache.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        cache.clear();

        let mut dirty = self.dirty.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        *dirty = true;

        Ok(())
    }
}

impl Drop for FileStore {
    fn drop(&mut self) {
        // Attempt to flush on drop
        let _ = self.flush();
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BINARY STORE (COMPACT FORMAT)
// ═══════════════════════════════════════════════════════════════════════════════

/// Binary storage backend using bincode for compact serialization
#[derive(Debug)]
pub struct BinaryStore {
    /// Base directory for storage
    base_path: PathBuf,
    /// In-memory cache
    cache: RwLock<HashMap<Vec<u8>, Vec<u8>>>,
    /// Whether cache is dirty
    dirty: RwLock<bool>,
}

impl BinaryStore {
    /// Create a new binary store at the given path
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let base_path = path.as_ref().to_path_buf();

        if !base_path.exists() {
            fs::create_dir_all(&base_path).map_err(|e| {
                Error::Internal(format!("Failed to create storage directory: {}", e))
            })?;
        }

        let store = Self {
            base_path,
            cache: RwLock::new(HashMap::new()),
            dirty: RwLock::new(false),
        };

        store.load_from_disk()?;

        Ok(store)
    }

    fn data_file_path(&self) -> PathBuf {
        self.base_path.join("data.bin")
    }

    fn load_from_disk(&self) -> Result<()> {
        let path = self.data_file_path();

        if !path.exists() {
            return Ok(());
        }

        let mut file = File::open(&path).map_err(|e| {
            Error::Internal(format!("Failed to open data file: {}", e))
        })?;

        let mut data = Vec::new();
        file.read_to_end(&mut data).map_err(|e| {
            Error::Internal(format!("Failed to read data file: {}", e))
        })?;

        let loaded: HashMap<Vec<u8>, Vec<u8>> = bincode::deserialize(&data).map_err(|e| {
            Error::Internal(format!("Failed to deserialize data: {}", e))
        })?;

        let mut cache = self.cache.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        *cache = loaded;

        Ok(())
    }

    fn save_to_disk(&self) -> Result<()> {
        let cache = self.cache.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;

        let data = bincode::serialize(&*cache).map_err(|e| {
            Error::Internal(format!("Failed to serialize data: {}", e))
        })?;

        let path = self.data_file_path();
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| Error::Internal(format!("Failed to open data file: {}", e)))?;

        file.write_all(&data).map_err(|e| {
            Error::Internal(format!("Failed to write data file: {}", e))
        })?;

        let mut dirty = self.dirty.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        *dirty = false;

        Ok(())
    }
}

impl StorageBackend for BinaryStore {
    fn get(&self, key: &[u8]) -> Result<Option<StorageValue>> {
        let cache = self.cache.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        Ok(cache.get(key).cloned())
    }

    fn set(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut cache = self.cache.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        cache.insert(key.to_vec(), value.to_vec());

        let mut dirty = self.dirty.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        *dirty = true;

        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<bool> {
        let mut cache = self.cache.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        let existed = cache.remove(key).is_some();

        if existed {
            let mut dirty = self.dirty.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
            *dirty = true;
        }

        Ok(existed)
    }

    fn exists(&self, key: &[u8]) -> Result<bool> {
        let cache = self.cache.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        Ok(cache.contains_key(key))
    }

    fn list_prefix(&self, prefix: &[u8]) -> Result<Vec<StorageKey>> {
        let cache = self.cache.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        Ok(cache
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }

    fn flush(&self) -> Result<()> {
        let dirty = *self.dirty.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        if dirty {
            self.save_to_disk()?;
        }
        Ok(())
    }

    fn keys(&self) -> Result<Vec<StorageKey>> {
        let cache = self.cache.read().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        Ok(cache.keys().cloned().collect())
    }

    fn clear(&self) -> Result<()> {
        let mut cache = self.cache.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        cache.clear();

        let mut dirty = self.dirty.write().map_err(|e| Error::Internal(format!("Lock error: {}", e)))?;
        *dirty = true;

        Ok(())
    }
}

impl Drop for BinaryStore {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TYPED STORE WRAPPER
// ═══════════════════════════════════════════════════════════════════════════════

/// Type-safe wrapper around a storage backend
pub struct TypedStore<B: StorageBackend> {
    backend: B,
}

impl<B: StorageBackend> TypedStore<B> {
    /// Create a new typed store
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    /// Get a typed value
    pub fn get<T: DeserializeOwned>(&self, key: &[u8]) -> Result<Option<T>> {
        match self.backend.get(key)? {
            Some(data) => {
                let value = bincode::deserialize(&data).map_err(|e| {
                    Error::Deserialization(format!("Failed to deserialize value: {}", e))
                })?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Set a typed value
    pub fn set<T: Serialize>(&self, key: &[u8], value: &T) -> Result<()> {
        let data = bincode::serialize(value).map_err(|e| {
            Error::Serialization(format!("Failed to serialize value: {}", e))
        })?;
        self.backend.set(key, &data)
    }

    /// Delete a value
    pub fn delete(&self, key: &[u8]) -> Result<bool> {
        self.backend.delete(key)
    }

    /// Check if a key exists
    pub fn exists(&self, key: &[u8]) -> Result<bool> {
        self.backend.exists(key)
    }

    /// List keys with prefix
    pub fn list_prefix(&self, prefix: &[u8]) -> Result<Vec<StorageKey>> {
        self.backend.list_prefix(prefix)
    }

    /// Flush pending writes
    pub fn flush(&self) -> Result<()> {
        self.backend.flush()
    }

    /// Get all keys
    pub fn keys(&self) -> Result<Vec<StorageKey>> {
        self.backend.keys()
    }

    /// Clear all data
    pub fn clear(&self) -> Result<()> {
        self.backend.clear()
    }

    /// Get the underlying backend
    pub fn backend(&self) -> &B {
        &self.backend
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// KEY PREFIXES
// ═══════════════════════════════════════════════════════════════════════════════

/// Key prefixes for different data types
pub mod prefixes {
    /// CDP data prefix
    pub const CDP: &[u8] = b"cdp:";
    /// Token balance prefix
    pub const BALANCE: &[u8] = b"bal:";
    /// Protocol config prefix
    pub const CONFIG: &[u8] = b"cfg:";
    /// Price data prefix
    pub const PRICE: &[u8] = b"prc:";
    /// Transaction history prefix
    pub const TX: &[u8] = b"tx:";
    /// Stability pool prefix
    pub const STABILITY_POOL: &[u8] = b"sp:";
    /// Deposit prefix
    pub const DEPOSIT: &[u8] = b"dep:";
}

/// Create a key with a prefix
pub fn make_key(prefix: &[u8], key: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(prefix.len() + key.len());
    result.extend_from_slice(prefix);
    result.extend_from_slice(key);
    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_store() {
        let store = InMemoryStore::new();

        // Test set and get
        store.set(b"key1", b"value1").unwrap();
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        // Test non-existent key
        assert_eq!(store.get(b"nonexistent").unwrap(), None);

        // Test exists
        assert!(store.exists(b"key1").unwrap());
        assert!(!store.exists(b"nonexistent").unwrap());

        // Test delete
        assert!(store.delete(b"key1").unwrap());
        assert!(!store.exists(b"key1").unwrap());
    }

    #[test]
    fn test_in_memory_store_prefix() {
        let store = InMemoryStore::new();

        store.set(b"cdp:1", b"data1").unwrap();
        store.set(b"cdp:2", b"data2").unwrap();
        store.set(b"bal:user1", b"100").unwrap();

        let cdp_keys = store.list_prefix(b"cdp:").unwrap();
        assert_eq!(cdp_keys.len(), 2);

        let bal_keys = store.list_prefix(b"bal:").unwrap();
        assert_eq!(bal_keys.len(), 1);
    }

    #[test]
    fn test_typed_store() {
        let backend = InMemoryStore::new();
        let store = TypedStore::new(backend);

        // Store and retrieve a u64
        store.set(b"number", &12345u64).unwrap();
        let value: u64 = store.get(b"number").unwrap().unwrap();
        assert_eq!(value, 12345);

        // Store and retrieve a string
        store.set(b"string", &"hello".to_string()).unwrap();
        let value: String = store.get(b"string").unwrap().unwrap();
        assert_eq!(value, "hello");
    }

    #[test]
    fn test_make_key() {
        let key = make_key(prefixes::CDP, b"12345");
        assert!(key.starts_with(b"cdp:"));
        assert_eq!(&key[4..], b"12345");
    }

    #[test]
    fn test_file_store() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = FileStore::new(temp_dir.path()).unwrap();

        // Test set and get
        store.set(b"key1", b"value1").unwrap();
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        // Test flush
        store.flush().unwrap();

        // Verify file was created
        assert!(temp_dir.path().join("data.json").exists());
    }

    #[test]
    fn test_file_store_persistence() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().to_path_buf();

        // Create and populate store
        {
            let store = FileStore::new(&path).unwrap();
            store.set(b"persistent", b"data").unwrap();
            store.flush().unwrap();
        }

        // Create new store and verify data persisted
        {
            let store = FileStore::new(&path).unwrap();
            assert_eq!(store.get(b"persistent").unwrap(), Some(b"data".to_vec()));
        }
    }

    #[test]
    fn test_binary_store() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = BinaryStore::new(temp_dir.path()).unwrap();

        store.set(b"key1", b"value1").unwrap();
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        store.flush().unwrap();
        assert!(temp_dir.path().join("data.bin").exists());
    }
}
