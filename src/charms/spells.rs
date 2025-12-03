//! Charm spells for zkUSD operations.

use serde::{Deserialize, Serialize};
use crate::error::{Error, Result};
use crate::utils::crypto::{Hash, PublicKey, Signature};
use crate::charms::token::CharmId;

/// Spell type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ZkUSDSpellType {
    Transfer = 1,
    Approve = 2,
    OpenCDP = 10,
    CloseCDP = 11,
    DepositCollateral = 12,
    WithdrawCollateral = 13,
    MintDebt = 14,
    RepayDebt = 15,
    Liquidate = 20,
    Redeem = 21,
    StabilityDeposit = 30,
    StabilityWithdraw = 31,
    ClaimGains = 32,
}

impl From<u8> for ZkUSDSpellType {
    fn from(v: u8) -> Self {
        match v {
            1 => Self::Transfer, 2 => Self::Approve,
            10 => Self::OpenCDP, 11 => Self::CloseCDP,
            12 => Self::DepositCollateral, 13 => Self::WithdrawCollateral,
            14 => Self::MintDebt, 15 => Self::RepayDebt,
            20 => Self::Liquidate, 21 => Self::Redeem,
            30 => Self::StabilityDeposit, 31 => Self::StabilityWithdraw, 32 => Self::ClaimGains,
            _ => Self::Transfer,
        }
    }
}

/// Base spell structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharmSpell {
    pub spell_type: ZkUSDSpellType,
    pub charm_id: CharmId,
    pub caster: PublicKey,
    pub data: Vec<u8>,
    pub signature: Signature,
    pub nonce: u64,
    pub deadline: u64,
}

impl CharmSpell {
    /// Create new spell
    pub fn new(spell_type: ZkUSDSpellType, caster: PublicKey, data: Vec<u8>, signature: Signature, nonce: u64, deadline: u64) -> Self {
        Self { spell_type, charm_id: CharmId::ZKUSD, caster, data, signature, nonce, deadline }
    }

    /// Compute hash
    pub fn hash(&self) -> Hash {
        let mut msg = Vec::new();
        msg.push(self.spell_type as u8);
        msg.extend_from_slice(self.charm_id.as_bytes());
        msg.extend_from_slice(self.caster.as_bytes());
        msg.extend_from_slice(&self.data);
        msg.extend_from_slice(&self.nonce.to_le_bytes());
        msg.extend_from_slice(&self.deadline.to_le_bytes());
        Hash::sha256(&msg)
    }

    /// Verify signature
    pub fn verify_signature(&self) -> Result<()> {
        let hash = self.hash();
        if !crate::utils::crypto::verify_signature(&self.caster, &hash, &self.signature) {
            return Err(Error::InvalidSignature);
        }
        Ok(())
    }

    /// Check if expired
    pub fn is_expired(&self, current_block: u64) -> bool { current_block > self.deadline }

    /// Validate spell
    pub fn validate(&self, current_block: u64) -> Result<()> {
        self.verify_signature()?;
        if self.is_expired(current_block) {
            return Err(Error::InvalidParameter { name: "deadline".into(), reason: "Spell expired".into() });
        }
        Ok(())
    }
}

/// Transfer parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferParams {
    pub to: PublicKey,
    pub amount: u64,
}

impl TransferParams {
    pub fn encode(&self) -> Vec<u8> { bincode::serialize(self).unwrap_or_default() }
    pub fn decode(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(|e| Error::Serialization(e.to_string()))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CDP OPERATION PARAMETERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Open CDP parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCDPParams {
    /// Initial collateral in sats
    pub collateral_sats: u64,
    /// Initial debt to mint in cents (optional)
    pub initial_debt_cents: u64,
    /// UTXO txid containing the collateral
    pub utxo_txid: [u8; 32],
    /// UTXO output index
    pub utxo_vout: u32,
}

impl OpenCDPParams {
    pub fn encode(&self) -> Vec<u8> { bincode::serialize(self).unwrap_or_default() }
    pub fn decode(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(|e| Error::Serialization(e.to_string()))
    }
}

/// Close CDP parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseCDPParams {
    /// CDP ID to close
    pub cdp_id: [u8; 32],
}

impl CloseCDPParams {
    pub fn encode(&self) -> Vec<u8> { bincode::serialize(self).unwrap_or_default() }
    pub fn decode(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(|e| Error::Serialization(e.to_string()))
    }
}

/// Deposit collateral parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositCollateralParams {
    /// CDP ID
    pub cdp_id: [u8; 32],
    /// Amount to deposit in sats
    pub amount_sats: u64,
    /// UTXO txid containing the collateral
    pub utxo_txid: [u8; 32],
    /// UTXO output index
    pub utxo_vout: u32,
}

