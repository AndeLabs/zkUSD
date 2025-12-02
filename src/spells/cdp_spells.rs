//! CDP operation spells.
//!
//! These spells handle all CDP-related operations:
//! - Open CDP
//! - Deposit collateral
//! - Withdraw collateral
//! - Mint zkUSD
//! - Repay debt
//! - Close CDP

use serde::{Deserialize, Serialize};

use crate::core::cdp::{CDP, CDPId};
use crate::core::config::ProtocolConfig;
use crate::core::token::TokenAmount;
use crate::core::vault::CollateralAmount;
use crate::error::{Error, Result};
use crate::spells::types::*;
use crate::utils::constants::*;
use crate::utils::crypto::{Hash, PublicKey, Signature};
use crate::utils::math::*;
use crate::utils::validation::*;

// ═══════════════════════════════════════════════════════════════════════════════
// OPEN CDP SPELL
// ═══════════════════════════════════════════════════════════════════════════════

/// Spell to open a new CDP with initial collateral
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCDPSpell {
    /// Owner of the new CDP
    pub owner: PublicKey,
    /// Initial collateral amount
    pub collateral: CollateralAmount,
    /// Initial debt to mint (optional)
    pub initial_debt: Option<TokenAmount>,
    /// Current BTC price
    pub btc_price: u64,
    /// Price proof hash
    pub price_proof_hash: Hash,
    /// Authorization
    pub auth: SpellAuth,
    /// Metadata
    pub meta: SpellMeta,
}

impl OpenCDPSpell {
    /// Create a new OpenCDP spell
    pub fn new(
        owner: PublicKey,
        collateral: CollateralAmount,
        initial_debt: Option<TokenAmount>,
        btc_price: u64,
        nonce: u64,
        block_height: u64,
    ) -> Self {
        Self {
            owner,
            collateral,
            initial_debt,
            btc_price,
            price_proof_hash: Hash::zero(),
            auth: SpellAuth {
                signer: owner,
                signature: Signature::new([0u8; 64]),
                nonce,
            },
            meta: SpellMeta {
                spell_type: "OpenCDP".to_string(),
                version: 1,
                block_height,
                timestamp: 0,
            },
        }
    }

    /// Validate spell inputs
    pub fn validate(&self, config: &ProtocolConfig) -> Result<()> {
        // Validate collateral
        validate_collateral_amount(self.collateral.sats())?;

        // Validate BTC price
        validate_btc_price(self.btc_price)?;

        // If initial debt, validate it
        if let Some(debt) = self.initial_debt {
            validate_debt_amount(debt.cents())?;

            // Validate collateralization ratio
            let ratio = calculate_collateral_ratio(
                self.collateral.sats(),
                self.btc_price,
                debt.cents(),
            )?;

            validate_collateral_ratio(ratio, config.effective_mcr())?;

            // Validate debt ceiling
            validate_debt_ceiling(
                config.total_system_debt,
                debt.cents(),
                config.debt_ceiling,
            )?;
        }

        Ok(())
    }

    /// Execute spell and create CDP
    pub fn execute(&self, config: &ProtocolConfig) -> Result<(CDP, TokenAmount)> {
        self.validate(config)?;

        // Create CDP
        let mut cdp = CDP::with_collateral(
            self.owner,
            self.collateral.sats(),
            self.auth.nonce,
            self.meta.block_height,
        )?;

        // Mint initial debt if specified
        let minted = if let Some(debt) = self.initial_debt {
            let net_mint = cdp.mint_debt(
                debt.cents(),
                self.btc_price,
                config.effective_mcr(),
                self.meta.block_height,
            )?;
            TokenAmount::from_cents(net_mint)
        } else {
            TokenAmount::ZERO
        };

        Ok((cdp, minted))
    }

