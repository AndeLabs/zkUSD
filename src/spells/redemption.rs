//! Redemption spell for exchanging zkUSD for collateral.
//!
//! Redemptions allow zkUSD holders to exchange their tokens for
//! underlying collateral at face value, minus a fee.

use serde::{Deserialize, Serialize};

use crate::core::cdp::{CDP, CDPId, CDPManager};
use crate::core::config::ProtocolConfig;
use crate::core::token::TokenAmount;
use crate::core::vault::CollateralAmount;
use crate::error::{Error, Result};
use crate::spells::types::*;
use crate::utils::constants::*;
use crate::utils::crypto::{Hash, PublicKey, Signature};
use crate::utils::math::*;

// ═══════════════════════════════════════════════════════════════════════════════
// REDEMPTION SPELL
// ═══════════════════════════════════════════════════════════════════════════════

/// Spell to redeem zkUSD for collateral
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedemptionSpell {
    /// Redeemer's public key
    pub redeemer: PublicKey,
    /// Amount of zkUSD to redeem
    pub amount: TokenAmount,
    /// Maximum fee willing to pay (in basis points)
    pub max_fee_bps: u64,
    /// Current BTC price
    pub btc_price: u64,
    /// Hint for first CDP to redeem from (optimization)
    pub first_redemption_hint: Option<CDPId>,
    /// Authorization
    pub auth: SpellAuth,
    /// Metadata
    pub meta: SpellMeta,
}

impl RedemptionSpell {
    /// Create a new redemption spell
    pub fn new(
        redeemer: PublicKey,
        amount: TokenAmount,
        max_fee_bps: u64,
        btc_price: u64,
        block_height: u64,
        nonce: u64,
    ) -> Self {
        Self {
            redeemer,
            amount,
            max_fee_bps,
            btc_price,
            first_redemption_hint: None,
            auth: SpellAuth {
                signer: redeemer,
                signature: Signature::new([0u8; 64]),
                nonce,
            },
            meta: SpellMeta {
                spell_type: "Redemption".to_string(),
                version: 1,
                block_height,
                timestamp: 0,
            },
        }
    }

    /// Validate spell
    pub fn validate(&self, config: &ProtocolConfig) -> Result<()> {
        if self.amount.is_zero() {
            return Err(Error::ZeroAmount);
        }

        // Calculate current fee
        let current_fee = config.calculate_redemption_fee(self.meta.timestamp);
        if current_fee > self.max_fee_bps {
            return Err(Error::InvalidParameter {
                name: "fee".into(),
                reason: format!(
                    "current fee {}bps exceeds max {}bps",
                    current_fee, self.max_fee_bps
                ),
            });
        }

        Ok(())
    }

    /// Execute redemption against multiple CDPs
    pub fn execute(
        &self,
        cdp_manager: &mut CDPManager,
        config: &mut ProtocolConfig,
    ) -> Result<RedemptionResult> {
        self.validate(config)?;

        // Calculate fee
        let fee_bps = config.calculate_redemption_fee(self.meta.timestamp);
        let fee_amount = calculate_fee_bps(self.amount.cents(), fee_bps)?;
        let net_redemption = self.amount.cents() - fee_amount;

        // Calculate collateral to receive
        let collateral_sats = safe_mul_div(net_redemption, SATS_PER_BTC, self.btc_price)?;

        // Get CDPs sorted by ratio (ascending - riskiest first)
        let sorted_cdps = cdp_manager.get_sorted_by_ratio(self.btc_price);

        let mut remaining_to_redeem = net_redemption;
        let mut total_collateral_received = 0u64;
        let mut cdps_affected = Vec::new();

        for (cdp, _ratio) in sorted_cdps {
            if remaining_to_redeem == 0 {
                break;
            }

            // Skip CDPs with no debt
            if cdp.debt_cents == 0 {
                continue;
            }

            // Calculate how much to redeem from this CDP
            let redeem_from_this = remaining_to_redeem.min(cdp.debt_cents);

            // Calculate collateral to take
            let collateral_to_take = safe_mul_div(
                redeem_from_this,
                SATS_PER_BTC,
                self.btc_price,
            )?;

            // Record affected CDP
            cdps_affected.push(RedemptionCDPEffect {
                cdp_id: cdp.id,
                debt_redeemed: TokenAmount::from_cents(redeem_from_this),
                collateral_taken: CollateralAmount::from_sats(collateral_to_take),
            });

            remaining_to_redeem -= redeem_from_this;
            total_collateral_received += collateral_to_take;
        }

        // Note: In a real implementation, we would actually modify the CDPs here
        // For now, we just return what would happen

        // Update protocol base rate
        config.update_base_rate(self.amount.cents() - remaining_to_redeem, self.meta.timestamp);

        Ok(RedemptionResult {
            redeemer: self.redeemer,
            zkusd_redeemed: TokenAmount::from_cents(self.amount.cents() - remaining_to_redeem),
            collateral_received: CollateralAmount::from_sats(total_collateral_received),
            fee_paid: TokenAmount::from_cents(fee_amount),
            fee_rate_bps: fee_bps,
            cdps_affected,
            remaining_unredeemed: TokenAmount::from_cents(remaining_to_redeem),
        })
    }
}

/// Effect on a single CDP from redemption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedemptionCDPEffect {
    /// CDP that was affected
    pub cdp_id: CDPId,
    /// Debt redeemed from this CDP
    pub debt_redeemed: TokenAmount,
    /// Collateral taken from this CDP
    pub collateral_taken: CollateralAmount,
}

