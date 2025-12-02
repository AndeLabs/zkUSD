//! Protocol state management with persistence.
//!
//! This module provides high-level state management for the zkUSD protocol,
//! including CDP state, token balances, and protocol configuration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::cdp::{CDP, CDPId, CDPStatus};
use crate::core::config::ProtocolConfig;
use crate::error::{Error, Result};
use crate::liquidation::stability_pool::StabilityPool;
use crate::storage::backend::{make_key, prefixes, StorageBackend, TypedStore};
use crate::utils::crypto::{Hash, PublicKey};

// ═══════════════════════════════════════════════════════════════════════════════
// PROTOCOL STATE
// ═══════════════════════════════════════════════════════════════════════════════

/// Complete protocol state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolState {
    /// Protocol configuration
    pub config: ProtocolConfig,
    /// Total zkUSD supply
    pub total_supply: u64,
    /// Total collateral locked
    pub total_collateral: u64,
    /// Total debt (should equal total_supply)
    pub total_debt: u64,
    /// Number of active CDPs
    pub active_cdps: u64,
    /// Current block height
    pub block_height: u64,
    /// Last updated timestamp
    pub last_update: u64,
    /// State version (for migrations)
    pub version: u32,
}

impl Default for ProtocolState {
    fn default() -> Self {
        Self {
            config: ProtocolConfig::default(),
            total_supply: 0,
            total_collateral: 0,
            total_debt: 0,
            active_cdps: 0,
            block_height: 0,
            last_update: 0,
            version: 1,
        }
    }
}

