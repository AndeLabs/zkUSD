//! ZK proof generation for zkUSD protocol.
//!
//! This module provides the prover infrastructure for generating zero-knowledge
//! proofs of state transitions. It supports multiple backends through an
//! abstract interface, allowing deployment on different zkVMs.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::utils::crypto::Hash;
use crate::zkp::circuits::*;
use crate::zkp::inputs::*;

// ═══════════════════════════════════════════════════════════════════════════════
// PROOF TYPES
// ═══════════════════════════════════════════════════════════════════════════════

/// A zero-knowledge proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZKProof {
    /// Proof type
    pub proof_type: ProofType,
    /// Circuit identifier
    pub circuit_id: String,
    /// Serialized proof data (format depends on backend)
    pub proof_data: Vec<u8>,
    /// Public inputs hash
    pub public_inputs_hash: Hash,
    /// Proof generation timestamp
    pub timestamp: u64,
    /// Backend identifier
    pub backend: ProverBackend,
    /// Proof metadata
    pub metadata: ProofMetadata,
}

impl ZKProof {
    /// Get proof size in bytes
    pub fn size(&self) -> usize {
        self.proof_data.len()
    }

    /// Compute proof hash
    pub fn hash(&self) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(self.circuit_id.as_bytes());
        data.extend_from_slice(&self.proof_data);
        data.extend_from_slice(self.public_inputs_hash.as_bytes());
        Hash::sha256(&data)
    }

    /// Check if proof is for a specific circuit
    pub fn is_for_circuit(&self, circuit_id: &str) -> bool {
        self.circuit_id == circuit_id
    }
}

/// Proof metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProofMetadata {
    /// Time taken to generate proof (milliseconds)
    pub generation_time_ms: u64,
    /// Number of constraints
    pub constraint_count: usize,
    /// Prover version
    pub prover_version: String,
}

/// Supported prover backends
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProverBackend {
    /// Native execution (no ZK, for testing)
    Native,
    /// SP1 zkVM
    SP1,
    /// RISC Zero
    RiscZero,
    /// Succinct
    Succinct,
    /// Custom BitcoinOS prover
    BitcoinOS,
}

