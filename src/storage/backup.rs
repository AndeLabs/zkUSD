//! Backup and Restore System.
//!
//! Provides reliable backup and restore capabilities for protocol data.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::utils::crypto::Hash;

// ═══════════════════════════════════════════════════════════════════════════════
// BACKUP METADATA
// ═══════════════════════════════════════════════════════════════════════════════

/// Backup format version
pub const BACKUP_VERSION: u32 = 1;

/// Magic bytes for backup file identification
pub const BACKUP_MAGIC: &[u8; 8] = b"ZKUSD_BK";

/// Backup metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMetadata {
    /// Format version
    pub version: u32,
    /// Creation timestamp
    pub created_at: u64,
    /// Block height at backup time
    pub block_height: u64,
    /// State root hash
    pub state_root: [u8; 32],
    /// Number of records
    pub record_count: u64,
    /// Total data size (bytes)
    pub data_size: u64,
    /// Checksum of data
    pub checksum: [u8; 32],
    /// Optional description
    pub description: Option<String>,
    /// Included data types
    pub data_types: Vec<BackupDataType>,
    /// Compression used
    pub compression: CompressionType,
}

impl BackupMetadata {
    /// Create new metadata
    pub fn new(block_height: u64) -> Self {
        Self {
            version: BACKUP_VERSION,
            created_at: current_timestamp(),
            block_height,
            state_root: [0u8; 32],
            record_count: 0,
            data_size: 0,
            checksum: [0u8; 32],
            description: None,
            data_types: Vec::new(),
            compression: CompressionType::None,
        }
    }

    /// Set description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Validate metadata
    pub fn validate(&self) -> Result<()> {
        if self.version > BACKUP_VERSION {
            return Err(Error::Internal(format!(
                "Backup version {} not supported (max: {})",
                self.version, BACKUP_VERSION
            )));
        }
        Ok(())
    }
}

/// Types of data that can be backed up
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupDataType {
    /// CDP data
    CDPs,
    /// Vault data
    Vault,
    /// Token balances
    Balances,
    /// Stability pool
    StabilityPool,
    /// Oracle prices
    Prices,
    /// Protocol configuration
    Config,
    /// Events/history
    Events,
    /// Governance state
    Governance,
}

impl BackupDataType {
    /// Get all data types
    pub fn all() -> &'static [BackupDataType] {
        &[
            BackupDataType::CDPs,
            BackupDataType::Vault,
            BackupDataType::Balances,
            BackupDataType::StabilityPool,
            BackupDataType::Prices,
            BackupDataType::Config,
            BackupDataType::Events,
            BackupDataType::Governance,
        ]
    }

    /// Get essential types (minimum for restore)
    pub fn essential() -> &'static [BackupDataType] {
        &[
            BackupDataType::CDPs,
            BackupDataType::Vault,
            BackupDataType::Balances,
            BackupDataType::StabilityPool,
            BackupDataType::Config,
        ]
    }
}

/// Compression type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionType {
    /// No compression
    None,
    /// Zstd compression (placeholder)
    Zstd,
}

// ═══════════════════════════════════════════════════════════════════════════════
// BACKUP RECORD
// ═══════════════════════════════════════════════════════════════════════════════

/// A single backup record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRecord {
    /// Data type
    pub data_type: BackupDataType,
    /// Key
    pub key: Vec<u8>,
    /// Value
    pub value: Vec<u8>,
}

impl BackupRecord {
    /// Create new record
    pub fn new(data_type: BackupDataType, key: Vec<u8>, value: Vec<u8>) -> Self {
        Self { data_type, key, value }
    }

    /// Get size in bytes
    pub fn size(&self) -> usize {
        1 + self.key.len() + self.value.len() + 8 // type + key + value + lengths
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BACKUP FILE
// ═══════════════════════════════════════════════════════════════════════════════

/// Complete backup file structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupFile {
    /// Metadata
    pub metadata: BackupMetadata,
    /// Records
    pub records: Vec<BackupRecord>,
}

impl BackupFile {
    /// Create new backup file
    pub fn new(metadata: BackupMetadata) -> Self {
        Self {
            metadata,
            records: Vec::new(),
        }
    }

    /// Add record
    pub fn add_record(&mut self, record: BackupRecord) {
        self.metadata.data_size += record.size() as u64;
        self.metadata.record_count += 1;
        self.records.push(record);
    }

