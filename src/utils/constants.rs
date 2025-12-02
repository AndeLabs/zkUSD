//! Protocol constants and magic numbers.
//!
//! All protocol-wide constants are defined here for easy auditing and modification.

// ═══════════════════════════════════════════════════════════════════════════════
// BITCOIN CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Satoshis per Bitcoin (1 BTC = 100,000,000 satoshis)
pub const SATS_PER_BTC: u64 = 100_000_000;

/// Minimum dust limit in satoshis
pub const DUST_LIMIT_SATS: u64 = 546;

/// Average Bitcoin block time in seconds
pub const BLOCK_TIME_SECS: u64 = 600;

// ═══════════════════════════════════════════════════════════════════════════════
// ZKUSD CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// zkUSD decimals (same as USD cents, 2 decimals for display, stored as cents)
pub const ZKUSD_DECIMALS: u8 = 2;

/// Base unit for zkUSD (1 zkUSD = 100 cents)
pub const ZKUSD_BASE_UNIT: u64 = 100;

/// Maximum zkUSD supply (100 billion zkUSD in cents)
pub const MAX_ZKUSD_SUPPLY: u64 = 100_000_000_000 * ZKUSD_BASE_UNIT;

// ═══════════════════════════════════════════════════════════════════════════════
// COLLATERALIZATION CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Minimum Collateralization Ratio (MCR) - 110%
/// Below this ratio, a CDP can be liquidated
pub const MIN_COLLATERAL_RATIO: u64 = 110;

/// Recommended Collateralization Ratio - 150%
/// Users should maintain at least this ratio for safety
pub const RECOMMENDED_COLLATERAL_RATIO: u64 = 150;

/// Critical Collateralization Ratio (CCR) for Recovery Mode - 150%
/// When system TCR falls below this, Recovery Mode activates
pub const CRITICAL_COLLATERAL_RATIO: u64 = 150;

/// Maximum Collateralization Ratio for calculations - 10000% (100x)
pub const MAX_COLLATERAL_RATIO: u64 = 10000;

/// Ratio precision (basis points, 100 = 1%)
pub const RATIO_PRECISION: u64 = 100;

// ═══════════════════════════════════════════════════════════════════════════════
// FEE CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Borrowing fee - 0.5% (50 basis points)
pub const BORROWING_FEE_BPS: u64 = 50;

/// Redemption fee floor - 0.5% (50 basis points)
pub const REDEMPTION_FEE_FLOOR_BPS: u64 = 50;

/// Redemption fee ceiling - 5% (500 basis points)
pub const REDEMPTION_FEE_CEILING_BPS: u64 = 500;

/// Liquidation bonus - 10% (1000 basis points)
/// This is the discount liquidators receive when buying collateral
pub const LIQUIDATION_BONUS_BPS: u64 = 1000;

/// Basis points divisor (10000 = 100%)
pub const BPS_DIVISOR: u64 = 10000;

// ═══════════════════════════════════════════════════════════════════════════════
// DEBT LIMITS
// ═══════════════════════════════════════════════════════════════════════════════

/// Minimum debt per CDP - $100 (10000 cents)
pub const MIN_DEBT: u64 = 100 * ZKUSD_BASE_UNIT;

/// Maximum debt per CDP - $10 million (1,000,000,000 cents)
pub const MAX_DEBT_PER_CDP: u64 = 10_000_000 * ZKUSD_BASE_UNIT;

/// Initial system debt ceiling - $100 million
pub const INITIAL_DEBT_CEILING: u64 = 100_000_000 * ZKUSD_BASE_UNIT;

// ═══════════════════════════════════════════════════════════════════════════════
// ORACLE CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Minimum number of oracle sources required
pub const MIN_ORACLE_SOURCES: usize = 3;

/// Maximum price staleness in seconds (1 hour)
pub const MAX_PRICE_STALENESS_SECS: u64 = 3600;

/// Maximum allowed price deviation between sources - 5%
pub const MAX_PRICE_DEVIATION_BPS: u64 = 500;

/// Price precision (8 decimals for BTC price in cents)
pub const PRICE_PRECISION: u64 = 100_000_000;

/// Minimum sane BTC price - $1,000
pub const MIN_SANE_BTC_PRICE: u64 = 1_000 * ZKUSD_BASE_UNIT;

/// Maximum sane BTC price - $10,000,000
pub const MAX_SANE_BTC_PRICE: u64 = 10_000_000 * ZKUSD_BASE_UNIT;

// ═══════════════════════════════════════════════════════════════════════════════
// STABILITY POOL CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Minimum stability pool deposit - $10
pub const MIN_SP_DEPOSIT: u64 = 10 * ZKUSD_BASE_UNIT;

/// Scale factor for stability pool calculations
pub const SP_SCALE_FACTOR: u128 = 1_000_000_000_000_000_000; // 10^18

// ═══════════════════════════════════════════════════════════════════════════════
// TIME CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Redemption fee decay half-life - 12 hours
pub const REDEMPTION_FEE_DECAY_HALF_LIFE: u64 = 12 * 3600;

/// Minimum time between price updates - 60 seconds
pub const MIN_PRICE_UPDATE_INTERVAL: u64 = 60;

// ═══════════════════════════════════════════════════════════════════════════════
// CRYPTOGRAPHIC CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Length of a public key in bytes (compressed secp256k1)
pub const PUBKEY_LENGTH: usize = 33;

/// Length of a signature in bytes
pub const SIGNATURE_LENGTH: usize = 64;

/// Length of a hash in bytes (SHA256 or Blake3)
pub const HASH_LENGTH: usize = 32;

/// Length of a CDP ID in bytes
pub const CDP_ID_LENGTH: usize = 32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fee_calculations() {
        // Verify fee constants make sense
        assert!(BORROWING_FEE_BPS < BPS_DIVISOR);
        assert!(REDEMPTION_FEE_FLOOR_BPS < REDEMPTION_FEE_CEILING_BPS);
        assert!(LIQUIDATION_BONUS_BPS < BPS_DIVISOR);
    }

    #[test]
    fn test_ratio_constants() {
        assert!(MIN_COLLATERAL_RATIO < RECOMMENDED_COLLATERAL_RATIO);
        assert!(RECOMMENDED_COLLATERAL_RATIO <= CRITICAL_COLLATERAL_RATIO);
        assert!(CRITICAL_COLLATERAL_RATIO < MAX_COLLATERAL_RATIO);
    }

    #[test]
    fn test_debt_limits() {
        assert!(MIN_DEBT < MAX_DEBT_PER_CDP);
        assert!(MAX_DEBT_PER_CDP < INITIAL_DEBT_CEILING);
        assert!(INITIAL_DEBT_CEILING < MAX_ZKUSD_SUPPLY);
    }

    #[test]
    fn test_price_bounds() {
        assert!(MIN_SANE_BTC_PRICE < MAX_SANE_BTC_PRICE);
    }
}
