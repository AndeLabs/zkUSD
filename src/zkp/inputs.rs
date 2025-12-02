//! ZK proof inputs - public and private data for circuit execution.
//!
//! These structures define what data is committed to publicly (verifiable)
//! and what remains private (not revealed to verifiers).

use serde::{Deserialize, Serialize};

use crate::core::cdp::CDPId;
use crate::core::token::TokenAmount;
use crate::core::vault::CollateralAmount;
use crate::utils::crypto::{Hash, PublicKey, Signature};

// ═══════════════════════════════════════════════════════════════════════════════
// PUBLIC INPUTS - Committed to the blockchain
// ═══════════════════════════════════════════════════════════════════════════════

/// Public inputs for CDP state transition proofs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CDPTransitionPublicInputs {
    /// Hash of state before transition
    pub state_root_before: Hash,
    /// Hash of state after transition
    pub state_root_after: Hash,
    /// CDP identifier
    pub cdp_id: CDPId,
    /// Operation type code
    pub operation_type: u8,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

impl CDPTransitionPublicInputs {
    /// Encode as bytes for commitment
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    /// Compute hash of public inputs
    pub fn hash(&self) -> Hash {
        Hash::sha256(&self.encode())
    }
}

/// Public inputs for liquidation proofs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationPublicInputs {
    /// State root before liquidation
    pub state_root_before: Hash,
    /// State root after liquidation
    pub state_root_after: Hash,
    /// CDP being liquidated
    pub cdp_id: CDPId,
    /// BTC price at liquidation (cents)
    pub btc_price: u64,
    /// Minimum collateralization ratio used
    pub mcr: u64,
    /// Debt amount covered (cents)
    pub debt_covered: u64,
    /// Collateral seized (sats)
    pub collateral_seized: u64,
    /// Block height
    pub block_height: u64,
}

impl LiquidationPublicInputs {
    /// Encode as bytes
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    /// Compute hash
    pub fn hash(&self) -> Hash {
        Hash::sha256(&self.encode())
    }
}

/// Public inputs for redemption proofs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedemptionPublicInputs {
    /// State root before redemption
    pub state_root_before: Hash,
    /// State root after redemption
    pub state_root_after: Hash,
    /// Redeemer's public key
    pub redeemer: PublicKey,
    /// zkUSD redeemed (cents)
    pub amount_redeemed: u64,
    /// Collateral received (sats)
    pub collateral_received: u64,
    /// Fee paid (cents)
    pub fee_paid: u64,
    /// BTC price used
    pub btc_price: u64,
    /// Number of CDPs affected
    pub cdps_affected: u32,
    /// Block height
    pub block_height: u64,
}

impl RedemptionPublicInputs {
    /// Encode as bytes
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    /// Compute hash
    pub fn hash(&self) -> Hash {
        Hash::sha256(&self.encode())
    }
}

/// Public inputs for price oracle attestation proofs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceAttestationPublicInputs {
    /// BTC price (cents)
    pub price: u64,
    /// Timestamp of attestation
    pub timestamp: u64,
    /// Number of sources aggregated
    pub source_count: u8,
    /// Median deviation from mean (basis points)
    pub deviation_bps: u16,
    /// Oracle aggregator public key
    pub oracle_pubkey: PublicKey,
    /// Signature over price data
    pub signature: Signature,
}

impl PriceAttestationPublicInputs {
    /// Encode as bytes
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    /// Compute hash
    pub fn hash(&self) -> Hash {
        Hash::sha256(&self.encode())
    }

    /// Get the price data that was signed
    pub fn signed_data(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(24);
        data.extend_from_slice(&self.price.to_le_bytes());
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data.push(self.source_count);
        data.extend_from_slice(&self.deviation_bps.to_le_bytes());
        data
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRIVATE INPUTS - Known only to the prover
// ═══════════════════════════════════════════════════════════════════════════════

/// Private inputs for CDP operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CDPPrivateInputs {
    /// Owner's public key
    pub owner: PublicKey,
    /// Current collateral (sats)
    pub collateral_before: u64,
    /// Collateral after operation (sats)
    pub collateral_after: u64,
    /// Current debt (cents)
    pub debt_before: u64,
    /// Debt after operation (cents)
    pub debt_after: u64,
    /// Operation signature
    pub signature: Signature,
    /// Nonce used
    pub nonce: u64,
    /// BTC price (for ratio calculations)
    pub btc_price: u64,
    /// Merkle proof of CDP in state tree
    pub merkle_proof: MerkleProof,
}

/// Private inputs for liquidation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationPrivateInputs {
    /// CDP owner
    pub cdp_owner: PublicKey,
    /// CDP collateral before
    pub collateral: u64,
    /// CDP debt before
    pub debt: u64,
    /// Collateralization ratio (proving it's below MCR)
    pub ratio: u64,
    /// Liquidator's public key
    pub liquidator: PublicKey,
    /// Liquidator's signature
    pub liquidator_signature: Signature,
    /// Merkle proof of CDP
    pub merkle_proof: MerkleProof,
    /// Stability pool state (if used)
    pub sp_total_deposits: Option<u64>,
}

