//! Dynamic Fee System for zkUSD Protocol.
//!
//! This module implements a production-grade dynamic fee system that adjusts
//! borrowing and redemption fees based on market conditions and protocol usage.
//!
//! # Fee Types
//!
//! - **Borrowing Fee**: One-time fee charged when minting zkUSD
//! - **Redemption Fee**: Fee charged when redeeming zkUSD for BTC
//!
//! # Dynamic Adjustment
//!
//! Fees are adjusted based on:
//! - Base rate that decays over time
//! - System utilization (total debt vs. debt ceiling)
//! - Redemption activity (recent redemptions increase fees)
//! - Time since last fee event

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use crate::error::{Error, Result};
use crate::utils::math::safe_mul_div;

// ═══════════════════════════════════════════════════════════════════════════════
// CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Minimum borrowing fee (basis points): 0.5%
pub const MIN_BORROWING_FEE_BPS: u64 = 50;

/// Maximum borrowing fee (basis points): 5%
pub const MAX_BORROWING_FEE_BPS: u64 = 500;

/// Minimum redemption fee (basis points): 0.5%
pub const MIN_REDEMPTION_FEE_BPS: u64 = 50;

/// Maximum redemption fee (basis points): 5%
pub const MAX_REDEMPTION_FEE_BPS: u64 = 500;

/// Base rate decay half-life in blocks (~12 hours at 15s blocks)
pub const BASE_RATE_HALF_LIFE_BLOCKS: u64 = 2880;

/// Blocks to look back for redemption history
pub const REDEMPTION_LOOKBACK_BLOCKS: u64 = 5760; // ~24 hours

/// Maximum redemption history entries
const MAX_REDEMPTION_HISTORY: usize = 1000;

/// Basis points divisor
const BPS_DIVISOR: u64 = 10000;

// ═══════════════════════════════════════════════════════════════════════════════
// FEE CONFIGURATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Configuration for the fee system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeConfig {
    /// Minimum borrowing fee (basis points)
    pub min_borrowing_fee_bps: u64,
    /// Maximum borrowing fee (basis points)
    pub max_borrowing_fee_bps: u64,
    /// Minimum redemption fee (basis points)
    pub min_redemption_fee_bps: u64,
    /// Maximum redemption fee (basis points)
    pub max_redemption_fee_bps: u64,
    /// Base rate decay half-life in blocks
    pub base_rate_half_life_blocks: u64,
    /// Fee recipient public key bytes (protocol treasury, 33 bytes compressed)
    pub fee_recipient: Option<Vec<u8>>,
    /// Enable dynamic fee adjustment
    pub dynamic_fees_enabled: bool,
    /// Utilization threshold to start increasing fees (basis points)
    pub utilization_threshold_bps: u64,
    /// Maximum fee increase from utilization (basis points)
    pub utilization_fee_max_bps: u64,
}

impl Default for FeeConfig {
    fn default() -> Self {
        Self {
            min_borrowing_fee_bps: MIN_BORROWING_FEE_BPS,
            max_borrowing_fee_bps: MAX_BORROWING_FEE_BPS,
            min_redemption_fee_bps: MIN_REDEMPTION_FEE_BPS,
            max_redemption_fee_bps: MAX_REDEMPTION_FEE_BPS,
            base_rate_half_life_blocks: BASE_RATE_HALF_LIFE_BLOCKS,
            fee_recipient: None,
            dynamic_fees_enabled: true,
            utilization_threshold_bps: 8000, // 80%
            utilization_fee_max_bps: 200,    // +2% max
        }
    }
}