impl DepositCollateralParams {
    pub fn encode(&self) -> Vec<u8> { bincode::serialize(self).unwrap_or_default() }
    pub fn decode(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(|e| Error::Serialization(e.to_string()))
    }
}

/// Withdraw collateral parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawCollateralParams {
    /// CDP ID
    pub cdp_id: [u8; 32],
    /// Amount to withdraw in sats
    pub amount_sats: u64,
    /// Destination address (script pubkey)
    pub destination: Vec<u8>,
}

impl WithdrawCollateralParams {
    pub fn encode(&self) -> Vec<u8> { bincode::serialize(self).unwrap_or_default() }
    pub fn decode(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(|e| Error::Serialization(e.to_string()))
    }
}

/// Mint debt parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintDebtParams {
    /// CDP ID
    pub cdp_id: [u8; 32],
    /// Amount to mint in cents
    pub amount_cents: u64,
}

impl MintDebtParams {
    pub fn encode(&self) -> Vec<u8> { bincode::serialize(self).unwrap_or_default() }
    pub fn decode(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(|e| Error::Serialization(e.to_string()))
    }
}

/// Repay debt parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepayDebtParams {
    /// CDP ID
    pub cdp_id: [u8; 32],
    /// Amount to repay in cents
    pub amount_cents: u64,
}

impl RepayDebtParams {
    pub fn encode(&self) -> Vec<u8> { bincode::serialize(self).unwrap_or_default() }
    pub fn decode(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(|e| Error::Serialization(e.to_string()))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// LIQUIDATION PARAMETERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Liquidate CDP parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidateParams {
    /// CDP ID to liquidate
    pub cdp_id: [u8; 32],
    /// Maximum debt to liquidate (cents)
    pub max_debt_cents: u64,
}

impl LiquidateParams {
    pub fn encode(&self) -> Vec<u8> { bincode::serialize(self).unwrap_or_default() }
    pub fn decode(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(|e| Error::Serialization(e.to_string()))
    }
}

/// Redeem zkUSD parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedeemParams {
    /// Amount of zkUSD to redeem (cents)
    pub amount_cents: u64,
    /// Maximum fee willing to pay (basis points)
    pub max_fee_bps: u64,
    /// Destination for BTC (script pubkey)
    pub destination: Vec<u8>,
}

impl RedeemParams {
    pub fn encode(&self) -> Vec<u8> { bincode::serialize(self).unwrap_or_default() }
    pub fn decode(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(|e| Error::Serialization(e.to_string()))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// STABILITY POOL PARAMETERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Stability pool deposit parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityDepositParams {
    /// Amount to deposit (cents)
    pub amount_cents: u64,
}

impl StabilityDepositParams {
    pub fn encode(&self) -> Vec<u8> { bincode::serialize(self).unwrap_or_default() }
    pub fn decode(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(|e| Error::Serialization(e.to_string()))
    }
}

/// Stability pool withdraw parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityWithdrawParams {
    /// Amount to withdraw (cents), 0 for all
    pub amount_cents: u64,
}

impl StabilityWithdrawParams {
    pub fn encode(&self) -> Vec<u8> { bincode::serialize(self).unwrap_or_default() }
    pub fn decode(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(|e| Error::Serialization(e.to_string()))
    }
}

/// Claim gains parameters (empty, but included for consistency)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimGainsParams {
    /// Destination for BTC gains (script pubkey)
    pub destination: Vec<u8>,
}

impl ClaimGainsParams {
    pub fn encode(&self) -> Vec<u8> { bincode::serialize(self).unwrap_or_default() }
    pub fn decode(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(|e| Error::Serialization(e.to_string()))
    }
}

/// Spell result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpellResult {
    pub success: bool,
    pub spell_hash: Hash,
    pub data: Vec<u8>,
    pub error: Option<String>,
    pub block_height: u64,
    pub gas_used: u64,
}

impl SpellResult {
    pub fn success(spell_hash: Hash, data: Vec<u8>, block_height: u64, gas_used: u64) -> Self {
        Self { success: true, spell_hash, data, error: None, block_height, gas_used }
    }
    pub fn failure(spell_hash: Hash, error: impl Into<String>, block_height: u64) -> Self {
        Self { success: false, spell_hash, data: Vec::new(), error: Some(error.into()), block_height, gas_used: 0 }
    }
}

