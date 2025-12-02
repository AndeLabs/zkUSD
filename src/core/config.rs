//! Protocol configuration and parameters.
//!
//! This module defines all configurable parameters for the zkUSD protocol.
//! Parameters are divided into:
//! - Immutable: Cannot be changed after deployment
//! - Governable: Can be adjusted through governance
//! - Dynamic: Automatically adjusted by protocol

use serde::{Deserialize, Serialize};

use crate::utils::constants::*;

// ═══════════════════════════════════════════════════════════════════════════════
// PROTOCOL PARAMETERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Immutable protocol parameters (set at deployment)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolParams {
    /// Protocol version
    pub version: String,

    /// Minimum collateralization ratio (MCR)
    /// Below this, CDPs can be liquidated
    pub min_collateral_ratio: u64,

    /// Critical collateralization ratio (CCR)
    /// When system TCR falls below this, recovery mode activates
    pub critical_collateral_ratio: u64,

    /// Borrowing fee in basis points (one-time fee)
    pub borrowing_fee_bps: u64,

    /// Liquidation bonus in basis points
    /// Discount liquidators receive when buying collateral
    pub liquidation_bonus_bps: u64,

    /// Minimum debt per CDP in cents
    pub min_debt: u64,

    /// Maximum debt per CDP in cents
    pub max_debt_per_cdp: u64,

    /// Redemption fee floor in basis points
    pub redemption_fee_floor_bps: u64,

    /// Redemption fee ceiling in basis points
    pub redemption_fee_ceiling_bps: u64,

    /// Minimum oracle sources required for price updates
    pub min_oracle_sources: usize,

    /// Maximum price staleness in seconds
    pub max_price_staleness_secs: u64,

    /// Maximum price deviation between sources in basis points
    pub max_price_deviation_bps: u64,
}

impl Default for ProtocolParams {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            min_collateral_ratio: MIN_COLLATERAL_RATIO,
            critical_collateral_ratio: CRITICAL_COLLATERAL_RATIO,
            borrowing_fee_bps: BORROWING_FEE_BPS,
            liquidation_bonus_bps: LIQUIDATION_BONUS_BPS,
            min_debt: MIN_DEBT,
            max_debt_per_cdp: MAX_DEBT_PER_CDP,
            redemption_fee_floor_bps: REDEMPTION_FEE_FLOOR_BPS,
            redemption_fee_ceiling_bps: REDEMPTION_FEE_CEILING_BPS,
            min_oracle_sources: MIN_ORACLE_SOURCES,
            max_price_staleness_secs: MAX_PRICE_STALENESS_SECS,
            max_price_deviation_bps: MAX_PRICE_DEVIATION_BPS,
        }
    }
}

impl ProtocolParams {
    /// Create with custom MCR (for testing)
    pub fn with_mcr(mut self, mcr: u64) -> Self {
        self.min_collateral_ratio = mcr;
        self
    }

    /// Create with custom fees (for testing)
    pub fn with_fees(mut self, borrowing_bps: u64, liquidation_bps: u64) -> Self {
        self.borrowing_fee_bps = borrowing_bps;
        self.liquidation_bonus_bps = liquidation_bps;
        self
    }

