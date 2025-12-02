//! Charms-compatible token interface for zkUSD.

use serde::{Deserialize, Serialize};

use crate::core::token::{TokenAmount, ZkUSD};
use crate::error::{Error, Result};
use crate::utils::crypto::{Hash, PublicKey, Signature};

/// Standard Charms token interface
pub trait CharmsToken {
    /// Get token identifier
    fn token_id(&self) -> &CharmId;
    /// Get token name
    fn name(&self) -> &str;
    /// Get token symbol
    fn symbol(&self) -> &str;
    /// Get decimal places
    fn decimals(&self) -> u8;
    /// Get total supply
    fn total_supply(&self) -> u128;
    /// Get balance for an address
    fn balance_of(&self, owner: &PublicKey) -> u128;
    /// Get allowance
    fn allowance(&self, owner: &PublicKey, spender: &PublicKey) -> u128;
    /// Transfer tokens
    fn transfer(&mut self, from: PublicKey, to: PublicKey, amount: u128, signature: &Signature, nonce: u64) -> Result<TransferReceipt>;
    /// Approve spending
    fn approve(&mut self, owner: PublicKey, spender: PublicKey, amount: u128, signature: &Signature, nonce: u64) -> Result<ApprovalReceipt>;
    /// Transfer from approved account
    fn transfer_from(&mut self, spender: PublicKey, from: PublicKey, to: PublicKey, amount: u128, signature: &Signature, nonce: u64) -> Result<TransferReceipt>;
}

/// Unique identifier for a Charm token
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CharmId([u8; 32]);

impl CharmId {
    /// zkUSD's official Charm ID
    pub const ZKUSD: Self = Self([
        0x7a, 0x6b, 0x55, 0x53, 0x44, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
    ]);

    /// Create new Charm ID
    pub fn new(bytes: [u8; 32]) -> Self { Self(bytes) }

    /// Get as bytes
    pub fn as_bytes(&self) -> &[u8; 32] { &self.0 }

    /// Check if this is zkUSD
    pub fn is_zkusd(&self) -> bool { *self == Self::ZKUSD }

    /// Convert to hex
    pub fn to_hex(&self) -> String { hex::encode(self.0) }
}

impl Default for CharmId {
    fn default() -> Self { Self::ZKUSD }
}

/// Receipt for token transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferReceipt {
    pub charm_id: CharmId,
    pub from: PublicKey,
    pub to: PublicKey,
    pub amount: u128,
    pub tx_hash: Hash,
    pub block_height: u64,
    pub nonce: u64,
}

/// Receipt for approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalReceipt {
    pub charm_id: CharmId,
    pub owner: PublicKey,
    pub spender: PublicKey,
    pub amount: u128,
    pub block_height: u64,
    pub nonce: u64,
}

/// zkUSD token implementing Charms interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZkUSDCharm {
    inner: ZkUSD,
    allowances: std::collections::HashMap<(PublicKey, PublicKey), u128>,
    used_nonces: std::collections::HashMap<PublicKey, u64>,
    block_height: u64,
}

impl ZkUSDCharm {
    /// Create new zkUSD Charm token
    pub fn new() -> Self {
        Self {
            inner: ZkUSD::new(),
            allowances: std::collections::HashMap::new(),
            used_nonces: std::collections::HashMap::new(),
            block_height: 0,
        }
    }

    /// Create from existing ZkUSD
    pub fn from_zkusd(token: ZkUSD) -> Self {
        Self {
            inner: token,
            allowances: std::collections::HashMap::new(),
            used_nonces: std::collections::HashMap::new(),
            block_height: 0,
        }
    }

    /// Update block height
    pub fn set_block_height(&mut self, height: u64) { self.block_height = height; }

    /// Get inner token
    pub fn inner(&self) -> &ZkUSD { &self.inner }

    /// Get mutable inner token
    pub fn inner_mut(&mut self) -> &mut ZkUSD { &mut self.inner }

    fn verify_nonce(&mut self, account: &PublicKey, nonce: u64) -> Result<()> {
        let last = self.used_nonces.get(account).copied().unwrap_or(0);
        if nonce <= last {
            return Err(Error::InvalidParameter {
                name: "nonce".into(),
                reason: format!("Nonce {} already used", nonce),
            });
        }
        self.used_nonces.insert(*account, nonce);
        Ok(())
    }
}

impl Default for ZkUSDCharm {
    fn default() -> Self { Self::new() }
}

