//! ZK circuit definitions for zkUSD protocol.
//!
//! These circuits define the constraints that must be satisfied for valid
//! state transitions. They are designed to be compiled to different zkVM
//! targets (SP1, RISC Zero, etc.).

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::utils::constants::{RATIO_PRECISION, SATS_PER_BTC};
use crate::utils::crypto::Hash;
use crate::utils::math::{safe_mul_div, calculate_collateral_ratio};
use crate::zkp::inputs::*;

// ═══════════════════════════════════════════════════════════════════════════════
// CIRCUIT TRAIT
// ═══════════════════════════════════════════════════════════════════════════════

/// Trait for ZK circuit execution
pub trait Circuit: Sized {
    /// Public input type
    type PublicInputs;
    /// Private input type
    type PrivateInputs;
    /// Output type
    type Output;

    /// Execute the circuit and verify constraints
    fn execute(
        public: &Self::PublicInputs,
        private: &Self::PrivateInputs,
    ) -> Result<Self::Output>;

    /// Get the circuit identifier
    fn circuit_id() -> &'static str;

    /// Get the expected constraint count (for gas estimation)
    fn constraint_count() -> usize;
}

// ═══════════════════════════════════════════════════════════════════════════════
// CDP DEPOSIT CIRCUIT
// ═══════════════════════════════════════════════════════════════════════════════

/// Circuit for verifying collateral deposits
pub struct DepositCircuit;

/// Output of deposit circuit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositOutput {
    /// New collateral balance
    pub new_collateral: u64,
    /// Verified state transition hash
    pub transition_hash: Hash,
}

impl Circuit for DepositCircuit {
    type PublicInputs = CDPTransitionPublicInputs;
    type PrivateInputs = CDPPrivateInputs;
    type Output = DepositOutput;

    fn execute(
        public: &Self::PublicInputs,
        private: &Self::PrivateInputs,
    ) -> Result<Self::Output> {
        // Constraint 1: Operation type must be Deposit
        if public.operation_type != OperationType::Deposit as u8 {
            return Err(Error::InvalidParameter {
                name: "operation_type".into(),
                reason: "Must be Deposit".into(),
            });
        }

        // Constraint 2: Collateral must increase
        if private.collateral_after <= private.collateral_before {
            return Err(Error::InvalidParameter {
                name: "collateral".into(),
                reason: "Collateral must increase on deposit".into(),
            });
        }

        // Constraint 3: Debt must not change
        if private.debt_after != private.debt_before {
            return Err(Error::InvalidParameter {
                name: "debt".into(),
                reason: "Debt cannot change on deposit".into(),
            });
        }

        // Constraint 4: Merkle proof must verify (state inclusion)
        if !private.merkle_proof.path.is_empty() && !private.merkle_proof.verify() {
            return Err(Error::InvalidParameter {
                name: "merkle_proof".into(),
                reason: "Invalid state inclusion proof".into(),
            });
        }

        // Compute transition hash
        let deposit_amount = private.collateral_after - private.collateral_before;
        let mut data = Vec::new();
        data.extend_from_slice(public.state_root_before.as_bytes());
        data.extend_from_slice(public.cdp_id.as_bytes());
        data.extend_from_slice(&deposit_amount.to_le_bytes());
        data.extend_from_slice(&public.block_height.to_le_bytes());

        Ok(DepositOutput {
            new_collateral: private.collateral_after,
            transition_hash: Hash::sha256(&data),
        })
    }

    fn circuit_id() -> &'static str {
        "zkusd_deposit_v1"
    }

    fn constraint_count() -> usize {
        1024 // Approximate constraint count
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CDP WITHDRAW CIRCUIT
// ═══════════════════════════════════════════════════════════════════════════════

/// Circuit for verifying collateral withdrawals
pub struct WithdrawCircuit;

/// Output of withdraw circuit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawOutput {
    /// New collateral balance
    pub new_collateral: u64,
    /// New collateralization ratio
    pub new_ratio: u64,
    /// Verified transition hash
    pub transition_hash: Hash,
}

impl Circuit for WithdrawCircuit {
    type PublicInputs = CDPTransitionPublicInputs;
    type PrivateInputs = CDPPrivateInputs;
    type Output = WithdrawOutput;