impl ProtocolState {
    /// Create a new protocol state
    pub fn new(config: ProtocolConfig) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }

    /// Update CDP counters when a CDP is created
    pub fn on_cdp_created(&mut self, collateral: u64, debt: u64) {
        self.active_cdps += 1;
        self.total_collateral += collateral;
        self.total_debt += debt;
        self.total_supply += debt;
    }

    /// Update counters when a CDP is closed
    pub fn on_cdp_closed(&mut self, collateral: u64, debt: u64) {
        self.active_cdps = self.active_cdps.saturating_sub(1);
        self.total_collateral = self.total_collateral.saturating_sub(collateral);
        self.total_debt = self.total_debt.saturating_sub(debt);
        self.total_supply = self.total_supply.saturating_sub(debt);
    }

    /// Update collateral
    pub fn on_collateral_change(&mut self, delta: i64) {
        if delta >= 0 {
            self.total_collateral += delta as u64;
        } else {
            self.total_collateral = self.total_collateral.saturating_sub((-delta) as u64);
        }
    }

    /// Update debt/supply
    pub fn on_debt_change(&mut self, delta: i64) {
        if delta >= 0 {
            self.total_debt += delta as u64;
            self.total_supply += delta as u64;
        } else {
            self.total_debt = self.total_debt.saturating_sub((-delta) as u64);
            self.total_supply = self.total_supply.saturating_sub((-delta) as u64);
        }
    }

    /// Verify state invariants
    pub fn verify_invariants(&self) -> Result<()> {
        if self.total_supply != self.total_debt {
            return Err(Error::InvariantViolation(format!(
                "Supply {} != Debt {}",
                self.total_supply, self.total_debt
            )));
        }
        Ok(())
    }

    /// Compute state hash
    pub fn hash(&self) -> Hash {
        let data = bincode::serialize(self).unwrap_or_default();
        Hash::sha256(&data)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// STATE MANAGER
// ═══════════════════════════════════════════════════════════════════════════════

/// High-level state manager for the protocol
pub struct StateManager<B: StorageBackend> {
    /// Underlying storage
    store: TypedStore<B>,
}

impl<B: StorageBackend> StateManager<B> {
    /// Create a new state manager
    pub fn new(backend: B) -> Self {
        Self {
            store: TypedStore::new(backend),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // PROTOCOL STATE
    // ═══════════════════════════════════════════════════════════════════════════

    /// Load protocol state
    pub fn load_protocol_state(&self) -> Result<ProtocolState> {
        let key = make_key(prefixes::CONFIG, b"state");
        self.store.get(&key)?.ok_or_else(|| {
            Error::Internal("Protocol state not found".into())
        })
    }

    /// Save protocol state
    pub fn save_protocol_state(&self, state: &ProtocolState) -> Result<()> {
        let key = make_key(prefixes::CONFIG, b"state");
        self.store.set(&key, state)
    }

    /// Initialize protocol state if not exists
    pub fn initialize_if_needed(&self) -> Result<ProtocolState> {
        let key = make_key(prefixes::CONFIG, b"state");
        if self.store.exists(&key)? {
            self.load_protocol_state()
        } else {
            let state = ProtocolState::default();
            self.save_protocol_state(&state)?;
            Ok(state)
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // CDP MANAGEMENT
    // ═══════════════════════════════════════════════════════════════════════════

    /// Load a CDP by ID
    pub fn load_cdp(&self, id: &CDPId) -> Result<Option<CDP>> {
        let key = make_key(prefixes::CDP, id.as_bytes());
        self.store.get(&key)
    }

    /// Save a CDP
    pub fn save_cdp(&self, cdp: &CDP) -> Result<()> {
        let key = make_key(prefixes::CDP, cdp.id.as_bytes());
        self.store.set(&key, cdp)
    }

    /// Delete a CDP
    pub fn delete_cdp(&self, id: &CDPId) -> Result<bool> {
        let key = make_key(prefixes::CDP, id.as_bytes());
        self.store.delete(&key)
    }

    /// Load all CDPs
    pub fn load_all_cdps(&self) -> Result<Vec<CDP>> {
        let keys = self.store.list_prefix(prefixes::CDP)?;
        let mut cdps = Vec::new();

        for key in keys {
            if let Some(cdp) = self.store.get::<CDP>(&key)? {
                cdps.push(cdp);
            }
        }

        Ok(cdps)
    }

    /// Load CDPs by owner
    pub fn load_cdps_by_owner(&self, owner: &PublicKey) -> Result<Vec<CDP>> {
        let all_cdps = self.load_all_cdps()?;
        Ok(all_cdps.into_iter().filter(|c| c.owner == *owner).collect())
    }

    /// Load active CDPs
    pub fn load_active_cdps(&self) -> Result<Vec<CDP>> {
        let all_cdps = self.load_all_cdps()?;
        Ok(all_cdps
            .into_iter()
            .filter(|c| c.status == CDPStatus::Active || c.status == CDPStatus::AtRisk)
            .collect())
    }

    /// Count CDPs
    pub fn count_cdps(&self) -> Result<usize> {
        let keys = self.store.list_prefix(prefixes::CDP)?;
        Ok(keys.len())
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TOKEN BALANCES
    // ═══════════════════════════════════════════════════════════════════════════

    /// Load token balance for an account
    pub fn load_balance(&self, account: &PublicKey) -> Result<u64> {
        let key = make_key(prefixes::BALANCE, account.as_bytes());
        Ok(self.store.get::<u64>(&key)?.unwrap_or(0))
    }

    /// Save token balance for an account
    pub fn save_balance(&self, account: &PublicKey, balance: u64) -> Result<()> {
        let key = make_key(prefixes::BALANCE, account.as_bytes());
        self.store.set(&key, &balance)
    }

    /// Load all balances
    pub fn load_all_balances(&self) -> Result<HashMap<Vec<u8>, u64>> {
        let keys = self.store.list_prefix(prefixes::BALANCE)?;
        let mut balances = HashMap::new();

        for key in keys {
            if let Some(balance) = self.store.get::<u64>(&key)? {
                // Extract account key (remove prefix)
                let account_key = key[prefixes::BALANCE.len()..].to_vec();
                balances.insert(account_key, balance);
            }
        }

        Ok(balances)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // STABILITY POOL
    // ═══════════════════════════════════════════════════════════════════════════

    /// Load stability pool state
    pub fn load_stability_pool(&self) -> Result<Option<StabilityPool>> {
        let key = make_key(prefixes::STABILITY_POOL, b"main");
        self.store.get(&key)
    }

    /// Save stability pool state
    pub fn save_stability_pool(&self, pool: &StabilityPool) -> Result<()> {
        let key = make_key(prefixes::STABILITY_POOL, b"main");
        self.store.set(&key, pool)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // PRICE DATA
    // ═══════════════════════════════════════════════════════════════════════════

    /// Save latest price data
    pub fn save_price(&self, price_cents: u64, timestamp: u64) -> Result<()> {
        let key = make_key(prefixes::PRICE, b"latest");
        let data = (price_cents, timestamp);
        self.store.set(&key, &data)
    }

    /// Load latest price data
    pub fn load_price(&self) -> Result<Option<(u64, u64)>> {
        let key = make_key(prefixes::PRICE, b"latest");
        self.store.get(&key)
    }

    /// Save price history entry
    pub fn save_price_history(&self, timestamp: u64, price_cents: u64) -> Result<()> {
        let key = make_key(prefixes::PRICE, &timestamp.to_be_bytes());
        self.store.set(&key, &price_cents)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TRANSACTIONS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Save a transaction record
    pub fn save_transaction(&self, tx: &TransactionRecord) -> Result<()> {
        let key = make_key(prefixes::TX, tx.hash.as_bytes());
        self.store.set(&key, tx)
    }

    /// Load a transaction by hash
    pub fn load_transaction(&self, hash: &Hash) -> Result<Option<TransactionRecord>> {
        let key = make_key(prefixes::TX, hash.as_bytes());
        self.store.get(&key)
    }

    /// Load recent transactions (last N)
    pub fn load_recent_transactions(&self, limit: usize) -> Result<Vec<TransactionRecord>> {
        let keys = self.store.list_prefix(prefixes::TX)?;
        let mut txs = Vec::new();

        for key in keys.iter().take(limit) {
            if let Some(tx) = self.store.get::<TransactionRecord>(key)? {
                txs.push(tx);
            }
        }

        // Sort by timestamp descending
        txs.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        txs.truncate(limit);

        Ok(txs)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // UTILITY METHODS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Flush all pending writes
    pub fn flush(&self) -> Result<()> {
        self.store.flush()
    }

    /// Clear all data (for testing)
    pub fn clear(&self) -> Result<()> {
        self.store.clear()
    }

    /// Compute state root hash (Merkle root of all data)
    pub fn compute_state_root(&self) -> Result<Hash> {
        use crate::utils::crypto::merkle_root;

        let mut hashes = Vec::new();

        // Hash protocol state
        if let Ok(state) = self.load_protocol_state() {
            hashes.push(state.hash());
        }

        // Hash all CDPs
        for cdp in self.load_all_cdps()? {
            hashes.push(cdp.state_hash());
        }

        // Compute Merkle root
        Ok(merkle_root(&hashes))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TRANSACTION RECORD
// ═══════════════════════════════════════════════════════════════════════════════

/// Transaction type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionType {
    /// CDP created
    OpenCDP,
    /// Collateral deposited
    Deposit,
    /// Collateral withdrawn
    Withdraw,
    /// zkUSD minted
    Mint,
    /// zkUSD repaid
    Repay,
    /// CDP closed
    CloseCDP,
    /// CDP liquidated
    Liquidation,
    /// zkUSD redeemed
    Redemption,
    /// Stability pool deposit
    SPDeposit,
    /// Stability pool withdrawal
    SPWithdraw,
}

/// Record of a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    /// Transaction hash
    pub hash: Hash,
    /// Transaction type
    pub tx_type: TransactionType,
    /// Associated CDP (if any)
    pub cdp_id: Option<CDPId>,
    /// Account involved
    pub account: PublicKey,
    /// Amount (interpretation depends on tx_type)
    pub amount: u64,
    /// Timestamp
    pub timestamp: u64,
    /// Block height
    pub block_height: u64,
    /// Additional data (JSON-encoded)
    pub metadata: Option<String>,
}

impl TransactionRecord {
    /// Create a new transaction record
    pub fn new(
        tx_type: TransactionType,
        account: PublicKey,
        amount: u64,
        timestamp: u64,
        block_height: u64,
    ) -> Self {
        let mut data = Vec::new();
        data.extend_from_slice(&[tx_type as u8]);
        data.extend_from_slice(account.as_bytes());
        data.extend_from_slice(&amount.to_be_bytes());
        data.extend_from_slice(&timestamp.to_be_bytes());
        data.extend_from_slice(&block_height.to_be_bytes());

        let hash = Hash::sha256(&data);

        Self {
            hash,
            tx_type,
            cdp_id: None,
            account,
            amount,
            timestamp,
            block_height,
            metadata: None,
        }
    }

    /// Set CDP ID
    pub fn with_cdp(mut self, cdp_id: CDPId) -> Self {
        self.cdp_id = Some(cdp_id);
        self
    }

    /// Set metadata
    pub fn with_metadata(mut self, metadata: String) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SNAPSHOT
// ═══════════════════════════════════════════════════════════════════════════════

/// Complete state snapshot for backup/restore
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Protocol state
    pub protocol_state: ProtocolState,
    /// All CDPs
    pub cdps: Vec<CDP>,
    /// All balances (pubkey hash -> balance)
    pub balances: HashMap<String, u64>,
    /// Snapshot timestamp
    pub timestamp: u64,
    /// Snapshot block height
    pub block_height: u64,
    /// State root hash
    pub state_root: Hash,
}

impl<B: StorageBackend> StateManager<B> {
    /// Create a state snapshot
    pub fn create_snapshot(&self, timestamp: u64, block_height: u64) -> Result<StateSnapshot> {
        let protocol_state = self.load_protocol_state().unwrap_or_default();
        let cdps = self.load_all_cdps()?;
        let balances = self.load_all_balances()?
            .into_iter()
            .map(|(k, v)| (hex::encode(k), v))
            .collect();
        let state_root = self.compute_state_root()?;

        Ok(StateSnapshot {
            protocol_state,
            cdps,
            balances,
            timestamp,
            block_height,
            state_root,
        })
    }

    /// Restore from a snapshot
    pub fn restore_from_snapshot(&self, snapshot: &StateSnapshot) -> Result<()> {
        // Clear existing data
        self.clear()?;

        // Restore protocol state
        self.save_protocol_state(&snapshot.protocol_state)?;

        // Restore CDPs
        for cdp in &snapshot.cdps {
            self.save_cdp(cdp)?;
        }

        // Restore balances
        for (key_hex, balance) in &snapshot.balances {
            let key_bytes = hex::decode(key_hex).map_err(|e| {
                Error::Deserialization(format!("Invalid balance key: {}", e))
            })?;
            let key = make_key(prefixes::BALANCE, &key_bytes);
            self.store.set(&key, balance)?;
        }

        self.flush()?;
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::backend::InMemoryStore;
    use crate::utils::crypto::KeyPair;

    fn create_test_manager() -> StateManager<InMemoryStore> {
        StateManager::new(InMemoryStore::new())
    }

    #[test]
    fn test_protocol_state_initialization() {
        let manager = create_test_manager();

        // Should create default state if not exists
        let state = manager.initialize_if_needed().unwrap();
        assert_eq!(state.version, 1);
        assert_eq!(state.total_supply, 0);

        // Should return existing state on second call
        let state2 = manager.initialize_if_needed().unwrap();
        assert_eq!(state.version, state2.version);
    }

    #[test]
    fn test_cdp_persistence() {
        let manager = create_test_manager();

        let keypair = KeyPair::generate();
        let cdp = CDP::with_collateral(*keypair.public_key(), 100_000_000, 1, 100).unwrap();
        let cdp_id = cdp.id;

        // Save CDP
        manager.save_cdp(&cdp).unwrap();

        // Load CDP
        let loaded = manager.load_cdp(&cdp_id).unwrap().unwrap();
        assert_eq!(loaded.id, cdp_id);
        assert_eq!(loaded.collateral_sats, 100_000_000);

        // Load all CDPs
        let all = manager.load_all_cdps().unwrap();
        assert_eq!(all.len(), 1);

        // Delete CDP
        assert!(manager.delete_cdp(&cdp_id).unwrap());
        assert!(manager.load_cdp(&cdp_id).unwrap().is_none());
    }

    #[test]
    fn test_balance_persistence() {
        let manager = create_test_manager();

        let keypair = KeyPair::generate();

        // Save balance
        manager.save_balance(keypair.public_key(), 1000).unwrap();

        // Load balance
        let balance = manager.load_balance(keypair.public_key()).unwrap();
        assert_eq!(balance, 1000);

        // Non-existent balance should be 0
        let other_key = KeyPair::generate();
        let balance = manager.load_balance(other_key.public_key()).unwrap();
        assert_eq!(balance, 0);
    }

    #[test]
    fn test_transaction_record() {
        let keypair = KeyPair::generate();
        let tx = TransactionRecord::new(
            TransactionType::Mint,
            *keypair.public_key(),
            1000,
            1234567890,
            100,
        );

        assert!(!tx.hash.is_zero());
        assert_eq!(tx.tx_type, TransactionType::Mint);
        assert_eq!(tx.amount, 1000);
    }

    #[test]
    fn test_state_snapshot() {
        let manager = create_test_manager();

        // Setup some state
        manager.initialize_if_needed().unwrap();

        let keypair = KeyPair::generate();
        let cdp = CDP::with_collateral(*keypair.public_key(), 100_000_000, 1, 100).unwrap();
        manager.save_cdp(&cdp).unwrap();
        manager.save_balance(keypair.public_key(), 5000).unwrap();

        // Create snapshot
        let snapshot = manager.create_snapshot(1234567890, 1000).unwrap();
        assert_eq!(snapshot.cdps.len(), 1);
        assert!(!snapshot.state_root.is_zero());

        // Clear and restore
        manager.clear().unwrap();
        assert!(manager.load_all_cdps().unwrap().is_empty());

        manager.restore_from_snapshot(&snapshot).unwrap();
        assert_eq!(manager.load_all_cdps().unwrap().len(), 1);
    }

    #[test]
    fn test_protocol_state_invariants() {
        let mut state = ProtocolState::default();

        // Valid state
        assert!(state.verify_invariants().is_ok());

        // Invalid state (supply != debt)
        state.total_supply = 100;
        state.total_debt = 50;
        assert!(state.verify_invariants().is_err());
    }
}