    /// Add multiple records
    pub fn add_records(&mut self, records: impl IntoIterator<Item = BackupRecord>) {
        for record in records {
            self.add_record(record);
        }
    }

    /// Finalize backup (compute checksum)
    pub fn finalize(&mut self) {
        let data = bincode::serialize(&self.records).unwrap_or_default();
        self.metadata.checksum = *Hash::sha256(&data).as_bytes();
    }

    /// Verify checksum
    pub fn verify(&self) -> Result<()> {
        let data = bincode::serialize(&self.records).unwrap_or_default();
        let computed = *Hash::sha256(&data).as_bytes();

        if computed != self.metadata.checksum {
            return Err(Error::Storage("Backup checksum mismatch".into()));
        }
        Ok(())
    }

    /// Write to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut buffer = Vec::new();

        // Magic bytes
        buffer.extend_from_slice(BACKUP_MAGIC);

        // Version
        buffer.extend_from_slice(&BACKUP_VERSION.to_le_bytes());

        // Serialized content
        let content = bincode::serialize(self)
            .map_err(|e| Error::Storage(format!("Serialization error: {}", e)))?;

        // Content length
        buffer.extend_from_slice(&(content.len() as u64).to_le_bytes());

        // Content
        buffer.extend_from_slice(&content);

        Ok(buffer)
    }

    /// Read from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 20 {
            return Err(Error::Storage("Backup too small".into()));
        }

        // Check magic
        if &data[0..8] != BACKUP_MAGIC {
            return Err(Error::Storage("Invalid backup magic".into()));
        }

        // Check version
        let version = u32::from_le_bytes(data[8..12].try_into().unwrap());
        if version > BACKUP_VERSION {
            return Err(Error::Storage(format!(
                "Unsupported backup version: {}",
                version
            )));
        }

        // Get content length
        let content_len = u64::from_le_bytes(data[12..20].try_into().unwrap()) as usize;

        if data.len() < 20 + content_len {
            return Err(Error::Storage("Backup truncated".into()));
        }

        // Deserialize content
        let backup: BackupFile = bincode::deserialize(&data[20..20 + content_len])
            .map_err(|e| Error::Storage(format!("Deserialization error: {}", e)))?;

        Ok(backup)
    }

    /// Save to file
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        let data = self.to_bytes()?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::Storage(format!("Cannot create directory: {}", e)))?;
        }

        std::fs::write(path, &data)
            .map_err(|e| Error::Storage(format!("Cannot write backup: {}", e)))?;

        Ok(())
    }

    /// Load from file
    pub fn load(path: &PathBuf) -> Result<Self> {
        let data = std::fs::read(path)
            .map_err(|e| Error::Storage(format!("Cannot read backup: {}", e)))?;

        Self::from_bytes(&data)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BACKUP MANAGER
// ═══════════════════════════════════════════════════════════════════════════════

/// Configuration for backup manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    /// Backup directory
    pub backup_dir: PathBuf,
    /// Maximum backups to retain
    pub max_backups: usize,
    /// Auto-backup interval (blocks)
    pub auto_backup_interval: Option<u64>,
    /// Include events in backup
    pub include_events: bool,
    /// Compression enabled
    pub compression: CompressionType,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            backup_dir: PathBuf::from("./backups"),
            max_backups: 10,
            auto_backup_interval: Some(10000), // Every 10k blocks
            include_events: false,
            compression: CompressionType::None,
        }
    }
}

/// Backup manager
#[derive(Debug)]
pub struct BackupManager {
    /// Configuration
    config: BackupConfig,
    /// Last backup block
    last_backup_block: u64,
    /// Backup history
    backup_history: Vec<BackupInfo>,
}

/// Brief backup info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    /// Backup file path
    pub path: PathBuf,
    /// Creation timestamp
    pub created_at: u64,
    /// Block height
    pub block_height: u64,
    /// File size in bytes
    pub size_bytes: u64,
    /// Record count
    pub record_count: u64,
}

impl BackupManager {
    /// Create new backup manager
    pub fn new(config: BackupConfig) -> Self {
        Self {
            config,
            last_backup_block: 0,
            backup_history: Vec::new(),
        }
    }

