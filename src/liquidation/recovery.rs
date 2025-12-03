//! Recovery Mode Manager for zkUSD protocol.
//!
//! Recovery Mode activates when the Total Collateralization Ratio (TCR) falls
//! below the Critical Collateralization Ratio (CCR) of 150%. In this mode:
//!
//! 1. **Liquidation threshold raised**: CDPs with ratio < 150% (instead of < 110%)
//!    can be liquidated
//! 2. **Minting restricted**: New debt can only be minted if it improves TCR
//! 3. **Withdrawal restricted**: Collateral withdrawals blocked if they would lower TCR
//!
//! This module provides the RecoveryModeManager which encapsulates all recovery
//! mode logic and provides methods for checking operations.

use serde::{Deserialize, Serialize};

use crate::core::cdp::{CDP, CDPId, CDPManager};
use crate::error::{Error, Result};
use crate::utils::constants::*;
use crate::utils::math::*;

// ═══════════════════════════════════════════════════════════════════════════════
// RECOVERY MODE STATUS
// ═══════════════════════════════════════════════════════════════════════════════

/// Current recovery mode status and metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryModeStatus {
    /// Whether recovery mode is active
    pub is_active: bool,
    /// Current Total Collateralization Ratio (basis points)
    pub tcr_bps: u64,
    /// TCR needed to exit recovery mode (basis points)
    pub exit_threshold_bps: u64,
    /// Distance from exit threshold (basis points, negative if above)
    pub distance_to_exit_bps: i64,
    /// Total system collateral (sats)
    pub total_collateral_sats: u64,
    /// Total system debt (cents)
    pub total_debt_cents: u64,
    /// BTC price used for calculation (cents)
    pub btc_price_cents: u64,
    /// Timestamp of last check
    pub last_check_timestamp: u64,
    /// Block height of last check
    pub last_check_block: u64,
    /// Number of CDPs at risk (ratio < CCR)
    pub cdps_at_risk: u64,
    /// Total debt at risk (cents)
    pub debt_at_risk_cents: u64,
}

impl RecoveryModeStatus {
    /// Create status showing recovery mode is NOT active
    pub fn normal(tcr_bps: u64, total_collateral: u64, total_debt: u64, btc_price: u64) -> Self {
        Self {
            is_active: false,
            tcr_bps,
            exit_threshold_bps: CRITICAL_COLLATERAL_RATIO,
            distance_to_exit_bps: tcr_bps as i64 - CRITICAL_COLLATERAL_RATIO as i64,
            total_collateral_sats: total_collateral,
            total_debt_cents: total_debt,
            btc_price_cents: btc_price,
            last_check_timestamp: 0,
            last_check_block: 0,
            cdps_at_risk: 0,
            debt_at_risk_cents: 0,
        }
    }

    /// Create status showing recovery mode IS active
    pub fn recovery(
        tcr_bps: u64,
        total_collateral: u64,
        total_debt: u64,
        btc_price: u64,
        cdps_at_risk: u64,
        debt_at_risk: u64,
    ) -> Self {
        Self {
            is_active: true,
            tcr_bps,
            exit_threshold_bps: CRITICAL_COLLATERAL_RATIO,
            distance_to_exit_bps: tcr_bps as i64 - CRITICAL_COLLATERAL_RATIO as i64,
            total_collateral_sats: total_collateral,
            total_debt_cents: total_debt,
            btc_price_cents: btc_price,
            last_check_timestamp: 0,
            last_check_block: 0,
            cdps_at_risk,
            debt_at_risk_cents: debt_at_risk,
        }
    }

    /// Update timestamps
    pub fn with_timestamps(mut self, timestamp: u64, block: u64) -> Self {
        self.last_check_timestamp = timestamp;
        self.last_check_block = block;
        self
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATION VALIDATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of validating an operation in recovery mode
#[derive(Debug, Clone)]
pub enum RecoveryModeValidation {
    /// Operation is allowed
    Allowed,
    /// Operation is blocked with reason
    Blocked(String),
    /// Operation is allowed but with restrictions
    AllowedWithRestrictions(String),
}

impl RecoveryModeValidation {
    /// Check if operation is allowed
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed | Self::AllowedWithRestrictions(_))
    }

