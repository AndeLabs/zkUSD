//! SP1 zkVM prover implementation for zkUSD protocol.
//!
//! This module provides production-grade zero-knowledge proof generation using
//! Succinct's SP1 zkVM. It implements the Prover trait for seamless integration
//! with the rest of the protocol.
//!
//! ## Architecture
//!
//! SP1 proofs require two components:
//! 1. **Guest program**: A RISC-V binary that runs inside the zkVM (ELF)
//! 2. **Host program**: Code that submits the guest program for proving
//!
//! This module handles the host-side proving logic. Guest programs are compiled
//! separately and loaded at runtime.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use zkusd::zkp::sp1_prover::{SP1Prover, SP1ProverConfig};
//!
//! let config = SP1ProverConfig::default();
//! let prover = SP1Prover::new(config)?;
//!
//! let proof = prover.prove_cdp_transition(&public, &private)?;
//! ```

#[cfg(feature = "sp1-prover")]
use sp1_sdk::{ProverClient, SP1Stdin, SP1ProofWithPublicValues};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::utils::crypto::Hash;
use crate::zkp::circuits::*;
use crate::zkp::inputs::{
    CDPTransitionPublicInputs, CDPPrivateInputs,
    LiquidationPublicInputs, LiquidationPrivateInputs,
    RedemptionPublicInputs, RedemptionPrivateInputs,
    PriceAttestationPublicInputs, PricePrivateInputs,
    ProofInputs, ProofType, OperationType,
};
use crate::zkp::prover::{Prover, ProverBackend, ProofMetadata, ZKProof};

// ═══════════════════════════════════════════════════════════════════════════════
// SP1 PROVER CONFIGURATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Configuration for SP1 prover
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SP1ProverConfig {
    /// Directory containing compiled ELF binaries
    pub elf_directory: PathBuf,
    /// Whether to use network proving (Succinct's proving network)
    pub use_network: bool,
    /// Network API key (required for network proving)
    pub network_api_key: Option<String>,
    /// Maximum proving time in seconds
    pub max_proving_time_secs: u64,
    /// Enable proof compression (PLONK wrapping)
    pub compress_proofs: bool,
    /// Cache proofs to disk
    pub cache_proofs: bool,
    /// Proof cache directory
    pub cache_directory: Option<PathBuf>,
}

impl Default for SP1ProverConfig {
    fn default() -> Self {
        Self {
            elf_directory: PathBuf::from("./elf"),
            use_network: false,
            network_api_key: None,
            max_proving_time_secs: 300, // 5 minutes
            compress_proofs: true,
            cache_proofs: true,
            cache_directory: Some(PathBuf::from("./.proof_cache")),
        }
    }
}

impl SP1ProverConfig {
    /// Create config for local proving
    pub fn local(elf_directory: impl Into<PathBuf>) -> Self {
        Self {
            elf_directory: elf_directory.into(),
            use_network: false,
            ..Default::default()
        }
    }

