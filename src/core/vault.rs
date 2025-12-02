//! Vault management for zkBTC collateral.
//!
//! This module manages the vault that holds zkBTC collateral:
//! - Collateral deposits and withdrawals
//! - Integration with Grail Pro for BTC<->zkBTC conversion
//! - Collateral accounting

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::utils::constants::*;
use crate::utils::crypto::{CDPId, Hash, PublicKey};
use crate::utils::math::*;

// ═══════════════════════════════════════════════════════════════════════════════
// COLLATERAL AMOUNT
// ═══════════════════════════════════════════════════════════════════════════════

/// Strongly-typed collateral amount in satoshis
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CollateralAmount(u64);

impl CollateralAmount {
    /// Zero amount
    pub const ZERO: Self = Self(0);

    /// Create from satoshis
    pub const fn from_sats(sats: u64) -> Self {
        Self(sats)
    }

    /// Create from BTC (for convenience)
    pub fn from_btc(btc: u64) -> Self {
        Self(btc * SATS_PER_BTC)
    }

    /// Create from fractional BTC
    pub fn from_btc_decimal(btc: f64) -> Self {
        Self((btc * SATS_PER_BTC as f64) as u64)
    }

    /// Get raw satoshi value
    pub fn sats(&self) -> u64 {
        self.0
    }

    /// Get value in BTC (truncated)
    pub fn btc(&self) -> u64 {
        self.0 / SATS_PER_BTC
    }

    /// Get formatted string representation
    pub fn to_string_formatted(&self) -> String {
        let btc = self.0 as f64 / SATS_PER_BTC as f64;
        format!("{:.8} BTC", btc)
    }

    /// Check if zero
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Calculate value in USD cents
    pub fn value_in_cents(&self, btc_price_cents: u64) -> u64 {
        calculate_collateral_value(self.0, btc_price_cents).unwrap_or(0)
    }

    /// Saturating addition
    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// Saturating subtraction
    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    /// Checked addition
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    /// Checked subtraction
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }
}

impl std::fmt::Display for CollateralAmount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string_formatted())
    }
}

impl From<u64> for CollateralAmount {
    fn from(sats: u64) -> Self {
        Self(sats)
    }
}