    /// Get error if blocked
    pub fn to_result(&self) -> Result<()> {
        match self {
            Self::Allowed | Self::AllowedWithRestrictions(_) => Ok(()),
            Self::Blocked(reason) => Err(Error::RecoveryMode),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RECOVERY MODE MANAGER
// ═══════════════════════════════════════════════════════════════════════════════

/// Manager for recovery mode logic
#[derive(Debug, Clone, Default)]
pub struct RecoveryModeManager {
    /// Historical recovery mode events
    history: Vec<RecoveryModeEvent>,
    /// Maximum history to keep
    max_history: usize,
}

/// Event when recovery mode status changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryModeEvent {
    /// Whether entering (true) or exiting (false) recovery mode
    pub entering: bool,
    /// TCR at the time of change
    pub tcr_bps: u64,
    /// Timestamp
    pub timestamp: u64,
    /// Block height
    pub block_height: u64,
}

impl RecoveryModeManager {
    /// Create a new recovery mode manager
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            max_history: 100,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TCR CALCULATIONS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Calculate Total Collateralization Ratio
    pub fn calculate_tcr(
        total_collateral_sats: u64,
        total_debt_cents: u64,
        btc_price_cents: u64,
    ) -> u64 {
        if total_debt_cents == 0 {
            return u64::MAX;
        }

        // calculate_collateral_ratio expects: (collateral_sats, btc_price_cents, debt_cents)
        calculate_collateral_ratio(total_collateral_sats, btc_price_cents, total_debt_cents)
            .unwrap_or(u64::MAX)
    }

    /// Check if TCR indicates recovery mode
    pub fn is_recovery_mode(tcr_bps: u64) -> bool {
        tcr_bps < CRITICAL_COLLATERAL_RATIO
    }

    /// Calculate TCR after a hypothetical operation
    pub fn calculate_tcr_after_operation(
        current_collateral_sats: u64,
        current_debt_cents: u64,
        collateral_delta_sats: i64,
        debt_delta_cents: i64,
        btc_price_cents: u64,
    ) -> u64 {
        let new_collateral = if collateral_delta_sats >= 0 {
            current_collateral_sats.saturating_add(collateral_delta_sats as u64)
        } else {
            current_collateral_sats.saturating_sub((-collateral_delta_sats) as u64)
        };

        let new_debt = if debt_delta_cents >= 0 {
            current_debt_cents.saturating_add(debt_delta_cents as u64)
        } else {
            current_debt_cents.saturating_sub((-debt_delta_cents) as u64)
        };

        Self::calculate_tcr(new_collateral, new_debt, btc_price_cents)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // OPERATION VALIDATION
    // ═══════════════════════════════════════════════════════════════════════════

    /// Validate a mint operation in recovery mode
    pub fn validate_mint(
        &self,
        is_recovery_mode: bool,
        current_tcr: u64,
        total_collateral: u64,
        total_debt: u64,
        mint_amount_cents: u64,
        cdp_collateral: u64,
        cdp_debt_after: u64,
        btc_price: u64,
    ) -> RecoveryModeValidation {
        if !is_recovery_mode {
            return RecoveryModeValidation::Allowed;
        }

        // In recovery mode, new debt must improve TCR
        let new_tcr = Self::calculate_tcr_after_operation(
            total_collateral,
            total_debt,
            0,
            mint_amount_cents as i64,
            btc_price,
        );

        if new_tcr >= current_tcr {
            // TCR didn't decrease, operation allowed
            // But CDP must still have ratio >= CCR
            let cdp_ratio = calculate_collateral_ratio(cdp_collateral, btc_price, cdp_debt_after)
                .unwrap_or(0);

            if cdp_ratio >= CRITICAL_COLLATERAL_RATIO {
                RecoveryModeValidation::AllowedWithRestrictions(
                    "Minting allowed: CDP maintains CCR in recovery mode".into(),
                )
            } else {
                RecoveryModeValidation::Blocked(
                    "Minting blocked: CDP would fall below CCR in recovery mode".into(),
                )
            }
        } else {
            RecoveryModeValidation::Blocked(
                "Minting blocked: Would decrease system TCR in recovery mode".into(),
            )
        }
    }

    /// Validate a withdrawal operation in recovery mode
    pub fn validate_withdrawal(
        &self,
        is_recovery_mode: bool,
        current_tcr: u64,
        total_collateral: u64,
        total_debt: u64,
        withdraw_amount_sats: u64,
        cdp_collateral_after: u64,
        cdp_debt: u64,
        btc_price: u64,
    ) -> RecoveryModeValidation {
        if !is_recovery_mode {
            return RecoveryModeValidation::Allowed;
        }

        // In recovery mode, withdrawals must not decrease TCR
        let new_tcr = Self::calculate_tcr_after_operation(
            total_collateral,
            total_debt,
            -(withdraw_amount_sats as i64),
            0,
            btc_price,
        );

        if new_tcr >= current_tcr {
            // TCR didn't decrease
            // But CDP must still have ratio >= CCR
            let cdp_ratio = calculate_collateral_ratio(cdp_collateral_after, btc_price, cdp_debt)
                .unwrap_or(0);

            if cdp_ratio >= CRITICAL_COLLATERAL_RATIO {
                RecoveryModeValidation::AllowedWithRestrictions(
                    "Withdrawal allowed: Maintains TCR in recovery mode".into(),
                )
            } else {
                RecoveryModeValidation::Blocked(
                    "Withdrawal blocked: CDP would fall below CCR in recovery mode".into(),
                )
            }
        } else {
            RecoveryModeValidation::Blocked(
                "Withdrawal blocked: Would decrease system TCR in recovery mode".into(),
            )
        }
    }

    /// Validate opening a new CDP in recovery mode
    pub fn validate_open_cdp(
        &self,
        is_recovery_mode: bool,
        collateral_sats: u64,
        debt_cents: u64,
        btc_price: u64,
    ) -> RecoveryModeValidation {
        // New CDPs must have ratio >= CCR in recovery mode
        if !is_recovery_mode {
            let ratio = calculate_collateral_ratio(collateral_sats, btc_price, debt_cents)
                .unwrap_or(0);
            if ratio >= MIN_COLLATERAL_RATIO {
                return RecoveryModeValidation::Allowed;
            } else {
                return RecoveryModeValidation::Blocked(
                    "CDP ratio below minimum".into(),
                );
            }
        }

        let ratio = calculate_collateral_ratio(collateral_sats, btc_price, debt_cents)
            .unwrap_or(0);

        if ratio >= CRITICAL_COLLATERAL_RATIO {
            RecoveryModeValidation::AllowedWithRestrictions(
                "New CDP allowed: Meets CCR requirement in recovery mode".into(),
            )
        } else {
            RecoveryModeValidation::Blocked(
                "New CDP blocked: Must have ratio >= 150% in recovery mode".into(),
            )
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // LIQUIDATION IN RECOVERY MODE
    // ═══════════════════════════════════════════════════════════════════════════

    /// Get minimum ratio for liquidation (MCR in normal, CCR in recovery)
    pub fn liquidation_threshold(is_recovery_mode: bool) -> u64 {
        if is_recovery_mode {
            CRITICAL_COLLATERAL_RATIO
        } else {
            MIN_COLLATERAL_RATIO
        }
    }

    /// Check if a CDP is liquidatable
    pub fn is_cdp_liquidatable(
        cdp_collateral: u64,
        cdp_debt: u64,
        btc_price: u64,
        is_recovery_mode: bool,
    ) -> bool {
        if cdp_debt == 0 {
            return false;
        }

        let ratio = calculate_collateral_ratio(cdp_collateral, btc_price, cdp_debt)
            .unwrap_or(u64::MAX);
        let threshold = Self::liquidation_threshold(is_recovery_mode);

        ratio < threshold
    }

    /// Find all CDPs liquidatable in recovery mode
    pub fn find_recovery_mode_liquidatable<'a>(
        cdp_manager: &'a CDPManager,
        btc_price: u64,
    ) -> Vec<&'a CDP> {
        // In recovery mode, CDPs below CCR (150%) are liquidatable
        cdp_manager.get_liquidatable(btc_price, CRITICAL_COLLATERAL_RATIO)
    }

    /// Calculate recovery mode metrics
    pub fn calculate_at_risk_metrics(
        cdp_manager: &CDPManager,
        btc_price: u64,
    ) -> (u64, u64) {
        let mut cdps_at_risk = 0u64;
        let mut debt_at_risk = 0u64;

        for cdp in cdp_manager.all_cdps() {
            // Skip closed/liquidated CDPs
            if cdp.status.is_terminal() {
                continue;
            }

            let ratio = calculate_collateral_ratio(
                cdp.collateral_sats,
                btc_price,
                cdp.debt_cents,
            ).unwrap_or(u64::MAX);

            if ratio < CRITICAL_COLLATERAL_RATIO {
                cdps_at_risk += 1;
                debt_at_risk += cdp.debt_cents;
            }
        }

        (cdps_at_risk, debt_at_risk)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // STATUS AND HISTORY
    // ═══════════════════════════════════════════════════════════════════════════

    /// Get current recovery mode status
    pub fn get_status(
        &self,
        cdp_manager: &CDPManager,
        total_collateral: u64,
        total_debt: u64,
        btc_price: u64,
        timestamp: u64,
        block_height: u64,
    ) -> RecoveryModeStatus {
        let tcr = Self::calculate_tcr(total_collateral, total_debt, btc_price);
        let is_active = Self::is_recovery_mode(tcr);

        if is_active {
            let (cdps_at_risk, debt_at_risk) = Self::calculate_at_risk_metrics(cdp_manager, btc_price);
            RecoveryModeStatus::recovery(
                tcr,
                total_collateral,
                total_debt,
                btc_price,
                cdps_at_risk,
                debt_at_risk,
            ).with_timestamps(timestamp, block_height)
        } else {
            RecoveryModeStatus::normal(tcr, total_collateral, total_debt, btc_price)
                .with_timestamps(timestamp, block_height)
        }
    }

    /// Record a recovery mode change event
    pub fn record_event(&mut self, entering: bool, tcr: u64, timestamp: u64, block: u64) {
        let event = RecoveryModeEvent {
            entering,
            tcr_bps: tcr,
            timestamp,
            block_height: block,
        };

        self.history.push(event);

        // Trim history if needed
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
    }

    /// Get recovery mode history
    pub fn history(&self) -> &[RecoveryModeEvent] {
        &self.history
    }

    /// Get last event
    pub fn last_event(&self) -> Option<&RecoveryModeEvent> {
        self.history.last()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SORTED CDPS FOR REDEMPTION HINTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Sorted CDP list for efficient operations
/// CDPs are sorted by their ICR (Individual Collateral Ratio) ascending
#[derive(Debug, Clone, Default)]
pub struct SortedCDPs {
    /// CDP IDs sorted by ratio (lowest first)
    sorted_ids: Vec<(CDPId, u64)>, // (id, ratio_bps)
}

impl SortedCDPs {
    /// Create a new sorted CDP list
    pub fn new() -> Self {
        Self {
            sorted_ids: Vec::new(),
        }
    }

    /// Rebuild the sorted list from CDP manager
    pub fn rebuild(&mut self, cdp_manager: &CDPManager, btc_price: u64) {
        self.sorted_ids.clear();

        for cdp in cdp_manager.all_cdps() {
            // Only include active CDPs with debt
            if cdp.status.is_terminal() {
                continue;
            }

            if cdp.debt_cents > 0 {
                let ratio = calculate_collateral_ratio(
                    cdp.collateral_sats,
                    btc_price,
                    cdp.debt_cents,
                ).unwrap_or(u64::MAX);

                self.sorted_ids.push((cdp.id, ratio));
            }
        }

        // Sort by ratio ascending (lowest first)
        self.sorted_ids.sort_by_key(|(_, ratio)| *ratio);
    }

    /// Insert a CDP at the correct position
    pub fn insert(&mut self, cdp_id: CDPId, ratio_bps: u64) {
        // Find insertion point using binary search
        let pos = self.sorted_ids
            .binary_search_by_key(&ratio_bps, |(_, r)| *r)
            .unwrap_or_else(|p| p);

        self.sorted_ids.insert(pos, (cdp_id, ratio_bps));
    }

    /// Update a CDP's position (remove and re-insert)
    pub fn update(&mut self, cdp_id: CDPId, new_ratio_bps: u64) {
        // Remove old entry
        self.sorted_ids.retain(|(id, _)| *id != cdp_id);
        // Insert at new position
        self.insert(cdp_id, new_ratio_bps);
    }

    /// Remove a CDP
    pub fn remove(&mut self, cdp_id: &CDPId) {
        self.sorted_ids.retain(|(id, _)| id != cdp_id);
    }

    /// Get CDPs with ratio below threshold
    pub fn get_below_threshold(&self, threshold_bps: u64) -> Vec<CDPId> {
        self.sorted_ids
            .iter()
            .take_while(|(_, ratio)| *ratio < threshold_bps)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Get the CDP with lowest ratio
    pub fn lowest(&self) -> Option<(CDPId, u64)> {
        self.sorted_ids.first().copied()
    }

    /// Get the CDP with highest ratio
    pub fn highest(&self) -> Option<(CDPId, u64)> {
        self.sorted_ids.last().copied()
    }

    /// Get hint for inserting a CDP with given ratio
    /// Returns the CDP ID that should come after the new one
    pub fn get_insert_hint(&self, ratio_bps: u64) -> Option<CDPId> {
        let pos = self.sorted_ids
            .binary_search_by_key(&ratio_bps, |(_, r)| *r)
            .unwrap_or_else(|p| p);

        self.sorted_ids.get(pos).map(|(id, _)| *id)
    }

    /// Get number of CDPs
    pub fn len(&self) -> usize {
        self.sorted_ids.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.sorted_ids.is_empty()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_tcr() {
        // 1 BTC collateral at $100,000, $50,000 debt = 200% TCR
        let tcr = RecoveryModeManager::calculate_tcr(
            100_000_000, // 1 BTC in sats
            5_000_000,   // $50,000 in cents
            10_000_000,  // $100,000 per BTC in cents
        );
        assert_eq!(tcr, 200); // 200% (RATIO_PRECISION = 100)

        // Same collateral, $75,000 debt = 133% TCR
        let tcr = RecoveryModeManager::calculate_tcr(
            100_000_000,
            7_500_000,
            10_000_000,
        );
        assert!(tcr < 150); // Below 150% CCR
    }

    #[test]
    fn test_is_recovery_mode() {
        assert!(RecoveryModeManager::is_recovery_mode(149)); // Below 150%
        assert!(!RecoveryModeManager::is_recovery_mode(150)); // Exactly 150%
        assert!(!RecoveryModeManager::is_recovery_mode(151)); // Above 150%
    }

    #[test]
    fn test_liquidation_threshold() {
        // Normal mode: MCR (110%)
        assert_eq!(
            RecoveryModeManager::liquidation_threshold(false),
            MIN_COLLATERAL_RATIO
        );

        // Recovery mode: CCR (150%)
        assert_eq!(
            RecoveryModeManager::liquidation_threshold(true),
            CRITICAL_COLLATERAL_RATIO
        );
    }

    #[test]
    fn test_validate_mint_recovery_mode() {
        let manager = RecoveryModeManager::new();

        // In normal mode, allow mint
        let validation = manager.validate_mint(
            false,   // not recovery mode
            200,     // 200% TCR
            100_000_000,
            5_000_000,
            1_000_000, // mint $10,000
            50_000_000,
            3_000_000, // CDP debt after
            10_000_000,
        );
        assert!(validation.is_allowed());

        // In recovery mode, block mint that would lower TCR
        let validation = manager.validate_mint(
            true,    // recovery mode
            140,     // 140% TCR (in recovery)
            100_000_000,
            7_000_000,
            1_000_000, // mint more debt
            50_000_000,
            4_000_000,
            10_000_000,
        );
        assert!(!validation.is_allowed());
    }

    #[test]
    fn test_sorted_cdps() {
        let mut sorted = SortedCDPs::new();

        let id1 = CDPId::new([1u8; 32]);
        let id2 = CDPId::new([2u8; 32]);
        let id3 = CDPId::new([3u8; 32]);

        sorted.insert(id2, 150); // 150%
        sorted.insert(id1, 120); // 120%
        sorted.insert(id3, 200); // 200%

        // Should be sorted: id1 (120%), id2 (150%), id3 (200%)
        assert_eq!(sorted.lowest(), Some((id1, 120)));
        assert_eq!(sorted.highest(), Some((id3, 200)));

        // Get CDPs below 150%
        let below = sorted.get_below_threshold(150);
        assert_eq!(below.len(), 1);
        assert_eq!(below[0], id1);
    }

    #[test]
    fn test_recovery_mode_status() {
        let status = RecoveryModeStatus::recovery(
            140, // 140% TCR
            100_000_000,
            7_000_000,
            10_000_000,
            5,    // 5 CDPs at risk
            3_000_000, // $30k debt at risk
        );

        assert!(status.is_active);
        assert_eq!(status.tcr_bps, 140);
        assert!(status.distance_to_exit_bps < 0); // Below exit threshold
    }
}