/// Spell builder
pub struct SpellBuilder {
    spell_type: ZkUSDSpellType,
    data: Vec<u8>,
    nonce: u64,
    deadline: u64,
}

impl SpellBuilder {
    pub fn new(spell_type: ZkUSDSpellType) -> Self {
        Self { spell_type, data: Vec::new(), nonce: 0, deadline: u64::MAX }
    }

    pub fn data(mut self, data: Vec<u8>) -> Self { self.data = data; self }
    pub fn nonce(mut self, nonce: u64) -> Self { self.nonce = nonce; self }
    pub fn deadline(mut self, deadline: u64) -> Self { self.deadline = deadline; self }

    // Token operations
    pub fn transfer(to: PublicKey, amount: u64) -> Self {
        Self::new(ZkUSDSpellType::Transfer).data(TransferParams { to, amount }.encode())
    }

    // CDP operations
    pub fn open_cdp(collateral_sats: u64, initial_debt_cents: u64, utxo_txid: [u8; 32], utxo_vout: u32) -> Self {
        Self::new(ZkUSDSpellType::OpenCDP).data(
            OpenCDPParams { collateral_sats, initial_debt_cents, utxo_txid, utxo_vout }.encode()
        )
    }

    pub fn close_cdp(cdp_id: [u8; 32]) -> Self {
        Self::new(ZkUSDSpellType::CloseCDP).data(CloseCDPParams { cdp_id }.encode())
    }

    pub fn deposit_collateral(cdp_id: [u8; 32], amount_sats: u64, utxo_txid: [u8; 32], utxo_vout: u32) -> Self {
        Self::new(ZkUSDSpellType::DepositCollateral).data(
            DepositCollateralParams { cdp_id, amount_sats, utxo_txid, utxo_vout }.encode()
        )
    }

    pub fn withdraw_collateral(cdp_id: [u8; 32], amount_sats: u64, destination: Vec<u8>) -> Self {
        Self::new(ZkUSDSpellType::WithdrawCollateral).data(
            WithdrawCollateralParams { cdp_id, amount_sats, destination }.encode()
        )
    }

    pub fn mint_debt(cdp_id: [u8; 32], amount_cents: u64) -> Self {
        Self::new(ZkUSDSpellType::MintDebt).data(MintDebtParams { cdp_id, amount_cents }.encode())
    }

    pub fn repay_debt(cdp_id: [u8; 32], amount_cents: u64) -> Self {
        Self::new(ZkUSDSpellType::RepayDebt).data(RepayDebtParams { cdp_id, amount_cents }.encode())
    }

    // Liquidation operations
    pub fn liquidate(cdp_id: [u8; 32], max_debt_cents: u64) -> Self {
        Self::new(ZkUSDSpellType::Liquidate).data(LiquidateParams { cdp_id, max_debt_cents }.encode())
    }

    pub fn redeem(amount_cents: u64, max_fee_bps: u64, destination: Vec<u8>) -> Self {
        Self::new(ZkUSDSpellType::Redeem).data(RedeemParams { amount_cents, max_fee_bps, destination }.encode())
    }

    // Stability pool operations
    pub fn stability_deposit(amount_cents: u64) -> Self {
        Self::new(ZkUSDSpellType::StabilityDeposit).data(StabilityDepositParams { amount_cents }.encode())
    }

    pub fn stability_withdraw(amount_cents: u64) -> Self {
        Self::new(ZkUSDSpellType::StabilityWithdraw).data(StabilityWithdrawParams { amount_cents }.encode())
    }

    pub fn claim_gains(destination: Vec<u8>) -> Self {
        Self::new(ZkUSDSpellType::ClaimGains).data(ClaimGainsParams { destination }.encode())
    }

    pub fn build_and_sign(self, caster: &crate::utils::crypto::KeyPair) -> CharmSpell {
        let mut spell = CharmSpell::new(self.spell_type, *caster.public_key(), self.data, Signature::new([0u8; 64]), self.nonce, self.deadline);
        let hash = spell.hash();
        spell.signature = caster.sign(&hash);
        spell
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::crypto::KeyPair;

    #[test]
    fn test_spell_creation() {
        let kp = KeyPair::generate();
        let recipient = KeyPair::generate();
        let spell = SpellBuilder::transfer(*recipient.public_key(), 1000).nonce(1).deadline(1000).build_and_sign(&kp);
        assert!(spell.verify_signature().is_ok());
    }
}