    /// Compute spell hash
    pub fn hash(&self) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(self.owner.as_bytes());
        data.extend_from_slice(&self.collateral.sats().to_be_bytes());
        data.extend_from_slice(&self.btc_price.to_be_bytes());
        data.extend_from_slice(&self.auth.nonce.to_be_bytes());
        if let Some(debt) = self.initial_debt {
            data.extend_from_slice(&debt.cents().to_be_bytes());
        }
        Hash::sha256(&data)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DEPOSIT COLLATERAL SPELL
// ═══════════════════════════════════════════════════════════════════════════════

/// Spell to deposit additional collateral into a CDP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositCollateralSpell {
    /// CDP ID
    pub cdp_id: CDPId,
    /// Amount to deposit
    pub amount: CollateralAmount,
    /// Authorization
    pub auth: SpellAuth,
    /// Metadata
    pub meta: SpellMeta,
}

impl DepositCollateralSpell {
    /// Validate spell
    pub fn validate(&self) -> Result<()> {
        validate_collateral_amount(self.amount.sats())
    }

    /// Execute spell
    pub fn execute(&self, cdp: &mut CDP) -> Result<()> {
        self.validate()?;
        cdp.deposit_collateral(self.amount.sats(), self.meta.block_height)
    }

    /// Compute hash
    pub fn hash(&self) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(cdp_id_bytes(&self.cdp_id));
        data.extend_from_slice(&self.amount.sats().to_be_bytes());
        data.extend_from_slice(&self.auth.nonce.to_be_bytes());
        Hash::sha256(&data)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// WITHDRAW COLLATERAL SPELL
// ═══════════════════════════════════════════════════════════════════════════════

/// Spell to withdraw collateral from a CDP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawCollateralSpell {
    /// CDP ID
    pub cdp_id: CDPId,
    /// Amount to withdraw
    pub amount: CollateralAmount,
    /// Current BTC price
    pub btc_price: u64,
    /// Authorization (must be CDP owner)
    pub auth: SpellAuth,
    /// Metadata
    pub meta: SpellMeta,
}

impl WithdrawCollateralSpell {
    /// Validate spell
    pub fn validate(&self, cdp: &CDP, config: &ProtocolConfig) -> Result<()> {
        // Verify ownership
        cdp.verify_owner(&self.auth.signer)?;

        // Validate amount
        if self.amount.sats() > cdp.collateral_sats {
            return Err(Error::InsufficientCollateral {
                required: self.amount.sats(),
                available: cdp.collateral_sats,
            });
        }

        // Check ratio after withdrawal
        if cdp.debt_cents > 0 {
            let new_collateral = cdp.collateral_sats - self.amount.sats();
            let new_ratio = calculate_collateral_ratio(
                new_collateral,
                self.btc_price,
                cdp.debt_cents,
            )?;

            if new_ratio < config.effective_mcr() {
                return Err(Error::WithdrawalWouldUndercollateralize);
            }
        }

        Ok(())
    }