/// Private inputs for redemption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedemptionPrivateInputs {
    /// Redeemer's signature
    pub signature: Signature,
    /// Nonce
    pub nonce: u64,
    /// CDPs being redeemed from
    pub cdps: Vec<CDPRedemptionData>,
    /// Fee rate (basis points)
    pub fee_bps: u64,
}

/// Data for a single CDP in redemption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CDPRedemptionData {
    /// CDP ID
    pub cdp_id: CDPId,
    /// CDP owner
    pub owner: PublicKey,
    /// Debt before
    pub debt_before: u64,
    /// Debt redeemed
    pub debt_redeemed: u64,
    /// Collateral before
    pub collateral_before: u64,
    /// Collateral taken
    pub collateral_taken: u64,
    /// Merkle proof
    pub merkle_proof: MerkleProof,
}

/// Private inputs for price attestation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricePrivateInputs {
    /// Individual source prices
    pub source_prices: Vec<SourcePrice>,
    /// Oracle signing key (private - will sign the attestation)
    pub oracle_signature_data: Vec<u8>,
}

/// Single price source data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcePrice {
    /// Exchange/source identifier
    pub source_id: u8,
    /// Price from this source (cents)
    pub price: u64,
    /// Timestamp from source
    pub timestamp: u64,
    /// Weight for aggregation
    pub weight: u8,
}

// ═══════════════════════════════════════════════════════════════════════════════
// MERKLE PROOFS
// ═══════════════════════════════════════════════════════════════════════════════

/// Merkle proof for state inclusion
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MerkleProof {
    /// Leaf hash (item being proved)
    pub leaf: Hash,
    /// Path from leaf to root
    pub path: Vec<MerkleNode>,
    /// Root hash
    pub root: Hash,
}

impl MerkleProof {
    /// Create an empty proof
    pub fn empty() -> Self {
        Self {
            leaf: Hash::zero(),
            path: Vec::new(),
            root: Hash::zero(),
        }
    }

    /// Verify the merkle proof
    ///
    /// `is_left` in MerkleNode indicates if the sibling is on the left side:
    /// - `is_left: true` → hash(sibling || current)
    /// - `is_left: false` → hash(current || sibling)
    pub fn verify(&self) -> bool {
        if self.path.is_empty() {
            return self.leaf == self.root;
        }

        let mut current = self.leaf;
        for node in &self.path {
            current = if node.is_left {
                // Sibling is on left, current is on right
                Hash::sha256(&[node.hash.as_bytes().as_slice(), current.as_bytes()].concat())
            } else {
                // Current is on left, sibling is on right
                Hash::sha256(&[current.as_bytes().as_slice(), node.hash.as_bytes()].concat())
            };
        }

        current == self.root
    }

    /// Get the proof depth
    pub fn depth(&self) -> usize {
        self.path.len()
    }
}

/// Single node in a merkle proof path
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleNode {
    /// Hash of sibling node
    pub hash: Hash,
    /// Whether this sibling is on the left
    pub is_left: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMBINED PROOF INPUTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Operation type codes for CDP transitions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum OperationType {
    /// Open a new CDP
    OpenCDP = 1,
    /// Deposit collateral
    Deposit = 2,
    /// Withdraw collateral
    Withdraw = 3,
    /// Mint debt (borrow)
    Mint = 4,
    /// Repay debt
    Repay = 5,
    /// Close CDP
    Close = 6,
    /// Liquidate CDP
    Liquidate = 7,
    /// Redeem zkUSD
    Redeem = 8,
    /// Transfer tokens
    Transfer = 9,
    /// Stability pool deposit
    SPDeposit = 10,
    /// Stability pool withdraw
    SPWithdraw = 11,
    /// Claim stability gains
    ClaimGains = 12,
}

impl From<u8> for OperationType {
    fn from(v: u8) -> Self {
        match v {
            1 => Self::OpenCDP,
            2 => Self::Deposit,
            3 => Self::Withdraw,
            4 => Self::Mint,
            5 => Self::Repay,
            6 => Self::Close,
            7 => Self::Liquidate,
            8 => Self::Redeem,
            9 => Self::Transfer,
            10 => Self::SPDeposit,
            11 => Self::SPWithdraw,
            12 => Self::ClaimGains,
            _ => Self::OpenCDP,
        }
    }
}

impl From<OperationType> for u8 {
    fn from(op: OperationType) -> Self {
        op as u8
    }
}

/// Complete proof inputs bundle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofInputs {
    /// Type of proof
    pub proof_type: ProofType,
    /// Public inputs (committed to verifier)
    pub public_data: Vec<u8>,
    /// Private inputs (known only to prover)
    pub private_data: Vec<u8>,
}