    /// Create config for network proving
    pub fn network(elf_directory: impl Into<PathBuf>, api_key: impl Into<String>) -> Self {
        Self {
            elf_directory: elf_directory.into(),
            use_network: true,
            network_api_key: Some(api_key.into()),
            ..Default::default()
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ELF REGISTRY
// ═══════════════════════════════════════════════════════════════════════════════

/// Registry for guest program ELF binaries
#[derive(Debug)]
pub struct ElfRegistry {
    /// Loaded ELF binaries by circuit ID
    elfs: HashMap<String, Vec<u8>>,
    /// ELF directory
    directory: PathBuf,
}

impl ElfRegistry {
    /// Create a new ELF registry
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            elfs: HashMap::new(),
            directory: directory.into(),
        }
    }

    /// Load an ELF binary for a circuit
    pub fn load(&mut self, circuit_id: &str) -> Result<&[u8]> {
        if !self.elfs.contains_key(circuit_id) {
            let elf_path = self.directory.join(format!("{}.elf", circuit_id));
            let elf_data = std::fs::read(&elf_path).map_err(|e| {
                Error::InvalidParameter {
                    name: "elf_path".into(),
                    reason: format!("Failed to load ELF {}: {}", elf_path.display(), e),
                }
            })?;
            self.elfs.insert(circuit_id.to_string(), elf_data);
        }
        Ok(self.elfs.get(circuit_id).unwrap())
    }

    /// Check if ELF exists for circuit
    pub fn has_elf(&self, circuit_id: &str) -> bool {
        self.elfs.contains_key(circuit_id) ||
        self.directory.join(format!("{}.elf", circuit_id)).exists()
    }

    /// Get all available circuit IDs
    pub fn available_circuits(&self) -> Vec<String> {
        let mut circuits: Vec<String> = self.elfs.keys().cloned().collect();

        // Also check directory for unloaded ELFs
        if let Ok(entries) = std::fs::read_dir(&self.directory) {
            for entry in entries.flatten() {
                if let Some(name) = entry.path().file_stem() {
                    if entry.path().extension().map_or(false, |e| e == "elf") {
                        let circuit_id = name.to_string_lossy().to_string();
                        if !circuits.contains(&circuit_id) {
                            circuits.push(circuit_id);
                        }
                    }
                }
            }
        }

        circuits
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SP1 PROVER IMPLEMENTATION
// ═══════════════════════════════════════════════════════════════════════════════

/// SP1 zkVM prover for production use
pub struct SP1Prover {
    /// Configuration
    config: SP1ProverConfig,
    /// ELF registry
    elf_registry: ElfRegistry,
    /// SP1 prover client (when feature is enabled)
    #[cfg(feature = "sp1-prover")]
    client: ProverClient,
    /// Version string
    version: String,
}

impl SP1Prover {
    /// Create a new SP1 prover
    #[cfg(feature = "sp1-prover")]
    pub fn new(config: SP1ProverConfig) -> Result<Self> {
        // Initialize prover client
        let client = if config.use_network {
            let api_key = config.network_api_key.as_ref().ok_or_else(|| {
                Error::InvalidParameter {
                    name: "network_api_key".into(),
                    reason: "API key required for network proving".into(),
                }
            })?;

            // Set environment variable for SP1 network
            std::env::set_var("SP1_PROVER", "network");
            std::env::set_var("SP1_PRIVATE_KEY", api_key);

            ProverClient::builder().build()
        } else {
            std::env::set_var("SP1_PROVER", "local");
            ProverClient::builder().build()
        };

        let elf_registry = ElfRegistry::new(&config.elf_directory);

        // Create cache directory if needed
        if config.cache_proofs {
            if let Some(ref cache_dir) = config.cache_directory {
                std::fs::create_dir_all(cache_dir).ok();
            }
        }

        Ok(Self {
            config,
            elf_registry,
            client,
            version: format!("sp1-v{}", env!("CARGO_PKG_VERSION")),
        })
    }

    /// Create a new SP1 prover (stub when feature is disabled)
    #[cfg(not(feature = "sp1-prover"))]
    pub fn new(config: SP1ProverConfig) -> Result<Self> {
        let elf_registry = ElfRegistry::new(&config.elf_directory);

        Ok(Self {
            config,
            elf_registry,
            version: format!("sp1-stub-v{}", env!("CARGO_PKG_VERSION")),
        })
    }

    /// Generate proof using SP1
    #[cfg(feature = "sp1-prover")]
    fn generate_sp1_proof<T: Serialize>(
        &mut self,
        circuit_id: &str,
        proof_type: ProofType,
        stdin: SP1Stdin,
        public_hash: Hash,
    ) -> Result<ZKProof> {
        let start = Instant::now();

        // Load ELF
        let elf = self.elf_registry.load(circuit_id)?;

        // Setup the program
        let (pk, vk) = self.client.setup(elf);

        // Generate proof
        let proof_result = if self.config.compress_proofs {
            self.client.prove(&pk, &stdin).compressed().run()
        } else {
            self.client.prove(&pk, &stdin).run()
        };

        let proof = proof_result.map_err(|e| {
            Error::InvalidParameter {
                name: "proof_generation".into(),
                reason: format!("SP1 proving failed: {}", e),
            }
        })?;

        // Verify the proof
        self.client.verify(&proof, &vk).map_err(|e| {
            Error::InvalidParameter {
                name: "proof_verification".into(),
                reason: format!("SP1 proof verification failed: {}", e),
            }
        })?;

        let generation_time = start.elapsed();

        // Serialize proof
        let proof_data = bincode::serialize(&proof).map_err(|e| {
            Error::Serialization(format!("Failed to serialize SP1 proof: {}", e))
        })?;

        Ok(ZKProof {
            proof_type,
            circuit_id: circuit_id.to_string(),
            proof_data,
            public_inputs_hash: public_hash,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            backend: ProverBackend::SP1,
            metadata: ProofMetadata {
                generation_time_ms: generation_time.as_millis() as u64,
                constraint_count: 0, // SP1 doesn't expose this directly
                prover_version: self.version.clone(),
            },
        })
    }

    /// Stub implementation when SP1 feature is disabled
    #[cfg(not(feature = "sp1-prover"))]
    fn generate_sp1_proof<T: Serialize>(
        &mut self,
        circuit_id: &str,
        proof_type: ProofType,
        _data: T,
        public_hash: Hash,
    ) -> Result<ZKProof> {
        Err(Error::InvalidParameter {
            name: "sp1-prover".into(),
            reason: "SP1 prover feature not enabled. Rebuild with --features sp1-prover".into(),
        })
    }

    /// Check if circuit is supported
    pub fn supports_circuit(&self, circuit_id: &str) -> bool {
        self.elf_registry.has_elf(circuit_id)
    }

    /// Get available circuits
    pub fn available_circuits(&self) -> Vec<String> {
        self.elf_registry.available_circuits()
    }
}

#[cfg(feature = "sp1-prover")]
impl Prover for SP1Prover {
    fn backend(&self) -> ProverBackend {
        ProverBackend::SP1
    }

    fn prove_cdp_transition(
        &self,
        public: &CDPTransitionPublicInputs,
        private: &CDPPrivateInputs,
    ) -> Result<ZKProof> {
        let op_type = OperationType::from(public.operation_type);
        let circuit_id = match op_type {
            OperationType::Deposit => DepositCircuit::circuit_id(),
            OperationType::Withdraw => WithdrawCircuit::circuit_id(),
            OperationType::Mint => MintCircuit::circuit_id(),
            OperationType::Repay => RepayCircuit::circuit_id(),
            _ => {
                return Err(Error::InvalidParameter {
                    name: "operation_type".into(),
                    reason: format!("Unsupported operation: {:?}", op_type),
                });
            }
        };

        // Create stdin with inputs
        let mut stdin = SP1Stdin::new();

        // Write public inputs
        let public_bytes = bincode::serialize(public).map_err(|e| {
            Error::Serialization(format!("Failed to serialize public inputs: {}", e))
        })?;
        stdin.write(&public_bytes);

        // Write private inputs
        let private_bytes = bincode::serialize(private).map_err(|e| {
            Error::Serialization(format!("Failed to serialize private inputs: {}", e))
        })?;
        stdin.write(&private_bytes);

        // Generate proof - need mutable self for generate_sp1_proof
        let mut this = unsafe { &mut *(self as *const Self as *mut Self) };
        this.generate_sp1_proof::<()>(circuit_id, ProofType::CDPTransition, stdin, public.hash())
    }

    fn prove_liquidation(
        &self,
        public: &LiquidationPublicInputs,
        private: &LiquidationPrivateInputs,
    ) -> Result<ZKProof> {
        let circuit_id = LiquidationCircuit::circuit_id();

        let mut stdin = SP1Stdin::new();

        let public_bytes = bincode::serialize(public).map_err(|e| {
            Error::Serialization(format!("Failed to serialize public inputs: {}", e))
        })?;
        stdin.write(&public_bytes);

        let private_bytes = bincode::serialize(private).map_err(|e| {
            Error::Serialization(format!("Failed to serialize private inputs: {}", e))
        })?;
        stdin.write(&private_bytes);

        let mut this = unsafe { &mut *(self as *const Self as *mut Self) };
        this.generate_sp1_proof::<()>(circuit_id, ProofType::Liquidation, stdin, public.hash())
    }

    fn prove_redemption(
        &self,
        public: &RedemptionPublicInputs,
        private: &RedemptionPrivateInputs,
    ) -> Result<ZKProof> {
        let circuit_id = "zkusd_redemption_v1";

        let mut stdin = SP1Stdin::new();

        let public_bytes = bincode::serialize(public).map_err(|e| {
            Error::Serialization(format!("Failed to serialize public inputs: {}", e))
        })?;
        stdin.write(&public_bytes);

        let private_bytes = bincode::serialize(private).map_err(|e| {
            Error::Serialization(format!("Failed to serialize private inputs: {}", e))
        })?;
        stdin.write(&private_bytes);

        let mut this = unsafe { &mut *(self as *const Self as *mut Self) };
        this.generate_sp1_proof::<()>(circuit_id, ProofType::Redemption, stdin, public.hash())
    }

    fn prove_price_attestation(
        &self,
        public: &PriceAttestationPublicInputs,
        private: &PricePrivateInputs,
    ) -> Result<ZKProof> {
        let circuit_id = PriceAttestationCircuit::circuit_id();

        let mut stdin = SP1Stdin::new();

        let public_bytes = bincode::serialize(public).map_err(|e| {
            Error::Serialization(format!("Failed to serialize public inputs: {}", e))
        })?;
        stdin.write(&public_bytes);

        let private_bytes = bincode::serialize(private).map_err(|e| {
            Error::Serialization(format!("Failed to serialize private inputs: {}", e))
        })?;
        stdin.write(&private_bytes);

        let mut this = unsafe { &mut *(self as *const Self as *mut Self) };
        this.generate_sp1_proof::<()>(circuit_id, ProofType::PriceAttestation, stdin, public.hash())
    }

    fn is_ready(&self) -> bool {
        // Check if at least one circuit ELF is available
        !self.elf_registry.available_circuits().is_empty()
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

// Stub implementation when SP1 is not enabled
#[cfg(not(feature = "sp1-prover"))]
impl Prover for SP1Prover {
    fn backend(&self) -> ProverBackend {
        ProverBackend::SP1
    }

    fn prove_cdp_transition(
        &self,
        _public: &CDPTransitionPublicInputs,
        _private: &CDPPrivateInputs,
    ) -> Result<ZKProof> {
        Err(Error::InvalidParameter {
            name: "sp1-prover".into(),
            reason: "SP1 prover feature not enabled".into(),
        })
    }

    fn prove_liquidation(
        &self,
        _public: &LiquidationPublicInputs,
        _private: &LiquidationPrivateInputs,
    ) -> Result<ZKProof> {
        Err(Error::InvalidParameter {
            name: "sp1-prover".into(),
            reason: "SP1 prover feature not enabled".into(),
        })
    }

    fn prove_redemption(
        &self,
        _public: &RedemptionPublicInputs,
        _private: &RedemptionPrivateInputs,
    ) -> Result<ZKProof> {
        Err(Error::InvalidParameter {
            name: "sp1-prover".into(),
            reason: "SP1 prover feature not enabled".into(),
        })
    }

    fn prove_price_attestation(
        &self,
        _public: &PriceAttestationPublicInputs,
        _private: &PricePrivateInputs,
    ) -> Result<ZKProof> {
        Err(Error::InvalidParameter {
            name: "sp1-prover".into(),
            reason: "SP1 prover feature not enabled".into(),
        })
    }

    fn is_ready(&self) -> bool {
        false
    }

    fn supported_circuits(&self) -> Vec<&'static str> {
        vec![]
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SP1 VERIFIER
// ═══════════════════════════════════════════════════════════════════════════════

/// SP1 proof verifier
pub struct SP1Verifier {
    /// ELF registry for loading verification keys
    elf_registry: ElfRegistry,
    #[cfg(feature = "sp1-prover")]
    client: ProverClient,
}

impl SP1Verifier {
    /// Create a new verifier
    #[cfg(feature = "sp1-prover")]
    pub fn new(elf_directory: impl Into<PathBuf>) -> Self {
        Self {
            elf_registry: ElfRegistry::new(elf_directory),
            client: ProverClient::builder().build(),
        }
    }

    #[cfg(not(feature = "sp1-prover"))]
    pub fn new(elf_directory: impl Into<PathBuf>) -> Self {
        Self {
            elf_registry: ElfRegistry::new(elf_directory),
        }
    }

    /// Verify an SP1 proof
    #[cfg(feature = "sp1-prover")]
    pub fn verify(&mut self, proof: &ZKProof) -> Result<bool> {
        if proof.backend != ProverBackend::SP1 {
            return Err(Error::InvalidParameter {
                name: "backend".into(),
                reason: format!("Expected SP1 proof, got {:?}", proof.backend),
            });
        }

        // Load ELF for this circuit
        let elf = self.elf_registry.load(&proof.circuit_id)?;

        // Setup to get verification key
        let (_pk, vk) = self.client.setup(elf);

        // Deserialize proof
        let sp1_proof: SP1ProofWithPublicValues = bincode::deserialize(&proof.proof_data)
            .map_err(|e| Error::Serialization(format!("Failed to deserialize proof: {}", e)))?;

        // Verify
        self.client.verify(&sp1_proof, &vk).map_err(|e| {
            Error::InvalidParameter {
                name: "verification".into(),
                reason: format!("Verification failed: {}", e),
            }
        })?;

        Ok(true)
    }

    #[cfg(not(feature = "sp1-prover"))]
    pub fn verify(&mut self, _proof: &ZKProof) -> Result<bool> {
        Err(Error::InvalidParameter {
            name: "sp1-prover".into(),
            reason: "SP1 prover feature not enabled".into(),
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = SP1ProverConfig::default();
        assert!(!config.use_network);
        assert!(config.compress_proofs);
        assert!(config.cache_proofs);
    }

    #[test]
    fn test_config_local() {
        let config = SP1ProverConfig::local("./test_elf");
        assert_eq!(config.elf_directory, PathBuf::from("./test_elf"));
        assert!(!config.use_network);
    }

    #[test]
    fn test_config_network() {
        let config = SP1ProverConfig::network("./test_elf", "api_key_123");
        assert!(config.use_network);
        assert_eq!(config.network_api_key, Some("api_key_123".to_string()));
    }

    #[test]
    fn test_elf_registry() {
        let registry = ElfRegistry::new("./nonexistent");
        assert!(registry.available_circuits().is_empty());
    }

    #[test]
    #[cfg(not(feature = "sp1-prover"))]
    fn test_sp1_prover_disabled() {
        let config = SP1ProverConfig::default();
        let prover = SP1Prover::new(config).unwrap();
        assert!(!prover.is_ready());
    }
}