impl FeeConfig {
    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        if self.min_borrowing_fee_bps > self.max_borrowing_fee_bps {
            return Err(Error::InvalidParameter {
                name: "borrowing_fee".into(),
                reason: "min exceeds max".into(),
            });
        }
        if self.min_redemption_fee_bps > self.max_redemption_fee_bps {
            return Err(Error::InvalidParameter {
                name: "redemption_fee".into(),
                reason: "min exceeds max".into(),
            });
        }
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// REDEMPTION RECORD
// ═══════════════════════════════════════════════════════════════════════════════

/// Record of a redemption event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedemptionRecord {
    /// Block height when redemption occurred
    pub block_height: u64,
    /// Amount redeemed (in cents)
    pub amount_cents: u64,
    /// Fee paid (in cents)
    pub fee_cents: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// DYNAMIC FEE CALCULATOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Main fee calculator with dynamic adjustment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicFeeCalculator {
    /// Configuration
    config: FeeConfig,
    /// Current base rate (basis points * 1000 for precision)
    base_rate: u64,
    /// Block height of last redemption
    last_redemption_block: u64,
    /// Recent redemption history
    redemption_history: VecDeque<RedemptionRecord>,
    /// Total fees collected (cents)
    total_borrowing_fees: u64,
    /// Total redemption fees collected (cents)
    total_redemption_fees: u64,
    /// Current block height
    current_block: u64,
}

impl DynamicFeeCalculator {
    /// Create new fee calculator
    pub fn new(config: FeeConfig) -> Self {
        Self {
            config,
            base_rate: 0,
            last_redemption_block: 0,
            redemption_history: VecDeque::with_capacity(MAX_REDEMPTION_HISTORY),
            total_borrowing_fees: 0,
            total_redemption_fees: 0,
            current_block: 0,
        }
    }

    /// Create with default config
    pub fn default_config() -> Self {
        Self::new(FeeConfig::default())
    }

    /// Update current block height
    pub fn set_block_height(&mut self, block_height: u64) {
        self.current_block = block_height;
    }

    /// Calculate borrowing fee for a given amount
    pub fn calculate_borrowing_fee(
        &self,
        debt_amount_cents: u64,
        total_debt: u64,
        debt_ceiling: u64,
    ) -> FeeCalculation {
        let base_fee_bps = self.get_decayed_base_rate();

        // Add utilization premium if enabled
        let utilization_fee_bps = if self.config.dynamic_fees_enabled {
            self.calculate_utilization_premium(total_debt, debt_ceiling)
        } else {
            0
        };

        let total_fee_bps = base_fee_bps + utilization_fee_bps + self.config.min_borrowing_fee_bps;
        let capped_fee_bps = total_fee_bps.min(self.config.max_borrowing_fee_bps);

        let fee_cents = safe_mul_div(debt_amount_cents, capped_fee_bps, BPS_DIVISOR)
            .unwrap_or(0);

        FeeCalculation {
            amount_cents: debt_amount_cents,
            fee_cents,
            fee_bps: capped_fee_bps,
            base_rate_bps: base_fee_bps,
            utilization_premium_bps: utilization_fee_bps,
            net_amount_cents: debt_amount_cents.saturating_sub(fee_cents),
        }
    }

    /// Calculate redemption fee for a given amount
    pub fn calculate_redemption_fee(&self, redemption_amount_cents: u64) -> FeeCalculation {
        let base_fee_bps = self.get_decayed_base_rate();
        let recent_redemption_premium = self.calculate_recent_redemption_premium();

        let total_fee_bps = base_fee_bps + recent_redemption_premium + self.config.min_redemption_fee_bps;
        let capped_fee_bps = total_fee_bps.min(self.config.max_redemption_fee_bps);

        let fee_cents = safe_mul_div(redemption_amount_cents, capped_fee_bps, BPS_DIVISOR)
            .unwrap_or(0);

        FeeCalculation {
            amount_cents: redemption_amount_cents,
            fee_cents,
            fee_bps: capped_fee_bps,
            base_rate_bps: base_fee_bps,
            utilization_premium_bps: recent_redemption_premium,
            net_amount_cents: redemption_amount_cents.saturating_sub(fee_cents),
        }
    }

