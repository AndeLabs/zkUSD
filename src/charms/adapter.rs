//! Adapter between zkUSD core and Charms SDK.
//!
//! This module provides the bridge that allows zkUSD to operate as a
//! Charms-compatible token on BitcoinOS.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::cdp::CDPManager;
use crate::core::config::ProtocolConfig;
use crate::core::token::ZkUSD;
use crate::core::vault::Vault;
use crate::error::{Error, Result};
use crate::liquidation::stability_pool::StabilityPool;
use crate::utils::crypto::{Hash, PublicKey};
use crate::charms::token::{CharmId, ZkUSDCharm};
use crate::charms::spells::{CharmSpell, SpellResult, ZkUSDSpellType};
use crate::charms::metadata::{CharmMetadata, MetadataRegistry};

// ═══════════════════════════════════════════════════════════════════════════════
// CHARMS ADAPTER
// ═══════════════════════════════════════════════════════════════════════════════

/// Adapter that bridges zkUSD protocol with Charms SDK
#[derive(Debug, Clone)]
pub struct CharmsAdapter {
    /// zkUSD token with Charms interface
    pub token: ZkUSDCharm,
    /// Metadata registry
    pub metadata: MetadataRegistry,
    /// Protocol configuration
    pub config: ProtocolConfig,
    /// Current block height
    pub block_height: u64,
    /// Executed spell hashes (for replay protection)
    executed_spells: HashMap<Hash, u64>,
}

impl CharmsAdapter {
    /// Create new Charms adapter
    pub fn new(creator: PublicKey, block_height: u64) -> Self {
        Self {
            token: ZkUSDCharm::new(),
            metadata: MetadataRegistry::with_zkusd(creator, block_height),
            config: ProtocolConfig::default(),
            block_height,
            executed_spells: HashMap::new(),
        }
    }

    /// Create from existing components
    pub fn from_components(
        token: ZkUSD,
        config: ProtocolConfig,
        creator: PublicKey,
        block_height: u64,
    ) -> Self {
        Self {
            token: ZkUSDCharm::from_zkusd(token),
            metadata: MetadataRegistry::with_zkusd(creator, block_height),
            config,
            block_height,
            executed_spells: HashMap::new(),
        }
    }

    /// Update block height
    pub fn set_block_height(&mut self, height: u64) {
        self.block_height = height;
        self.token.set_block_height(height);
    }

    /// Get token metadata
    pub fn get_metadata(&self) -> Option<&CharmMetadata> {
        self.metadata.get(&CharmId::ZKUSD)
    }

    /// Execute a Charm spell
    pub fn execute_spell(&mut self, spell: CharmSpell) -> SpellResult {
        let spell_hash = spell.hash();

        // Check if spell already executed
        if self.executed_spells.contains_key(&spell_hash) {
            return SpellResult::failure(
                spell_hash,
                "Spell already executed",
                self.block_height,
            );
        }

        // Validate spell
        if let Err(e) = spell.validate(self.block_height) {
            return SpellResult::failure(spell_hash, e.to_string(), self.block_height);
        }

        // Execute based on type
        let result = match spell.spell_type {
            ZkUSDSpellType::Transfer => self.execute_transfer_spell(&spell),
            ZkUSDSpellType::Approve => self.execute_approve_spell(&spell),
            _ => Err(Error::InvalidParameter {
                name: "spell_type".into(),
                reason: format!("Spell type {:?} not supported by adapter", spell.spell_type),
            }),
        };

        match result {
            Ok(data) => {
                self.executed_spells.insert(spell_hash, self.block_height);
                SpellResult::success(spell_hash, data, self.block_height, 1000)
            }
            Err(e) => SpellResult::failure(spell_hash, e.to_string(), self.block_height),
        }
    }

    /// Execute transfer spell
    fn execute_transfer_spell(&mut self, spell: &CharmSpell) -> Result<Vec<u8>> {
        use crate::charms::spells::TransferParams;
        use crate::charms::token::CharmsToken;

        let params = TransferParams::decode(&spell.data)?;

        let receipt = self.token.transfer(
            spell.caster,
            params.to,
            params.amount as u128,
            &spell.signature,
            spell.nonce,
        )?;

        bincode::serialize(&receipt).map_err(|e| Error::Serialization(e.to_string()))
    }

    /// Execute approve spell
    fn execute_approve_spell(&mut self, spell: &CharmSpell) -> Result<Vec<u8>> {
        use crate::charms::token::CharmsToken;

        #[derive(serde::Deserialize)]
        struct ApproveParams {
            spender: PublicKey,
            amount: u64,
        }

        let params: ApproveParams = bincode::deserialize(&spell.data)
            .map_err(|e| Error::Serialization(e.to_string()))?;

        let receipt = self.token.approve(
            spell.caster,
            params.spender,
            params.amount as u128,
            &spell.signature,
            spell.nonce,
        )?;

        bincode::serialize(&receipt).map_err(|e| Error::Serialization(e.to_string()))
    }