/// Type of proof being generated
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProofType {
    /// CDP state transition
    CDPTransition,
    /// Liquidation
    Liquidation,
    /// Redemption
    Redemption,
    /// Price attestation
    PriceAttestation,
    /// Batch of operations
    Batch,
}

impl ProofInputs {
    /// Create CDP transition inputs
    pub fn cdp_transition(
        public: CDPTransitionPublicInputs,
        private: CDPPrivateInputs,
    ) -> Self {
        Self {
            proof_type: ProofType::CDPTransition,
            public_data: public.encode(),
            private_data: bincode::serialize(&private).unwrap_or_default(),
        }
    }

    /// Create liquidation inputs
    pub fn liquidation(
        public: LiquidationPublicInputs,
        private: LiquidationPrivateInputs,
    ) -> Self {
        Self {
            proof_type: ProofType::Liquidation,
            public_data: public.encode(),
            private_data: bincode::serialize(&private).unwrap_or_default(),
        }
    }

    /// Create redemption inputs
    pub fn redemption(
        public: RedemptionPublicInputs,
        private: RedemptionPrivateInputs,
    ) -> Self {
        Self {
            proof_type: ProofType::Redemption,
            public_data: public.encode(),
            private_data: bincode::serialize(&private).unwrap_or_default(),
        }
    }

    /// Create price attestation inputs
    pub fn price_attestation(
        public: PriceAttestationPublicInputs,
        private: PricePrivateInputs,
    ) -> Self {
        Self {
            proof_type: ProofType::PriceAttestation,
            public_data: public.encode(),
            private_data: bincode::serialize(&private).unwrap_or_default(),
        }
    }

    /// Get public inputs hash
    pub fn public_hash(&self) -> Hash {
        Hash::sha256(&self.public_data)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::crypto::KeyPair;

    #[test]
    fn test_merkle_proof_empty() {
        let proof = MerkleProof::empty();
        assert!(proof.leaf.is_zero());
        assert!(proof.path.is_empty());
    }

    #[test]
    fn test_merkle_proof_single_leaf() {
        let leaf = Hash::sha256(b"test data");
        let proof = MerkleProof {
            leaf,
            path: Vec::new(),
            root: leaf,
        };
        assert!(proof.verify());
    }

    #[test]
    fn test_merkle_proof_with_path() {
        let leaf = Hash::sha256(b"leaf");
        let sibling = Hash::sha256(b"sibling");

        // Calculate expected root
        let combined = [leaf.as_bytes().as_slice(), sibling.as_bytes()].concat();
        let root = Hash::sha256(&combined);

        let proof = MerkleProof {
            leaf,
            path: vec![MerkleNode {
                hash: sibling,
                is_left: false,
            }],
            root,
        };

        assert!(proof.verify());
    }

    #[test]
    fn test_operation_type_conversion() {
        assert_eq!(OperationType::from(1u8), OperationType::OpenCDP);
        assert_eq!(OperationType::from(7u8), OperationType::Liquidate);
        assert_eq!(u8::from(OperationType::Mint), 4);
    }

    #[test]
    fn test_cdp_transition_inputs() {
        let public = CDPTransitionPublicInputs {
            state_root_before: Hash::sha256(b"before"),
            state_root_after: Hash::sha256(b"after"),
            cdp_id: CDPId::new([0u8; 32]),
            operation_type: OperationType::Deposit as u8,
            block_height: 100,
            timestamp: 1234567890,
        };

        let encoded = public.encode();
        assert!(!encoded.is_empty());

        let hash = public.hash();
        assert!(!hash.is_zero());
    }

    #[test]
    fn test_proof_inputs_creation() {
        let keypair = KeyPair::generate();

        let public = CDPTransitionPublicInputs {
            state_root_before: Hash::sha256(b"before"),
            state_root_after: Hash::sha256(b"after"),
            cdp_id: CDPId::generate(keypair.public_key(), 1),
            operation_type: OperationType::Deposit as u8,
            block_height: 100,
            timestamp: 1234567890,
        };

        let private = CDPPrivateInputs {
            owner: *keypair.public_key(),
            collateral_before: 0,
            collateral_after: 100_000_000,
            debt_before: 0,
            debt_after: 0,
            signature: Signature::new([0u8; 64]),
            nonce: 1,
            btc_price: 10_000_000,
            merkle_proof: MerkleProof::empty(),
        };

        let inputs = ProofInputs::cdp_transition(public, private);
        assert_eq!(inputs.proof_type, ProofType::CDPTransition);
        assert!(!inputs.public_hash().is_zero());
    }
}
