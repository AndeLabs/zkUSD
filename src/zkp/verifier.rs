//! ZK proof verification for zkUSD protocol.
//!
//! This module provides verification of zero-knowledge proofs. Verifiers
//! check that proofs are valid without learning any private information.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

use crate::error::{Error, Result};
use crate::utils::crypto::Hash;
use crate::zkp::circuits::*;
use crate::zkp::inputs::*;
use crate::zkp::prover::{ProverBackend, ZKProof};

// ═══════════════════════════════════════════════════════════════════════════════
// VERIFICATION RESULT
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of proof verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether proof is valid
    pub valid: bool,
    /// Public inputs hash
    pub public_inputs_hash: Hash,
    /// Verification time (microseconds)
    pub verification_time_us: u64,
    /// Error message if invalid
    pub error: Option<String>,
}

impl VerificationResult {
    /// Create successful verification result
    pub fn success(public_inputs_hash: Hash, verification_time_us: u64) -> Self {
        Self {
            valid: true,
            public_inputs_hash,
            verification_time_us,
            error: None,
        }
    }

    /// Create failed verification result
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            valid: false,
            public_inputs_hash: Hash::zero(),
            verification_time_us: 0,
            error: Some(error.into()),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VERIFIER TRAIT
// ═══════════════════════════════════════════════════════════════════════════════

/// Trait for proof verification backends
pub trait Verifier: Send + Sync {
    /// Verify a ZK proof
    fn verify(&self, proof: &ZKProof) -> Result<VerificationResult>;

    /// Verify proof with explicit public inputs
    fn verify_with_inputs(
        &self,
        proof: &ZKProof,
        public_inputs: &[u8],
    ) -> Result<VerificationResult>;

    /// Get supported backend
    fn backend(&self) -> ProverBackend;

    /// Check if verifier supports a circuit
    fn supports_circuit(&self, circuit_id: &str) -> bool;
}

// ═══════════════════════════════════════════════════════════════════════════════
// NATIVE VERIFIER
// ═══════════════════════════════════════════════════════════════════════════════

/// Native verifier for testing (verifies execution traces, not real ZK proofs)
#[derive(Debug, Clone)]
pub struct NativeVerifier {
    /// Expected circuit versions
    circuit_versions: HashMap<String, u32>,
}

impl Default for NativeVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeVerifier {
    /// Create new native verifier
    pub fn new() -> Self {
        let mut circuit_versions = HashMap::new();
        circuit_versions.insert(DepositCircuit::circuit_id().to_string(), 1);
        circuit_versions.insert(WithdrawCircuit::circuit_id().to_string(), 1);
        circuit_versions.insert(MintCircuit::circuit_id().to_string(), 1);
        circuit_versions.insert(RepayCircuit::circuit_id().to_string(), 1);
        circuit_versions.insert(LiquidationCircuit::circuit_id().to_string(), 1);
        circuit_versions.insert(PriceAttestationCircuit::circuit_id().to_string(), 1);
        circuit_versions.insert("zkusd_redemption_v1".to_string(), 1);

        Self { circuit_versions }
    }
}

impl Verifier for NativeVerifier {
    fn verify(&self, proof: &ZKProof) -> Result<VerificationResult> {
        let start = Instant::now();

        // Check proof backend matches
        if proof.backend != ProverBackend::Native {
            return Ok(VerificationResult::failure(format!(
                "Expected Native backend, got {:?}",
                proof.backend
            )));
        }

        // Check circuit is supported
        if !self.circuit_versions.contains_key(&proof.circuit_id) {
            return Ok(VerificationResult::failure(format!(
                "Unknown circuit: {}",
                proof.circuit_id
            )));
        }

        // Verify proof data is not empty
        if proof.proof_data.is_empty() {
            return Ok(VerificationResult::failure("Empty proof data"));
        }

        // Verify public inputs hash is not zero
        if proof.public_inputs_hash.is_zero() {
            return Ok(VerificationResult::failure("Zero public inputs hash"));
        }

        // For native proofs, we trust the execution trace
        // In a real ZK system, this would verify the cryptographic proof

        let verification_time = start.elapsed().as_micros() as u64;

        Ok(VerificationResult::success(
            proof.public_inputs_hash,
            verification_time,
        ))
    }

