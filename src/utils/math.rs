//! Fixed-point arithmetic and mathematical utilities.
//!
//! This module provides safe arithmetic operations with overflow protection
//! and fixed-point calculations for precise financial computations.

use crate::error::{Error, Result};
use crate::utils::constants::{BPS_DIVISOR, RATIO_PRECISION, SATS_PER_BTC, ZKUSD_BASE_UNIT};
use std::ops::{Add, Div, Mul, Sub};

// ═══════════════════════════════════════════════════════════════════════════════
// FIXED POINT TYPE
// ═══════════════════════════════════════════════════════════════════════════════

/// Fixed-point number with 18 decimal places precision
/// Used for precise calculations without floating-point errors
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct FixedPoint(u128);

impl FixedPoint {
    /// Scale factor: 10^18
    pub const SCALE: u128 = 1_000_000_000_000_000_000;

    /// Zero value
    pub const ZERO: Self = Self(0);

    /// One (1.0)
    pub const ONE: Self = Self(Self::SCALE);

    /// Create a new FixedPoint from raw value
    pub const fn from_raw(raw: u128) -> Self {
        Self(raw)
    }

    /// Create from an integer (scales up)
    pub fn from_integer(value: u64) -> Self {
        Self((value as u128) * Self::SCALE)
    }

    /// Create from basis points (100 bps = 1%)
    pub fn from_bps(bps: u64) -> Self {
        Self((bps as u128) * Self::SCALE / (BPS_DIVISOR as u128))
    }

    /// Create from percentage (100 = 100%)
    pub fn from_percentage(pct: u64) -> Self {
        Self((pct as u128) * Self::SCALE / 100)
    }

    /// Get the raw underlying value
    pub fn raw(&self) -> u128 {
        self.0
    }

    /// Convert to u64, rounding down (truncating)
    pub fn to_u64_floor(&self) -> u64 {
        (self.0 / Self::SCALE) as u64
    }

    /// Convert to u64, rounding up
    pub fn to_u64_ceil(&self) -> u64 {
        ((self.0 + Self::SCALE - 1) / Self::SCALE) as u64
    }

    /// Convert to u64, rounding to nearest
    pub fn to_u64_round(&self) -> u64 {
        ((self.0 + Self::SCALE / 2) / Self::SCALE) as u64
    }

    /// Multiply by a u64 value
    pub fn mul_u64(&self, value: u64) -> Self {
        Self(self.0 * (value as u128))
    }

    /// Divide by a u64 value
    pub fn div_u64(&self, value: u64) -> Option<Self> {
        if value == 0 {
            None
        } else {
            Some(Self(self.0 / (value as u128)))
        }
    }

    /// Check if value is zero
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Saturating subtraction
    pub fn saturating_sub(&self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    /// Minimum of two values
    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    /// Maximum of two values
    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }
}

impl Add for FixedPoint {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for FixedPoint {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl Mul for FixedPoint {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        // Multiply and divide by scale to maintain precision
        Self((self.0 * rhs.0) / Self::SCALE)
    }
}

impl Div for FixedPoint {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        // Multiply by scale first to maintain precision
        Self((self.0 * Self::SCALE) / rhs.0)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SAFE ARITHMETIC OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Safe addition with overflow check
pub fn safe_add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).ok_or(Error::Overflow {
        operation: format!("{} + {}", a, b),
    })
}

/// Safe subtraction with underflow check
pub fn safe_sub(a: u64, b: u64) -> Result<u64> {
    a.checked_sub(b).ok_or(Error::Underflow {
        operation: format!("{} - {}", a, b),
    })
}

/// Safe multiplication with overflow check
pub fn safe_mul(a: u64, b: u64) -> Result<u64> {
    a.checked_mul(b).ok_or(Error::Overflow {
        operation: format!("{} * {}", a, b),
    })
}