    /// Check if backup is needed
    pub fn needs_backup(&self, current_block: u64) -> bool {
        if let Some(interval) = self.config.auto_backup_interval {
            current_block >= self.last_backup_block + interval
        } else {
            false
        }
    }

    /// Generate backup filename
    pub fn generate_filename(&self, block_height: u64) -> PathBuf {
        let timestamp = current_timestamp();
        self.config.backup_dir.join(format!(
            "zkusd_backup_{}_block_{}.bak",
            timestamp, block_height
        ))
    }

    /// Create backup
    pub fn create_backup(
        &mut self,
        block_height: u64,
        state_root: [u8; 32],
        data: HashMap<BackupDataType, Vec<BackupRecord>>,
    ) -> Result<BackupInfo> {
        let mut metadata = BackupMetadata::new(block_height);
        metadata.state_root = state_root;
        metadata.compression = self.config.compression;

        // Collect data types
        metadata.data_types = data.keys().copied().collect();

        let mut backup = BackupFile::new(metadata);

        // Add all records
        for (_data_type, records) in data {
            backup.add_records(records);
        }

        // Finalize
        backup.finalize();

        // Save
        let path = self.generate_filename(block_height);
        backup.save(&path)?;

        // Get file size
        let size_bytes = std::fs::metadata(&path)
            .map(|m| m.len())
            .unwrap_or(0);

        let info = BackupInfo {
            path: path.clone(),
            created_at: backup.metadata.created_at,
            block_height,
            size_bytes,
            record_count: backup.metadata.record_count,
        };

        // Update state
        self.last_backup_block = block_height;
        self.backup_history.push(info.clone());

        // Cleanup old backups
        self.cleanup_old_backups()?;

        Ok(info)
    }

    /// Restore from backup
    pub fn restore(&self, path: &PathBuf) -> Result<BackupFile> {
        let backup = BackupFile::load(path)?;

        // Verify checksum
        backup.verify()?;

        // Validate metadata
        backup.metadata.validate()?;

        Ok(backup)
    }

    /// List available backups
    pub fn list_backups(&self) -> Result<Vec<BackupInfo>> {
        let mut backups = Vec::new();

        if !self.config.backup_dir.exists() {
            return Ok(backups);
        }

        for entry in std::fs::read_dir(&self.config.backup_dir)
            .map_err(|e| Error::Storage(format!("Cannot read backup dir: {}", e)))?
        {
            let entry = entry.map_err(|e| Error::Storage(e.to_string()))?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("bak") {
                if let Ok(backup) = BackupFile::load(&path) {
                    let size_bytes = std::fs::metadata(&path)
                        .map(|m| m.len())
                        .unwrap_or(0);

                    backups.push(BackupInfo {
                        path,
                        created_at: backup.metadata.created_at,
                        block_height: backup.metadata.block_height,
                        size_bytes,
                        record_count: backup.metadata.record_count,
                    });
                }
            }
        }

        // Sort by block height descending
        backups.sort_by(|a, b| b.block_height.cmp(&a.block_height));

        Ok(backups)
    }

    /// Get latest backup
    pub fn latest_backup(&self) -> Result<Option<BackupInfo>> {
        self.list_backups().map(|mut v| v.pop())
    }

    /// Cleanup old backups
    fn cleanup_old_backups(&self) -> Result<()> {
        let mut backups = self.list_backups()?;

        while backups.len() > self.config.max_backups {
            if let Some(oldest) = backups.pop() {
                std::fs::remove_file(&oldest.path)
                    .map_err(|e| Error::Storage(format!("Cannot remove backup: {}", e)))?;
            }
        }

        Ok(())
    }

    /// Verify backup integrity
    pub fn verify_backup(&self, path: &PathBuf) -> Result<VerificationResult> {
        let backup = BackupFile::load(path)?;

        let mut result = VerificationResult {
            valid: true,
            version_ok: true,
            checksum_ok: true,
            records_readable: true,
            block_height: backup.metadata.block_height,
            record_count: backup.metadata.record_count,
            errors: Vec::new(),
        };

        // Check version
        if backup.metadata.version > BACKUP_VERSION {
            result.valid = false;
            result.version_ok = false;
            result.errors.push(format!(
                "Unsupported version: {}",
                backup.metadata.version
            ));
        }

        // Verify checksum
        if backup.verify().is_err() {
            result.valid = false;
            result.checksum_ok = false;
            result.errors.push("Checksum mismatch".into());
        }

        // Check records are readable
        if backup.records.is_empty() && backup.metadata.record_count > 0 {
            result.valid = false;
            result.records_readable = false;
            result.errors.push("Records missing or corrupt".into());
        }

        Ok(result)
    }

