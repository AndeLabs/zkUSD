//! Liquidation Engine for zkUSD protocol.
//!
//! This module handles the liquidation of undercollateralized CDPs:
//! - Detection of liquidatable positions
//! - Liquidation execution via Stability Pool
//! - Redistribution as fallback

use serde::{Deserialize, Serialize};

use crate::core::cdp::{CDP, CDPId, CDPManager, LiquidationResult};
use crate::core::config::ProtocolConfig;
use crate::core::token::TokenAmount;
use crate::core::vault::CollateralAmount;
use crate::error::{Error, Result};
use crate::liquidation::stability_pool::StabilityPool;
use crate::utils::constants::*;
use crate::utils::crypto::{Hash, PublicKey};
use crate::utils::math::*;

// ═══════════════════════════════════════════════════════════════════════════════
// LIQUIDATION EVENT
// ═══════════════════════════════════════════════════════════════════════════════

/// Record of a liquidation event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationEvent {
    /// CDP that was liquidated
    pub cdp_id: CDPId,
    /// Owner of the liquidated CDP
    pub cdp_owner: PublicKey,
    /// Liquidator who triggered the liquidation
    pub liquidator: PublicKey,
    /// Debt that was covered
    pub debt_covered: TokenAmount,
    /// Collateral that was seized
    pub collateral_seized: CollateralAmount,
    /// Bonus given to liquidator/stability pool
    pub liquidator_bonus: CollateralAmount,
    /// Whether liquidation was absorbed by stability pool
    pub absorbed_by_sp: bool,
    /// BTC price at time of liquidation
    pub btc_price: u64,
    /// Collateralization ratio at liquidation
    pub ratio_at_liquidation: u64,
    /// Block height
    pub block_height: u64,
    /// Transaction hash
    pub tx_hash: Hash,
}

// ═══════════════════════════════════════════════════════════════════════════════
// LIQUIDATION BATCH
// ═══════════════════════════════════════════════════════════════════════════════

/// Batch of CDPs to liquidate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationBatch {
    /// CDPs to liquidate
    pub cdp_ids: Vec<CDPId>,
    /// Total debt to be covered
    pub total_debt: TokenAmount,
    /// Total collateral to be seized
    pub total_collateral: CollateralAmount,
    /// BTC price for this batch
    pub btc_price: u64,
}

impl LiquidationBatch {
    /// Create a new empty batch
    pub fn new(btc_price: u64) -> Self {
        Self {
            cdp_ids: Vec::new(),
            total_debt: TokenAmount::ZERO,
            total_collateral: CollateralAmount::ZERO,
            btc_price,
        }
    }

    /// Add a CDP to the batch
    pub fn add(&mut self, cdp_id: CDPId, debt: TokenAmount, collateral: CollateralAmount) {
        self.cdp_ids.push(cdp_id);
        self.total_debt = self.total_debt.saturating_add(debt);
        self.total_collateral = self.total_collateral.saturating_add(collateral);
    }

    /// Get number of CDPs in batch
    pub fn len(&self) -> usize {
        self.cdp_ids.len()
    }

    /// Check if batch is empty
    pub fn is_empty(&self) -> bool {
        self.cdp_ids.is_empty()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// LIQUIDATION ENGINE
// ═══════════════════════════════════════════════════════════════════════════════

/// Engine for liquidating undercollateralized CDPs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationEngine {
    /// Events history
    events: Vec<LiquidationEvent>,
    /// Maximum events to keep
    max_events: usize,
    /// Total liquidations performed
    total_liquidations: u64,
    /// Total debt liquidated
    total_debt_liquidated: TokenAmount,
    /// Total collateral seized
    total_collateral_seized: CollateralAmount,
}

impl Default for LiquidationEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LiquidationEngine {
    /// Create a new liquidation engine
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            max_events: 1000,
            total_liquidations: 0,
            total_debt_liquidated: TokenAmount::ZERO,
            total_collateral_seized: CollateralAmount::ZERO,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // LIQUIDATION DETECTION
    // ═══════════════════════════════════════════════════════════════════════════