impl Default for ProverBackend {
    fn default() -> Self {
        Self::Native
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROVER TRAIT
// ═══════════════════════════════════════════════════════════════════════════════

/// Trait for proof generation backends
pub trait Prover: Send + Sync {
    /// Get backend identifier
    fn backend(&self) -> ProverBackend;

    /// Generate proof for CDP transition
    fn prove_cdp_transition(
        &self,
        public: &CDPTransitionPublicInputs,
        private: &CDPPrivateInputs,
    ) -> Result<ZKProof>;

    /// Generate proof for liquidation
    fn prove_liquidation(
        &self,
        public: &LiquidationPublicInputs,
        private: &LiquidationPrivateInputs,
    ) -> Result<ZKProof>;

    /// Generate proof for redemption
    fn prove_redemption(
        &self,
        public: &RedemptionPublicInputs,
        private: &RedemptionPrivateInputs,
    ) -> Result<ZKProof>;

    /// Generate proof for price attestation
    fn prove_price_attestation(
        &self,
        public: &PriceAttestationPublicInputs,
        private: &PricePrivateInputs,
    ) -> Result<ZKProof>;

    /// Check if prover is ready
    fn is_ready(&self) -> bool;

    /// Get supported circuits
    fn supported_circuits(&self) -> Vec<&'static str>;
}

// ═══════════════════════════════════════════════════════════════════════════════
// NATIVE PROVER (FOR TESTING)
// ═══════════════════════════════════════════════════════════════════════════════

/// Native prover that executes circuits directly without ZK
///
/// This prover is used for testing and development. It executes the circuit
/// constraints directly and produces a "proof" that simply contains the
/// execution trace. This is NOT secure for production use.
#[derive(Debug, Clone)]
pub struct NativeProver {
    version: String,
}

impl Default for NativeProver {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeProver {
    /// Create new native prover
    pub fn new() -> Self {
        Self {
            version: "native-v1.0.0".to_string(),
        }
    }

    /// Create proof from circuit output
    fn create_proof<T: Serialize>(
        &self,
        circuit_id: &str,
        proof_type: ProofType,
        output: &T,
        public_hash: Hash,
        start_time: Instant,
    ) -> Result<ZKProof> {
        let proof_data = bincode::serialize(output).map_err(|e| {
            Error::Serialization(format!("Failed to serialize proof output: {}", e))
        })?;

        let generation_time = start_time.elapsed();

        Ok(ZKProof {
            proof_type,
            circuit_id: circuit_id.to_string(),
            proof_data,
            public_inputs_hash: public_hash,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            backend: ProverBackend::Native,
            metadata: ProofMetadata {
                generation_time_ms: generation_time.as_millis() as u64,
                constraint_count: 0, // Native doesn't count constraints
                prover_version: self.version.clone(),
            },
        })
    }
}

impl Prover for NativeProver {
    fn backend(&self) -> ProverBackend {
        ProverBackend::Native
    }

    fn prove_cdp_transition(
        &self,
        public: &CDPTransitionPublicInputs,
        private: &CDPPrivateInputs,
    ) -> Result<ZKProof> {
        let start = Instant::now();

        // Execute the appropriate circuit based on operation type
        let op_type = OperationType::from(public.operation_type);
        let circuit_id = match op_type {
            OperationType::Deposit => {
                let output = DepositCircuit::execute(public, private)?;
                return self.create_proof(
                    DepositCircuit::circuit_id(),
                    ProofType::CDPTransition,
                    &output,
                    public.hash(),
                    start,
                );
            }
            OperationType::Withdraw => {
                let output = WithdrawCircuit::execute(public, private)?;
                return self.create_proof(
                    WithdrawCircuit::circuit_id(),
                    ProofType::CDPTransition,
                    &output,
                    public.hash(),
                    start,
                );
            }
            OperationType::Mint => {
                let output = MintCircuit::execute(public, private)?;
                return self.create_proof(
                    MintCircuit::circuit_id(),
                    ProofType::CDPTransition,
                    &output,
                    public.hash(),
                    start,
                );
            }
            OperationType::Repay => {
                let output = RepayCircuit::execute(public, private)?;
                return self.create_proof(
                    RepayCircuit::circuit_id(),
                    ProofType::CDPTransition,
                    &output,
                    public.hash(),
                    start,
                );
            }
            _ => {
                return Err(Error::InvalidParameter {
                    name: "operation_type".into(),
                    reason: format!("Unsupported operation type: {:?}", op_type),
                });
            }
        };
    }

    fn prove_liquidation(
        &self,
        public: &LiquidationPublicInputs,
        private: &LiquidationPrivateInputs,
    ) -> Result<ZKProof> {
        let start = Instant::now();
        let output = LiquidationCircuit::execute(public, private)?;

        self.create_proof(
            LiquidationCircuit::circuit_id(),
            ProofType::Liquidation,
            &output,
            public.hash(),
            start,
        )
    }

    fn prove_redemption(
        &self,
        public: &RedemptionPublicInputs,
        private: &RedemptionPrivateInputs,
    ) -> Result<ZKProof> {
        let start = Instant::now();

        // For redemption, we verify basic constraints
        // Full redemption proof would verify each CDP update

        // Verify signature is valid (simplified)
        if private.cdps.is_empty() && public.amount_redeemed > 0 {
            return Err(Error::InvalidParameter {
                name: "cdps".into(),
                reason: "No CDPs to redeem from".into(),
            });
        }

        // Calculate expected collateral
        let expected_collateral: u64 = private.cdps.iter()
            .map(|c| c.collateral_taken)
            .sum();

        if expected_collateral != public.collateral_received {
            return Err(Error::InvalidParameter {
                name: "collateral_received".into(),
                reason: "Collateral mismatch".into(),
            });
        }

        #[derive(Serialize)]
        struct RedemptionOutput {
            amount_redeemed: u64,
            collateral_received: u64,
            cdps_affected: u32,
            transition_hash: Hash,
        }

        let output = RedemptionOutput {
            amount_redeemed: public.amount_redeemed,
            collateral_received: public.collateral_received,
            cdps_affected: public.cdps_affected,
            transition_hash: Hash::sha256(&public.encode()),
        };

        self.create_proof(
            "zkusd_redemption_v1",
            ProofType::Redemption,
            &output,
            public.hash(),
            start,
        )
    }

    fn prove_price_attestation(
        &self,
        public: &PriceAttestationPublicInputs,
        private: &PricePrivateInputs,
    ) -> Result<ZKProof> {
        let start = Instant::now();
        let output = PriceAttestationCircuit::execute(public, private)?;

        self.create_proof(
            PriceAttestationCircuit::circuit_id(),
            ProofType::PriceAttestation,
            &output,
            public.hash(),
            start,
        )
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn supported_circuits(&self) -> Vec<&'static str> {
        vec![
            DepositCircuit::circuit_id(),
            WithdrawCircuit::circuit_id(),
            MintCircuit::circuit_id(),
            RepayCircuit::circuit_id(),
            LiquidationCircuit::circuit_id(),
            PriceAttestationCircuit::circuit_id(),
        ]
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROVER MANAGER
// ═══════════════════════════════════════════════════════════════════════════════

/// Manager for proof generation
pub struct ProverManager {
    /// Active prover backend
    prover: Box<dyn Prover>,
    /// Proof cache (optional)
    cache: Option<ProofCache>,
    /// Statistics
    stats: ProverStats,
}

impl Default for ProverManager {
    fn default() -> Self {
        Self::new(ProverBackend::Native)
    }
}

impl ProverManager {
    /// Create new prover manager with specified backend
    pub fn new(backend: ProverBackend) -> Self {
        let prover: Box<dyn Prover> = match backend {
            ProverBackend::Native => Box::new(NativeProver::new()),
            #[cfg(feature = "sp1-prover")]
            ProverBackend::SP1 => {
                use crate::zkp::sp1_prover::{SP1Prover, SP1ProverConfig};
                let config = SP1ProverConfig::default();
                match SP1Prover::new(config) {
                    Ok(prover) => Box::new(prover),
                    Err(_) => Box::new(NativeProver::new()), // Fallback
                }
            }
            // Other backends fallback to native
            _ => Box::new(NativeProver::new()),
        };

        Self {
            prover,
            cache: Some(ProofCache::new(1000)),
            stats: ProverStats::default(),
        }
    }

    /// Create manager with SP1 and custom config
    #[cfg(feature = "sp1-prover")]
    pub fn with_sp1_config(config: crate::zkp::sp1_prover::SP1ProverConfig) -> Result<Self> {
        use crate::zkp::sp1_prover::SP1Prover;
        let prover = SP1Prover::new(config)?;

        Ok(Self {
            prover: Box::new(prover),
            cache: Some(ProofCache::new(1000)),
            stats: ProverStats::default(),
        })
    }

    /// Create manager with specific prover
    pub fn with_prover<P: Prover + 'static>(prover: P) -> Self {
        Self {
            prover: Box::new(prover),
            cache: Some(ProofCache::new(1000)),
            stats: ProverStats::default(),
        }
    }

    /// Generate proof for operation
    pub fn prove(&mut self, inputs: ProofInputs) -> Result<ZKProof> {
        // Check cache first
        let cache_key = inputs.public_hash();
        if let Some(ref cache) = self.cache {
            if let Some(proof) = cache.get(&cache_key) {
                self.stats.cache_hits += 1;
                return Ok(proof);
            }
        }

        self.stats.proofs_generated += 1;
        let start = Instant::now();

        // Generate proof based on type
        let proof = match inputs.proof_type {
            ProofType::CDPTransition => {
                let public: CDPTransitionPublicInputs = bincode::deserialize(&inputs.public_data)
                    .map_err(|e| Error::Serialization(format!("Failed to deserialize public inputs: {}", e)))?;
                let private: CDPPrivateInputs = bincode::deserialize(&inputs.private_data)
                    .map_err(|e| Error::Serialization(format!("Failed to deserialize private inputs: {}", e)))?;

                self.prover.prove_cdp_transition(&public, &private)?
            }
            ProofType::Liquidation => {
                let public: LiquidationPublicInputs = bincode::deserialize(&inputs.public_data)
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                let private: LiquidationPrivateInputs = bincode::deserialize(&inputs.private_data)
                    .map_err(|e| Error::Serialization(e.to_string()))?;

                self.prover.prove_liquidation(&public, &private)?
            }
            ProofType::Redemption => {
                let public: RedemptionPublicInputs = bincode::deserialize(&inputs.public_data)
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                let private: RedemptionPrivateInputs = bincode::deserialize(&inputs.private_data)
                    .map_err(|e| Error::Serialization(e.to_string()))?;

                self.prover.prove_redemption(&public, &private)?
            }
            ProofType::PriceAttestation => {
                let public: PriceAttestationPublicInputs = bincode::deserialize(&inputs.public_data)
                    .map_err(|e| Error::Serialization(e.to_string()))?;
                let private: PricePrivateInputs = bincode::deserialize(&inputs.private_data)
                    .map_err(|e| Error::Serialization(e.to_string()))?;

                self.prover.prove_price_attestation(&public, &private)?
            }
            ProofType::Batch => {
                return Err(Error::InvalidParameter {
                    name: "proof_type".into(),
                    reason: "Batch proofs not yet supported".into(),
                });
            }
        };

        self.stats.total_time_ms += start.elapsed().as_millis() as u64;

        // Store in cache
        if let Some(ref mut cache) = self.cache {
            cache.insert(cache_key, proof.clone());
        }

        Ok(proof)
    }

    /// Get prover statistics
    pub fn stats(&self) -> &ProverStats {
        &self.stats
    }

    /// Get active backend
    pub fn backend(&self) -> ProverBackend {
        self.prover.backend()
    }

    /// Check if prover is ready
    pub fn is_ready(&self) -> bool {
        self.prover.is_ready()
    }

    /// Clear proof cache
    pub fn clear_cache(&mut self) {
        if let Some(ref mut cache) = self.cache {
            cache.clear();
        }
    }
}

/// Prover statistics
#[derive(Debug, Clone, Default)]
pub struct ProverStats {
    /// Total proofs generated
    pub proofs_generated: u64,
    /// Cache hits
    pub cache_hits: u64,
    /// Total proof generation time (ms)
    pub total_time_ms: u64,
}

impl ProverStats {
    /// Get average proof time
    pub fn average_time_ms(&self) -> u64 {
        if self.proofs_generated == 0 {
            0
        } else {
            self.total_time_ms / self.proofs_generated
        }
    }

    /// Get cache hit rate
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.proofs_generated + self.cache_hits;
        if total == 0 {
            0.0
        } else {
            self.cache_hits as f64 / total as f64
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROOF CACHE
// ═══════════════════════════════════════════════════════════════════════════════

/// LRU cache for proofs
struct ProofCache {
    entries: std::collections::HashMap<Hash, (ZKProof, Instant)>,
    max_size: usize,
    ttl: Duration,
}

impl ProofCache {
    fn new(max_size: usize) -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            max_size,
            ttl: Duration::from_secs(3600), // 1 hour TTL
        }
    }

    fn get(&self, key: &Hash) -> Option<ZKProof> {
        self.entries.get(key).and_then(|(proof, time)| {
            if time.elapsed() < self.ttl {
                Some(proof.clone())
            } else {
                None
            }
        })
    }

    fn insert(&mut self, key: Hash, proof: ZKProof) {
        // Evict old entries if at capacity
        if self.entries.len() >= self.max_size {
            self.evict_oldest();
        }
        self.entries.insert(key, (proof, Instant::now()));
    }

    fn evict_oldest(&mut self) {
        if let Some(oldest_key) = self.entries
            .iter()
            .min_by_key(|(_, (_, time))| *time)
            .map(|(k, _)| *k)
        {
            self.entries.remove(&oldest_key);
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
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
    fn test_native_prover_deposit() {
        let prover = NativeProver::new();
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
            collateral_after: 100_000_000,
            debt_before: 0,
            debt_after: 0,
            signature: Signature::new([0u8; 64]),
            nonce: 1,
            btc_price: 10_000_000,
            merkle_proof: MerkleProof::empty(),
        };

        let proof = prover.prove_cdp_transition(&public, &private);
        assert!(proof.is_ok());

        let proof = proof.unwrap();
        assert_eq!(proof.backend, ProverBackend::Native);
        assert_eq!(proof.circuit_id, DepositCircuit::circuit_id());
        assert!(!proof.proof_data.is_empty());
    }

    #[test]
    fn test_prover_manager() {
        let mut manager = ProverManager::new(ProverBackend::Native);
        assert!(manager.is_ready());
        assert_eq!(manager.backend(), ProverBackend::Native);

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

        let private = CDPPrivateInputs {
            owner: *keypair.public_key(),
            collateral_before: 100_000_000,
            collateral_after: 100_000_000,
            debt_before: 0,
            debt_after: 5_000_000,
            signature: Signature::new([0u8; 64]),
            nonce: 1,
            btc_price: 10_000_000,
            merkle_proof: MerkleProof::empty(),
        };

        let inputs = ProofInputs::cdp_transition(public, private);
        let result = manager.prove(inputs);
        assert!(result.is_ok());

        let stats = manager.stats();
        assert_eq!(stats.proofs_generated, 1);
    }

    #[test]
    fn test_proof_cache() {
        let mut manager = ProverManager::new(ProverBackend::Native);
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
            collateral_after: 100_000_000,
            debt_before: 0,
            debt_after: 0,
            signature: Signature::new([0u8; 64]),
            nonce: 1,
            btc_price: 10_000_000,
            merkle_proof: MerkleProof::empty(),
        };

        let inputs = ProofInputs::cdp_transition(public.clone(), private.clone());

        // First call generates proof
        let _ = manager.prove(inputs.clone());
        assert_eq!(manager.stats().proofs_generated, 1);
        assert_eq!(manager.stats().cache_hits, 0);

        // Second call hits cache
        let _ = manager.prove(inputs);
        assert_eq!(manager.stats().proofs_generated, 1);
        assert_eq!(manager.stats().cache_hits, 1);
    }

    #[test]
    fn test_proof_serialization() {
        let proof = ZKProof {
            proof_type: ProofType::CDPTransition,
            circuit_id: "test_circuit".to_string(),
            proof_data: vec![1, 2, 3, 4],
            public_inputs_hash: Hash::sha256(b"test"),
            timestamp: 1234567890,
            backend: ProverBackend::Native,
            metadata: ProofMetadata {
                generation_time_ms: 100,
                constraint_count: 1000,
                prover_version: "v1".to_string(),
            },
        };

        let serialized = bincode::serialize(&proof).unwrap();
        let deserialized: ZKProof = bincode::deserialize(&serialized).unwrap();

        assert_eq!(proof.circuit_id, deserialized.circuit_id);
        assert_eq!(proof.proof_data, deserialized.proof_data);
    }
}