/// Safe division with zero check
pub fn safe_div(a: u64, b: u64) -> Result<u64> {
    if b == 0 {
        return Err(Error::InvalidParameter {
            name: "divisor".into(),
            reason: "division by zero".into(),
        });
    }
    Ok(a / b)
}

/// Safe multiplication then division (for better precision)
/// Computes (a * b) / c with u128 intermediate to prevent overflow
pub fn safe_mul_div(a: u64, b: u64, c: u64) -> Result<u64> {
    if c == 0 {
        return Err(Error::InvalidParameter {
            name: "divisor".into(),
            reason: "division by zero".into(),
        });
    }
    let result = (a as u128) * (b as u128) / (c as u128);
    if result > u64::MAX as u128 {
        return Err(Error::Overflow {
            operation: format!("({} * {}) / {}", a, b, c),
        });
    }
    Ok(result as u64)
}

/// Safe multiplication then division, rounding up
pub fn safe_mul_div_up(a: u64, b: u64, c: u64) -> Result<u64> {
    if c == 0 {
        return Err(Error::InvalidParameter {
            name: "divisor".into(),
            reason: "division by zero".into(),
        });
    }
    let numerator = (a as u128) * (b as u128);
    let result = (numerator + (c as u128) - 1) / (c as u128);
    if result > u64::MAX as u128 {
        return Err(Error::Overflow {
            operation: format!("ceil(({} * {}) / {})", a, b, c),
        });
    }
    Ok(result as u64)
}

// ═══════════════════════════════════════════════════════════════════════════════
// COLLATERALIZATION CALCULATIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Calculate collateralization ratio as a percentage
///
/// # Arguments
/// * `collateral_sats` - Collateral amount in satoshis
/// * `btc_price_cents` - BTC price in cents (e.g., $100,000 = 10,000,000)
/// * `debt_cents` - Debt amount in cents
///
/// # Returns
/// Collateralization ratio as percentage (e.g., 150 = 150%)
pub fn calculate_collateral_ratio(
    collateral_sats: u64,
    btc_price_cents: u64,
    debt_cents: u64,
) -> Result<u64> {
    if debt_cents == 0 {
        return Ok(u64::MAX); // Infinite ratio if no debt
    }

    // collateral_value = collateral_sats * btc_price_cents / SATS_PER_BTC
    // ratio = collateral_value * 100 / debt_cents
    // Combined: ratio = collateral_sats * btc_price_cents * 100 / (SATS_PER_BTC * debt_cents)

    let numerator = (collateral_sats as u128) * (btc_price_cents as u128) * (RATIO_PRECISION as u128);
    let denominator = (SATS_PER_BTC as u128) * (debt_cents as u128);

    let ratio = numerator / denominator;

    if ratio > u64::MAX as u128 {
        return Ok(u64::MAX);
    }

    Ok(ratio as u64)
}

/// Calculate maximum debt for given collateral
///
/// # Arguments
/// * `collateral_sats` - Collateral amount in satoshis
/// * `btc_price_cents` - BTC price in cents
/// * `min_ratio` - Minimum collateralization ratio (e.g., 110 for 110%)
///
/// # Returns
/// Maximum debt in cents
pub fn calculate_max_debt(
    collateral_sats: u64,
    btc_price_cents: u64,
    min_ratio: u64,
) -> Result<u64> {
    if min_ratio == 0 {
        return Err(Error::InvalidParameter {
            name: "min_ratio".into(),
            reason: "cannot be zero".into(),
        });
    }

    // max_debt = collateral_value * 100 / min_ratio
    // max_debt = (collateral_sats * btc_price_cents / SATS_PER_BTC) * 100 / min_ratio
    // Combined: max_debt = collateral_sats * btc_price_cents * 100 / (SATS_PER_BTC * min_ratio)

    let numerator = (collateral_sats as u128) * (btc_price_cents as u128) * (RATIO_PRECISION as u128);
    let denominator = (SATS_PER_BTC as u128) * (min_ratio as u128);

    let result = numerator / denominator;

    if result > u64::MAX as u128 {
        return Err(Error::Overflow {
            operation: "calculate_max_debt".into(),
        });
    }

    Ok(result as u64)
}

