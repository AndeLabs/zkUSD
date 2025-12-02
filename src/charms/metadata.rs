//! Token metadata for Charms integration.

use serde::{Deserialize, Serialize};
use crate::utils::crypto::{Hash, PublicKey};
use crate::charms::token::CharmId;

/// On-chain token metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharmMetadata {
    pub charm_id: CharmId,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub description: String,
    pub website: Option<String>,
    pub creator: PublicKey,
    pub created_at: u64,
}

impl CharmMetadata {
    /// Create zkUSD metadata
    pub fn zkusd(creator: PublicKey, created_at: u64) -> Self {
        Self {
            charm_id: CharmId::ZKUSD,
            name: "zkUSD".to_string(),
            symbol: "zkUSD".to_string(),
            decimals: 2,
            description: "Decentralized stablecoin backed by Bitcoin on BitcoinOS".to_string(),
            website: Some("https://zkusd.io".to_string()),
            creator,
            created_at,
        }
    }

    /// Compute hash
    pub fn hash(&self) -> Hash {
        Hash::sha256(&bincode::serialize(self).unwrap_or_default())
    }
}

/// Registry of token metadata
#[derive(Debug, Clone, Default)]
pub struct MetadataRegistry {
    entries: std::collections::HashMap<CharmId, CharmMetadata>,
}

impl MetadataRegistry {
    /// Create new registry
    pub fn new() -> Self { Self { entries: std::collections::HashMap::new() } }

    /// Create with zkUSD pre-registered
    pub fn with_zkusd(creator: PublicKey, created_at: u64) -> Self {
        let mut reg = Self::new();
        reg.register(CharmMetadata::zkusd(creator, created_at));
        reg
    }

    /// Register metadata
    pub fn register(&mut self, metadata: CharmMetadata) {
        self.entries.insert(metadata.charm_id, metadata);
    }

    /// Get metadata
    pub fn get(&self, charm_id: &CharmId) -> Option<&CharmMetadata> {
        self.entries.get(charm_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::crypto::KeyPair;

    #[test]
    fn test_zkusd_metadata() {
        let kp = KeyPair::generate();
        let meta = CharmMetadata::zkusd(*kp.public_key(), 100);
        assert_eq!(meta.name, "zkUSD");
    }
}