    fn execute(
        public: &Self::PublicInputs,
        private: &Self::PrivateInputs,
    ) -> Result<Self::Output> {
        // Constraint 1: Operation type must be Withdraw
        if public.operation_type != OperationType::Withdraw as u8 {
            return Err(Error::InvalidParameter {
                name: "operation_type".into(),
                reason: "Must be Withdraw".into(),
            });
        }

        // Constraint 2: Collateral must decrease
        if private.collateral_after >= private.collateral_before {
            return Err(Error::InvalidParameter {
                name: "collateral".into(),
                reason: "Collateral must decrease on withdrawal".into(),
            });
        }

        // Constraint 3: Debt must not change
        if private.debt_after != private.debt_before {
            return Err(Error::InvalidParameter {
                name: "debt".into(),
                reason: "Debt cannot change on withdrawal".into(),
            });
        }

        // Constraint 4: New ratio must be >= MCR (if there's debt)
        let new_ratio = if private.debt_after > 0 {
            calculate_collateral_ratio(
                private.collateral_after,
                private.btc_price,
                private.debt_after,
            )?
        } else {
            u64::MAX
        };

        // Constraint 5: Merkle proof verification
        if !private.merkle_proof.path.is_empty() && !private.merkle_proof.verify() {
            return Err(Error::InvalidParameter {
                name: "merkle_proof".into(),
                reason: "Invalid state inclusion proof".into(),
            });
        }

        // Compute transition hash
        let withdraw_amount = private.collateral_before - private.collateral_after;
        let mut data = Vec::new();
        data.extend_from_slice(public.state_root_before.as_bytes());
        data.extend_from_slice(public.cdp_id.as_bytes());
        data.extend_from_slice(&withdraw_amount.to_le_bytes());
        data.extend_from_slice(&public.block_height.to_le_bytes());

        Ok(WithdrawOutput {
            new_collateral: private.collateral_after,
            new_ratio,
            transition_hash: Hash::sha256(&data),
        })
    }

    fn circuit_id() -> &'static str {
        "zkusd_withdraw_v1"
    }

    fn constraint_count() -> usize {
        2048 // More constraints due to ratio calculation
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CDP MINT CIRCUIT
// ═══════════════════════════════════════════════════════════════════════════════

/// Circuit for verifying debt minting
pub struct MintCircuit;

/// Output of mint circuit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintOutput {
    /// Amount minted
    pub amount_minted: u64,
    /// New debt balance
    pub new_debt: u64,
    /// New collateralization ratio
    pub new_ratio: u64,
    /// Transition hash
    pub transition_hash: Hash,
}

impl Circuit for MintCircuit {
    type PublicInputs = CDPTransitionPublicInputs;
    type PrivateInputs = CDPPrivateInputs;
    type Output = MintOutput;

    fn execute(
        public: &Self::PublicInputs,
        private: &Self::PrivateInputs,
    ) -> Result<Self::Output> {
        // Constraint 1: Operation type must be Mint
        if public.operation_type != OperationType::Mint as u8 {
            return Err(Error::InvalidParameter {
                name: "operation_type".into(),
                reason: "Must be Mint".into(),
            });
        }

        // Constraint 2: Debt must increase
        if private.debt_after <= private.debt_before {
            return Err(Error::InvalidParameter {
                name: "debt".into(),
                reason: "Debt must increase on mint".into(),
            });
        }

        // Constraint 3: Collateral must not change
        if private.collateral_after != private.collateral_before {
            return Err(Error::InvalidParameter {
                name: "collateral".into(),
                reason: "Collateral cannot change on mint".into(),
            });
        }

        // Constraint 4: New ratio must be >= MCR
        let new_ratio = calculate_collateral_ratio(
            private.collateral_after,
            private.btc_price,
            private.debt_after,
        )?;

        // Constraint 5: Merkle proof
        if !private.merkle_proof.path.is_empty() && !private.merkle_proof.verify() {
            return Err(Error::InvalidParameter {
                name: "merkle_proof".into(),
                reason: "Invalid state inclusion proof".into(),
            });
        }

        let amount_minted = private.debt_after - private.debt_before;

        // Compute transition hash
        let mut data = Vec::new();
        data.extend_from_slice(public.state_root_before.as_bytes());
        data.extend_from_slice(public.cdp_id.as_bytes());
        data.extend_from_slice(&amount_minted.to_le_bytes());
        data.extend_from_slice(&public.block_height.to_le_bytes());

        Ok(MintOutput {
            amount_minted,
            new_debt: private.debt_after,
            new_ratio,
            transition_hash: Hash::sha256(&data),
        })
    }