/// Calculate minimum collateral required for given debt
///
/// # Arguments
/// * `debt_cents` - Debt amount in cents
/// * `btc_price_cents` - BTC price in cents
/// * `min_ratio` - Minimum collateralization ratio (e.g., 110 for 110%)
///
/// # Returns
/// Minimum collateral in satoshis
pub fn calculate_min_collateral(
    debt_cents: u64,
    btc_price_cents: u64,
    min_ratio: u64,
) -> Result<u64> {
    if btc_price_cents == 0 {
        return Err(Error::InvalidParameter {
            name: "btc_price".into(),
            reason: "cannot be zero".into(),
        });
    }

    // min_collateral_value = debt_cents * min_ratio / 100
    // min_collateral_sats = min_collateral_value * SATS_PER_BTC / btc_price_cents
    // Combined: min_collateral_sats = debt_cents * min_ratio * SATS_PER_BTC / (100 * btc_price_cents)

    let numerator = (debt_cents as u128) * (min_ratio as u128) * (SATS_PER_BTC as u128);
    let denominator = (RATIO_PRECISION as u128) * (btc_price_cents as u128);

    // Round up to ensure minimum collateral
    let result = (numerator + denominator - 1) / denominator;

    if result > u64::MAX as u128 {
        return Err(Error::Overflow {
            operation: "calculate_min_collateral".into(),
        });
    }

    Ok(result as u64)
}

/// Calculate collateral value in cents (USD)
pub fn calculate_collateral_value(collateral_sats: u64, btc_price_cents: u64) -> Result<u64> {
    safe_mul_div(collateral_sats, btc_price_cents, SATS_PER_BTC)
}

// ═══════════════════════════════════════════════════════════════════════════════
// FEE CALCULATIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Calculate fee in basis points
pub fn calculate_fee_bps(amount: u64, fee_bps: u64) -> Result<u64> {
    safe_mul_div(amount, fee_bps, BPS_DIVISOR)
}

/// Calculate amount after fee deduction
pub fn amount_after_fee(amount: u64, fee_bps: u64) -> Result<u64> {
    let fee = calculate_fee_bps(amount, fee_bps)?;
    safe_sub(amount, fee)
}

/// Calculate liquidation amounts
///
/// # Returns
/// (debt_to_cover, collateral_to_seize, liquidator_bonus)
pub fn calculate_liquidation_amounts(
    total_collateral_sats: u64,
    total_debt_cents: u64,
    btc_price_cents: u64,
    bonus_bps: u64,
) -> Result<(u64, u64, u64)> {
    // Debt to cover is the full debt
    let debt_to_cover = total_debt_cents;

    // Collateral value needed to cover debt + bonus
    let debt_plus_bonus = safe_mul_div(debt_to_cover, BPS_DIVISOR + bonus_bps, BPS_DIVISOR)?;

    // Convert to satoshis
    let collateral_needed_sats = safe_mul_div(debt_plus_bonus, SATS_PER_BTC, btc_price_cents)?;

    // Collateral to seize is minimum of needed and available
    let collateral_to_seize = collateral_needed_sats.min(total_collateral_sats);

    // Calculate actual bonus given to liquidator
    let collateral_value = calculate_collateral_value(collateral_to_seize, btc_price_cents)?;
    let liquidator_bonus = if collateral_value > debt_to_cover {
        collateral_value - debt_to_cover
    } else {
        0
    };

    Ok((debt_to_cover, collateral_to_seize, liquidator_bonus))
}

// ═══════════════════════════════════════════════════════════════════════════════
// UTILITY FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Calculate median of a slice (modifies the slice by sorting)
pub fn median(values: &mut [u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        Some((values[mid - 1] + values[mid]) / 2)
    } else {
        Some(values[mid])
    }
}