    /// Get backup statistics
    pub fn statistics(&self) -> Result<BackupStatistics> {
        let backups = self.list_backups()?;

        let total_size: u64 = backups.iter().map(|b| b.size_bytes).sum();
        let total_records: u64 = backups.iter().map(|b| b.record_count).sum();

        let oldest = backups.last().map(|b| b.created_at);
        let newest = backups.first().map(|b| b.created_at);

        Ok(BackupStatistics {
            backup_count: backups.len(),
            total_size_bytes: total_size,
            total_records,
            oldest_backup: oldest,
            newest_backup: newest,
            last_backup_block: self.last_backup_block,
        })
    }
}

impl Default for BackupManager {
    fn default() -> Self {
        Self::new(BackupConfig::default())
    }
}

/// Verification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Overall validity
    pub valid: bool,
    /// Version check passed
    pub version_ok: bool,
    /// Checksum check passed
    pub checksum_ok: bool,
    /// Records are readable
    pub records_readable: bool,
    /// Block height
    pub block_height: u64,
    /// Record count
    pub record_count: u64,
    /// Error messages
    pub errors: Vec<String>,
}

/// Backup statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupStatistics {
    /// Number of backups
    pub backup_count: usize,
    /// Total size of all backups
    pub total_size_bytes: u64,
    /// Total records across all backups
    pub total_records: u64,
    /// Oldest backup timestamp
    pub oldest_backup: Option<u64>,
    /// Newest backup timestamp
    pub newest_backup: Option<u64>,
    /// Last backup block height
    pub last_backup_block: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// HELPER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Get current timestamp
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_metadata() {
        let meta = BackupMetadata::new(1000);
        assert_eq!(meta.version, BACKUP_VERSION);
        assert_eq!(meta.block_height, 1000);
        assert!(meta.validate().is_ok());
    }

    #[test]
    fn test_backup_record() {
        let record = BackupRecord::new(
            BackupDataType::CDPs,
            vec![1, 2, 3],
            vec![4, 5, 6, 7],
        );
        assert!(record.size() > 0);
    }

    #[test]
    fn test_backup_file_roundtrip() {
        let mut backup = BackupFile::new(BackupMetadata::new(100));

        backup.add_record(BackupRecord::new(
            BackupDataType::CDPs,
            b"cdp:1".to_vec(),
            b"test data".to_vec(),
        ));

        backup.finalize();

        // Serialize and deserialize
        let bytes = backup.to_bytes().unwrap();
        let restored = BackupFile::from_bytes(&bytes).unwrap();

        assert_eq!(restored.metadata.block_height, 100);
        assert_eq!(restored.records.len(), 1);
        assert!(restored.verify().is_ok());
    }

    #[test]
    fn test_backup_verification() {
        let mut backup = BackupFile::new(BackupMetadata::new(100));
        backup.add_record(BackupRecord::new(
            BackupDataType::Config,
            b"key".to_vec(),
            b"value".to_vec(),
        ));
        backup.finalize();

        assert!(backup.verify().is_ok());
    }

    #[test]
    fn test_backup_data_types() {
        assert!(!BackupDataType::all().is_empty());
        assert!(!BackupDataType::essential().is_empty());
        assert!(BackupDataType::essential().len() < BackupDataType::all().len());
    }

    #[test]
    fn test_backup_config_default() {
        let config = BackupConfig::default();
        assert!(config.max_backups > 0);
        assert!(config.auto_backup_interval.is_some());
    }

    #[test]
    fn test_backup_manager() {
        let config = BackupConfig {
            backup_dir: PathBuf::from("/tmp/zkusd_test_backups"),
            max_backups: 3,
            auto_backup_interval: Some(100),
            include_events: false,
            compression: CompressionType::None,
        };

        let manager = BackupManager::new(config);

        assert!(!manager.needs_backup(50));
        assert!(manager.needs_backup(100));
    }
}
