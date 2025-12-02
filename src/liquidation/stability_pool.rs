//! Stability Pool implementation (Liquity-style).
//!
//! The stability pool allows users to deposit zkUSD which is used to absorb
//! liquidations. In return, depositors receive a share of the liquidated collateral
//! at a discount.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::token::TokenAmount;
use crate::core::vault::CollateralAmount;
use crate::error::{Error, Result};
use crate::utils::constants::*;
use crate::utils::crypto::{Hash, PublicKey};
use crate::utils::math::*;

// ═══════════════════════════════════════════════════════════════════════════════
// DEPOSITOR SNAPSHOT
// ═══════════════════════════════════════════════════════════════════════════════

/// Snapshot of pool state when a depositor made their deposit
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DepositorSnapshot {
    /// Product factor at time of deposit (tracks zkUSD losses)
    pub p: u128,
    /// Sum factor at time of deposit (tracks BTC gains)
    pub s: u128,
    /// Epoch at time of deposit
    pub epoch: u64,
    /// Scale at time of deposit
    pub scale: u64,
}

impl Default for DepositorSnapshot {
    fn default() -> Self {
        Self {
            p: SP_SCALE_FACTOR,
            s: 0,
            epoch: 0,
            scale: 0,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DEPOSIT
// ═══════════════════════════════════════════════════════════════════════════════

/// A single deposit in the stability pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deposit {
    /// Depositor's public key
    pub owner: PublicKey,
    /// Initial deposit amount in cents
    pub initial_amount: TokenAmount,
    /// Snapshot at time of deposit
    pub snapshot: DepositorSnapshot,
    /// Block height of deposit
    pub deposited_at: u64,
}

impl Deposit {
    /// Create a new deposit
    pub fn new(owner: PublicKey, amount: TokenAmount, snapshot: DepositorSnapshot, block_height: u64) -> Self {
        Self {
            owner,
            initial_amount: amount,
            snapshot,
            deposited_at: block_height,
        }
    }

    /// Calculate current deposit value (after absorbing liquidations)
    pub fn current_value(&self, current_p: u128, current_epoch: u64, current_scale: u64) -> TokenAmount {
        // If epochs don't match, deposit has been fully consumed
        if current_epoch != self.snapshot.epoch {
            return TokenAmount::ZERO;
        }

        // If scale differs by more than 1, deposit is negligible
        if current_scale > self.snapshot.scale + 1 {
            return TokenAmount::ZERO;
        }

        // Calculate compounded deposit
        let scale_diff = current_scale - self.snapshot.scale;
        let p_ratio = if scale_diff == 0 {
            current_p * SP_SCALE_FACTOR / self.snapshot.p
        } else {
            // Scale changed, so deposit is much smaller
            current_p * SP_SCALE_FACTOR / (self.snapshot.p * SP_SCALE_FACTOR)
        };

        let compounded = (self.initial_amount.cents() as u128 * p_ratio / SP_SCALE_FACTOR) as u64;
        TokenAmount::from_cents(compounded)
    }

    /// Calculate BTC gains from liquidations
    pub fn btc_gains(&self, current_s: u128, current_epoch: u64, current_scale: u64) -> CollateralAmount {
        // If epochs don't match, calculate gains differently
        if current_epoch != self.snapshot.epoch {
            return CollateralAmount::ZERO;
        }

        // Calculate gains based on S factor difference
        let s_diff = current_s.saturating_sub(self.snapshot.s);

        let gains = (self.initial_amount.cents() as u128 * s_diff / SP_SCALE_FACTOR) as u64;
        CollateralAmount::from_sats(gains)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// STABILITY POOL
// ═══════════════════════════════════════════════════════════════════════════════

/// The Stability Pool for absorbing liquidations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityPool {
    /// Total zkUSD in the pool
    total_deposits: TokenAmount,
    /// Total BTC gained from liquidations
    total_btc_gains: CollateralAmount,
    /// Product factor (tracks zkUSD losses)
    p: u128,
    /// Sum factor (tracks BTC gains per unit deposit)
    s: u128,
    /// Current epoch (increments when P becomes very small)
    epoch: u64,
    /// Current scale (for precision management)
    scale: u64,
    /// Individual deposits
    deposits: HashMap<PublicKey, Deposit>,
    /// Pending BTC rewards by depositor
    pending_btc: HashMap<PublicKey, CollateralAmount>,
    /// Total liquidations absorbed
    total_liquidations: u64,
    /// Total debt absorbed
    total_debt_absorbed: TokenAmount,
}

impl Default for StabilityPool {
    fn default() -> Self {
        Self::new()
    }
}

impl StabilityPool {
    /// Create a new stability pool
    pub fn new() -> Self {
        Self {
            total_deposits: TokenAmount::ZERO,
            total_btc_gains: CollateralAmount::ZERO,
            p: SP_SCALE_FACTOR,
            s: 0,
            epoch: 0,
            scale: 0,
            deposits: HashMap::new(),
            pending_btc: HashMap::new(),
            total_liquidations: 0,
            total_debt_absorbed: TokenAmount::ZERO,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // DEPOSITS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Deposit zkUSD into the stability pool
    pub fn deposit(
        &mut self,
        owner: PublicKey,
        amount: TokenAmount,
        block_height: u64,
    ) -> Result<()> {
        if amount.is_zero() {
            return Err(Error::ZeroAmount);
        }

        if amount.cents() < MIN_SP_DEPOSIT {
            return Err(Error::InvalidParameter {
                name: "amount".into(),
                reason: format!("below minimum deposit of {} cents", MIN_SP_DEPOSIT),
            });
        }

        // If existing deposit, first claim rewards and withdraw
        if let Some(existing) = self.deposits.get(&owner) {
            let current_value = existing.current_value(self.p, self.epoch, self.scale);
            let btc_gains = existing.btc_gains(self.s, self.epoch, self.scale);

            // Store pending BTC for later claim
            let pending = self.pending_btc.entry(owner).or_insert(CollateralAmount::ZERO);
            *pending = pending.saturating_add(btc_gains);

            // Adjust total deposits
            self.total_deposits = self.total_deposits.saturating_sub(current_value);
        }

        // Create snapshot
        let snapshot = DepositorSnapshot {
            p: self.p,
            s: self.s,
            epoch: self.epoch,
            scale: self.scale,
        };

        // Create new deposit
        let deposit = Deposit::new(owner, amount, snapshot, block_height);
        self.deposits.insert(owner, deposit);

        // Update total
        self.total_deposits = self.total_deposits.saturating_add(amount);

        Ok(())
    }

    /// Withdraw zkUSD from the stability pool
    pub fn withdraw(
        &mut self,
        owner: &PublicKey,
        amount: TokenAmount,
        block_height: u64,
    ) -> Result<(TokenAmount, CollateralAmount)> {
        let deposit = self.deposits.get(owner)
            .ok_or_else(|| Error::InvalidParameter {
                name: "owner".into(),
                reason: "no deposit found".into(),
            })?;

        // Calculate current values
        let current_value = deposit.current_value(self.p, self.epoch, self.scale);
        let btc_gains = deposit.btc_gains(self.s, self.epoch, self.scale);

        // Add any pending BTC
        let pending = self.pending_btc.remove(owner).unwrap_or(CollateralAmount::ZERO);
        let total_btc = btc_gains.saturating_add(pending);

        // Validate withdrawal amount
        let withdraw_amount = if amount > current_value {
            current_value
        } else {
            amount
        };

        // Calculate proportional BTC to withdraw
        let btc_to_withdraw = if current_value.is_zero() {
            total_btc
        } else {
            let ratio = withdraw_amount.cents() as u128 * SP_SCALE_FACTOR / current_value.cents() as u128;
            CollateralAmount::from_sats((total_btc.sats() as u128 * ratio / SP_SCALE_FACTOR) as u64)
        };

        // Update deposit or remove if fully withdrawn
        let remaining = current_value.saturating_sub(withdraw_amount);
        if remaining.is_zero() {
            self.deposits.remove(owner);
        } else {
            // Create new deposit with remaining amount
            let snapshot = DepositorSnapshot {
                p: self.p,
                s: self.s,
                epoch: self.epoch,
                scale: self.scale,
            };
            let new_deposit = Deposit::new(*owner, remaining, snapshot, block_height);
            self.deposits.insert(*owner, new_deposit);

            // Store remaining BTC
            let remaining_btc = total_btc.saturating_sub(btc_to_withdraw);
            if !remaining_btc.is_zero() {
                self.pending_btc.insert(*owner, remaining_btc);
            }
        }

        // Update totals
        self.total_deposits = self.total_deposits.saturating_sub(withdraw_amount);
        self.total_btc_gains = self.total_btc_gains.saturating_sub(btc_to_withdraw);

        Ok((withdraw_amount, btc_to_withdraw))
    }

    /// Claim BTC gains without withdrawing zkUSD
    pub fn claim_btc(&mut self, owner: &PublicKey) -> Result<CollateralAmount> {
        let deposit = self.deposits.get(owner)
            .ok_or_else(|| Error::InvalidParameter {
                name: "owner".into(),
                reason: "no deposit found".into(),
            })?;

        // Calculate BTC gains
        let btc_gains = deposit.btc_gains(self.s, self.epoch, self.scale);
        let pending = self.pending_btc.remove(owner).unwrap_or(CollateralAmount::ZERO);
        let total_btc = btc_gains.saturating_add(pending);

        if total_btc.is_zero() {
            return Ok(CollateralAmount::ZERO);
        }

        // Update deposit snapshot to current
        let snapshot = DepositorSnapshot {
            p: self.p,
            s: self.s,
            epoch: self.epoch,
            scale: self.scale,
        };

        let current_value = deposit.current_value(self.p, self.epoch, self.scale);
        let new_deposit = Deposit::new(*owner, current_value, snapshot, deposit.deposited_at);
        self.deposits.insert(*owner, new_deposit);

        // Update total
        self.total_btc_gains = self.total_btc_gains.saturating_sub(total_btc);

        Ok(total_btc)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // LIQUIDATION ABSORPTION
    // ═══════════════════════════════════════════════════════════════════════════

    /// Absorb a liquidation (called by liquidation engine)
    pub fn absorb_liquidation(
        &mut self,
        debt_to_absorb: TokenAmount,
        collateral_gained: CollateralAmount,
    ) -> Result<bool> {
        if self.total_deposits.is_zero() {
            return Ok(false);
        }

        if debt_to_absorb > self.total_deposits {
            // Can only partially absorb
            return Ok(false);
        }

        // Update P (product factor) - represents zkUSD reduction
        // P_new = P * (1 - debt/total_deposits)
        let debt_ratio = (debt_to_absorb.cents() as u128) * SP_SCALE_FACTOR
            / (self.total_deposits.cents() as u128);
        let reduction_factor = SP_SCALE_FACTOR - debt_ratio;

        let new_p = self.p * reduction_factor / SP_SCALE_FACTOR;

        // Check if P is getting too small (precision loss)
        if new_p < SP_SCALE_FACTOR / 1_000_000_000 {
            // Increment scale and reset P
            self.scale += 1;
            self.p = new_p * SP_SCALE_FACTOR;
        } else {
            self.p = new_p;
        }

        // Update S (sum factor) - represents BTC gains per unit deposit
        // S_new = S + collateral / total_deposits
        let gains_per_unit = (collateral_gained.sats() as u128) * SP_SCALE_FACTOR
            / (self.total_deposits.cents() as u128);
        self.s = self.s.saturating_add(gains_per_unit);

        // Update totals
        self.total_deposits = self.total_deposits.saturating_sub(debt_to_absorb);
        self.total_btc_gains = self.total_btc_gains.saturating_add(collateral_gained);
        self.total_liquidations += 1;
        self.total_debt_absorbed = self.total_debt_absorbed.saturating_add(debt_to_absorb);

        Ok(true)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // QUERIES
    // ═══════════════════════════════════════════════════════════════════════════

    /// Get total deposits
    pub fn total_deposits(&self) -> TokenAmount {
        self.total_deposits
    }

    /// Get total BTC gains
    pub fn total_btc_gains(&self) -> CollateralAmount {
        self.total_btc_gains
    }

    /// Get deposit info for an owner
    pub fn get_deposit(&self, owner: &PublicKey) -> Option<&Deposit> {
        self.deposits.get(owner)
    }

    /// Get current deposit value for an owner
    pub fn get_current_value(&self, owner: &PublicKey) -> TokenAmount {
        self.deposits.get(owner)
            .map(|d| d.current_value(self.p, self.epoch, self.scale))
            .unwrap_or(TokenAmount::ZERO)
    }

    /// Get pending BTC gains for an owner
    pub fn get_btc_gains(&self, owner: &PublicKey) -> CollateralAmount {
        let from_deposit = self.deposits.get(owner)
            .map(|d| d.btc_gains(self.s, self.epoch, self.scale))
            .unwrap_or(CollateralAmount::ZERO);

        let pending = self.pending_btc.get(owner)
            .copied()
            .unwrap_or(CollateralAmount::ZERO);

        from_deposit.saturating_add(pending)
    }

    /// Get number of depositors
    pub fn depositor_count(&self) -> usize {
        self.deposits.len()
    }

    /// Get total liquidations absorbed
    pub fn total_liquidations(&self) -> u64 {
        self.total_liquidations
    }

    /// Get total debt absorbed
    pub fn total_debt_absorbed(&self) -> TokenAmount {
        self.total_debt_absorbed
    }

    /// Check if pool can absorb a liquidation
    pub fn can_absorb(&self, debt: TokenAmount) -> bool {
        self.total_deposits >= debt
    }

    /// Get pool statistics
    pub fn statistics(&self) -> StabilityPoolStats {
        StabilityPoolStats {
            total_deposits: self.total_deposits,
            total_btc_gains: self.total_btc_gains,
            depositor_count: self.deposits.len() as u64,
            total_liquidations: self.total_liquidations,
            total_debt_absorbed: self.total_debt_absorbed,
            current_epoch: self.epoch,
            current_scale: self.scale,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // SERIALIZATION
    // ═══════════════════════════════════════════════════════════════════════════

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
        data.extend_from_slice(&self.total_deposits.cents().to_be_bytes());
        data.extend_from_slice(&self.total_btc_gains.sats().to_be_bytes());
        data.extend_from_slice(&self.p.to_be_bytes());
        data.extend_from_slice(&self.s.to_be_bytes());
        data.extend_from_slice(&self.epoch.to_be_bytes());
        data.extend_from_slice(&self.scale.to_be_bytes());
        Hash::sha256(&data)
    }
}

/// Stability pool statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityPoolStats {
    pub total_deposits: TokenAmount,
    pub total_btc_gains: CollateralAmount,
    pub depositor_count: u64,
    pub total_liquidations: u64,
    pub total_debt_absorbed: TokenAmount,
    pub current_epoch: u64,
    pub current_scale: u64,
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
    fn test_deposit() {
        let mut pool = StabilityPool::new();

        pool.deposit(test_pubkey(), TokenAmount::from_dollars(1000), 1).unwrap();

        assert_eq!(pool.total_deposits(), TokenAmount::from_dollars(1000));
        assert_eq!(pool.depositor_count(), 1);
    }

    #[test]
    fn test_deposit_minimum() {
        let mut pool = StabilityPool::new();

        // Below minimum should fail
        let result = pool.deposit(test_pubkey(), TokenAmount::from_cents(100), 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_withdraw() {
        let mut pool = StabilityPool::new();
        let owner = test_pubkey();

        pool.deposit(owner, TokenAmount::from_dollars(1000), 1).unwrap();

        let (withdrawn, btc) = pool.withdraw(&owner, TokenAmount::from_dollars(500), 2).unwrap();

        assert_eq!(withdrawn, TokenAmount::from_dollars(500));
        assert_eq!(btc, CollateralAmount::ZERO);
        assert_eq!(pool.total_deposits(), TokenAmount::from_dollars(500));
    }

    #[test]
    fn test_absorb_liquidation() {
        let mut pool = StabilityPool::new();
        let owner = test_pubkey();

        // Deposit $1000
        pool.deposit(owner, TokenAmount::from_dollars(1000), 1).unwrap();

        // Absorb liquidation: $100 debt, 0.0011 BTC collateral (~10% bonus)
        let absorbed = pool.absorb_liquidation(
            TokenAmount::from_dollars(100),
            CollateralAmount::from_sats(11_000_000), // 0.11 BTC at $100k = $110
        ).unwrap();

        assert!(absorbed);
        assert_eq!(pool.total_deposits(), TokenAmount::from_dollars(900));
        assert_eq!(pool.total_btc_gains(), CollateralAmount::from_sats(11_000_000));
        assert_eq!(pool.total_liquidations(), 1);
    }

    #[test]
    fn test_btc_gains() {
        let mut pool = StabilityPool::new();
        let owner = test_pubkey();

        pool.deposit(owner, TokenAmount::from_dollars(1000), 1).unwrap();

        // Absorb liquidation
        pool.absorb_liquidation(
            TokenAmount::from_dollars(100),
            CollateralAmount::from_sats(11_000_000),
        ).unwrap();

        // Check gains
        let gains = pool.get_btc_gains(&owner);
        assert!(gains.sats() > 0);
    }

    #[test]
    fn test_multiple_depositors() {
        let mut pool = StabilityPool::new();
        let owner1 = test_pubkey();
        let owner2 = test_pubkey_2();

        // Two depositors with equal amounts
        pool.deposit(owner1, TokenAmount::from_dollars(1000), 1).unwrap();
        pool.deposit(owner2, TokenAmount::from_dollars(1000), 2).unwrap();

        // Absorb liquidation
        pool.absorb_liquidation(
            TokenAmount::from_dollars(200),
            CollateralAmount::from_sats(22_000_000),
        ).unwrap();

        // Both should have roughly equal gains
        let gains1 = pool.get_btc_gains(&owner1);
        let gains2 = pool.get_btc_gains(&owner2);

        // Allow for small rounding differences
        let diff = if gains1.sats() > gains2.sats() {
            gains1.sats() - gains2.sats()
        } else {
            gains2.sats() - gains1.sats()
        };

        assert!(diff < 1000); // Less than 1000 sats difference
    }

    #[test]
    fn test_claim_btc() {
        let mut pool = StabilityPool::new();
        let owner = test_pubkey();

        pool.deposit(owner, TokenAmount::from_dollars(1000), 1).unwrap();

        pool.absorb_liquidation(
            TokenAmount::from_dollars(100),
            CollateralAmount::from_sats(11_000_000),
        ).unwrap();

        let claimed = pool.claim_btc(&owner).unwrap();
        assert!(claimed.sats() > 0);

        // Claiming again should return zero
        let claimed_again = pool.claim_btc(&owner).unwrap();
        assert_eq!(claimed_again, CollateralAmount::ZERO);
    }

    #[test]
    fn test_can_absorb() {
        let mut pool = StabilityPool::new();

        pool.deposit(test_pubkey(), TokenAmount::from_dollars(1000), 1).unwrap();

        assert!(pool.can_absorb(TokenAmount::from_dollars(500)));
        assert!(pool.can_absorb(TokenAmount::from_dollars(1000)));
        assert!(!pool.can_absorb(TokenAmount::from_dollars(1001)));
    }
}