    fn verify_with_inputs(
        &self,
        proof: &ZKProof,
        public_inputs: &[u8],
    ) -> Result<VerificationResult> {
        let start = Instant::now();

        // First do basic verification
        let basic_result = self.verify(proof)?;
        if !basic_result.valid {
            return Ok(basic_result);
        }

        // Verify public inputs hash matches
        let computed_hash = Hash::sha256(public_inputs);
        if computed_hash != proof.public_inputs_hash {
            return Ok(VerificationResult::failure(
                "Public inputs hash mismatch",
            ));
        }

        let verification_time = start.elapsed().as_micros() as u64;

        Ok(VerificationResult::success(
            proof.public_inputs_hash,
            verification_time,
        ))
    }

    fn backend(&self) -> ProverBackend {
        ProverBackend::Native
    }

    fn supports_circuit(&self, circuit_id: &str) -> bool {
        self.circuit_versions.contains_key(circuit_id)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VERIFICATION MANAGER
// ═══════════════════════════════════════════════════════════════════════════════

/// Manager for proof verification
pub struct VerificationManager {
    /// Verifiers by backend
    verifiers: HashMap<ProverBackend, Box<dyn Verifier>>,
    /// Verification statistics
    stats: VerifierStats,
    /// Known valid proof hashes (for caching)
    known_valid: HashMap<Hash, bool>,
}

impl Default for VerificationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VerificationManager {
    /// Create new verification manager with default verifiers
    pub fn new() -> Self {
        let mut verifiers: HashMap<ProverBackend, Box<dyn Verifier>> = HashMap::new();
        verifiers.insert(ProverBackend::Native, Box::new(NativeVerifier::new()));

        Self {
            verifiers,
            stats: VerifierStats::default(),
            known_valid: HashMap::new(),
        }
    }

    /// Add a verifier for a backend
    pub fn add_verifier<V: Verifier + 'static>(&mut self, backend: ProverBackend, verifier: V) {
        self.verifiers.insert(backend, Box::new(verifier));
    }

    /// Verify a proof
    pub fn verify(&mut self, proof: &ZKProof) -> Result<VerificationResult> {
        // Check cache
        let proof_hash = proof.hash();
        if let Some(&valid) = self.known_valid.get(&proof_hash) {
            self.stats.cache_hits += 1;
            return Ok(if valid {
                VerificationResult::success(proof.public_inputs_hash, 0)
            } else {
                VerificationResult::failure("Previously failed verification")
            });
        }

        // Get appropriate verifier
        let verifier = self.verifiers.get(&proof.backend)
            .ok_or_else(|| Error::InvalidParameter {
                name: "backend".into(),
                reason: format!("No verifier for backend {:?}", proof.backend),
            })?;

        // Verify
        let result = verifier.verify(proof)?;

        // Update stats
        self.stats.total_verifications += 1;
        if result.valid {
            self.stats.successful_verifications += 1;
        }
        self.stats.total_time_us += result.verification_time_us;

        // Cache result
        self.known_valid.insert(proof_hash, result.valid);

        Ok(result)
    }

    /// Verify proof with explicit public inputs
    pub fn verify_with_inputs(
        &mut self,
        proof: &ZKProof,
        public_inputs: &[u8],
    ) -> Result<VerificationResult> {
        let verifier = self.verifiers.get(&proof.backend)
            .ok_or_else(|| Error::InvalidParameter {
                name: "backend".into(),
                reason: format!("No verifier for backend {:?}", proof.backend),
            })?;

        let result = verifier.verify_with_inputs(proof, public_inputs)?;

        self.stats.total_verifications += 1;
        if result.valid {
            self.stats.successful_verifications += 1;
        }
        self.stats.total_time_us += result.verification_time_us;

        Ok(result)
    }

    /// Batch verify multiple proofs
    pub fn batch_verify(&mut self, proofs: &[ZKProof]) -> Result<Vec<VerificationResult>> {
        proofs.iter().map(|p| self.verify(p)).collect()
    }

    /// Get verification statistics
    pub fn stats(&self) -> &VerifierStats {
        &self.stats
    }

    /// Clear verification cache
    pub fn clear_cache(&mut self) {
        self.known_valid.clear();
    }

    /// Check if a backend is supported
    pub fn supports_backend(&self, backend: ProverBackend) -> bool {
        self.verifiers.contains_key(&backend)
    }
}

/// Verifier statistics
#[derive(Debug, Clone, Default)]
pub struct VerifierStats {
    /// Total verifications performed
    pub total_verifications: u64,
    /// Successful verifications
    pub successful_verifications: u64,
    /// Cache hits
    pub cache_hits: u64,
    /// Total verification time (microseconds)
    pub total_time_us: u64,
}

impl VerifierStats {
    /// Get success rate
    pub fn success_rate(&self) -> f64 {
        if self.total_verifications == 0 {
            0.0
        } else {
            self.successful_verifications as f64 / self.total_verifications as f64
        }
    }