    /// Find all liquidatable CDPs
    pub fn find_liquidatable<'a>(
        &self,
        cdp_manager: &'a CDPManager,
        btc_price: u64,
        config: &ProtocolConfig,
    ) -> Vec<&'a CDP> {
        let min_ratio = config.effective_mcr();
        cdp_manager.get_liquidatable(btc_price, min_ratio)
    }

    /// Create a liquidation batch from CDPs
    pub fn create_batch(
        &self,
        cdps: &[&CDP],
        btc_price: u64,
        max_batch_size: usize,
    ) -> LiquidationBatch {
        let mut batch = LiquidationBatch::new(btc_price);

        for cdp in cdps.iter().take(max_batch_size) {
            batch.add(
                cdp.id,
                TokenAmount::from_cents(cdp.debt_cents),
                CollateralAmount::from_sats(cdp.collateral_sats),
            );
        }

        batch
    }

    /// Sort CDPs by priority for liquidation (lowest ratio first)
    pub fn prioritize_liquidations<'a>(
        &self,
        cdps: &'a [&'a CDP],
        btc_price: u64,
    ) -> Vec<(&'a CDP, u64)> {
        let mut sorted: Vec<_> = cdps
            .iter()
            .map(|cdp| (*cdp, cdp.calculate_ratio(btc_price)))
            .collect();

        // Sort by ratio (ascending - lowest ratio = highest priority)
        sorted.sort_by_key(|(_, ratio)| *ratio);
        sorted
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // LIQUIDATION EXECUTION
    // ═══════════════════════════════════════════════════════════════════════════

    /// Execute a single liquidation
    pub fn liquidate_single(
        &mut self,
        cdp: &mut CDP,
        stability_pool: &mut StabilityPool,
        config: &ProtocolConfig,
        btc_price: u64,
        liquidator: PublicKey,
        block_height: u64,
        tx_hash: Hash,
    ) -> Result<LiquidationEvent> {
        // Verify CDP is liquidatable
        let min_ratio = config.effective_mcr();
        if !cdp.is_liquidatable(btc_price, min_ratio) {
            return Err(Error::CDPHealthy(cdp.id.to_hex()));
        }

        let ratio_at_liquidation = cdp.calculate_ratio(btc_price);
        let debt = TokenAmount::from_cents(cdp.debt_cents);
        let collateral = CollateralAmount::from_sats(cdp.collateral_sats);

        // Try to absorb via stability pool first
        let absorbed_by_sp = if stability_pool.can_absorb(debt) {
            // Calculate collateral to give to stability pool (debt + bonus)
            let (_, collateral_needed, bonus) = calculate_liquidation_amounts(
                collateral.sats(),
                debt.cents(),
                btc_price,
                LIQUIDATION_BONUS_BPS,
            )?;

            let collateral_for_sp = CollateralAmount::from_sats(collateral_needed);

            stability_pool.absorb_liquidation(debt, collateral_for_sp)?
        } else {
            false
        };

        // Perform liquidation on CDP
        let liq_result = cdp.liquidate(btc_price, min_ratio, block_height)?;

        let event = LiquidationEvent {
            cdp_id: cdp.id,
            cdp_owner: cdp.owner,
            liquidator,
            debt_covered: TokenAmount::from_cents(liq_result.debt_covered),
            collateral_seized: CollateralAmount::from_sats(liq_result.collateral_seized),
            liquidator_bonus: CollateralAmount::from_sats(liq_result.liquidator_bonus),
            absorbed_by_sp,
            btc_price,
            ratio_at_liquidation,
            block_height,
            tx_hash,
        };

        // Update stats
        self.total_liquidations += 1;
        self.total_debt_liquidated = self.total_debt_liquidated
            .saturating_add(event.debt_covered);
        self.total_collateral_seized = self.total_collateral_seized
            .saturating_add(event.collateral_seized);

        // Record event
        self.add_event(event.clone());

        Ok(event)
    }

    /// Execute batch liquidation
    pub fn liquidate_batch(
        &mut self,
        cdp_manager: &mut CDPManager,
        stability_pool: &mut StabilityPool,
        config: &ProtocolConfig,
        btc_price: u64,
        liquidator: PublicKey,
        block_height: u64,
        tx_hash: Hash,
        max_liquidations: usize,
    ) -> Result<Vec<LiquidationEvent>> {
        let min_ratio = config.effective_mcr();

        // Find liquidatable CDPs
        let liquidatable_ids: Vec<CDPId> = cdp_manager
            .get_liquidatable(btc_price, min_ratio)
            .iter()
            .take(max_liquidations)
            .map(|cdp| cdp.id)
            .collect();

        let mut events = Vec::new();

        for cdp_id in liquidatable_ids {
            if let Some(cdp) = cdp_manager.get_mut(&cdp_id) {
                // Generate unique tx hash for each liquidation in batch
                let mut hash_data = tx_hash.as_bytes().to_vec();
                hash_data.extend_from_slice(&events.len().to_be_bytes());
                let liq_tx_hash = Hash::sha256(&hash_data);

                match self.liquidate_single(
                    cdp,
                    stability_pool,
                    config,
                    btc_price,
                    liquidator,
                    block_height,
                    liq_tx_hash,
                ) {
                    Ok(event) => events.push(event),
                    Err(e) => {
                        // Log error but continue with other liquidations
                        tracing::warn!("Liquidation failed for {}: {}", cdp_id.short(), e);
                    }
                }
            }
        }

        Ok(events)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // INCENTIVE CALCULATIONS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Calculate liquidator incentive
    pub fn calculate_liquidator_incentive(
        debt: TokenAmount,
        collateral: CollateralAmount,
        btc_price: u64,
    ) -> Result<LiquidatorIncentive> {
        let collateral_value = collateral.value_in_cents(btc_price);

        // Calculate max bonus (10% of debt)
        let max_bonus_cents = calculate_fee_bps(debt.cents(), LIQUIDATION_BONUS_BPS)?;
        let max_bonus_sats = calculate_min_collateral(max_bonus_cents, btc_price, 100)?;

        // Actual bonus is minimum of max and available surplus
        let surplus_value = collateral_value.saturating_sub(debt.cents());
        let actual_bonus_cents = surplus_value.min(max_bonus_cents);
        let actual_bonus_sats = if actual_bonus_cents > 0 {
            safe_mul_div(actual_bonus_cents, SATS_PER_BTC, btc_price)?
        } else {
            0
        };

        // Gas compensation (fixed amount)
        let gas_comp_cents = 2_00; // $2 fixed gas compensation
        let gas_comp_sats = safe_mul_div(gas_comp_cents, SATS_PER_BTC, btc_price)?;

        Ok(LiquidatorIncentive {
            collateral_bonus: CollateralAmount::from_sats(actual_bonus_sats),
            gas_compensation: CollateralAmount::from_sats(gas_comp_sats),
            total_incentive: CollateralAmount::from_sats(actual_bonus_sats + gas_comp_sats),
            is_profitable: actual_bonus_sats > 0,
        })
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // QUERIES
    // ═══════════════════════════════════════════════════════════════════════════

    /// Get total liquidations
    pub fn total_liquidations(&self) -> u64 {
        self.total_liquidations
    }

    /// Get total debt liquidated
    pub fn total_debt_liquidated(&self) -> TokenAmount {
        self.total_debt_liquidated
    }

    /// Get total collateral seized
    pub fn total_collateral_seized(&self) -> CollateralAmount {
        self.total_collateral_seized
    }

    /// Get recent events
    pub fn recent_events(&self) -> &[LiquidationEvent] {
        &self.events
    }

    /// Get events for a specific CDP
    pub fn events_for_cdp(&self, cdp_id: &CDPId) -> Vec<&LiquidationEvent> {
        self.events.iter().filter(|e| e.cdp_id == *cdp_id).collect()
    }

    /// Get statistics
    pub fn statistics(&self) -> LiquidationStats {
        let avg_ratio = if !self.events.is_empty() {
            let sum: u64 = self.events.iter().map(|e| e.ratio_at_liquidation).sum();
            sum / self.events.len() as u64
        } else {
            0
        };

        let sp_absorbed = self.events.iter().filter(|e| e.absorbed_by_sp).count() as u64;

        LiquidationStats {
            total_liquidations: self.total_liquidations,
            total_debt_liquidated: self.total_debt_liquidated,
            total_collateral_seized: self.total_collateral_seized,
            average_ratio_at_liquidation: avg_ratio,
            sp_absorbed_count: sp_absorbed,
            redistribution_count: self.total_liquidations.saturating_sub(sp_absorbed),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // INTERNAL
    // ═══════════════════════════════════════════════════════════════════════════

    /// Add an event (with pruning)
    fn add_event(&mut self, event: LiquidationEvent) {
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
}

/// Liquidator incentive breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidatorIncentive {
    /// Bonus from collateral surplus
    pub collateral_bonus: CollateralAmount,
    /// Fixed gas compensation
    pub gas_compensation: CollateralAmount,
    /// Total incentive
    pub total_incentive: CollateralAmount,
    /// Whether liquidation is profitable
    pub is_profitable: bool,
}

/// Liquidation statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationStats {
    pub total_liquidations: u64,
    pub total_debt_liquidated: TokenAmount,
    pub total_collateral_seized: CollateralAmount,
    pub average_ratio_at_liquidation: u64,
    pub sp_absorbed_count: u64,
    pub redistribution_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pubkey() -> PublicKey {
        PublicKey::new([0x02; PUBKEY_LENGTH])
    }

    fn test_hash() -> Hash {
        Hash::sha256(b"test")
    }

    #[test]
    fn test_create_batch() {
        let engine = LiquidationEngine::new();
        let btc_price = 10_000_000; // $100,000

        let pubkey = test_pubkey();
        let cdp1 = CDP::with_collateral(pubkey, SATS_PER_BTC, 1, 100).unwrap();
        let cdp2 = CDP::with_collateral(pubkey, SATS_PER_BTC * 2, 2, 100).unwrap();

        let cdps: Vec<&CDP> = vec![&cdp1, &cdp2];
        let batch = engine.create_batch(&cdps, btc_price, 10);

        assert_eq!(batch.len(), 2);
        assert_eq!(batch.total_collateral.sats(), SATS_PER_BTC * 3);
    }

    #[test]
    fn test_prioritize_liquidations() {
        let engine = LiquidationEngine::new();
        let btc_price = 10_000_000;

        let pubkey = test_pubkey();

        // Create CDPs with different ratios
        let mut cdp1 = CDP::with_collateral(pubkey, SATS_PER_BTC, 1, 100).unwrap();
        cdp1.debt_cents = 9_000_000; // 111% ratio

        let mut cdp2 = CDP::with_collateral(pubkey, SATS_PER_BTC, 2, 100).unwrap();
        cdp2.debt_cents = 8_000_000; // 125% ratio

        let mut cdp3 = CDP::with_collateral(pubkey, SATS_PER_BTC, 3, 100).unwrap();
        cdp3.debt_cents = 9_500_000; // 105% ratio

        let cdps: Vec<&CDP> = vec![&cdp1, &cdp2, &cdp3];
        let prioritized = engine.prioritize_liquidations(&cdps, btc_price);

        // Should be sorted by ratio: cdp3 (105%), cdp1 (111%), cdp2 (125%)
        assert_eq!(prioritized[0].1, cdp3.calculate_ratio(btc_price));
        assert_eq!(prioritized[2].1, cdp2.calculate_ratio(btc_price));
    }

    #[test]
    fn test_liquidator_incentive() {
        let debt = TokenAmount::from_dollars(50000); // $50,000
        let collateral = CollateralAmount::from_btc(1); // 1 BTC
        let btc_price = 10_000_000; // $100,000/BTC

        let incentive = LiquidationEngine::calculate_liquidator_incentive(
            debt, collateral, btc_price
        ).unwrap();

        // Should be profitable (collateral value > debt)
        assert!(incentive.is_profitable);

        // Bonus should be ~10% of debt = $5,000 worth of BTC
        // At $100k/BTC = 0.05 BTC = 5,000,000 sats
        assert!(incentive.collateral_bonus.sats() > 0);
    }

    #[test]
    fn test_liquidate_single() {
        let mut engine = LiquidationEngine::new();
        let mut stability_pool = StabilityPool::new();
        let config = ProtocolConfig::default();
        let btc_price = 5_000_000; // $50,000/BTC (price dropped)

        // Setup: Deposit in stability pool
        let depositor = PublicKey::new([0x03; PUBKEY_LENGTH]);
        stability_pool.deposit(depositor, TokenAmount::from_dollars(100_000), 1).unwrap();

        // Create undercollateralized CDP
        let owner = test_pubkey();
        let mut cdp = CDP::with_collateral(owner, SATS_PER_BTC, 1, 100).unwrap();
        cdp.debt_cents = 5_000_000; // $50,000 debt
        // Ratio at $50k: $50k / $50k = 100% (below 110% MCR)

        let liquidator = PublicKey::new([0x04; PUBKEY_LENGTH]);

        let event = engine.liquidate_single(
            &mut cdp,
            &mut stability_pool,
            &config,
            btc_price,
            liquidator,
            200,
            test_hash(),
        ).unwrap();

        assert!(event.absorbed_by_sp);
        assert_eq!(event.ratio_at_liquidation, 100);
        assert_eq!(engine.total_liquidations(), 1);
    }

    #[test]
    fn test_cannot_liquidate_healthy_cdp() {
        let mut engine = LiquidationEngine::new();
        let mut stability_pool = StabilityPool::new();
        let config = ProtocolConfig::default();
        let btc_price = 10_000_000; // $100,000/BTC

        let owner = test_pubkey();
        let mut cdp = CDP::with_collateral(owner, SATS_PER_BTC, 1, 100).unwrap();
        cdp.debt_cents = 5_000_000; // $50,000 debt = 200% ratio

        let liquidator = PublicKey::new([0x04; PUBKEY_LENGTH]);

        let result = engine.liquidate_single(
            &mut cdp,
            &mut stability_pool,
            &config,
            btc_price,
            liquidator,
            200,
            test_hash(),
        );

        assert!(result.is_err());
    }
}
