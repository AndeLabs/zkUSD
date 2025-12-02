//! Input validation utilities for zkUSD protocol.
//!
//! This module provides validation functions to ensure inputs meet
//! protocol requirements before processing.

use crate::error::{Error, Result};
use crate::utils::constants::*;
use crate::utils::crypto::PublicKey;

// ═══════════════════════════════════════════════════════════════════════════════
// AMOUNT VALIDATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate that an amount is non-zero
pub fn validate_non_zero(amount: u64, name: &str) -> Result<()> {
    if amount == 0 {
        return Err(Error::ZeroAmount);
    }
    Ok(())
}

/// Validate debt amount meets minimum requirement
pub fn validate_debt_amount(debt_cents: u64) -> Result<()> {
    validate_non_zero(debt_cents, "debt")?;

    if debt_cents < MIN_DEBT {
        return Err(Error::DebtBelowMinimum {
            amount: debt_cents,
            minimum: MIN_DEBT,
        });
    }

    if debt_cents > MAX_DEBT_PER_CDP {
        return Err(Error::DebtExceedsMaximum {
            amount: debt_cents,
            maximum: MAX_DEBT_PER_CDP,
        });
    }

    Ok(())
}

/// Validate collateral amount is above dust limit
pub fn validate_collateral_amount(collateral_sats: u64) -> Result<()> {
    validate_non_zero(collateral_sats, "collateral")?;

    if collateral_sats < DUST_LIMIT_SATS {
        return Err(Error::InvalidParameter {
            name: "collateral".into(),
            reason: format!(
                "amount {} sats below dust limit {} sats",
                collateral_sats, DUST_LIMIT_SATS
            ),
        });
    }

    Ok(())
}