/// Check if a value is within percentage deviation of a target
pub fn within_deviation(value: u64, target: u64, max_deviation_bps: u64) -> bool {
    if target == 0 {
        return value == 0;
    }

    let diff = if value > target {
        value - target
    } else {
        target - value
    };

    // deviation = diff * 10000 / target
    let deviation_bps = (diff as u128) * (BPS_DIVISOR as u128) / (target as u128);
    deviation_bps <= max_deviation_bps as u128
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_point_basic() {
        let one = FixedPoint::ONE;
        let two = FixedPoint::from_integer(2);

        assert_eq!(one + one, two);
        assert_eq!(two - one, one);
        assert_eq!(one * two, two);
        assert_eq!(two / one, two);
    }

    #[test]
    fn test_fixed_point_from_bps() {
        let half = FixedPoint::from_bps(5000); // 50%
        let one = FixedPoint::ONE;

        assert_eq!(one * half, FixedPoint::from_raw(FixedPoint::SCALE / 2));
    }

    #[test]
    fn test_safe_arithmetic() {
        assert!(safe_add(1, 2).is_ok());
        assert!(safe_add(u64::MAX, 1).is_err());

        assert!(safe_sub(5, 3).is_ok());
        assert!(safe_sub(3, 5).is_err());

        assert!(safe_mul(100, 200).is_ok());
        assert!(safe_mul(u64::MAX, 2).is_err());

        assert!(safe_div(100, 10).is_ok());
        assert!(safe_div(100, 0).is_err());
    }

    #[test]
    fn test_collateral_ratio() {
        // 1 BTC ($100,000) collateral, $50,000 debt = 200%
        let ratio = calculate_collateral_ratio(
            SATS_PER_BTC,        // 1 BTC
            10_000_000,          // $100,000 in cents
            5_000_000,           // $50,000 in cents
        ).unwrap();
        assert_eq!(ratio, 200);

        // 1 BTC ($100,000) collateral, $90,909 debt = 110%
        let ratio = calculate_collateral_ratio(
            SATS_PER_BTC,
            10_000_000,
            9_090_909,
        ).unwrap();
        assert_eq!(ratio, 110);
    }

    #[test]
    fn test_max_debt() {
        // 1 BTC at $100,000 with 110% MCR
        let max_debt = calculate_max_debt(
            SATS_PER_BTC,
            10_000_000,  // $100,000
            110,         // 110% MCR
        ).unwrap();

        // max_debt should be ~$90,909
        assert!(max_debt >= 9_090_000 && max_debt <= 9_091_000);
    }

    #[test]
    fn test_min_collateral() {
        // $50,000 debt, $100,000/BTC, 150% MCR
        let min_coll = calculate_min_collateral(
            5_000_000,   // $50,000 debt
            10_000_000,  // $100,000 BTC price
            150,         // 150% MCR
        ).unwrap();

        // min_collateral should be 0.75 BTC = 75,000,000 sats
        assert_eq!(min_coll, 75_000_000);
    }

    #[test]
    fn test_fee_calculation() {
        // 0.5% fee on $10,000
        let fee = calculate_fee_bps(1_000_000, 50).unwrap();
        assert_eq!(fee, 5_000); // $50
    }

    #[test]
    fn test_median() {
        assert_eq!(median(&mut [1, 2, 3]), Some(2));
        assert_eq!(median(&mut [1, 2, 3, 4]), Some(2)); // (2+3)/2 = 2
        assert_eq!(median(&mut [3, 1, 2]), Some(2));
        assert_eq!(median(&mut []), None);
    }

    #[test]
    fn test_within_deviation() {
        assert!(within_deviation(100, 100, 500)); // 0% deviation
        assert!(within_deviation(105, 100, 500)); // 5% deviation
        assert!(!within_deviation(106, 100, 500)); // 6% > 5%
        assert!(within_deviation(95, 100, 500)); // -5% deviation
    }
}
