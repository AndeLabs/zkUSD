//! Charm spells for zkUSD operations.

use serde::{Deserialize, Serialize};
use crate::core::cdp::CDPId;
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

    pub fn transfer(to: PublicKey, amount: u64) -> Self {
        Self::new(ZkUSDSpellType::Transfer).data(TransferParams { to, amount }.encode())
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