    /// Get spell execution count
    pub fn executed_spell_count(&self) -> usize {
        self.executed_spells.len()
    }

    /// Check if spell was executed
    pub fn was_spell_executed(&self, spell_hash: &Hash) -> bool {
        self.executed_spells.contains_key(spell_hash)
    }

    /// Clear old executed spells (garbage collection)
    pub fn cleanup_old_spells(&mut self, max_age_blocks: u64) {
        let cutoff = self.block_height.saturating_sub(max_age_blocks);
        self.executed_spells.retain(|_, height| *height > cutoff);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROTOCOL ADAPTER
// ═══════════════════════════════════════════════════════════════════════════════

/// Full protocol adapter with CDP and stability pool support
pub struct ProtocolCharmsAdapter {
    /// Base Charms adapter
    pub adapter: CharmsAdapter,
    /// CDP manager
    pub cdp_manager: CDPManager,
    /// Vault
    pub vault: Vault,
    /// Stability pool
    pub stability_pool: StabilityPool,
    /// Current BTC price (cents)
    pub btc_price: u64,
}

impl ProtocolCharmsAdapter {
    /// Create new protocol adapter
    pub fn new(creator: PublicKey, block_height: u64, btc_price: u64) -> Self {
        Self {
            adapter: CharmsAdapter::new(creator, block_height),
            cdp_manager: CDPManager::new(),
            vault: Vault::new(),
            stability_pool: StabilityPool::new(),
            btc_price,
        }
    }

    /// Update block height across all components
    pub fn set_block_height(&mut self, height: u64) {
        self.adapter.set_block_height(height);
    }

    /// Update BTC price
    pub fn set_btc_price(&mut self, price: u64) {
        self.btc_price = price;
    }

    /// Get protocol statistics
    pub fn statistics(&self) -> ProtocolStats {
        ProtocolStats {
            total_supply: self.adapter.token.inner().total_supply().cents(),
            total_collateral: self.vault.total_collateral().sats(),
            active_cdps: self.cdp_manager.active_count() as u64,
            stability_pool_deposits: self.stability_pool.total_deposits().cents(),
            btc_price: self.btc_price,
            block_height: self.adapter.block_height,
        }
    }
}

/// Protocol statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolStats {
    /// Total zkUSD supply (cents)
    pub total_supply: u64,
    /// Total collateral locked (sats)
    pub total_collateral: u64,
    /// Number of active CDPs
    pub active_cdps: u64,
    /// Stability pool deposits (cents)
    pub stability_pool_deposits: u64,
    /// Current BTC price (cents)
    pub btc_price: u64,
    /// Current block height
    pub block_height: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::crypto::KeyPair;
    use crate::charms::spells::SpellBuilder;

    #[test]
    fn test_charms_adapter_creation() {
        let keypair = KeyPair::generate();
        let adapter = CharmsAdapter::new(*keypair.public_key(), 100);

        assert!(adapter.get_metadata().is_some());
        assert_eq!(adapter.block_height, 100);
    }

    #[test]
    fn test_spell_validation() {
        let adapter = CharmsAdapter::new(PublicKey::new([0u8; 33]), 100);

        let sender = KeyPair::generate();
        let recipient = KeyPair::generate();

        // Create a spell
        let spell = SpellBuilder::transfer(*recipient.public_key(), 5000)
            .nonce(1)
            .deadline(200)
            .build_and_sign(&sender);

        // Spell should be valid
        assert!(spell.verify_signature().is_ok());
        assert!(!spell.is_expired(100));
        assert!(spell.is_expired(201));
    }

    #[test]
    fn test_protocol_adapter() {
        let keypair = KeyPair::generate();
        let adapter = ProtocolCharmsAdapter::new(
            *keypair.public_key(),
            100,
            10_000_000, // $100k BTC
        );

        let stats = adapter.statistics();
        assert_eq!(stats.total_supply, 0);
        assert_eq!(stats.btc_price, 10_000_000);
        assert_eq!(stats.block_height, 100);
    }

    #[test]
    fn test_spell_cleanup() {
        let keypair = KeyPair::generate();
        let mut adapter = CharmsAdapter::new(*keypair.public_key(), 100);

        // Add some executed spells
        adapter.executed_spells.insert(Hash::sha256(b"1"), 50);
        adapter.executed_spells.insert(Hash::sha256(b"2"), 80);
        adapter.executed_spells.insert(Hash::sha256(b"3"), 95);

        assert_eq!(adapter.executed_spell_count(), 3);

        // Cleanup spells older than 30 blocks
        adapter.cleanup_old_spells(30);

        // Only spells from block 70+ should remain
        assert_eq!(adapter.executed_spell_count(), 2);
    }
}