/// Result of redemption operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedemptionResult {
    /// Redeemer
    pub redeemer: PublicKey,
    /// Total zkUSD redeemed
    pub zkusd_redeemed: TokenAmount,
    /// Total collateral received
    pub collateral_received: CollateralAmount,
    /// Fee paid
    pub fee_paid: TokenAmount,
    /// Fee rate in basis points
    pub fee_rate_bps: u64,
    /// CDPs affected by redemption
    pub cdps_affected: Vec<RedemptionCDPEffect>,
    /// Amount that couldn't be redeemed (insufficient CDPs)
    pub remaining_unredeemed: TokenAmount,
}

// ═══════════════════════════════════════════════════════════════════════════════
// LIQUIDATION SPELL
// ═══════════════════════════════════════════════════════════════════════════════

/// Spell to liquidate an undercollateralized CDP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationSpell {
    /// CDP to liquidate
    pub cdp_id: CDPId,
    /// Liquidator
    pub liquidator: PublicKey,
    /// Current BTC price
    pub btc_price: u64,
    /// Authorization
    pub auth: SpellAuth,
    /// Metadata
    pub meta: SpellMeta,
}

impl LiquidationSpell {
    /// Create a new liquidation spell
    pub fn new(
        cdp_id: CDPId,
        liquidator: PublicKey,
        btc_price: u64,
        block_height: u64,
        nonce: u64,
    ) -> Self {
        Self {
            cdp_id,
            liquidator,
            btc_price,
            auth: SpellAuth {
                signer: liquidator,
                signature: Signature::new([0u8; 64]),
                nonce,
            },
            meta: SpellMeta {
                spell_type: "Liquidation".to_string(),
                version: 1,
                block_height,
                timestamp: 0,
            },
        }
    }

    /// Validate spell
    pub fn validate(&self, cdp: &CDP, config: &ProtocolConfig) -> Result<()> {
        if !cdp.is_liquidatable(self.btc_price, config.effective_mcr()) {
            return Err(Error::CDPHealthy(self.cdp_id.to_hex()));
        }

        Ok(())
    }

    /// Execute liquidation
    pub fn execute(
        &self,
        cdp: &mut CDP,
        config: &ProtocolConfig,
    ) -> Result<LiquidationSpellResult> {
        self.validate(cdp, config)?;

        let ratio_before = cdp.calculate_ratio(self.btc_price);
        let debt_before = cdp.debt_cents;
        let collateral_before = cdp.collateral_sats;

        // Perform liquidation
        let liq_result = cdp.liquidate(
            self.btc_price,
            config.effective_mcr(),
            self.meta.block_height,
        )?;

        Ok(LiquidationSpellResult {
            cdp_id: self.cdp_id,
            liquidator: self.liquidator,
            debt_covered: TokenAmount::from_cents(liq_result.debt_covered),
            collateral_seized: CollateralAmount::from_sats(liq_result.collateral_seized),
            liquidator_bonus: CollateralAmount::from_sats(liq_result.liquidator_bonus),
            ratio_at_liquidation: ratio_before,
            btc_price: self.btc_price,
        })
    }
}

/// Result of liquidation spell
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationSpellResult {
    /// CDP that was liquidated
    pub cdp_id: CDPId,
    /// Liquidator
    pub liquidator: PublicKey,
    /// Debt covered
    pub debt_covered: TokenAmount,
    /// Collateral seized
    pub collateral_seized: CollateralAmount,
    /// Bonus for liquidator
    pub liquidator_bonus: CollateralAmount,
    /// Ratio at time of liquidation
    pub ratio_at_liquidation: u64,
    /// BTC price at liquidation
    pub btc_price: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pubkey() -> PublicKey {
        PublicKey::new([0x02; PUBKEY_LENGTH])
    }

    #[test]
    fn test_redemption_fee_check() {
        let config = ProtocolConfig::default();

        let spell = RedemptionSpell::new(
            test_pubkey(),
            TokenAmount::from_dollars(10000),
            10, // Very low max fee
            10_000_000,
            100,
            1,
        );

        // Default fee is 50bps, so 10bps max should fail
        let result = spell.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_liquidation_spell_healthy_cdp() {
        let config = ProtocolConfig::default();

        let pubkey = test_pubkey();
        let mut cdp = CDP::with_collateral(pubkey, SATS_PER_BTC, 1, 100).unwrap();
        cdp.debt_cents = 5_000_000; // 200% ratio at $100k

        let spell = LiquidationSpell::new(
            cdp.id,
            pubkey,
            10_000_000, // $100k
            100,
            1,
        );

        // Should fail - CDP is healthy
        let result = spell.execute(&mut cdp, &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_liquidation_spell_underwater_cdp() {
        let config = ProtocolConfig::default();

        let pubkey = test_pubkey();
        let mut cdp = CDP::with_collateral(pubkey, SATS_PER_BTC, 1, 100).unwrap();
        cdp.debt_cents = 5_000_000; // At $50k, this is 100% ratio

        let spell = LiquidationSpell::new(
            cdp.id,
            test_pubkey(),
            5_000_000, // $50k - makes CDP underwater
            100,
            1,
        );

        // Should succeed - CDP is underwater
        let result = spell.execute(&mut cdp, &config);
        assert!(result.is_ok());
    }
}