    fn circuit_id() -> &'static str {
        "zkusd_mint_v1"
    }

    fn constraint_count() -> usize {
        2048
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CDP REPAY CIRCUIT
// ═══════════════════════════════════════════════════════════════════════════════

/// Circuit for verifying debt repayment
pub struct RepayCircuit;

/// Output of repay circuit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepayOutput {
    /// Amount repaid
    pub amount_repaid: u64,
    /// New debt balance
    pub new_debt: u64,
    /// Transition hash
    pub transition_hash: Hash,
}

impl Circuit for RepayCircuit {
    type PublicInputs = CDPTransitionPublicInputs;
    type PrivateInputs = CDPPrivateInputs;
    type Output = RepayOutput;

    fn execute(
        public: &Self::PublicInputs,
        private: &Self::PrivateInputs,
    ) -> Result<Self::Output> {
        // Constraint 1: Operation type must be Repay
        if public.operation_type != OperationType::Repay as u8 {
            return Err(Error::InvalidParameter {
                name: "operation_type".into(),
                reason: "Must be Repay".into(),
            });
        }

        // Constraint 2: Debt must decrease
        if private.debt_after >= private.debt_before {
            return Err(Error::InvalidParameter {
                name: "debt".into(),
                reason: "Debt must decrease on repay".into(),
            });
        }

        // Constraint 3: Collateral must not change
        if private.collateral_after != private.collateral_before {
            return Err(Error::InvalidParameter {
                name: "collateral".into(),
                reason: "Collateral cannot change on repay".into(),
            });
        }

        // Constraint 4: Merkle proof
        if !private.merkle_proof.path.is_empty() && !private.merkle_proof.verify() {
            return Err(Error::InvalidParameter {
                name: "merkle_proof".into(),
                reason: "Invalid state inclusion proof".into(),
            });
        }

        let amount_repaid = private.debt_before - private.debt_after;

        // Compute transition hash
        let mut data = Vec::new();
        data.extend_from_slice(public.state_root_before.as_bytes());
        data.extend_from_slice(public.cdp_id.as_bytes());
        data.extend_from_slice(&amount_repaid.to_le_bytes());
        data.extend_from_slice(&public.block_height.to_le_bytes());

        Ok(RepayOutput {
            amount_repaid,
            new_debt: private.debt_after,
            transition_hash: Hash::sha256(&data),
        })
    }

    fn circuit_id() -> &'static str {
        "zkusd_repay_v1"
    }

    fn constraint_count() -> usize {
        1024
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// LIQUIDATION CIRCUIT
// ═══════════════════════════════════════════════════════════════════════════════

/// Circuit for verifying liquidations
pub struct LiquidationCircuit;

/// Output of liquidation circuit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationOutput {
    /// Ratio at liquidation (proving it was below MCR)
    pub ratio_at_liquidation: u64,
    /// Debt covered
    pub debt_covered: u64,
    /// Collateral seized
    pub collateral_seized: u64,
    /// Transition hash
    pub transition_hash: Hash,
}

impl Circuit for LiquidationCircuit {
    type PublicInputs = LiquidationPublicInputs;
    type PrivateInputs = LiquidationPrivateInputs;
    type Output = LiquidationOutput;