    /// Execute spell
    pub fn execute(&self, cdp: &mut CDP, config: &ProtocolConfig) -> Result<CollateralAmount> {
        self.validate(cdp, config)?;

        cdp.withdraw_collateral(
            self.amount.sats(),
            self.btc_price,
            config.effective_mcr(),
            self.meta.block_height,
        )?;

        Ok(self.amount)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MINT ZKUSD SPELL
// ═══════════════════════════════════════════════════════════════════════════════

/// Spell to mint zkUSD against CDP collateral
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintZkUSDSpell {
    /// CDP ID
    pub cdp_id: CDPId,
    /// Amount to mint
    pub amount: TokenAmount,
    /// Current BTC price
    pub btc_price: u64,
    /// Price proof hash
    pub price_proof_hash: Hash,
    /// Authorization (must be CDP owner)
    pub auth: SpellAuth,
    /// Metadata
    pub meta: SpellMeta,
}

impl MintZkUSDSpell {
    /// Validate spell
    pub fn validate(&self, cdp: &CDP, config: &ProtocolConfig) -> Result<()> {
        // Verify ownership
        cdp.verify_owner(&self.auth.signer)?;

        // Validate amount
        validate_debt_amount(self.amount.cents())?;

        // Check debt ceiling
        validate_debt_ceiling(
            config.total_system_debt,
            self.amount.cents(),
            config.debt_ceiling,
        )?;

        // Check ratio after minting
        let new_debt = cdp.debt_cents + self.amount.cents();
        let new_ratio = calculate_collateral_ratio(
            cdp.collateral_sats,
            self.btc_price,
            new_debt,
        )?;

        if new_ratio < config.effective_mcr() {
            return Err(Error::CollateralizationRatioTooLow {
                current: new_ratio,
                minimum: config.effective_mcr(),
            });
        }

        Ok(())
    }

    /// Execute spell
    pub fn execute(&self, cdp: &mut CDP, config: &ProtocolConfig) -> Result<MintResult> {
        self.validate(cdp, config)?;

        let net_mint = cdp.mint_debt(
            self.amount.cents(),
            self.btc_price,
            config.effective_mcr(),
            self.meta.block_height,
        )?;

        let fee = self.amount.cents() - net_mint;

        Ok(MintResult {
            gross_amount: self.amount,
            fee: TokenAmount::from_cents(fee),
            net_amount: TokenAmount::from_cents(net_mint),
            new_debt: TokenAmount::from_cents(cdp.debt_cents),
            new_ratio: cdp.calculate_ratio(self.btc_price),
        })
    }
}

/// Result of mint operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintResult {
    pub gross_amount: TokenAmount,
    pub fee: TokenAmount,
    pub net_amount: TokenAmount,
    pub new_debt: TokenAmount,
    pub new_ratio: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// REPAY DEBT SPELL
// ═══════════════════════════════════════════════════════════════════════════════

/// Spell to repay zkUSD debt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepayDebtSpell {
    /// CDP ID
    pub cdp_id: CDPId,
    /// Amount to repay
    pub amount: TokenAmount,
    /// Authorization
    pub auth: SpellAuth,
    /// Metadata
    pub meta: SpellMeta,
}

impl RepayDebtSpell {
    /// Validate spell
    pub fn validate(&self, cdp: &CDP) -> Result<()> {
        if self.amount.is_zero() {
            return Err(Error::ZeroAmount);
        }

        if cdp.debt_cents == 0 {
            return Err(Error::InvalidParameter {
                name: "debt".into(),
                reason: "CDP has no debt to repay".into(),
            });
        }

        Ok(())
    }

    /// Execute spell
    pub fn execute(&self, cdp: &mut CDP) -> Result<RepayResult> {
        self.validate(cdp)?;

        let debt_before = cdp.debt_cents;
        let actual_repaid = cdp.repay_debt(self.amount.cents(), self.meta.block_height)?;

        Ok(RepayResult {
            amount_requested: self.amount,
            amount_repaid: TokenAmount::from_cents(actual_repaid),
            debt_remaining: TokenAmount::from_cents(cdp.debt_cents),
            fully_repaid: cdp.debt_cents == 0,
        })
    }
}

/// Result of repay operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepayResult {
    pub amount_requested: TokenAmount,
    pub amount_repaid: TokenAmount,
    pub debt_remaining: TokenAmount,
    pub fully_repaid: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
// CLOSE CDP SPELL
// ═══════════════════════════════════════════════════════════════════════════════

/// Spell to close a CDP (must have no debt)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseCDPSpell {
    /// CDP ID
    pub cdp_id: CDPId,
    /// Authorization (must be CDP owner)
    pub auth: SpellAuth,
    /// Metadata
    pub meta: SpellMeta,
}

impl CloseCDPSpell {
    /// Validate spell
    pub fn validate(&self, cdp: &CDP) -> Result<()> {
        cdp.verify_owner(&self.auth.signer)?;

        if cdp.debt_cents > 0 {
            return Err(Error::InvalidParameter {
                name: "debt".into(),
                reason: "must repay all debt before closing CDP".into(),
            });
        }

        Ok(())
    }