    /// Get average verification time (microseconds)
    pub fn average_time_us(&self) -> u64 {
        if self.total_verifications == 0 {
            0
        } else {
            self.total_time_us / self.total_verifications
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BATCH VERIFICATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Batch of proofs for verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofBatch {
    /// Proofs in the batch
    pub proofs: Vec<ZKProof>,
    /// Batch hash
    pub batch_hash: Hash,
    /// Creation timestamp
    pub timestamp: u64,
}

impl ProofBatch {
    /// Create new batch from proofs
    pub fn new(proofs: Vec<ZKProof>) -> Self {
        let mut hasher_data = Vec::new();
        for proof in &proofs {
            hasher_data.extend_from_slice(proof.hash().as_bytes());
        }
        let batch_hash = Hash::sha256(&hasher_data);

        Self {
            proofs,
            batch_hash,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Get batch size
    pub fn size(&self) -> usize {
        self.proofs.len()
    }

    /// Check if batch is empty
    pub fn is_empty(&self) -> bool {
        self.proofs.is_empty()
    }
}

/// Result of batch verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchVerificationResult {
    /// Whether all proofs are valid
    pub all_valid: bool,
    /// Individual results
    pub results: Vec<VerificationResult>,
    /// Number of valid proofs
    pub valid_count: usize,
    /// Number of invalid proofs
    pub invalid_count: usize,
    /// Total verification time (microseconds)
    pub total_time_us: u64,
}

impl BatchVerificationResult {
    /// Create from individual results
    pub fn from_results(results: Vec<VerificationResult>) -> Self {
        let valid_count = results.iter().filter(|r| r.valid).count();
        let invalid_count = results.len() - valid_count;
        let total_time_us: u64 = results.iter()
            .map(|r| r.verification_time_us)
            .sum();

        Self {
            all_valid: invalid_count == 0,
            results,
            valid_count,
            invalid_count,
            total_time_us,
        }
    }

    /// Get failed proof indices
    pub fn failed_indices(&self) -> Vec<usize> {
        self.results.iter()
            .enumerate()
            .filter(|(_, r)| !r.valid)
            .map(|(i, _)| i)
            .collect()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VERIFICATION KEY
// ═══════════════════════════════════════════════════════════════════════════════

/// Verification key for a circuit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationKey {
    /// Circuit identifier
    pub circuit_id: String,
    /// Key version
    pub version: u32,
    /// Serialized key data
    pub key_data: Vec<u8>,
    /// Key hash for integrity
    pub key_hash: Hash,
}

impl VerificationKey {
    /// Create verification key
    pub fn new(circuit_id: impl Into<String>, version: u32, key_data: Vec<u8>) -> Self {
        let key_hash = Hash::sha256(&key_data);
        Self {
            circuit_id: circuit_id.into(),
            version,
            key_data,
            key_hash,
        }
    }

    /// Verify key integrity
    pub fn verify_integrity(&self) -> bool {
        Hash::sha256(&self.key_data) == self.key_hash
    }
}

/// Registry of verification keys
#[derive(Debug, Clone, Default)]
pub struct VerificationKeyRegistry {
    keys: HashMap<String, VerificationKey>,
}

impl VerificationKeyRegistry {
    /// Create new registry
    pub fn new() -> Self {
        Self {
            keys: HashMap::new(),
        }
    }

    /// Register a verification key
    pub fn register(&mut self, key: VerificationKey) {
        self.keys.insert(key.circuit_id.clone(), key);
    }

    /// Get verification key for circuit
    pub fn get(&self, circuit_id: &str) -> Option<&VerificationKey> {
        self.keys.get(circuit_id)
    }

    /// Check if circuit is registered
    pub fn contains(&self, circuit_id: &str) -> bool {
        self.keys.contains_key(circuit_id)
    }

    /// Get all registered circuits
    pub fn circuits(&self) -> impl Iterator<Item = &str> {
        self.keys.keys().map(|s| s.as_str())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zkp::prover::{NativeProver, Prover, ProofMetadata};

    #[test]
    fn test_native_verifier() {
        let verifier = NativeVerifier::new();

        // Create a valid proof
        let proof = ZKProof {
            proof_type: ProofType::CDPTransition,
            circuit_id: DepositCircuit::circuit_id().to_string(),
            proof_data: vec![1, 2, 3, 4],
            public_inputs_hash: Hash::sha256(b"test"),
            timestamp: 1234567890,
            backend: ProverBackend::Native,
            metadata: ProofMetadata::default(),
        };

        let result = verifier.verify(&proof).unwrap();
        assert!(result.valid);
    }

    #[test]
    fn test_native_verifier_wrong_backend() {
        let verifier = NativeVerifier::new();

        let proof = ZKProof {
            proof_type: ProofType::CDPTransition,
            circuit_id: DepositCircuit::circuit_id().to_string(),
            proof_data: vec![1, 2, 3, 4],
            public_inputs_hash: Hash::sha256(b"test"),
            timestamp: 1234567890,
            backend: ProverBackend::SP1, // Wrong backend
            metadata: ProofMetadata::default(),
        };

        let result = verifier.verify(&proof).unwrap();
        assert!(!result.valid);
    }

    #[test]
    fn test_verification_manager() {
        let mut manager = VerificationManager::new();

        let proof = ZKProof {
            proof_type: ProofType::CDPTransition,
            circuit_id: DepositCircuit::circuit_id().to_string(),
            proof_data: vec![1, 2, 3, 4],
            public_inputs_hash: Hash::sha256(b"test"),
            timestamp: 1234567890,
            backend: ProverBackend::Native,
            metadata: ProofMetadata::default(),
        };

        let result = manager.verify(&proof).unwrap();
        assert!(result.valid);

        // Second verification should hit cache
        let _ = manager.verify(&proof);
        assert_eq!(manager.stats().cache_hits, 1);
    }

    #[test]
    fn test_batch_verification() {
        let mut manager = VerificationManager::new();

        let proofs = vec![
            ZKProof {
                proof_type: ProofType::CDPTransition,
                circuit_id: DepositCircuit::circuit_id().to_string(),
                proof_data: vec![1],
                public_inputs_hash: Hash::sha256(b"test1"),
                timestamp: 1234567890,
                backend: ProverBackend::Native,
                metadata: ProofMetadata::default(),
            },
            ZKProof {
                proof_type: ProofType::CDPTransition,
                circuit_id: MintCircuit::circuit_id().to_string(),
                proof_data: vec![2],
                public_inputs_hash: Hash::sha256(b"test2"),
                timestamp: 1234567891,
                backend: ProverBackend::Native,
                metadata: ProofMetadata::default(),
            },
        ];

        let results = manager.batch_verify(&proofs).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.valid));
    }

    #[test]
    fn test_proof_batch() {
        let proofs = vec![
            ZKProof {
                proof_type: ProofType::CDPTransition,
                circuit_id: "test".to_string(),
                proof_data: vec![1],
                public_inputs_hash: Hash::sha256(b"1"),
                timestamp: 0,
                backend: ProverBackend::Native,
                metadata: ProofMetadata::default(),
            },
            ZKProof {
                proof_type: ProofType::CDPTransition,
                circuit_id: "test".to_string(),
                proof_data: vec![2],
                public_inputs_hash: Hash::sha256(b"2"),
                timestamp: 0,
                backend: ProverBackend::Native,
                metadata: ProofMetadata::default(),
            },
        ];

        let batch = ProofBatch::new(proofs);
        assert_eq!(batch.size(), 2);
        assert!(!batch.batch_hash.is_zero());
    }

    #[test]
    fn test_verification_key() {
        let key = VerificationKey::new(
            "test_circuit",
            1,
            vec![1, 2, 3, 4, 5],
        );

        assert!(key.verify_integrity());
        assert_eq!(key.circuit_id, "test_circuit");
    }

    #[test]
    fn test_verification_key_registry() {
        let mut registry = VerificationKeyRegistry::new();

        let key = VerificationKey::new("test_circuit", 1, vec![1, 2, 3]);
        registry.register(key);

        assert!(registry.contains("test_circuit"));
        assert!(registry.get("test_circuit").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_batch_verification_result() {
        let results = vec![
            VerificationResult::success(Hash::sha256(b"1"), 100),
            VerificationResult::failure("test error"),
            VerificationResult::success(Hash::sha256(b"2"), 200),
        ];

        let batch_result = BatchVerificationResult::from_results(results);

        assert!(!batch_result.all_valid);
        assert_eq!(batch_result.valid_count, 2);
        assert_eq!(batch_result.invalid_count, 1);
        assert_eq!(batch_result.failed_indices(), vec![1]);
    }
}