    fn execute(
        public: &Self::PublicInputs,
        private: &Self::PrivateInputs,
    ) -> Result<Self::Output> {
        // Constraint 1: CDP must be undercollateralized (ratio < MCR)
        let ratio = calculate_collateral_ratio(
            private.collateral,
            public.btc_price,
            private.debt,
        )?;

        if ratio >= public.mcr {
            return Err(Error::InvalidParameter {
                name: "ratio".into(),
                reason: format!(
                    "CDP ratio {} is above MCR {}, cannot liquidate",
                    ratio, public.mcr
                ),
            });
        }

        // Constraint 2: Debt covered matches public input
        if public.debt_covered != private.debt {
            return Err(Error::InvalidParameter {
                name: "debt_covered".into(),
                reason: "Debt covered mismatch".into(),
            });
        }

        // Constraint 3: Collateral seized calculation
        // collateral_seized = (debt / price) * (1 + bonus)
        let expected_collateral = safe_mul_div(
            private.debt,
            SATS_PER_BTC,
            public.btc_price,
        )?;

        // Allow some margin for rounding
        let max_collateral = expected_collateral.saturating_mul(115).saturating_div(100);
        if public.collateral_seized > max_collateral || public.collateral_seized > private.collateral {
            return Err(Error::InvalidParameter {
                name: "collateral_seized".into(),
                reason: "Collateral seized exceeds allowed amount".into(),
            });
        }

        // Constraint 4: Merkle proof
        if !private.merkle_proof.path.is_empty() && !private.merkle_proof.verify() {
            return Err(Error::InvalidParameter {
                name: "merkle_proof".into(),
                reason: "Invalid state inclusion proof".into(),
            });
        }

        // Compute transition hash
        let mut data = Vec::new();
        data.extend_from_slice(public.state_root_before.as_bytes());
        data.extend_from_slice(public.cdp_id.as_bytes());
        data.extend_from_slice(&public.debt_covered.to_le_bytes());
        data.extend_from_slice(&public.collateral_seized.to_le_bytes());
        data.extend_from_slice(&public.block_height.to_le_bytes());

        Ok(LiquidationOutput {
            ratio_at_liquidation: ratio,
            debt_covered: public.debt_covered,
            collateral_seized: public.collateral_seized,
            transition_hash: Hash::sha256(&data),
        })
    }

    fn circuit_id() -> &'static str {
        "zkusd_liquidation_v1"
    }

    fn constraint_count() -> usize {
        4096
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRICE ATTESTATION CIRCUIT
// ═══════════════════════════════════════════════════════════════════════════════

/// Circuit for verifying price attestations
pub struct PriceAttestationCircuit;

/// Output of price attestation circuit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceAttestationOutput {
    /// Verified price
    pub price: u64,
    /// Median price from sources
    pub median_price: u64,
    /// Attestation hash
    pub attestation_hash: Hash,
}

impl Circuit for PriceAttestationCircuit {
    type PublicInputs = PriceAttestationPublicInputs;
    type PrivateInputs = PricePrivateInputs;
    type Output = PriceAttestationOutput;

    fn execute(
        public: &Self::PublicInputs,
        private: &Self::PrivateInputs,
    ) -> Result<Self::Output> {
        // Constraint 1: Must have minimum sources
        if private.source_prices.len() < public.source_count as usize {
            return Err(Error::InvalidParameter {
                name: "source_count".into(),
                reason: "Insufficient price sources".into(),
            });
        }

        if private.source_prices.is_empty() {
            return Err(Error::InvalidParameter {
                name: "source_prices".into(),
                reason: "No price sources provided".into(),
            });
        }

        // Constraint 2: Calculate median price
        let mut prices: Vec<u64> = private.source_prices.iter().map(|s| s.price).collect();
        prices.sort_unstable();

        let median = if prices.len() % 2 == 0 {
            let mid = prices.len() / 2;
            (prices[mid - 1] + prices[mid]) / 2
        } else {
            prices[prices.len() / 2]
        };

        // Constraint 3: Public price must match median (within tolerance)
        let deviation = if public.price > median {
            ((public.price - median) as u128 * 10000 / median as u128) as u16
        } else {
            ((median - public.price) as u128 * 10000 / median as u128) as u16
        };

        if deviation > public.deviation_bps {
            return Err(Error::InvalidParameter {
                name: "price".into(),
                reason: format!(
                    "Price deviation {} exceeds allowed {}",
                    deviation, public.deviation_bps
                ),
            });
        }

        // Constraint 4: Timestamps must be recent
        for source in &private.source_prices {
            if public.timestamp > source.timestamp &&
               public.timestamp - source.timestamp > 300 {
                return Err(Error::InvalidParameter {
                    name: "timestamp".into(),
                    reason: "Source price too stale".into(),
                });
            }
        }

        // Compute attestation hash
        let mut data = Vec::new();
        data.extend_from_slice(&public.price.to_le_bytes());
        data.extend_from_slice(&public.timestamp.to_le_bytes());
        data.push(public.source_count);
        data.extend_from_slice(public.oracle_pubkey.as_bytes());

        Ok(PriceAttestationOutput {
            price: public.price,
            median_price: median,
            attestation_hash: Hash::sha256(&data),
        })
    }

