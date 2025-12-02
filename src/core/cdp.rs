//! CDP (Collateralized Debt Position) management.
//!
//! This module implements the core CDP functionality:
//! - Creating and managing CDPs
//! - Depositing and withdrawing collateral
//! - Minting and repaying debt
//! - Checking CDP health

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::utils::constants::*;
use crate::utils::math::*;
use crate::utils::validation::*;

// Re-export CDPId for convenience
pub use crate::utils::crypto::{CDPId, Hash, PublicKey};

// ═══════════════════════════════════════════════════════════════════════════════
// CDP STATUS
// ═══════════════════════════════════════════════════════════════════════════════

/// Status of a CDP
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CDPStatus {
    /// CDP is active and healthy
    Active,
    /// CDP is at risk (ratio close to minimum)
    AtRisk,
    /// CDP is liquidatable (ratio below minimum)
    Liquidatable,
    /// CDP has been closed
    Closed,
    /// CDP has been liquidated
    Liquidated,
}

impl CDPStatus {
    /// Determine status based on collateralization ratio
    pub fn from_ratio(ratio: u64, min_ratio: u64) -> Self {
        if ratio < min_ratio {
            CDPStatus::Liquidatable
        } else if ratio < min_ratio + 20 {
            // Within 20% of liquidation threshold
            CDPStatus::AtRisk
        } else {
            CDPStatus::Active
        }
    }

    /// Check if CDP can be liquidated
    pub fn is_liquidatable(&self) -> bool {
        matches!(self, CDPStatus::Liquidatable)
    }

    /// Check if CDP is closed or liquidated
    pub fn is_terminal(&self) -> bool {
        matches!(self, CDPStatus::Closed | CDPStatus::Liquidated)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CDP STATE
// ═══════════════════════════════════════════════════════════════════════════════

/// State snapshot of a CDP at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CDPState {
    /// Collateralization ratio (percentage)
    pub ratio: u64,
    /// Status of the CDP
    pub status: CDPStatus,
    /// Collateral value in USD cents
    pub collateral_value_cents: u64,
    /// Maximum additional debt that can be minted
    pub max_additional_debt: u64,
    /// Amount of collateral that can be safely withdrawn
    pub withdrawable_collateral: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// CDP
// ═══════════════════════════════════════════════════════════════════════════════

/// A Collateralized Debt Position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CDP {
    /// Unique identifier
    pub id: CDPId,
    /// Owner's public key
    pub owner: PublicKey,
    /// Collateral amount in satoshis (zkBTC)
    pub collateral_sats: u64,
    /// Debt amount in cents (zkUSD)
    pub debt_cents: u64,
    /// Block height when CDP was created
    pub created_at: u64,
    /// Block height of last modification
    pub last_updated: u64,
    /// Current status
    pub status: CDPStatus,
    /// Nonce for operations (prevents replay attacks)
    pub nonce: u64,
}

impl CDP {
    /// Create a new CDP
    pub fn new(owner: PublicKey, nonce: u64, block_height: u64) -> Self {
        let id = CDPId::generate(&owner, nonce);
        Self {
            id,
            owner,
            collateral_sats: 0,
            debt_cents: 0,
            created_at: block_height,
            last_updated: block_height,
            status: CDPStatus::Active,
            nonce,
        }
    }