    /// Record a borrowing event and collect fee
    pub fn record_borrowing(&mut self, amount_cents: u64, fee_cents: u64) {
        self.total_borrowing_fees = self.total_borrowing_fees.saturating_add(fee_cents);
    }

    /// Record a redemption event and update base rate
    pub fn record_redemption(
        &mut self,
        block_height: u64,
        amount_cents: u64,
        fee_cents: u64,
        total_debt: u64,
    ) {
        // Calculate the base rate increase from this redemption
        // Larger redemptions relative to total debt increase the rate more
        if total_debt > 0 {
            let redemption_ratio = safe_mul_div(amount_cents, BPS_DIVISOR, total_debt)
                .unwrap_or(0);
            // Add to base rate (scaled by ratio of redemption to total debt)
            self.base_rate = self.base_rate.saturating_add(redemption_ratio / 2);
        }

        self.last_redemption_block = block_height;
        self.total_redemption_fees = self.total_redemption_fees.saturating_add(fee_cents);

        // Add to history
        let record = RedemptionRecord {
            block_height,
            amount_cents,
            fee_cents,
        };

        if self.redemption_history.len() >= MAX_REDEMPTION_HISTORY {
            self.redemption_history.pop_front();
        }
        self.redemption_history.push_back(record);

        // Cleanup old history
        self.cleanup_old_history(block_height);
    }

    /// Get current base rate with decay applied
    fn get_decayed_base_rate(&self) -> u64 {
        if self.base_rate == 0 || self.last_redemption_block == 0 {
            return 0;
        }

        let blocks_elapsed = self.current_block.saturating_sub(self.last_redemption_block);

        // Calculate decay: rate * 0.5^(elapsed/half_life)
        // Using approximation: multiply by (1 - elapsed/half_life/2) for small values
        let half_lives = blocks_elapsed as f64 / self.config.base_rate_half_life_blocks as f64;
        let decay_factor = 0.5_f64.powf(half_lives);

        ((self.base_rate as f64) * decay_factor) as u64
    }

    /// Calculate utilization premium based on debt usage
    fn calculate_utilization_premium(&self, total_debt: u64, debt_ceiling: u64) -> u64 {
        if debt_ceiling == 0 {
            return 0;
        }

        let utilization_bps = safe_mul_div(total_debt, BPS_DIVISOR, debt_ceiling)
            .unwrap_or(0);

        if utilization_bps <= self.config.utilization_threshold_bps {
            return 0;
        }

        // Linear increase above threshold
        let excess_bps = utilization_bps - self.config.utilization_threshold_bps;
        let remaining_bps = BPS_DIVISOR - self.config.utilization_threshold_bps;

        if remaining_bps == 0 {
            return self.config.utilization_fee_max_bps;
        }

        safe_mul_div(excess_bps, self.config.utilization_fee_max_bps, remaining_bps)
            .unwrap_or(self.config.utilization_fee_max_bps)
            .min(self.config.utilization_fee_max_bps)
    }

    /// Calculate premium based on recent redemption activity
    fn calculate_recent_redemption_premium(&self) -> u64 {
        if self.redemption_history.is_empty() {
            return 0;
        }

        let lookback_start = self.current_block.saturating_sub(REDEMPTION_LOOKBACK_BLOCKS);

        // Sum recent redemptions
        let recent_total: u64 = self.redemption_history
            .iter()
            .filter(|r| r.block_height >= lookback_start)
            .map(|r| r.amount_cents)
            .sum();

        // Convert to basis points (max 200 bps for high activity)
        // This is a rough heuristic - adjust based on protocol needs
        (recent_total / 10_000_000).min(200) // $100M = 200 bps
    }

    /// Cleanup old redemption history
    fn cleanup_old_history(&mut self, current_block: u64) {
        let cutoff = current_block.saturating_sub(REDEMPTION_LOOKBACK_BLOCKS * 2);
        while let Some(front) = self.redemption_history.front() {
            if front.block_height < cutoff {
                self.redemption_history.pop_front();
            } else {
                break;
            }
        }
    }

