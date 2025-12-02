//! Common types for spells.

use serde::{Deserialize, Serialize};

use crate::core::token::TokenAmount;
use crate::core::vault::CollateralAmount;
use crate::utils::crypto::{CDPId, Hash, PublicKey, Signature};

/// Spell execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpellResult {
    /// Spell executed successfully
    Success {
        /// Output hash for verification
        output_hash: Hash,
        /// Human-readable message
        message: String,
    },
    /// Spell execution failed
    Failure {
        /// Error code
        code: u32,
        /// Error message
        message: String,
    },
}

impl SpellResult {
    /// Create success result
    pub fn success(output_hash: Hash, message: impl Into<String>) -> Self {
        Self::Success {
            output_hash,
            message: message.into(),
        }
    }

    /// Create failure result
    pub fn failure(code: u32, message: impl Into<String>) -> Self {
        Self::Failure {
            code,
            message: message.into(),
        }
    }

    /// Check if spell succeeded
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }
}

/// Authorization data for spell execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpellAuth {
    /// Signer's public key
    pub signer: PublicKey,
    /// Signature over spell data
    pub signature: Signature,
    /// Nonce to prevent replay
    pub nonce: u64,
}

/// Spell metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpellMeta {
    /// Spell type identifier
    pub spell_type: String,
    /// Version
    pub version: u8,
    /// Block height when spell was created
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

/// Common spell inputs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpellInputs {
    /// CDP ID (if applicable)
    pub cdp_id: Option<CDPId>,
    /// Amount of collateral
    pub collateral: Option<CollateralAmount>,
    /// Amount of zkUSD
    pub debt: Option<TokenAmount>,
    /// BTC price for validation
    pub btc_price: u64,
    /// Price proof hash
    pub price_proof_hash: Hash,
}

/// Common spell outputs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpellOutputs {
    /// New CDP state hash (if modified)
    pub cdp_state_hash: Option<Hash>,
    /// Tokens minted
    pub tokens_minted: Option<TokenAmount>,
    /// Tokens burned
    pub tokens_burned: Option<TokenAmount>,
    /// Collateral deposited
    pub collateral_deposited: Option<CollateralAmount>,
    /// Collateral withdrawn
    pub collateral_withdrawn: Option<CollateralAmount>,
    /// Fee charged
    pub fee: Option<TokenAmount>,
}

/// Base spell trait
pub trait Spell {
    /// Get spell type name
    fn spell_type(&self) -> &'static str;

    /// Validate spell inputs
    fn validate(&self) -> crate::error::Result<()>;

    /// Compute spell hash for signing
    fn hash(&self) -> Hash;

    /// Execute the spell (generates outputs)
    fn execute(&self) -> crate::error::Result<SpellResult>;
}