impl CharmsToken for ZkUSDCharm {
    fn token_id(&self) -> &CharmId { &CharmId::ZKUSD }
    fn name(&self) -> &str { "zkUSD" }
    fn symbol(&self) -> &str { "zkUSD" }
    fn decimals(&self) -> u8 { 2 }
    fn total_supply(&self) -> u128 { self.inner.total_supply().cents() as u128 }
    fn balance_of(&self, owner: &PublicKey) -> u128 { self.inner.balance_of(owner).cents() as u128 }
    fn allowance(&self, owner: &PublicKey, spender: &PublicKey) -> u128 {
        self.allowances.get(&(*owner, *spender)).copied().unwrap_or(0)
    }

    fn transfer(&mut self, from: PublicKey, to: PublicKey, amount: u128, signature: &Signature, nonce: u64) -> Result<TransferReceipt> {
        // Verify signature
        let mut msg = Vec::new();
        msg.extend_from_slice(b"ZKUSD_TRANSFER");
        msg.extend_from_slice(from.as_bytes());
        msg.extend_from_slice(to.as_bytes());
        msg.extend_from_slice(&amount.to_le_bytes());
        msg.extend_from_slice(&nonce.to_le_bytes());
        let hash = Hash::sha256(&msg);
        if !crate::utils::crypto::verify_signature(&from, &hash, signature) {
            return Err(Error::InvalidSignature);
        }
        self.verify_nonce(&from, nonce)?;

        let amount_u64 = u64::try_from(amount).map_err(|_| Error::Overflow { operation: "amount conversion".into() })?;
        let tx_hash = Hash::sha256(&bincode::serialize(&(from, to, amount, nonce)).unwrap_or_default());
        self.inner.transfer(from, to, TokenAmount::from_cents(amount_u64), self.block_height, tx_hash)?;

        Ok(TransferReceipt { charm_id: CharmId::ZKUSD, from, to, amount, tx_hash, block_height: self.block_height, nonce })
    }

    fn approve(&mut self, owner: PublicKey, spender: PublicKey, amount: u128, signature: &Signature, nonce: u64) -> Result<ApprovalReceipt> {
        let mut msg = Vec::new();
        msg.extend_from_slice(b"ZKUSD_APPROVE");
        msg.extend_from_slice(owner.as_bytes());
        msg.extend_from_slice(spender.as_bytes());
        msg.extend_from_slice(&amount.to_le_bytes());
        msg.extend_from_slice(&nonce.to_le_bytes());
        let hash = Hash::sha256(&msg);
        if !crate::utils::crypto::verify_signature(&owner, &hash, signature) {
            return Err(Error::InvalidSignature);
        }
        self.verify_nonce(&owner, nonce)?;
        self.allowances.insert((owner, spender), amount);
        Ok(ApprovalReceipt { charm_id: CharmId::ZKUSD, owner, spender, amount, block_height: self.block_height, nonce })
    }

    fn transfer_from(&mut self, spender: PublicKey, from: PublicKey, to: PublicKey, amount: u128, signature: &Signature, nonce: u64) -> Result<TransferReceipt> {
        let mut msg = Vec::new();
        msg.extend_from_slice(b"ZKUSD_TRANSFER_FROM");
        msg.extend_from_slice(spender.as_bytes());
        msg.extend_from_slice(from.as_bytes());
        msg.extend_from_slice(to.as_bytes());
        msg.extend_from_slice(&amount.to_le_bytes());
        msg.extend_from_slice(&nonce.to_le_bytes());
        let hash = Hash::sha256(&msg);
        if !crate::utils::crypto::verify_signature(&spender, &hash, signature) {
            return Err(Error::InvalidSignature);
        }
        self.verify_nonce(&spender, nonce)?;

        let allowed = self.allowance(&from, &spender);
        if allowed < amount {
            return Err(Error::InsufficientCollateral { required: amount as u64, available: allowed as u64 });
        }
        self.allowances.insert((from, spender), allowed - amount);

        let amount_u64 = u64::try_from(amount).map_err(|_| Error::Overflow { operation: "amount conversion".into() })?;
        let tx_hash = Hash::sha256(&bincode::serialize(&(spender, from, to, amount, nonce)).unwrap_or_default());
        self.inner.transfer(from, to, TokenAmount::from_cents(amount_u64), self.block_height, tx_hash)?;

        Ok(TransferReceipt { charm_id: CharmId::ZKUSD, from, to, amount, tx_hash, block_height: self.block_height, nonce })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::crypto::KeyPair;

    #[test]
    fn test_charm_id() {
        let id = CharmId::ZKUSD;
        assert!(id.is_zkusd());
    }

    #[test]
    fn test_zkusd_charm() {
        let charm = ZkUSDCharm::new();
        assert_eq!(charm.name(), "zkUSD");
        assert_eq!(charm.decimals(), 2);
    }
}