/// Validate stability pool deposit amount
pub fn validate_sp_deposit(amount_cents: u64) -> Result<()> {
    validate_non_zero(amount_cents, "deposit")?;

    if amount_cents < MIN_SP_DEPOSIT {
        return Err(Error::InvalidParameter {
            name: "deposit".into(),
            reason: format!(
                "amount {} below minimum {} cents",
                amount_cents, MIN_SP_DEPOSIT
            ),
        });
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// RATIO VALIDATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate collateralization ratio
pub fn validate_collateral_ratio(ratio: u64, minimum: u64) -> Result<()> {
    if ratio < minimum {
        return Err(Error::CollateralizationRatioTooLow {
            current: ratio,
            minimum,
        });
    }
    Ok(())
}

/// Validate that ratio is within sane bounds
pub fn validate_ratio_bounds(ratio: u64) -> Result<()> {
    if ratio > MAX_COLLATERAL_RATIO {
        return Err(Error::InvalidParameter {
            name: "ratio".into(),
            reason: format!("ratio {} exceeds maximum {}", ratio, MAX_COLLATERAL_RATIO),
        });
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRICE VALIDATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate BTC price is within sane bounds
pub fn validate_btc_price(price_cents: u64) -> Result<()> {
    if price_cents < MIN_SANE_BTC_PRICE {
        return Err(Error::PriceOutOfBounds {
            price: price_cents,
            min: MIN_SANE_BTC_PRICE,
            max: MAX_SANE_BTC_PRICE,
        });
    }

    if price_cents > MAX_SANE_BTC_PRICE {
        return Err(Error::PriceOutOfBounds {
            price: price_cents,
            min: MIN_SANE_BTC_PRICE,
            max: MAX_SANE_BTC_PRICE,
        });
    }

    Ok(())
}

/// Validate price timestamp is not stale
pub fn validate_price_freshness(timestamp: u64, current_time: u64) -> Result<()> {
    if current_time < timestamp {
        return Err(Error::InvalidParameter {
            name: "timestamp".into(),
            reason: "price timestamp is in the future".into(),
        });
    }

    let age = current_time - timestamp;
    if age > MAX_PRICE_STALENESS_SECS {
        return Err(Error::StalePrice {
            last_update: age,
            max_age: MAX_PRICE_STALENESS_SECS,
        });
    }

    Ok(())
}

/// Validate price deviation between sources
pub fn validate_price_deviation(prices: &[u64]) -> Result<()> {
    if prices.is_empty() {
        return Err(Error::InsufficientOracleSources { got: 0, need: MIN_ORACLE_SOURCES });
    }

    let min_price = *prices.iter().min().unwrap();
    let max_price = *prices.iter().max().unwrap();

    if min_price == 0 {
        return Err(Error::InvalidParameter {
            name: "price".into(),
            reason: "price cannot be zero".into(),
        });
    }

    // Calculate deviation as basis points
    let deviation_bps = ((max_price - min_price) as u128) * (BPS_DIVISOR as u128) / (min_price as u128);

    if deviation_bps > MAX_PRICE_DEVIATION_BPS as u128 {
        return Err(Error::PriceDeviationTooHigh {
            deviation: (deviation_bps / 100) as u64, // Convert to percentage
            max_deviation: MAX_PRICE_DEVIATION_BPS / 100,
        });
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// CRYPTOGRAPHIC VALIDATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate a public key
pub fn validate_public_key(pubkey: &PublicKey) -> Result<()> {
    if !pubkey.is_valid() {
        return Err(Error::InvalidParameter {
            name: "public_key".into(),
            reason: "invalid secp256k1 compressed public key".into(),
        });
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// SYSTEM STATE VALIDATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate system debt ceiling is not exceeded
pub fn validate_debt_ceiling(current_debt: u64, new_debt: u64, ceiling: u64) -> Result<()> {
    let total = current_debt.saturating_add(new_debt);
    if total > ceiling {
        return Err(Error::DebtCeilingReached {
            current: total,
            max: ceiling,
        });
    }
    Ok(())
}

/// Validate number of oracle sources
pub fn validate_oracle_sources(count: usize) -> Result<()> {
    if count < MIN_ORACLE_SOURCES {
        return Err(Error::InsufficientOracleSources {
            got: count,
            need: MIN_ORACLE_SOURCES,
        });
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// BATCH VALIDATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Validation context for CDP operations
#[derive(Debug, Clone)]
pub struct CDPValidationContext {
    pub btc_price_cents: u64,
    pub min_ratio: u64,
    pub debt_ceiling: u64,
    pub current_system_debt: u64,
    pub protocol_paused: bool,
    pub recovery_mode: bool,
}

impl CDPValidationContext {
    /// Validate context for minting operations
    pub fn validate_for_mint(&self, debt_cents: u64) -> Result<()> {
        if self.protocol_paused {
            return Err(Error::ProtocolPaused);
        }

        validate_btc_price(self.btc_price_cents)?;
        validate_debt_amount(debt_cents)?;
        validate_debt_ceiling(self.current_system_debt, debt_cents, self.debt_ceiling)?;

        // In recovery mode, only allow minting if it improves TCR
        if self.recovery_mode {
            // Additional checks would go here
        }

        Ok(())
    }

    /// Validate context for withdrawal operations
    pub fn validate_for_withdraw(&self) -> Result<()> {
        if self.protocol_paused {
            return Err(Error::ProtocolPaused);
        }

        validate_btc_price(self.btc_price_cents)?;

        Ok(())
    }

    /// Validate context for liquidation operations
    pub fn validate_for_liquidation(&self) -> Result<()> {
        // Liquidations are allowed even when paused
        validate_btc_price(self.btc_price_cents)?;

        Ok(())
    }
}

impl Default for CDPValidationContext {
    fn default() -> Self {
        Self {
            btc_price_cents: 0,
            min_ratio: MIN_COLLATERAL_RATIO,
            debt_ceiling: INITIAL_DEBT_CEILING,
            current_system_debt: 0,
            protocol_paused: false,
            recovery_mode: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_debt_amount() {
        // Valid debt
        assert!(validate_debt_amount(MIN_DEBT).is_ok());
        assert!(validate_debt_amount(100_000_00).is_ok()); // $100,000

        // Too low
        assert!(validate_debt_amount(0).is_err());
        assert!(validate_debt_amount(MIN_DEBT - 1).is_err());

        // Too high
        assert!(validate_debt_amount(MAX_DEBT_PER_CDP + 1).is_err());
    }

    #[test]
    fn test_validate_collateral_amount() {
        assert!(validate_collateral_amount(DUST_LIMIT_SATS).is_ok());
        assert!(validate_collateral_amount(SATS_PER_BTC).is_ok());
        assert!(validate_collateral_amount(0).is_err());
        assert!(validate_collateral_amount(DUST_LIMIT_SATS - 1).is_err());
    }

    #[test]
    fn test_validate_btc_price() {
        // Valid prices
        assert!(validate_btc_price(10_000_000).is_ok()); // $100,000
        assert!(validate_btc_price(100_000).is_ok()); // $1,000

        // Invalid prices
        assert!(validate_btc_price(MIN_SANE_BTC_PRICE - 1).is_err());
        assert!(validate_btc_price(MAX_SANE_BTC_PRICE + 1).is_err());
    }

    #[test]
    fn test_validate_price_freshness() {
        let current = 1000000;

        // Fresh price (10 seconds old)
        assert!(validate_price_freshness(current - 10, current).is_ok());

        // Stale price
        assert!(validate_price_freshness(
            current - MAX_PRICE_STALENESS_SECS - 1,
            current
        ).is_err());

        // Future price (invalid)
        assert!(validate_price_freshness(current + 1, current).is_err());
    }

    #[test]
    fn test_validate_price_deviation() {
        // Within bounds (2% deviation)
        assert!(validate_price_deviation(&[100000, 101000, 102000]).is_ok());

        // Too much deviation (10%)
        assert!(validate_price_deviation(&[100000, 100000, 110000]).is_err());

        // Empty prices
        assert!(validate_price_deviation(&[]).is_err());
    }

    #[test]
    fn test_validate_collateral_ratio() {
        assert!(validate_collateral_ratio(150, 110).is_ok());
        assert!(validate_collateral_ratio(110, 110).is_ok());
        assert!(validate_collateral_ratio(109, 110).is_err());
    }

    #[test]
    fn test_cdp_validation_context() {
        let mut ctx = CDPValidationContext {
            btc_price_cents: 10_000_000,
            min_ratio: 110,
            debt_ceiling: 100_000_000_00,
            current_system_debt: 0,
            protocol_paused: false,
            recovery_mode: false,
        };

        // Valid mint
        assert!(ctx.validate_for_mint(100_000_00).is_ok());

        // Paused protocol
        ctx.protocol_paused = true;
        assert!(ctx.validate_for_mint(100_000_00).is_err());
    }
}