    /// Execute spell
    pub fn execute(&self, cdp: &mut CDP) -> Result<CloseResult> {
        self.validate(cdp)?;

        let collateral_returned = cdp.close(self.meta.block_height)?;

        Ok(CloseResult {
            cdp_id: self.cdp_id,
            collateral_returned: CollateralAmount::from_sats(collateral_returned),
        })
    }
}

/// Result of close operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseResult {
    pub cdp_id: CDPId,
    pub collateral_returned: CollateralAmount,
}

// ═══════════════════════════════════════════════════════════════════════════════
// HELPER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

fn cdp_id_bytes(id: &CDPId) -> &[u8] {
    id.as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pubkey() -> PublicKey {
        PublicKey::new([0x02; PUBKEY_LENGTH])
    }

    #[test]
    fn test_open_cdp_spell() {
        let config = ProtocolConfig::default();
        let spell = OpenCDPSpell::new(
            test_pubkey(),
            CollateralAmount::from_btc(1),
            Some(TokenAmount::from_dollars(50000)),
            10_000_000, // $100,000/BTC
            1,
            100,
        );

        let result = spell.execute(&config);
        assert!(result.is_ok());

        let (cdp, minted) = result.unwrap();
        assert_eq!(cdp.collateral_sats, SATS_PER_BTC);
        assert!(minted.cents() > 0);
    }

    #[test]
    fn test_open_cdp_insufficient_ratio() {
        let config = ProtocolConfig::default();
        let spell = OpenCDPSpell::new(
            test_pubkey(),
            CollateralAmount::from_btc(1),
            Some(TokenAmount::from_dollars(95000)), // Would be 105% ratio
            10_000_000,
            1,
            100,
        );

        let result = spell.execute(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_mint_spell() {
        let config = ProtocolConfig::default();

        // Create CDP first
        let open_spell = OpenCDPSpell::new(
            test_pubkey(),
            CollateralAmount::from_btc(2), // 2 BTC = $200k collateral
            None,
            10_000_000,
            1,
            100,
        );
        let (mut cdp, _) = open_spell.execute(&config).unwrap();

        // Now mint
        let mint_spell = MintZkUSDSpell {
            cdp_id: cdp.id,
            amount: TokenAmount::from_dollars(100000), // $100k debt = 200% ratio
            btc_price: 10_000_000,
            price_proof_hash: Hash::zero(),
            auth: SpellAuth {
                signer: test_pubkey(),
                signature: Signature::new([0u8; 64]),
                nonce: 2,
            },
            meta: SpellMeta {
                spell_type: "MintZkUSD".to_string(),
                version: 1,
                block_height: 101,
                timestamp: 0,
            },
        };

        let result = mint_spell.execute(&mut cdp, &config);
        assert!(result.is_ok());

        let mint_result = result.unwrap();
        assert!(mint_result.fee.cents() > 0);
        assert_eq!(mint_result.new_ratio, 200);
    }

    #[test]
    fn test_close_cdp_with_debt_fails() {
        let config = ProtocolConfig::default();

        let open_spell = OpenCDPSpell::new(
            test_pubkey(),
            CollateralAmount::from_btc(1),
            Some(TokenAmount::from_dollars(50000)),
            10_000_000,
            1,
            100,
        );
        let (mut cdp, _) = open_spell.execute(&config).unwrap();

        let close_spell = CloseCDPSpell {
            cdp_id: cdp.id,
            auth: SpellAuth {
                signer: test_pubkey(),
                signature: Signature::new([0u8; 64]),
                nonce: 2,
            },
            meta: SpellMeta {
                spell_type: "CloseCDP".to_string(),
                version: 1,
                block_height: 101,
                timestamp: 0,
            },
        };

        let result = close_spell.execute(&mut cdp);
        assert!(result.is_err());
    }
}