    /// Validate parameters are consistent
    pub fn validate(&self) -> bool {
        self.min_collateral_ratio < self.critical_collateral_ratio
            && self.min_debt <= self.max_debt_per_cdp
            && self.redemption_fee_floor_bps <= self.redemption_fee_ceiling_bps
            && self.min_oracle_sources > 0
            && self.max_price_staleness_secs > 0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROTOCOL CONFIGURATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Dynamic protocol configuration (can be updated by governance)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolConfig {
    /// Immutable parameters
    pub params: ProtocolParams,

    /// Current system debt ceiling
    pub debt_ceiling: u64,

    /// Whether protocol is paused
    pub paused: bool,

    /// Whether protocol is in recovery mode
    pub recovery_mode: bool,

    /// Current base rate for redemptions (dynamic)
    pub base_rate: u64,

    /// Last redemption timestamp
    pub last_redemption_time: u64,

    /// Total system debt (all CDP debts)
    pub total_system_debt: u64,

    /// Total system collateral in satoshis
    pub total_system_collateral: u64,
}

impl Default for ProtocolConfig {
    fn default() -> Self {
        Self {
            params: ProtocolParams::default(),
            debt_ceiling: INITIAL_DEBT_CEILING,
            paused: false,
            recovery_mode: false,
            base_rate: 0,
            last_redemption_time: 0,
            total_system_debt: 0,
            total_system_collateral: 0,
        }
    }
}

impl ProtocolConfig {
    /// Create a new protocol configuration
    pub fn new(params: ProtocolParams) -> Self {
        Self {
            params,
            ..Default::default()
        }
    }

    /// Calculate current total collateralization ratio (TCR)
    pub fn calculate_tcr(&self, btc_price_cents: u64) -> u64 {
        if self.total_system_debt == 0 {
            return u64::MAX;
        }

        let collateral_value = (self.total_system_collateral as u128)
            * (btc_price_cents as u128)
            / (SATS_PER_BTC as u128);

        let ratio = collateral_value * (RATIO_PRECISION as u128) / (self.total_system_debt as u128);

        ratio.min(u64::MAX as u128) as u64
    }

    /// Check if system should be in recovery mode
    pub fn should_enter_recovery_mode(&self, btc_price_cents: u64) -> bool {
        let tcr = self.calculate_tcr(btc_price_cents);
        tcr < self.params.critical_collateral_ratio
    }

    /// Update recovery mode status
    pub fn update_recovery_mode(&mut self, btc_price_cents: u64) {
        self.recovery_mode = self.should_enter_recovery_mode(btc_price_cents);
    }

    /// Calculate current redemption fee
    pub fn calculate_redemption_fee(&self, current_time: u64) -> u64 {
        // Fee decays exponentially from last redemption
        let time_since_redemption = current_time.saturating_sub(self.last_redemption_time);

        // Simple decay: halve every 12 hours
        let decay_periods = time_since_redemption / REDEMPTION_FEE_DECAY_HALF_LIFE;
        let decayed_rate = self.base_rate >> decay_periods.min(63);

        // Floor + decayed rate, capped at ceiling
        (self.params.redemption_fee_floor_bps + decayed_rate)
            .min(self.params.redemption_fee_ceiling_bps)
    }

    /// Update base rate after redemption
    pub fn update_base_rate(&mut self, redeemed_amount: u64, current_time: u64) {
        // Increase base rate based on redemption amount
        // Rate increase = redeemed_amount / total_debt * some_factor
        if self.total_system_debt > 0 {
            let rate_increase = (redeemed_amount as u128) * 100 / (self.total_system_debt as u128);
            self.base_rate = self.base_rate.saturating_add(rate_increase as u64);
        }
        self.last_redemption_time = current_time;
    }

    /// Check if debt ceiling allows new debt
    pub fn can_add_debt(&self, new_debt: u64) -> bool {
        self.total_system_debt.saturating_add(new_debt) <= self.debt_ceiling
    }

    /// Add to system totals
    pub fn add_position(&mut self, collateral_sats: u64, debt_cents: u64) {
        self.total_system_collateral = self.total_system_collateral.saturating_add(collateral_sats);
        self.total_system_debt = self.total_system_debt.saturating_add(debt_cents);
    }

    /// Remove from system totals
    pub fn remove_position(&mut self, collateral_sats: u64, debt_cents: u64) {
        self.total_system_collateral = self.total_system_collateral.saturating_sub(collateral_sats);
        self.total_system_debt = self.total_system_debt.saturating_sub(debt_cents);
    }

    /// Get effective MCR (higher in recovery mode)
    pub fn effective_mcr(&self) -> u64 {
        if self.recovery_mode {
            self.params.critical_collateral_ratio
        } else {
            self.params.min_collateral_ratio
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROTOCOL STATE
// ═══════════════════════════════════════════════════════════════════════════════

/// Complete protocol state snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolState {
    /// Protocol configuration
    pub config: ProtocolConfig,

    /// Current BTC price in cents
    pub btc_price_cents: u64,

    /// Timestamp of last price update
    pub price_timestamp: u64,

    /// Number of active CDPs
    pub active_cdp_count: u64,

    /// Total zkUSD in stability pool
    pub stability_pool_balance: u64,

    /// Block height
    pub block_height: u64,
}

impl ProtocolState {
    /// Create a new protocol state
    pub fn new(config: ProtocolConfig) -> Self {
        Self {
            config,
            btc_price_cents: 0,
            price_timestamp: 0,
            active_cdp_count: 0,
            stability_pool_balance: 0,
            block_height: 0,
        }
    }

    /// Update price
    pub fn update_price(&mut self, price_cents: u64, timestamp: u64) {
        self.btc_price_cents = price_cents;
        self.price_timestamp = timestamp;
        self.config.update_recovery_mode(price_cents);
    }

    /// Check if price is valid
    pub fn is_price_valid(&self, current_time: u64) -> bool {
        current_time.saturating_sub(self.price_timestamp)
            <= self.config.params.max_price_staleness_secs
    }

    /// Summary for logging
    pub fn summary(&self) -> String {
        format!(
            "TCR: {}%, Debt: ${:.2}, CDPs: {}, Recovery: {}",
            self.config.calculate_tcr(self.btc_price_cents),
            self.config.total_system_debt as f64 / ZKUSD_BASE_UNIT as f64,
            self.active_cdp_count,
            self.config.recovery_mode
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_params_default() {
        let params = ProtocolParams::default();
        assert!(params.validate());
        assert_eq!(params.min_collateral_ratio, MIN_COLLATERAL_RATIO);
    }

    #[test]
    fn test_tcr_calculation() {
        let mut config = ProtocolConfig::default();

        // Add 1 BTC collateral ($100,000) and $50,000 debt
        config.total_system_collateral = SATS_PER_BTC;
        config.total_system_debt = 5_000_000; // $50,000 in cents

        let tcr = config.calculate_tcr(10_000_000); // $100,000 BTC price
        assert_eq!(tcr, 200); // 200%
    }

    #[test]
    fn test_recovery_mode_detection() {
        let mut config = ProtocolConfig::default();
        config.total_system_collateral = SATS_PER_BTC; // 1 BTC
        config.total_system_debt = 8_000_000; // $80,000

        // At $100,000/BTC: TCR = 125% (below 150% CCR)
        assert!(config.should_enter_recovery_mode(10_000_000));

        // At $200,000/BTC: TCR = 250% (above 150% CCR)
        assert!(!config.should_enter_recovery_mode(20_000_000));
    }

    #[test]
    fn test_redemption_fee_decay() {
        let mut config = ProtocolConfig::default();
        config.base_rate = 100; // 1%
        config.last_redemption_time = 0;

        // Right after redemption: floor + base_rate
        let fee1 = config.calculate_redemption_fee(0);
        assert_eq!(fee1, REDEMPTION_FEE_FLOOR_BPS + 100);

        // After 12 hours: base_rate halved
        let fee2 = config.calculate_redemption_fee(REDEMPTION_FEE_DECAY_HALF_LIFE);
        assert_eq!(fee2, REDEMPTION_FEE_FLOOR_BPS + 50);

        // After 24 hours: base_rate quartered
        let fee3 = config.calculate_redemption_fee(REDEMPTION_FEE_DECAY_HALF_LIFE * 2);
        assert_eq!(fee3, REDEMPTION_FEE_FLOOR_BPS + 25);
    }

    #[test]
    fn test_debt_ceiling() {
        let mut config = ProtocolConfig::default();
        config.debt_ceiling = 100_000_000; // $1M in cents
        config.total_system_debt = 50_000_000; // $500k

        assert!(config.can_add_debt(40_000_000)); // Can add $400k
        assert!(!config.can_add_debt(60_000_000)); // Cannot add $600k
    }

    #[test]
    fn test_add_remove_position() {
        let mut config = ProtocolConfig::default();

        config.add_position(SATS_PER_BTC, 5_000_000);
        assert_eq!(config.total_system_collateral, SATS_PER_BTC);
        assert_eq!(config.total_system_debt, 5_000_000);

        config.remove_position(SATS_PER_BTC / 2, 2_500_000);
        assert_eq!(config.total_system_collateral, SATS_PER_BTC / 2);
        assert_eq!(config.total_system_debt, 2_500_000);
    }

    #[test]
    fn test_effective_mcr() {
        let mut config = ProtocolConfig::default();

        // Normal mode: MCR = 110%
        assert_eq!(config.effective_mcr(), MIN_COLLATERAL_RATIO);

        // Recovery mode: MCR = 150%
        config.recovery_mode = true;
        assert_eq!(config.effective_mcr(), CRITICAL_COLLATERAL_RATIO);
    }
}