    /// Create a CDP with initial collateral
    pub fn with_collateral(
        owner: PublicKey,
        collateral_sats: u64,
        nonce: u64,
        block_height: u64,
    ) -> Result<Self> {
        validate_collateral_amount(collateral_sats)?;

        let mut cdp = Self::new(owner, nonce, block_height);
        cdp.collateral_sats = collateral_sats;
        Ok(cdp)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // STATE QUERIES
    // ═══════════════════════════════════════════════════════════════════════════

    /// Calculate current collateralization ratio
    pub fn calculate_ratio(&self, btc_price_cents: u64) -> u64 {
        calculate_collateral_ratio(self.collateral_sats, btc_price_cents, self.debt_cents)
            .unwrap_or(u64::MAX)
    }

    /// Get full state snapshot
    pub fn get_state(&self, btc_price_cents: u64, min_ratio: u64) -> CDPState {
        let ratio = self.calculate_ratio(btc_price_cents);
        let status = if self.status.is_terminal() {
            self.status
        } else {
            CDPStatus::from_ratio(ratio, min_ratio)
        };

        let collateral_value_cents =
            calculate_collateral_value(self.collateral_sats, btc_price_cents).unwrap_or(0);

        let max_additional_debt = if self.debt_cents > 0 {
            calculate_max_debt(self.collateral_sats, btc_price_cents, min_ratio)
                .unwrap_or(0)
                .saturating_sub(self.debt_cents)
        } else {
            calculate_max_debt(self.collateral_sats, btc_price_cents, min_ratio).unwrap_or(0)
        };

        let min_collateral =
            calculate_min_collateral(self.debt_cents, btc_price_cents, min_ratio).unwrap_or(0);
        let withdrawable_collateral = self.collateral_sats.saturating_sub(min_collateral);

        CDPState {
            ratio,
            status,
            collateral_value_cents,
            max_additional_debt,
            withdrawable_collateral,
        }
    }

    /// Check if CDP is healthy (above minimum ratio)
    pub fn is_healthy(&self, btc_price_cents: u64, min_ratio: u64) -> bool {
        !self.status.is_terminal() && self.calculate_ratio(btc_price_cents) >= min_ratio
    }

    /// Check if CDP can be liquidated
    pub fn is_liquidatable(&self, btc_price_cents: u64, min_ratio: u64) -> bool {
        !self.status.is_terminal()
            && self.debt_cents > 0
            && self.calculate_ratio(btc_price_cents) < min_ratio
    }

    /// Check if CDP has any debt
    pub fn has_debt(&self) -> bool {
        self.debt_cents > 0
    }

    /// Check if CDP has any collateral
    pub fn has_collateral(&self) -> bool {
        self.collateral_sats > 0
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // STATE MUTATIONS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Deposit additional collateral
    pub fn deposit_collateral(&mut self, amount_sats: u64, block_height: u64) -> Result<()> {
        validate_collateral_amount(amount_sats)?;

        if self.status.is_terminal() {
            return Err(Error::CDPNotActive(self.id.to_hex()));
        }

        self.collateral_sats = safe_add(self.collateral_sats, amount_sats)?;
        self.last_updated = block_height;
        Ok(())
    }

    /// Withdraw collateral (must maintain minimum ratio)
    pub fn withdraw_collateral(
        &mut self,
        amount_sats: u64,
        btc_price_cents: u64,
        min_ratio: u64,
        block_height: u64,
    ) -> Result<()> {
        if self.status.is_terminal() {
            return Err(Error::CDPNotActive(self.id.to_hex()));
        }

        if amount_sats > self.collateral_sats {
            return Err(Error::InsufficientCollateral {
                required: amount_sats,
                available: self.collateral_sats,
            });
        }

        let new_collateral = self.collateral_sats - amount_sats;

        // Check ratio after withdrawal
        if self.debt_cents > 0 {
            let new_ratio =
                calculate_collateral_ratio(new_collateral, btc_price_cents, self.debt_cents)?;

            if new_ratio < min_ratio {
                return Err(Error::WithdrawalWouldUndercollateralize);
            }
        }

        self.collateral_sats = new_collateral;
        self.last_updated = block_height;
        Ok(())
    }

    /// Mint zkUSD (increase debt)
    pub fn mint_debt(
        &mut self,
        amount_cents: u64,
        btc_price_cents: u64,
        min_ratio: u64,
        block_height: u64,
    ) -> Result<u64> {
        if self.status.is_terminal() {
            return Err(Error::CDPNotActive(self.id.to_hex()));
        }

        let new_debt = safe_add(self.debt_cents, amount_cents)?;
        validate_debt_amount(new_debt)?;

        // Check ratio after minting
        let new_ratio = calculate_collateral_ratio(self.collateral_sats, btc_price_cents, new_debt)?;

        if new_ratio < min_ratio {
            return Err(Error::CollateralizationRatioTooLow {
                current: new_ratio,
                minimum: min_ratio,
            });
        }

        // Calculate borrowing fee
        let fee = calculate_fee_bps(amount_cents, BORROWING_FEE_BPS)?;
        let net_mint = safe_sub(amount_cents, fee)?;

        self.debt_cents = new_debt;
        self.last_updated = block_height;
        self.nonce += 1;

        Ok(net_mint)
    }

    /// Repay zkUSD (decrease debt)
    pub fn repay_debt(&mut self, amount_cents: u64, block_height: u64) -> Result<u64> {
        if self.status.is_terminal() {
            return Err(Error::CDPNotActive(self.id.to_hex()));
        }

        if amount_cents > self.debt_cents {
            // Overpayment: only repay what's owed
            let actual_repay = self.debt_cents;
            self.debt_cents = 0;
            self.last_updated = block_height;
            return Ok(actual_repay);
        }

        self.debt_cents = safe_sub(self.debt_cents, amount_cents)?;
        self.last_updated = block_height;

        // Check minimum debt requirement
        if self.debt_cents > 0 && self.debt_cents < MIN_DEBT {
            return Err(Error::DebtBelowMinimum {
                amount: self.debt_cents,
                minimum: MIN_DEBT,
            });
        }

        Ok(amount_cents)
    }

    /// Close CDP (must have no debt)
    pub fn close(&mut self, block_height: u64) -> Result<u64> {
        if self.status.is_terminal() {
            return Err(Error::CDPNotActive(self.id.to_hex()));
        }

        if self.debt_cents > 0 {
            return Err(Error::InvalidParameter {
                name: "debt".into(),
                reason: "must repay all debt before closing CDP".into(),
            });
        }

        let collateral_to_return = self.collateral_sats;
        self.collateral_sats = 0;
        self.status = CDPStatus::Closed;
        self.last_updated = block_height;

        Ok(collateral_to_return)
    }

    /// Liquidate CDP (called when undercollateralized)
    pub fn liquidate(
        &mut self,
        btc_price_cents: u64,
        min_ratio: u64,
        block_height: u64,
    ) -> Result<LiquidationResult> {
        if self.status.is_terminal() {
            return Err(Error::CDPNotActive(self.id.to_hex()));
        }

        if !self.is_liquidatable(btc_price_cents, min_ratio) {
            return Err(Error::CDPHealthy(self.id.to_hex()));
        }

        let (debt_to_cover, collateral_to_seize, liquidator_bonus) = calculate_liquidation_amounts(
            self.collateral_sats,
            self.debt_cents,
            btc_price_cents,
            LIQUIDATION_BONUS_BPS,
        )?;

        let result = LiquidationResult {
            cdp_id: self.id,
            debt_covered: debt_to_cover,
            collateral_seized: collateral_to_seize,
            liquidator_bonus,
            collateral_remaining: self.collateral_sats.saturating_sub(collateral_to_seize),
            debt_remaining: self.debt_cents.saturating_sub(debt_to_cover),
        };

        // Update CDP state
        self.collateral_sats = result.collateral_remaining;
        self.debt_cents = result.debt_remaining;

        if self.collateral_sats == 0 {
            self.status = CDPStatus::Liquidated;
        }

        self.last_updated = block_height;

        Ok(result)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // AUTHORIZATION
    // ═══════════════════════════════════════════════════════════════════════════

    /// Check if a public key is the owner
    pub fn is_owner(&self, pubkey: &PublicKey) -> bool {
        self.owner == *pubkey
    }

    /// Verify owner for privileged operations
    pub fn verify_owner(&self, pubkey: &PublicKey) -> Result<()> {
        if !self.is_owner(pubkey) {
            return Err(Error::Unauthorized(
                "only CDP owner can perform this operation".into(),
            ));
        }
        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // SERIALIZATION
    // ═══════════════════════════════════════════════════════════════════════════

    /// Serialize CDP to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(|e| Error::Serialization(e.to_string()))
    }

    /// Deserialize CDP from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes).map_err(|e| Error::Deserialization(e.to_string()))
    }

    /// Compute hash of CDP state (for ZK proofs)
    pub fn state_hash(&self) -> Hash {
        let bytes = self.to_bytes().unwrap_or_default();
        Hash::sha256(&bytes)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// LIQUIDATION RESULT
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of a liquidation operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationResult {
    /// CDP that was liquidated
    pub cdp_id: CDPId,
    /// Debt covered by liquidation
    pub debt_covered: u64,
    /// Collateral seized
    pub collateral_seized: u64,
    /// Bonus given to liquidator
    pub liquidator_bonus: u64,
    /// Collateral remaining in CDP (if partial liquidation)
    pub collateral_remaining: u64,
    /// Debt remaining in CDP (if partial liquidation)
    pub debt_remaining: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// CDP MANAGER
// ═══════════════════════════════════════════════════════════════════════════════

/// Manager for all CDPs in the system
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CDPManager {
    /// All CDPs indexed by ID
    cdps: HashMap<CDPId, CDP>,
    /// CDPs indexed by owner
    owner_cdps: HashMap<PublicKey, Vec<CDPId>>,
    /// Total number of active CDPs
    active_count: u64,
}

impl CDPManager {
    /// Create a new CDP manager
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new CDP
    pub fn register(&mut self, cdp: CDP) -> Result<()> {
        if self.cdps.contains_key(&cdp.id) {
            return Err(Error::CDPAlreadyExists(cdp.id.to_hex()));
        }

        let owner = cdp.owner;
        let id = cdp.id;

        self.cdps.insert(id, cdp);
        self.owner_cdps.entry(owner).or_default().push(id);
        self.active_count += 1;

        Ok(())
    }

    /// Get a CDP by ID
    pub fn get(&self, id: &CDPId) -> Option<&CDP> {
        self.cdps.get(id)
    }

    /// Get a mutable CDP by ID
    pub fn get_mut(&mut self, id: &CDPId) -> Option<&mut CDP> {
        self.cdps.get_mut(id)
    }

    /// Get all CDPs for an owner
    pub fn get_by_owner(&self, owner: &PublicKey) -> Vec<&CDP> {
        self.owner_cdps
            .get(owner)
            .map(|ids| ids.iter().filter_map(|id| self.cdps.get(id)).collect())
            .unwrap_or_default()
    }

    /// Get all liquidatable CDPs
    pub fn get_liquidatable(&self, btc_price_cents: u64, min_ratio: u64) -> Vec<&CDP> {
        self.cdps
            .values()
            .filter(|cdp| cdp.is_liquidatable(btc_price_cents, min_ratio))
            .collect()
    }

    /// Get sorted CDPs by ratio (ascending - most risky first)
    pub fn get_sorted_by_ratio(&self, btc_price_cents: u64) -> Vec<(&CDP, u64)> {
        let mut cdps_with_ratio: Vec<_> = self
            .cdps
            .values()
            .filter(|cdp| !cdp.status.is_terminal() && cdp.has_debt())
            .map(|cdp| (cdp, cdp.calculate_ratio(btc_price_cents)))
            .collect();

        cdps_with_ratio.sort_by_key(|(_, ratio)| *ratio);
        cdps_with_ratio
    }

    /// Remove a closed/liquidated CDP
    pub fn remove(&mut self, id: &CDPId) -> Option<CDP> {
        if let Some(cdp) = self.cdps.remove(id) {
            if let Some(owner_cdps) = self.owner_cdps.get_mut(&cdp.owner) {
                owner_cdps.retain(|i| i != id);
            }
            if !cdp.status.is_terminal() {
                self.active_count = self.active_count.saturating_sub(1);
            }
            Some(cdp)
        } else {
            None
        }
    }

    /// Update active count when CDP is closed
    pub fn mark_inactive(&mut self, id: &CDPId) {
        if let Some(cdp) = self.cdps.get(id) {
            if !cdp.status.is_terminal() {
                self.active_count = self.active_count.saturating_sub(1);
            }
        }
    }

    /// Get all CDPs as a vector
    pub fn all_cdps(&self) -> Vec<&CDP> {
        self.cdps.values().collect()
    }

    /// Get total number of CDPs
    pub fn total_count(&self) -> usize {
        self.cdps.len()
    }

    /// Get number of active CDPs
    pub fn active_count(&self) -> u64 {
        self.active_count
    }

    /// Calculate aggregate statistics
    pub fn statistics(&self, btc_price_cents: u64) -> CDPStatistics {
        let mut total_collateral = 0u64;
        let mut total_debt = 0u64;
        let mut liquidatable_count = 0u64;
        let mut liquidatable_debt = 0u64;

        for cdp in self.cdps.values() {
            if !cdp.status.is_terminal() {
                total_collateral = total_collateral.saturating_add(cdp.collateral_sats);
                total_debt = total_debt.saturating_add(cdp.debt_cents);

                if cdp.is_liquidatable(btc_price_cents, MIN_COLLATERAL_RATIO) {
                    liquidatable_count += 1;
                    liquidatable_debt = liquidatable_debt.saturating_add(cdp.debt_cents);
                }
            }
        }

        let avg_ratio = if total_debt > 0 {
            calculate_collateral_ratio(total_collateral, btc_price_cents, total_debt).unwrap_or(0)
        } else {
            0
        };

        CDPStatistics {
            total_cdps: self.cdps.len() as u64,
            active_cdps: self.active_count,
            total_collateral_sats: total_collateral,
            total_debt_cents: total_debt,
            average_ratio: avg_ratio,
            liquidatable_cdps: liquidatable_count,
            liquidatable_debt_cents: liquidatable_debt,
        }
    }
}

/// Aggregate CDP statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CDPStatistics {
    pub total_cdps: u64,
    pub active_cdps: u64,
    pub total_collateral_sats: u64,
    pub total_debt_cents: u64,
    pub average_ratio: u64,
    pub liquidatable_cdps: u64,
    pub liquidatable_debt_cents: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pubkey() -> PublicKey {
        PublicKey::new([0x02; PUBKEY_LENGTH])
    }

    fn test_pubkey_2() -> PublicKey {
        PublicKey::new([0x03; PUBKEY_LENGTH])
    }

    #[test]
    fn test_cdp_creation() {
        let cdp = CDP::new(test_pubkey(), 1, 100);
        assert_eq!(cdp.collateral_sats, 0);
        assert_eq!(cdp.debt_cents, 0);
        assert_eq!(cdp.status, CDPStatus::Active);
    }

    #[test]
    fn test_cdp_with_collateral() {
        let cdp = CDP::with_collateral(test_pubkey(), SATS_PER_BTC, 1, 100).unwrap();
        assert_eq!(cdp.collateral_sats, SATS_PER_BTC);
    }

    #[test]
    fn test_deposit_collateral() {
        let mut cdp = CDP::new(test_pubkey(), 1, 100);
        cdp.deposit_collateral(SATS_PER_BTC, 101).unwrap();
        assert_eq!(cdp.collateral_sats, SATS_PER_BTC);
        assert_eq!(cdp.last_updated, 101);
    }

    #[test]
    fn test_mint_debt() {
        let mut cdp = CDP::with_collateral(test_pubkey(), SATS_PER_BTC, 1, 100).unwrap();

        // BTC at $100,000, mint $50,000 (200% ratio)
        let net_mint = cdp.mint_debt(5_000_000, 10_000_000, 110, 101).unwrap();

        // Net mint = amount - 0.5% fee
        assert_eq!(net_mint, 5_000_000 - 25_000);
        assert_eq!(cdp.debt_cents, 5_000_000);
    }

    #[test]
    fn test_mint_insufficient_collateral() {
        let mut cdp = CDP::with_collateral(test_pubkey(), SATS_PER_BTC, 1, 100).unwrap();

        // Try to mint $95,000 (105% ratio, below 110% MCR)
        let result = cdp.mint_debt(9_500_000, 10_000_000, 110, 101);
        assert!(result.is_err());
    }

    #[test]
    fn test_repay_debt() {
        let mut cdp = CDP::with_collateral(test_pubkey(), SATS_PER_BTC, 1, 100).unwrap();
        cdp.mint_debt(5_000_000, 10_000_000, 110, 101).unwrap();

        cdp.repay_debt(2_000_000, 102).unwrap();
        assert_eq!(cdp.debt_cents, 3_000_000);
    }

    #[test]
    fn test_withdraw_collateral() {
        let mut cdp = CDP::with_collateral(test_pubkey(), SATS_PER_BTC, 1, 100).unwrap();
        cdp.mint_debt(5_000_000, 10_000_000, 110, 101).unwrap();

        // Can withdraw some collateral while maintaining ratio
        let result = cdp.withdraw_collateral(SATS_PER_BTC / 4, 10_000_000, 110, 102);
        assert!(result.is_ok());

        // Cannot withdraw too much
        let result = cdp.withdraw_collateral(SATS_PER_BTC / 2, 10_000_000, 110, 103);
        assert!(result.is_err());
    }

    #[test]
    fn test_close_cdp() {
        let mut cdp = CDP::with_collateral(test_pubkey(), SATS_PER_BTC, 1, 100).unwrap();

        let returned = cdp.close(101).unwrap();
        assert_eq!(returned, SATS_PER_BTC);
        assert_eq!(cdp.status, CDPStatus::Closed);
    }

    #[test]
    fn test_close_cdp_with_debt_fails() {
        let mut cdp = CDP::with_collateral(test_pubkey(), SATS_PER_BTC, 1, 100).unwrap();
        cdp.mint_debt(5_000_000, 10_000_000, 110, 101).unwrap();

        let result = cdp.close(102);
        assert!(result.is_err());
    }

    #[test]
    fn test_liquidation() {
        let mut cdp = CDP::with_collateral(test_pubkey(), SATS_PER_BTC, 1, 100).unwrap();
        cdp.mint_debt(5_000_000, 10_000_000, 110, 101).unwrap();

        // Price drops to $50,000 - ratio becomes 100% (below 110% MCR)
        assert!(cdp.is_liquidatable(5_000_000, 110));

        let result = cdp.liquidate(5_000_000, 110, 102).unwrap();
        assert_eq!(result.debt_covered, 5_000_000);
        assert!(result.collateral_seized > 0);
    }

    #[test]
    fn test_cdp_manager() {
        let mut manager = CDPManager::new();

        let cdp1 = CDP::with_collateral(test_pubkey(), SATS_PER_BTC, 1, 100).unwrap();
        let cdp2 = CDP::with_collateral(test_pubkey_2(), SATS_PER_BTC * 2, 1, 100).unwrap();

        manager.register(cdp1).unwrap();
        manager.register(cdp2).unwrap();

        assert_eq!(manager.total_count(), 2);
        assert_eq!(manager.active_count(), 2);

        let owner_cdps = manager.get_by_owner(&test_pubkey());
        assert_eq!(owner_cdps.len(), 1);
    }

    #[test]
    fn test_cdp_status_from_ratio() {
        assert_eq!(CDPStatus::from_ratio(200, 110), CDPStatus::Active);
        assert_eq!(CDPStatus::from_ratio(120, 110), CDPStatus::AtRisk);
        assert_eq!(CDPStatus::from_ratio(105, 110), CDPStatus::Liquidatable);
    }
}
