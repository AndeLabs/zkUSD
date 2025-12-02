//! Common types and utilities for zkUSD guest programs.
//!
//! These types are serialization-compatible with the host program types.

use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};

/// Hash type (32 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hash([u8; 32]);

impl Hash {
    pub fn sha256(data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&result);
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// CDP ID type (32 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CDPId([u8; 32]);

impl CDPId {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Public key type (33 bytes compressed)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicKey([u8; 33]);

/// Signature type (64 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature([u8; 64]);

/// Merkle proof placeholder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    pub siblings: Vec<Hash>,
    pub path: Vec<bool>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// CDP TRANSITION TYPES
// ═══════════════════════════════════════════════════════════════════════════════

/// Operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OperationType {
    Deposit = 1,
    Withdraw = 2,
    Mint = 3,
    Repay = 4,
    Liquidate = 5,
    Redeem = 6,
}

impl From<u8> for OperationType {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Deposit,
            2 => Self::Withdraw,
            3 => Self::Mint,
            4 => Self::Repay,
            5 => Self::Liquidate,
            6 => Self::Redeem,
            _ => Self::Deposit, // Default
        }
    }
}

/// Public inputs for CDP transitions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CDPTransitionPublicInputs {
    pub state_root_before: Hash,
    pub state_root_after: Hash,
    pub cdp_id: CDPId,
    pub operation_type: u8,
    pub block_height: u64,
    pub timestamp: u64,
}

/// Private inputs for CDP transitions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CDPPrivateInputs {
    pub owner: PublicKey,
    pub collateral_before: u64,
    pub collateral_after: u64,
    pub debt_before: u64,
    pub debt_after: u64,
    pub signature: Signature,
    pub nonce: u64,
    pub btc_price: u64,
    pub merkle_proof: MerkleProof,
}

/// Circuit output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitOutput {
    pub valid: bool,
    pub transition_hash: Hash,
    pub new_state_root: Hash,
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROTOCOL CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Minimum collateral ratio (150% = 15000 BPS)
pub const MIN_COLLATERAL_RATIO_BPS: u64 = 15000;

/// Basis points divisor
pub const BPS_DIVISOR: u64 = 10000;

/// Satoshis per BTC
pub const SATS_PER_BTC: u64 = 100_000_000;

// ═══════════════════════════════════════════════════════════════════════════════
// UTILITY FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Calculate collateral ratio
pub fn calculate_ratio(collateral_sats: u64, debt_cents: u64, btc_price_cents: u64) -> u64 {
    if debt_cents == 0 {
        return u64::MAX;
    }

    // collateral_value_cents = (collateral_sats * btc_price_cents) / SATS_PER_BTC
    // ratio_bps = (collateral_value_cents * BPS_DIVISOR) / debt_cents

    let collateral_value = (collateral_sats as u128 * btc_price_cents as u128) / SATS_PER_BTC as u128;
    let ratio = (collateral_value * BPS_DIVISOR as u128) / debt_cents as u128;

    ratio as u64
}

/// Safe multiplication and division
pub fn safe_mul_div(a: u64, b: u64, c: u64) -> Option<u64> {
    if c == 0 {
        return None;
    }
    let result = (a as u128 * b as u128) / c as u128;
    if result > u64::MAX as u128 {
        None
    } else {
        Some(result as u64)
    }
}