    fn circuit_id() -> &'static str {
        "zkusd_price_attestation_v1"
    }

    fn constraint_count() -> usize {
        512
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CIRCUIT REGISTRY
// ═══════════════════════════════════════════════════════════════════════════════

/// Registry of all available circuits
#[derive(Debug, Clone)]
pub struct CircuitRegistry {
    circuits: Vec<CircuitInfo>,
}

/// Information about a circuit
#[derive(Debug, Clone)]
pub struct CircuitInfo {
    /// Circuit identifier
    pub id: &'static str,
    /// Circuit version
    pub version: u32,
    /// Approximate constraint count
    pub constraints: usize,
    /// Description
    pub description: &'static str,
}

impl Default for CircuitRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CircuitRegistry {
    /// Create registry with all circuits
    pub fn new() -> Self {
        Self {
            circuits: vec![
                CircuitInfo {
                    id: DepositCircuit::circuit_id(),
                    version: 1,
                    constraints: DepositCircuit::constraint_count(),
                    description: "Verify collateral deposits",
                },
                CircuitInfo {
                    id: WithdrawCircuit::circuit_id(),
                    version: 1,
                    constraints: WithdrawCircuit::constraint_count(),
                    description: "Verify collateral withdrawals with MCR check",
                },
                CircuitInfo {
                    id: MintCircuit::circuit_id(),
                    version: 1,
                    constraints: MintCircuit::constraint_count(),
                    description: "Verify debt minting with MCR check",
                },
                CircuitInfo {
                    id: RepayCircuit::circuit_id(),
                    version: 1,
                    constraints: RepayCircuit::constraint_count(),
                    description: "Verify debt repayment",
                },
                CircuitInfo {
                    id: LiquidationCircuit::circuit_id(),
                    version: 1,
                    constraints: LiquidationCircuit::constraint_count(),
                    description: "Verify CDP liquidation",
                },
                CircuitInfo {
                    id: PriceAttestationCircuit::circuit_id(),
                    version: 1,
                    constraints: PriceAttestationCircuit::constraint_count(),
                    description: "Verify oracle price attestation",
                },
            ],
        }
    }

    /// Get all registered circuits
    pub fn circuits(&self) -> &[CircuitInfo] {
        &self.circuits
    }

    /// Find circuit by ID
    pub fn find(&self, id: &str) -> Option<&CircuitInfo> {
        self.circuits.iter().find(|c| c.id == id)
    }

    /// Total constraint count for all circuits
    pub fn total_constraints(&self) -> usize {
        self.circuits.iter().map(|c| c.constraints).sum()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cdp::CDPId;
    use crate::utils::crypto::{KeyPair, Signature};

    fn test_keypair() -> KeyPair {
        KeyPair::generate()
    }

    #[test]
    fn test_deposit_circuit() {
        let keypair = test_keypair();
        let cdp_id = CDPId::generate(keypair.public_key(), 1);

        let public = CDPTransitionPublicInputs {
            state_root_before: Hash::sha256(b"before"),
            state_root_after: Hash::sha256(b"after"),
            cdp_id,
            operation_type: OperationType::Deposit as u8,
            block_height: 100,
            timestamp: 1234567890,
        };

        let private = CDPPrivateInputs {
            owner: *keypair.public_key(),
            collateral_before: 0,
            collateral_after: 100_000_000, // 1 BTC
            debt_before: 0,
            debt_after: 0,
            signature: Signature::new([0u8; 64]),
            nonce: 1,
            btc_price: 10_000_000, // $100k
            merkle_proof: MerkleProof::empty(),
        };

        let result = DepositCircuit::execute(&public, &private);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert_eq!(output.new_collateral, 100_000_000);
    }

    #[test]
    fn test_deposit_circuit_wrong_operation() {
        let keypair = test_keypair();
        let cdp_id = CDPId::generate(keypair.public_key(), 1);

        let public = CDPTransitionPublicInputs {
            state_root_before: Hash::sha256(b"before"),
            state_root_after: Hash::sha256(b"after"),
            cdp_id,
            operation_type: OperationType::Withdraw as u8, // Wrong!
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

        let result = DepositCircuit::execute(&public, &private);
        assert!(result.is_err());
    }

    #[test]
    fn test_mint_circuit() {
        let keypair = test_keypair();
        let cdp_id = CDPId::generate(keypair.public_key(), 1);

        let public = CDPTransitionPublicInputs {
            state_root_before: Hash::sha256(b"before"),
            state_root_after: Hash::sha256(b"after"),
            cdp_id,
            operation_type: OperationType::Mint as u8,
            block_height: 100,
            timestamp: 1234567890,
        };

        // 1 BTC collateral at $100k, minting $50k (200% ratio)
        let private = CDPPrivateInputs {
            owner: *keypair.public_key(),
            collateral_before: 100_000_000,
            collateral_after: 100_000_000,
            debt_before: 0,
            debt_after: 5_000_000, // $50k
            signature: Signature::new([0u8; 64]),
            nonce: 1,
            btc_price: 10_000_000, // $100k
            merkle_proof: MerkleProof::empty(),
        };

        let result = MintCircuit::execute(&public, &private);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert_eq!(output.amount_minted, 5_000_000);
        assert_eq!(output.new_debt, 5_000_000);
        assert_eq!(output.new_ratio, 200 * RATIO_PRECISION / 100); // 200%
    }

    #[test]
    fn test_liquidation_circuit() {
        let keypair = test_keypair();
        let cdp_id = CDPId::generate(keypair.public_key(), 1);

        // CDP with 100% ratio (below 150% MCR)
        let public = LiquidationPublicInputs {
            state_root_before: Hash::sha256(b"before"),
            state_root_after: Hash::sha256(b"after"),
            cdp_id,
            btc_price: 5_000_000, // $50k (price dropped!)
            mcr: 150 * RATIO_PRECISION / 100, // 150%
            debt_covered: 5_000_000, // $50k
            collateral_seized: 100_000_000, // 1 BTC
            block_height: 100,
        };

        let private = LiquidationPrivateInputs {
            cdp_owner: *keypair.public_key(),
            collateral: 100_000_000,
            debt: 5_000_000,
            ratio: 100 * RATIO_PRECISION / 100, // 100%
            liquidator: *test_keypair().public_key(),
            liquidator_signature: Signature::new([0u8; 64]),
            merkle_proof: MerkleProof::empty(),
            sp_total_deposits: Some(1_000_000_000),
        };

        let result = LiquidationCircuit::execute(&public, &private);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ratio_at_liquidation < public.mcr);
    }

    #[test]
    fn test_liquidation_circuit_healthy_cdp() {
        let keypair = test_keypair();
        let cdp_id = CDPId::generate(keypair.public_key(), 1);

        // CDP with 200% ratio (above 150% MCR)
        let public = LiquidationPublicInputs {
            state_root_before: Hash::sha256(b"before"),
            state_root_after: Hash::sha256(b"after"),
            cdp_id,
            btc_price: 10_000_000, // $100k
            mcr: 150 * RATIO_PRECISION / 100,
            debt_covered: 5_000_000,
            collateral_seized: 100_000_000,
            block_height: 100,
        };

        let private = LiquidationPrivateInputs {
            cdp_owner: *keypair.public_key(),
            collateral: 100_000_000,
            debt: 5_000_000, // 200% ratio at $100k
            ratio: 200 * RATIO_PRECISION / 100,
            liquidator: *test_keypair().public_key(),
            liquidator_signature: Signature::new([0u8; 64]),
            merkle_proof: MerkleProof::empty(),
            sp_total_deposits: None,
        };

        let result = LiquidationCircuit::execute(&public, &private);
        assert!(result.is_err()); // Should fail - CDP is healthy
    }

    #[test]
    fn test_circuit_registry() {
        let registry = CircuitRegistry::new();

        assert!(!registry.circuits().is_empty());
        assert!(registry.find("zkusd_deposit_v1").is_some());
        assert!(registry.find("nonexistent").is_none());
        assert!(registry.total_constraints() > 0);
    }
}