impl From<CollateralAmount> for u64 {
    fn from(amount: CollateralAmount) -> Self {
        amount.0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VAULT OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Type of vault operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VaultOperation {
    /// Deposit collateral into vault
    Deposit,
    /// Withdraw collateral from vault
    Withdraw,
    /// Collateral seized during liquidation
    Seize,
    /// Collateral redistributed from liquidation
    Redistribute,
}

/// Record of a vault operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultEvent {
    /// Type of operation
    pub operation: VaultOperation,
    /// CDP ID
    pub cdp_id: CDPId,
    /// Amount in satoshis
    pub amount: CollateralAmount,
    /// Block height
    pub block_height: u64,
    /// Transaction hash
    pub tx_hash: Hash,
}

// ═══════════════════════════════════════════════════════════════════════════════
// VAULT STATE
// ═══════════════════════════════════════════════════════════════════════════════

/// Current state of the vault
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultState {
    /// Total collateral in the vault
    pub total_collateral: CollateralAmount,
    /// Collateral by CDP
    pub collateral_by_cdp: HashMap<CDPId, CollateralAmount>,
    /// Number of CDPs with collateral
    pub cdp_count: u64,
}

impl Default for VaultState {
    fn default() -> Self {
        Self {
            total_collateral: CollateralAmount::ZERO,
            collateral_by_cdp: HashMap::new(),
            cdp_count: 0,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VAULT
// ═══════════════════════════════════════════════════════════════════════════════

/// The collateral vault for zkUSD
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vault {
    /// Vault state
    state: VaultState,
    /// Recent events
    events: Vec<VaultEvent>,
    /// Maximum events to keep
    max_events: usize,
}

impl Default for Vault {
    fn default() -> Self {
        Self::new()
    }
}

impl Vault {
    /// Create a new vault
    pub fn new() -> Self {
        Self {
            state: VaultState::default(),
            events: Vec::new(),
            max_events: 1000,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // DEPOSIT/WITHDRAW
    // ═══════════════════════════════════════════════════════════════════════════

    /// Deposit collateral for a CDP
    pub fn deposit(
        &mut self,
        cdp_id: CDPId,
        amount: CollateralAmount,
        block_height: u64,
        tx_hash: Hash,
    ) -> Result<()> {
        if amount.is_zero() {
            return Err(Error::ZeroAmount);
        }

        if amount.sats() < DUST_LIMIT_SATS {
            return Err(Error::InvalidParameter {
                name: "amount".into(),
                reason: format!("below dust limit of {} sats", DUST_LIMIT_SATS),
            });
        }

        // Update CDP collateral
        let current = self.state.collateral_by_cdp.get(&cdp_id).copied()
            .unwrap_or(CollateralAmount::ZERO);

        let was_zero = current.is_zero();
        let new_amount = current.checked_add(amount).ok_or(Error::Overflow {
            operation: "deposit collateral".into(),
        })?;

        self.state.collateral_by_cdp.insert(cdp_id, new_amount);

        // Update totals
        self.state.total_collateral = self.state.total_collateral
            .checked_add(amount)
            .ok_or(Error::Overflow {
                operation: "total collateral".into(),
            })?;

        if was_zero {
            self.state.cdp_count += 1;
        }

        // Record event
        self.add_event(VaultEvent {
            operation: VaultOperation::Deposit,
            cdp_id,
            amount,
            block_height,
            tx_hash,
        });

        Ok(())
    }

    /// Withdraw collateral from a CDP
    pub fn withdraw(
        &mut self,
        cdp_id: CDPId,
        amount: CollateralAmount,
        block_height: u64,
        tx_hash: Hash,
    ) -> Result<()> {
        if amount.is_zero() {
            return Err(Error::ZeroAmount);
        }

        let current = self.state.collateral_by_cdp.get(&cdp_id).copied()
            .ok_or_else(|| Error::CDPNotFound(cdp_id.to_hex()))?;

        if amount > current {
            return Err(Error::InsufficientCollateral {
                required: amount.sats(),
                available: current.sats(),
            });
        }

        // Update CDP collateral
        let new_amount = current.saturating_sub(amount);
        if new_amount.is_zero() {
            self.state.collateral_by_cdp.remove(&cdp_id);
            self.state.cdp_count = self.state.cdp_count.saturating_sub(1);
        } else {
            self.state.collateral_by_cdp.insert(cdp_id, new_amount);
        }

        // Update total
        self.state.total_collateral = self.state.total_collateral.saturating_sub(amount);

        // Record event
        self.add_event(VaultEvent {
            operation: VaultOperation::Withdraw,
            cdp_id,
            amount,
            block_height,
            tx_hash,
        });

        Ok(())
    }

    /// Seize collateral during liquidation
    pub fn seize(
        &mut self,
        cdp_id: CDPId,
        amount: CollateralAmount,
        block_height: u64,
        tx_hash: Hash,
    ) -> Result<CollateralAmount> {
        let current = self.state.collateral_by_cdp.get(&cdp_id).copied()
            .ok_or_else(|| Error::CDPNotFound(cdp_id.to_hex()))?;

        // Seize up to available amount
        let seized = amount.min(current);

        let new_amount = current.saturating_sub(seized);
        if new_amount.is_zero() {
            self.state.collateral_by_cdp.remove(&cdp_id);
            self.state.cdp_count = self.state.cdp_count.saturating_sub(1);
        } else {
            self.state.collateral_by_cdp.insert(cdp_id, new_amount);
        }

        self.state.total_collateral = self.state.total_collateral.saturating_sub(seized);

        // Record event
        self.add_event(VaultEvent {
            operation: VaultOperation::Seize,
            cdp_id,
            amount: seized,
            block_height,
            tx_hash,
        });

        Ok(seized)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // QUERIES
    // ═══════════════════════════════════════════════════════════════════════════

    /// Get total collateral in vault
    pub fn total_collateral(&self) -> CollateralAmount {
        self.state.total_collateral
    }

    /// Get collateral for a specific CDP
    pub fn collateral_of(&self, cdp_id: &CDPId) -> CollateralAmount {
        self.state.collateral_by_cdp.get(cdp_id).copied()
            .unwrap_or(CollateralAmount::ZERO)
    }

    /// Get number of CDPs with collateral
    pub fn cdp_count(&self) -> u64 {
        self.state.cdp_count
    }

    /// Get total value of collateral in USD cents
    pub fn total_value(&self, btc_price_cents: u64) -> u64 {
        self.state.total_collateral.value_in_cents(btc_price_cents)
    }

    /// Verify vault invariant (total == sum of all CDP collateral)
    pub fn verify_invariant(&self) -> bool {
        let sum: u64 = self.state.collateral_by_cdp.values()
            .map(|c| c.sats())
            .sum();
        sum == self.state.total_collateral.sats()
    }

    /// Get vault state snapshot
    pub fn state(&self) -> &VaultState {
        &self.state
    }

    /// Get recent events
    pub fn recent_events(&self) -> &[VaultEvent] {
        &self.events
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // INTERNAL
    // ═══════════════════════════════════════════════════════════════════════════

    /// Add an event (with pruning)
    fn add_event(&mut self, event: VaultEvent) {
        self.events.push(event);

        if self.events.len() > self.max_events {
            self.events.drain(0..self.events.len() - self.max_events);
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(|e| Error::Serialization(e.to_string()))
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes).map_err(|e| Error::Deserialization(e.to_string()))
    }

    /// Compute state hash
    pub fn state_hash(&self) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(&self.state.total_collateral.sats().to_be_bytes());

        // Sort for deterministic hashing
        let mut sorted: Vec<_> = self.state.collateral_by_cdp.iter().collect();
        sorted.sort_by_key(|(k, _)| k.as_bytes());

        for (cdp_id, amount) in sorted {
            data.extend_from_slice(cdp_id.as_bytes());
            data.extend_from_slice(&amount.sats().to_be_bytes());
        }

        Hash::sha256(&data)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZKBTC INTERFACE (for Grail Pro integration)
// ═══════════════════════════════════════════════════════════════════════════════

/// Interface for zkBTC operations (to be implemented with actual Grail Pro)
pub trait ZkBTCInterface {
    /// Verify a zkBTC deposit proof
    fn verify_deposit_proof(&self, proof: &[u8]) -> Result<CollateralAmount>;

    /// Create a withdrawal request
    fn create_withdrawal_request(
        &self,
        amount: CollateralAmount,
        recipient: &PublicKey,
    ) -> Result<Hash>;

    /// Verify zkBTC ownership
    fn verify_ownership(&self, owner: &PublicKey, amount: CollateralAmount) -> Result<bool>;
}

/// Mock implementation for testing
#[derive(Debug, Default)]
pub struct MockZkBTCInterface;

impl ZkBTCInterface for MockZkBTCInterface {
    fn verify_deposit_proof(&self, _proof: &[u8]) -> Result<CollateralAmount> {
        // In production, this would verify a ZK proof from Grail Pro
        Ok(CollateralAmount::from_btc(1))
    }

    fn create_withdrawal_request(
        &self,
        _amount: CollateralAmount,
        _recipient: &PublicKey,
    ) -> Result<Hash> {
        Ok(Hash::sha256(b"withdrawal_request"))
    }

    fn verify_ownership(&self, _owner: &PublicKey, _amount: CollateralAmount) -> Result<bool> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cdp_id() -> CDPId {
        let pubkey = PublicKey::new([0x02; PUBKEY_LENGTH]);
        CDPId::generate(&pubkey, 1)
    }

    fn test_cdp_id_2() -> CDPId {
        let pubkey = PublicKey::new([0x03; PUBKEY_LENGTH]);
        CDPId::generate(&pubkey, 1)
    }

    fn test_hash() -> Hash {
        Hash::sha256(b"test")
    }

    #[test]
    fn test_collateral_amount() {
        let amount = CollateralAmount::from_btc(1);
        assert_eq!(amount.sats(), SATS_PER_BTC);
        assert_eq!(amount.btc(), 1);
    }

    #[test]
    fn test_collateral_value() {
        let amount = CollateralAmount::from_btc(1);
        // At $100,000/BTC
        let value = amount.value_in_cents(10_000_000);
        assert_eq!(value, 10_000_000); // $100,000 in cents
    }

    #[test]
    fn test_deposit() {
        let mut vault = Vault::new();
        let cdp_id = test_cdp_id();

        vault.deposit(
            cdp_id,
            CollateralAmount::from_btc(1),
            1,
            test_hash(),
        ).unwrap();

        assert_eq!(vault.collateral_of(&cdp_id), CollateralAmount::from_btc(1));
        assert_eq!(vault.total_collateral(), CollateralAmount::from_btc(1));
        assert_eq!(vault.cdp_count(), 1);
    }

    #[test]
    fn test_withdraw() {
        let mut vault = Vault::new();
        let cdp_id = test_cdp_id();

        vault.deposit(cdp_id, CollateralAmount::from_btc(2), 1, test_hash()).unwrap();
        vault.withdraw(cdp_id, CollateralAmount::from_btc(1), 2, test_hash()).unwrap();

        assert_eq!(vault.collateral_of(&cdp_id), CollateralAmount::from_btc(1));
        assert_eq!(vault.total_collateral(), CollateralAmount::from_btc(1));
    }

    #[test]
    fn test_withdraw_insufficient() {
        let mut vault = Vault::new();
        let cdp_id = test_cdp_id();

        vault.deposit(cdp_id, CollateralAmount::from_btc(1), 1, test_hash()).unwrap();
        let result = vault.withdraw(cdp_id, CollateralAmount::from_btc(2), 2, test_hash());

        assert!(result.is_err());
    }

    #[test]
    fn test_seize() {
        let mut vault = Vault::new();
        let cdp_id = test_cdp_id();

        vault.deposit(cdp_id, CollateralAmount::from_btc(1), 1, test_hash()).unwrap();
        let seized = vault.seize(
            cdp_id,
            CollateralAmount::from_sats(50_000_000),
            2,
            test_hash(),
        ).unwrap();

        assert_eq!(seized, CollateralAmount::from_sats(50_000_000));
        assert_eq!(vault.collateral_of(&cdp_id), CollateralAmount::from_sats(50_000_000));
    }

    #[test]
    fn test_multiple_cdps() {
        let mut vault = Vault::new();
        let cdp1 = test_cdp_id();
        let cdp2 = test_cdp_id_2();

        vault.deposit(cdp1, CollateralAmount::from_btc(1), 1, test_hash()).unwrap();
        vault.deposit(cdp2, CollateralAmount::from_btc(2), 2, test_hash()).unwrap();

        assert_eq!(vault.total_collateral(), CollateralAmount::from_btc(3));
        assert_eq!(vault.cdp_count(), 2);
    }

    #[test]
    fn test_invariant() {
        let mut vault = Vault::new();
        let cdp1 = test_cdp_id();
        let cdp2 = test_cdp_id_2();

        vault.deposit(cdp1, CollateralAmount::from_btc(1), 1, test_hash()).unwrap();
        vault.deposit(cdp2, CollateralAmount::from_btc(2), 2, test_hash()).unwrap();
        vault.withdraw(cdp1, CollateralAmount::from_sats(50_000_000), 3, test_hash()).unwrap();

        assert!(vault.verify_invariant());
    }

    #[test]
    fn test_state_hash_deterministic() {
        let mut vault1 = Vault::new();
        let mut vault2 = Vault::new();
        let cdp = test_cdp_id();

        vault1.deposit(cdp, CollateralAmount::from_btc(1), 1, test_hash()).unwrap();
        vault2.deposit(cdp, CollateralAmount::from_btc(1), 1, test_hash()).unwrap();

        assert_eq!(vault1.state_hash(), vault2.state_hash());
    }
}