    /// Get current fee rates
    pub fn current_rates(&self) -> FeeRates {
        FeeRates {
            base_rate_bps: self.get_decayed_base_rate(),
            min_borrowing_fee_bps: self.config.min_borrowing_fee_bps,
            max_borrowing_fee_bps: self.config.max_borrowing_fee_bps,
            min_redemption_fee_bps: self.config.min_redemption_fee_bps,
            max_redemption_fee_bps: self.config.max_redemption_fee_bps,
            dynamic_fees_enabled: self.config.dynamic_fees_enabled,
        }
    }

    /// Get fee statistics
    pub fn statistics(&self) -> FeeStatistics {
        FeeStatistics {
            total_borrowing_fees_collected: self.total_borrowing_fees,
            total_redemption_fees_collected: self.total_redemption_fees,
            current_base_rate_bps: self.get_decayed_base_rate(),
            last_redemption_block: self.last_redemption_block,
            redemption_history_size: self.redemption_history.len(),
        }
    }

    /// Get configuration
    pub fn config(&self) -> &FeeConfig {
        &self.config
    }

    /// Update configuration
    pub fn update_config(&mut self, config: FeeConfig) -> Result<()> {
        config.validate()?;
        self.config = config;
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// FEE CALCULATION RESULT
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of a fee calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeCalculation {
    /// Original amount (cents)
    pub amount_cents: u64,
    /// Fee amount (cents)
    pub fee_cents: u64,
    /// Fee rate (basis points)
    pub fee_bps: u64,
    /// Base rate component (basis points)
    pub base_rate_bps: u64,
    /// Utilization/activity premium (basis points)
    pub utilization_premium_bps: u64,
    /// Net amount after fee (cents)
    pub net_amount_cents: u64,
}

/// Current fee rates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeRates {
    /// Current base rate
    pub base_rate_bps: u64,
    /// Minimum borrowing fee
    pub min_borrowing_fee_bps: u64,
    /// Maximum borrowing fee
    pub max_borrowing_fee_bps: u64,
    /// Minimum redemption fee
    pub min_redemption_fee_bps: u64,
    /// Maximum redemption fee
    pub max_redemption_fee_bps: u64,
    /// Whether dynamic fees are enabled
    pub dynamic_fees_enabled: bool,
}

/// Fee system statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeStatistics {
    /// Total borrowing fees collected (cents)
    pub total_borrowing_fees_collected: u64,
    /// Total redemption fees collected (cents)
    pub total_redemption_fees_collected: u64,
    /// Current decayed base rate (basis points)
    pub current_base_rate_bps: u64,
    /// Block of last redemption
    pub last_redemption_block: u64,
    /// Number of redemptions in history
    pub redemption_history_size: usize,
}

// ═══════════════════════════════════════════════════════════════════════════════
// SIMPLE FEE CALCULATOR (for testing/simple use)
// ═══════════════════════════════════════════════════════════════════════════════

/// Simple fixed-fee calculator
#[derive(Debug, Clone)]
pub struct FixedFeeCalculator {
    borrowing_fee_bps: u64,
    redemption_fee_bps: u64,
}

impl FixedFeeCalculator {
    /// Create new fixed fee calculator
    pub fn new(borrowing_fee_bps: u64, redemption_fee_bps: u64) -> Self {
        Self {
            borrowing_fee_bps,
            redemption_fee_bps,
        }
    }

    /// Calculate borrowing fee
    pub fn borrowing_fee(&self, amount_cents: u64) -> u64 {
        safe_mul_div(amount_cents, self.borrowing_fee_bps, BPS_DIVISOR)
            .unwrap_or(0)
    }

    /// Calculate redemption fee
    pub fn redemption_fee(&self, amount_cents: u64) -> u64 {
        safe_mul_div(amount_cents, self.redemption_fee_bps, BPS_DIVISOR)
            .unwrap_or(0)
    }
}

impl Default for FixedFeeCalculator {
    fn default() -> Self {
        Self::new(50, 50) // 0.5% default
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_fee_calculator() {
        let calc = FixedFeeCalculator::new(50, 100);

        let fee = calc.borrowing_fee(100_000_00); // $100k
        assert_eq!(fee, 50_000); // $500 (0.5%)

        let fee = calc.redemption_fee(100_000_00);
        assert_eq!(fee, 100_000); // $1000 (1%)
    }

    #[test]
    fn test_dynamic_fee_basic() {
        let mut calc = DynamicFeeCalculator::default_config();
        calc.set_block_height(100);

        let result = calc.calculate_borrowing_fee(
            1_000_000_00, // $1M
            10_000_000_00, // $10M total debt
            100_000_000_00, // $100M ceiling
        );

        // Should be minimum fee (0.5%)
        assert_eq!(result.fee_bps, MIN_BORROWING_FEE_BPS);
        assert_eq!(result.fee_cents, 500_000); // $5000
    }

    #[test]
    fn test_utilization_premium() {
        let mut calc = DynamicFeeCalculator::default_config();
        calc.set_block_height(100);

        // High utilization (90%)
        let result = calc.calculate_borrowing_fee(
            1_000_000_00, // $1M
            90_000_000_00, // $90M total debt
            100_000_000_00, // $100M ceiling (90% utilization)
        );

        // Should have utilization premium above 80% threshold
        assert!(result.utilization_premium_bps > 0);
        assert!(result.fee_bps > MIN_BORROWING_FEE_BPS);
    }

    #[test]
    fn test_base_rate_decay() {
        let mut calc = DynamicFeeCalculator::default_config();

        // Record a redemption at block 100
        calc.set_block_height(100);
        calc.record_redemption(100, 10_000_000_00, 50_000, 100_000_000_00);

        let rate_at_100 = calc.get_decayed_base_rate();
        assert!(rate_at_100 > 0);

        // Advance by one half-life
        calc.set_block_height(100 + BASE_RATE_HALF_LIFE_BLOCKS);
        let rate_after_half_life = calc.get_decayed_base_rate();

        // Rate should be approximately half
        assert!(rate_after_half_life < rate_at_100);
        assert!(rate_after_half_life > rate_at_100 / 3);
    }

    #[test]
    fn test_redemption_increases_base_rate() {
        let mut calc = DynamicFeeCalculator::default_config();
        calc.set_block_height(100);

        let rate_before = calc.get_decayed_base_rate();
        assert_eq!(rate_before, 0);

        // Large redemption (10% of total debt)
        calc.record_redemption(
            100,
            10_000_000_00, // $10M redeemed
            50_000,
            100_000_000_00, // $100M total debt
        );

        let rate_after = calc.get_decayed_base_rate();
        assert!(rate_after > 0);
    }

    #[test]
    fn test_fee_cap() {
        let mut calc = DynamicFeeCalculator::default_config();
        calc.set_block_height(100);

        // Set a very high base rate
        calc.base_rate = 10000; // 100% (way over cap)
        calc.last_redemption_block = 100;

        let result = calc.calculate_borrowing_fee(
            1_000_000_00,
            90_000_000_00,
            100_000_000_00,
        );

        // Should be capped at max
        assert_eq!(result.fee_bps, MAX_BORROWING_FEE_BPS);
    }

    #[test]
    fn test_statistics() {
        let mut calc = DynamicFeeCalculator::default_config();
        calc.set_block_height(100);

        calc.record_borrowing(1_000_000_00, 5_000_00);
        calc.record_redemption(100, 500_000_00, 2_500_00, 10_000_000_00);

        let stats = calc.statistics();
        assert_eq!(stats.total_borrowing_fees_collected, 5_000_00);
        assert_eq!(stats.total_redemption_fees_collected, 2_500_00);
        assert_eq!(stats.last_redemption_block, 100);
    }
}
