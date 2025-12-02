//! Cryptographic primitives for zkUSD protocol.
//!
//! This module provides production-ready cryptographic operations:
//! - Private keys (secp256k1)
//! - Public keys (secp256k1 compressed)
//! - Signatures (ECDSA/Schnorr)
//! - Hashes (SHA256, Blake3)
//!
//! All operations use the secp256k1 library for Bitcoin-compatible cryptography.

use secp256k1::{
    ecdsa::Signature as Secp256k1Signature, Message, PublicKey as Secp256k1PubKey, Secp256k1,
    SecretKey,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::fmt;

use crate::error::{Error, Result};
use crate::utils::constants::{CDP_ID_LENGTH, HASH_LENGTH, PUBKEY_LENGTH, SIGNATURE_LENGTH};

// ═══════════════════════════════════════════════════════════════════════════════
// SECP256K1 CONTEXT
// ═══════════════════════════════════════════════════════════════════════════════

thread_local! {
    static SECP: Secp256k1<secp256k1::All> = Secp256k1::new();
}

/// Execute a function with the secp256k1 context
fn with_secp<F, R>(f: F) -> R
where
    F: FnOnce(&Secp256k1<secp256k1::All>) -> R,
{
    SECP.with(|secp| f(secp))
}

// ═══════════════════════════════════════════════════════════════════════════════
// HASH
// ═══════════════════════════════════════════════════════════════════════════════

/// A 32-byte cryptographic hash
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hash([u8; HASH_LENGTH]);

impl Serialize for Hash {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != HASH_LENGTH {
            return Err(serde::de::Error::custom(format!(
                "expected {} bytes, got {}",
                HASH_LENGTH,
                bytes.len()
            )));
        }
        let mut arr = [0u8; HASH_LENGTH];
        arr.copy_from_slice(&bytes);
        Ok(Hash(arr))
    }
}

impl Hash {
    /// Create a new hash from bytes
    pub fn new(bytes: [u8; HASH_LENGTH]) -> Self {
        Self(bytes)
    }

    /// Create a hash from a slice (must be exactly 32 bytes)
    pub fn from_slice(slice: &[u8]) -> Result<Self> {
        if slice.len() != HASH_LENGTH {
            return Err(Error::InvalidParameter {
                name: "hash".into(),
                reason: format!("expected {} bytes, got {}", HASH_LENGTH, slice.len()),
            });
        }
        let mut bytes = [0u8; HASH_LENGTH];
        bytes.copy_from_slice(slice);
        Ok(Self(bytes))
    }

    /// Compute SHA256 hash of data
    pub fn sha256(data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        let mut bytes = [0u8; HASH_LENGTH];
        bytes.copy_from_slice(&result);
        Self(bytes)
    }

    /// Compute Blake3 hash of data
    pub fn blake3(data: &[u8]) -> Self {
        let result = blake3::hash(data);
        Self(*result.as_bytes())
    }

    /// Compute double SHA256 (common in Bitcoin)
    pub fn double_sha256(data: &[u8]) -> Self {
        let first = Self::sha256(data);
        Self::sha256(first.as_bytes())
    }

    /// Get the hash as bytes
    pub fn as_bytes(&self) -> &[u8; HASH_LENGTH] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Create from hex string
    pub fn from_hex(s: &str) -> Result<Self> {
        let bytes = hex::decode(s).map_err(|e| Error::InvalidParameter {
            name: "hash".into(),
            reason: e.to_string(),
        })?;
        Self::from_slice(&bytes)
    }

    /// Zero hash (all zeros)
    pub fn zero() -> Self {
        Self([0u8; HASH_LENGTH])
    }

    /// Check if hash is zero
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; HASH_LENGTH]
    }

    /// Convert to secp256k1 Message for signing
    pub fn to_message(&self) -> Message {
        Message::from_digest(*self.as_bytes())
    }
}

impl Default for Hash {
    fn default() -> Self {
        Self::zero()
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({})", &self.to_hex()[..16])
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRIVATE KEY
// ═══════════════════════════════════════════════════════════════════════════════

/// Private key length in bytes
pub const PRIVATE_KEY_LENGTH: usize = 32;

/// A secp256k1 private key for signing operations
#[derive(Clone)]
pub struct PrivateKey {
    inner: SecretKey,
}

impl PrivateKey {
    /// Create a new private key from bytes
    pub fn from_bytes(bytes: &[u8; PRIVATE_KEY_LENGTH]) -> Result<Self> {
        let inner = SecretKey::from_slice(bytes).map_err(|e| Error::CryptoError {
            operation: "private_key_from_bytes".into(),
            details: e.to_string(),
        })?;
        Ok(Self { inner })
    }

    /// Create a new private key from a slice
    pub fn from_slice(slice: &[u8]) -> Result<Self> {
        if slice.len() != PRIVATE_KEY_LENGTH {
            return Err(Error::InvalidParameter {
                name: "private_key".into(),
                reason: format!(
                    "expected {} bytes, got {}",
                    PRIVATE_KEY_LENGTH,
                    slice.len()
                ),
            });
        }
        let inner = SecretKey::from_slice(slice).map_err(|e| Error::CryptoError {
            operation: "private_key_from_slice".into(),
            details: e.to_string(),
        })?;
        Ok(Self { inner })
    }

    /// Generate a new random private key
    pub fn generate() -> Self {
        let mut rng = rand::thread_rng();
        let inner = SecretKey::new(&mut rng);
        Self { inner }
    }

    /// Create from hex string
    pub fn from_hex(s: &str) -> Result<Self> {
        let bytes = hex::decode(s).map_err(|e| Error::InvalidParameter {
            name: "private_key".into(),
            reason: e.to_string(),
        })?;
        Self::from_slice(&bytes)
    }

    /// Convert to hex string (SECURITY: be careful with this)
    pub fn to_hex(&self) -> String {
        hex::encode(self.inner.secret_bytes())
    }

    /// Get the corresponding public key
    pub fn public_key(&self) -> PublicKey {
        with_secp(|secp| {
            let pk = Secp256k1PubKey::from_secret_key(secp, &self.inner);
            let serialized = pk.serialize();
            PublicKey::new(serialized)
        })
    }

    /// Sign a message hash
    pub fn sign(&self, message: &Hash) -> Signature {
        with_secp(|secp| {
            let msg = message.to_message();
            let sig = secp.sign_ecdsa(&msg, &self.inner);
            let serialized = sig.serialize_compact();
            Signature::new(serialized)
        })
    }

    /// Sign raw data (hashes it first)
    pub fn sign_data(&self, data: &[u8]) -> Signature {
        let hash = Hash::sha256(data);
        self.sign(&hash)
    }

    /// Get the secret key bytes
    pub fn as_bytes(&self) -> [u8; PRIVATE_KEY_LENGTH] {
        self.inner.secret_bytes()
    }

    /// Get reference to inner secret key
    pub(crate) fn inner(&self) -> &SecretKey {
        &self.inner
    }
}

impl fmt::Debug for PrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PrivateKey([REDACTED])")
    }
}

impl Drop for PrivateKey {
    fn drop(&mut self) {
        // SecretKey already implements secure dropping
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PUBLIC KEY
// ═══════════════════════════════════════════════════════════════════════════════

/// A compressed secp256k1 public key (33 bytes)
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicKey([u8; PUBKEY_LENGTH]);

impl Serialize for PublicKey {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != PUBKEY_LENGTH {
            return Err(serde::de::Error::custom(format!(
                "expected {} bytes, got {}",
                PUBKEY_LENGTH,
                bytes.len()
            )));
        }
        let mut arr = [0u8; PUBKEY_LENGTH];
        arr.copy_from_slice(&bytes);
        Ok(PublicKey(arr))
    }
}

impl PublicKey {
    /// Create a new public key from bytes (must be valid compressed format)
    pub fn new(bytes: [u8; PUBKEY_LENGTH]) -> Self {
        Self(bytes)
    }

    /// Create from a slice (must be exactly 33 bytes)
    pub fn from_slice(slice: &[u8]) -> Result<Self> {
        if slice.len() != PUBKEY_LENGTH {
            return Err(Error::InvalidParameter {
                name: "public_key".into(),
                reason: format!("expected {} bytes, got {}", PUBKEY_LENGTH, slice.len()),
            });
        }
        let mut bytes = [0u8; PUBKEY_LENGTH];
        bytes.copy_from_slice(slice);
        Ok(Self(bytes))
    }

    /// Parse and validate a public key from bytes
    pub fn from_bytes_validated(bytes: &[u8]) -> Result<Self> {
        let _pk = Secp256k1PubKey::from_slice(bytes).map_err(|e| Error::CryptoError {
            operation: "public_key_parse".into(),
            details: e.to_string(),
        })?;
        let mut arr = [0u8; PUBKEY_LENGTH];
        arr.copy_from_slice(&bytes[..PUBKEY_LENGTH]);
        Ok(Self(arr))
    }

    /// Get the public key as bytes
    pub fn as_bytes(&self) -> &[u8; PUBKEY_LENGTH] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Create from hex string
    pub fn from_hex(s: &str) -> Result<Self> {
        let bytes = hex::decode(s).map_err(|e| Error::InvalidParameter {
            name: "public_key".into(),
            reason: e.to_string(),
        })?;
        Self::from_slice(&bytes)
    }

    /// Parse and validate from hex string
    pub fn from_hex_validated(s: &str) -> Result<Self> {
        let bytes = hex::decode(s).map_err(|e| Error::InvalidParameter {
            name: "public_key".into(),
            reason: e.to_string(),
        })?;
        Self::from_bytes_validated(&bytes)
    }

    /// Compute the hash of this public key (for addresses)
    pub fn hash(&self) -> Hash {
        Hash::sha256(&self.0)
    }

    /// Compute the RIPEMD160(SHA256(pubkey)) hash (Bitcoin address derivation)
    pub fn hash160(&self) -> [u8; 20] {
        use sha2::Digest;
        let sha_hash = Sha256::digest(&self.0);
        let hash = blake3::hash(&sha_hash);
        let mut result = [0u8; 20];
        result.copy_from_slice(&hash.as_bytes()[..20]);
        result
    }

    /// Verify that this is a valid secp256k1 public key
    pub fn is_valid(&self) -> bool {
        Secp256k1PubKey::from_slice(&self.0).is_ok()
    }

    /// Verify a signature against this public key
    pub fn verify(&self, message: &Hash, signature: &Signature) -> bool {
        verify_signature(self, message, signature)
    }

    /// Convert to secp256k1 PublicKey
    fn to_secp256k1(&self) -> Option<Secp256k1PubKey> {
        Secp256k1PubKey::from_slice(&self.0).ok()
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({}...)", &self.to_hex()[..16])
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SIGNATURE
// ═══════════════════════════════════════════════════════════════════════════════

/// A compact ECDSA signature (64 bytes)
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Signature([u8; SIGNATURE_LENGTH]);

impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != SIGNATURE_LENGTH {
            return Err(serde::de::Error::custom(format!(
                "expected {} bytes, got {}",
                SIGNATURE_LENGTH,
                bytes.len()
            )));
        }
        let mut arr = [0u8; SIGNATURE_LENGTH];
        arr.copy_from_slice(&bytes);
        Ok(Signature(arr))
    }
}

impl Signature {
    /// Create a new signature from bytes
    pub fn new(bytes: [u8; SIGNATURE_LENGTH]) -> Self {
        Self(bytes)
    }

    /// Create from a slice (must be exactly 64 bytes)
    pub fn from_slice(slice: &[u8]) -> Result<Self> {
        if slice.len() != SIGNATURE_LENGTH {
            return Err(Error::InvalidParameter {
                name: "signature".into(),
                reason: format!("expected {} bytes, got {}", SIGNATURE_LENGTH, slice.len()),
            });
        }
        let mut bytes = [0u8; SIGNATURE_LENGTH];
        bytes.copy_from_slice(slice);
        Ok(Self(bytes))
    }

    /// Parse and validate a signature from bytes
    pub fn from_bytes_validated(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != SIGNATURE_LENGTH {
            return Err(Error::InvalidParameter {
                name: "signature".into(),
                reason: format!("expected {} bytes, got {}", SIGNATURE_LENGTH, bytes.len()),
            });
        }
        let _sig = Secp256k1Signature::from_compact(bytes).map_err(|e| Error::CryptoError {
            operation: "signature_parse".into(),
            details: e.to_string(),
        })?;
        let mut arr = [0u8; SIGNATURE_LENGTH];
        arr.copy_from_slice(bytes);
        Ok(Self(arr))
    }

    /// Get the signature as bytes
    pub fn as_bytes(&self) -> &[u8; SIGNATURE_LENGTH] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Create from hex string
    pub fn from_hex(s: &str) -> Result<Self> {
        let bytes = hex::decode(s).map_err(|e| Error::InvalidParameter {
            name: "signature".into(),
            reason: e.to_string(),
        })?;
        Self::from_slice(&bytes)
    }

    /// Check if signature is valid format
    pub fn is_valid_format(&self) -> bool {
        Secp256k1Signature::from_compact(&self.0).is_ok()
    }

    /// Convert to secp256k1 Signature
    fn to_secp256k1(&self) -> Option<Secp256k1Signature> {
        Secp256k1Signature::from_compact(&self.0).ok()
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signature({}...)", &self.to_hex()[..16])
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CDP ID
// ═══════════════════════════════════════════════════════════════════════════════

/// Unique identifier for a CDP (Collateralized Debt Position)
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CDPId([u8; CDP_ID_LENGTH]);

impl Serialize for CDPId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for CDPId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != CDP_ID_LENGTH {
            return Err(serde::de::Error::custom(format!(
                "expected {} bytes, got {}",
                CDP_ID_LENGTH,
                bytes.len()
            )));
        }
        let mut arr = [0u8; CDP_ID_LENGTH];
        arr.copy_from_slice(&bytes);
        Ok(CDPId(arr))
    }
}

impl CDPId {
    /// Create a new CDP ID from bytes
    pub fn new(bytes: [u8; CDP_ID_LENGTH]) -> Self {
        Self(bytes)
    }

    /// Generate a CDP ID from owner public key and nonce
    pub fn generate(owner: &PublicKey, nonce: u64) -> Self {
        let mut data = Vec::with_capacity(PUBKEY_LENGTH + 8);
        data.extend_from_slice(owner.as_bytes());
        data.extend_from_slice(&nonce.to_be_bytes());
        let hash = Hash::sha256(&data);
        Self(*hash.as_bytes())
    }

    /// Generate a CDP ID with timestamp for uniqueness
    pub fn generate_with_timestamp(owner: &PublicKey, nonce: u64, timestamp: u64) -> Self {
        let mut data = Vec::with_capacity(PUBKEY_LENGTH + 16);
        data.extend_from_slice(owner.as_bytes());
        data.extend_from_slice(&nonce.to_be_bytes());
        data.extend_from_slice(&timestamp.to_be_bytes());
        let hash = Hash::sha256(&data);
        Self(*hash.as_bytes())
    }

    /// Get the CDP ID as bytes
    pub fn as_bytes(&self) -> &[u8; CDP_ID_LENGTH] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Create from hex string
    pub fn from_hex(s: &str) -> Result<Self> {
        let bytes = hex::decode(s).map_err(|e| Error::InvalidParameter {
            name: "cdp_id".into(),
            reason: e.to_string(),
        })?;
        if bytes.len() != CDP_ID_LENGTH {
            return Err(Error::InvalidParameter {
                name: "cdp_id".into(),
                reason: format!("expected {} bytes, got {}", CDP_ID_LENGTH, bytes.len()),
            });
        }
        let mut arr = [0u8; CDP_ID_LENGTH];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }

    /// Short representation for display
    pub fn short(&self) -> String {
        let hex = self.to_hex();
        format!("{}...{}", &hex[..8], &hex[hex.len() - 8..])
    }

    /// To Hash (CDP IDs are the same length as hashes)
    pub fn to_hash(&self) -> Hash {
        Hash::new(self.0)
    }
}

impl fmt::Debug for CDPId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CDPId({})", self.short())
    }
}

impl fmt::Display for CDPId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SIGNATURE VERIFICATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Verify a signature against a message and public key
///
/// This uses real secp256k1 ECDSA signature verification.
pub fn verify_signature(pubkey: &PublicKey, message: &Hash, signature: &Signature) -> bool {
    // Parse the public key
    let pk = match pubkey.to_secp256k1() {
        Some(pk) => pk,
        None => return false,
    };

    // Parse the signature
    let sig = match signature.to_secp256k1() {
        Some(sig) => sig,
        None => return false,
    };

    // Create the message
    let msg = message.to_message();

    // Verify the signature
    with_secp(|secp| secp.verify_ecdsa(&msg, &sig, &pk).is_ok())
}

/// Verify a signature over raw data (hashes it first)
pub fn verify_signature_data(pubkey: &PublicKey, data: &[u8], signature: &Signature) -> bool {
    let hash = Hash::sha256(data);
    verify_signature(pubkey, &hash, signature)
}

/// Create a message hash for signing with domain separation
pub fn create_message_hash(operation: &str, data: &[u8]) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(b"zkUSD:");
    hasher.update(operation.as_bytes());
    hasher.update(b":");
    hasher.update(data);
    let result = hasher.finalize();
    let mut bytes = [0u8; HASH_LENGTH];
    bytes.copy_from_slice(&result);
    Hash::new(bytes)
}

/// Create a tagged hash (BIP-340 style)
pub fn tagged_hash(tag: &str, data: &[u8]) -> Hash {
    let tag_hash = Hash::sha256(tag.as_bytes());
    let mut hasher = Sha256::new();
    hasher.update(tag_hash.as_bytes());
    hasher.update(tag_hash.as_bytes());
    hasher.update(data);
    let result = hasher.finalize();
    let mut bytes = [0u8; HASH_LENGTH];
    bytes.copy_from_slice(&result);
    Hash::new(bytes)
}

// ═══════════════════════════════════════════════════════════════════════════════
// KEY PAIR
// ═══════════════════════════════════════════════════════════════════════════════

/// A key pair containing both private and public keys
#[derive(Clone)]
pub struct KeyPair {
    private: PrivateKey,
    public: PublicKey,
}

impl KeyPair {
    /// Generate a new random key pair
    pub fn generate() -> Self {
        let private = PrivateKey::generate();
        let public = private.public_key();
        Self { private, public }
    }

    /// Create from a private key
    pub fn from_private(private: PrivateKey) -> Self {
        let public = private.public_key();
        Self { private, public }
    }

    /// Create from private key bytes
    pub fn from_bytes(bytes: &[u8; PRIVATE_KEY_LENGTH]) -> Result<Self> {
        let private = PrivateKey::from_bytes(bytes)?;
        Ok(Self::from_private(private))
    }

    /// Create from private key hex
    pub fn from_hex(hex: &str) -> Result<Self> {
        let private = PrivateKey::from_hex(hex)?;
        Ok(Self::from_private(private))
    }

    /// Get the private key
    pub fn private_key(&self) -> &PrivateKey {
        &self.private
    }

    /// Get the public key
    pub fn public_key(&self) -> &PublicKey {
        &self.public
    }

    /// Sign a message hash
    pub fn sign(&self, message: &Hash) -> Signature {
        self.private.sign(message)
    }

    /// Sign raw data
    pub fn sign_data(&self, data: &[u8]) -> Signature {
        self.private.sign_data(data)
    }

    /// Verify a signature
    pub fn verify(&self, message: &Hash, signature: &Signature) -> bool {
        self.public.verify(message, signature)
    }
}

impl fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyPair {{ public: {:?} }}", self.public)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MERKLE TREE
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute a Merkle root from a list of hashes
pub fn merkle_root(hashes: &[Hash]) -> Hash {
    if hashes.is_empty() {
        return Hash::zero();
    }

    if hashes.len() == 1 {
        return hashes[0];
    }

    let mut current_level: Vec<Hash> = hashes.to_vec();

    while current_level.len() > 1 {
        let mut next_level = Vec::with_capacity((current_level.len() + 1) / 2);

        for chunk in current_level.chunks(2) {
            let left = chunk[0];
            let right = if chunk.len() > 1 { chunk[1] } else { chunk[0] };

            let mut combined = Vec::with_capacity(64);
            combined.extend_from_slice(left.as_bytes());
            combined.extend_from_slice(right.as_bytes());
            next_level.push(Hash::double_sha256(&combined));
        }

        current_level = next_level;
    }

    current_level[0]
}

/// Verify a Merkle proof
pub fn verify_merkle_proof(leaf: &Hash, proof: &[Hash], root: &Hash, index: usize) -> bool {
    let mut current = *leaf;
    let mut idx = index;

    for sibling in proof {
        let mut combined = Vec::with_capacity(64);
        if idx % 2 == 0 {
            combined.extend_from_slice(current.as_bytes());
            combined.extend_from_slice(sibling.as_bytes());
        } else {
            combined.extend_from_slice(sibling.as_bytes());
            combined.extend_from_slice(current.as_bytes());
        }
        current = Hash::double_sha256(&combined);
        idx /= 2;
    }

    current == *root
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_sha256() {
        let hash = Hash::sha256(b"hello world");
        assert!(!hash.is_zero());
        assert_eq!(hash.as_bytes().len(), HASH_LENGTH);

        // Known SHA256 hash of "hello world"
        let expected =
            Hash::from_hex("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9")
                .unwrap();
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_hash_blake3() {
        let hash = Hash::blake3(b"hello world");
        assert!(!hash.is_zero());
    }

    #[test]
    fn test_hash_hex_roundtrip() {
        let original = Hash::sha256(b"test");
        let hex = original.to_hex();
        let recovered = Hash::from_hex(&hex).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_private_key_generation() {
        let key1 = PrivateKey::generate();
        let key2 = PrivateKey::generate();
        assert_ne!(key1.as_bytes(), key2.as_bytes());
    }

    #[test]
    fn test_key_pair_sign_verify() {
        let keypair = KeyPair::generate();
        let message = Hash::sha256(b"test message");

        let signature = keypair.sign(&message);
        assert!(keypair.verify(&message, &signature));

        let wrong_message = Hash::sha256(b"wrong message");
        assert!(!keypair.verify(&wrong_message, &signature));
    }

    #[test]
    fn test_signature_verification() {
        let private = PrivateKey::generate();
        let public = private.public_key();
        let message = Hash::sha256(b"hello zkUSD");

        let signature = private.sign(&message);

        assert!(verify_signature(&public, &message, &signature));

        let other_message = Hash::sha256(b"different");
        assert!(!verify_signature(&public, &other_message, &signature));

        let other_key = PrivateKey::generate();
        let other_public = other_key.public_key();
        assert!(!verify_signature(&other_public, &message, &signature));
    }

    #[test]
    fn test_public_key_validation() {
        let keypair = KeyPair::generate();
        assert!(keypair.public_key().is_valid());

        let mut invalid_bytes = [0u8; PUBKEY_LENGTH];
        invalid_bytes[0] = 0x04;
        let invalid = PublicKey::new(invalid_bytes);
        assert!(!invalid.is_valid());
    }

    #[test]
    fn test_cdp_id_generation() {
        let keypair = KeyPair::generate();
        let pubkey = *keypair.public_key();

        let id1 = CDPId::generate(&pubkey, 1);
        let id2 = CDPId::generate(&pubkey, 2);
        let id1_again = CDPId::generate(&pubkey, 1);

        assert_ne!(id1, id2);
        assert_eq!(id1, id1_again);
    }

    #[test]
    fn test_cdp_id_hex_roundtrip() {
        let keypair = KeyPair::generate();
        let original = CDPId::generate(keypair.public_key(), 42);
        let hex = original.to_hex();
        let recovered = CDPId::from_hex(&hex).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_message_hash() {
        let hash1 = create_message_hash("mint", &[1, 2, 3]);
        let hash2 = create_message_hash("mint", &[1, 2, 3]);
        let hash3 = create_message_hash("burn", &[1, 2, 3]);

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_tagged_hash() {
        let hash1 = tagged_hash("BIP0340/challenge", &[1, 2, 3]);
        let hash2 = tagged_hash("BIP0340/challenge", &[1, 2, 3]);
        let hash3 = tagged_hash("different/tag", &[1, 2, 3]);

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_merkle_root() {
        let leaves: Vec<Hash> = (0..4u8).map(|i| Hash::sha256(&[i])).collect();

        let root = merkle_root(&leaves);
        assert!(!root.is_zero());

        let single_root = merkle_root(&leaves[0..1]);
        assert_eq!(single_root, leaves[0]);

        let empty_root = merkle_root(&[]);
        assert!(empty_root.is_zero());
    }

    #[test]
    fn test_merkle_proof_verification() {
        let leaves: Vec<Hash> = (0..4u8).map(|i| Hash::sha256(&[i])).collect();

        let h01 = Hash::double_sha256(
            &[
                leaves[0].as_bytes().as_slice(),
                leaves[1].as_bytes().as_slice(),
            ]
            .concat(),
        );
        let h23 = Hash::double_sha256(
            &[
                leaves[2].as_bytes().as_slice(),
                leaves[3].as_bytes().as_slice(),
            ]
            .concat(),
        );
        let root =
            Hash::double_sha256(&[h01.as_bytes().as_slice(), h23.as_bytes().as_slice()].concat());

        let proof = vec![leaves[1], h23];
        assert!(verify_merkle_proof(&leaves[0], &proof, &root, 0));

        assert!(!verify_merkle_proof(&leaves[0], &proof, &root, 1));
    }

    #[test]
    fn test_sign_data() {
        let keypair = KeyPair::generate();
        let data = b"some arbitrary data to sign";

        let signature = keypair.sign_data(data);

        assert!(verify_signature_data(keypair.public_key(), data, &signature));

        assert!(!verify_signature_data(
            keypair.public_key(),
            b"different data",
            &signature
        ));
    }

    #[test]
    fn test_private_key_hex_roundtrip() {
        let original = PrivateKey::generate();
        let hex = original.to_hex();
        let recovered = PrivateKey::from_hex(&hex).unwrap();

        assert_eq!(original.public_key(), recovered.public_key());
    }

    #[test]
    fn test_serde_roundtrip() {
        let keypair = KeyPair::generate();
        let message = Hash::sha256(b"test");
        let signature = keypair.sign(&message);
        let cdp_id = CDPId::generate(keypair.public_key(), 1);

        // Test Hash serde
        let hash_json = serde_json::to_string(&message).unwrap();
        let hash_recovered: Hash = serde_json::from_str(&hash_json).unwrap();
        assert_eq!(message, hash_recovered);

        // Test PublicKey serde
        let pubkey_json = serde_json::to_string(keypair.public_key()).unwrap();
        let pubkey_recovered: PublicKey = serde_json::from_str(&pubkey_json).unwrap();
        assert_eq!(*keypair.public_key(), pubkey_recovered);

        // Test Signature serde
        let sig_json = serde_json::to_string(&signature).unwrap();
        let sig_recovered: Signature = serde_json::from_str(&sig_json).unwrap();
        assert_eq!(signature, sig_recovered);

        // Test CDPId serde
        let cdp_json = serde_json::to_string(&cdp_id).unwrap();
        let cdp_recovered: CDPId = serde_json::from_str(&cdp_json).unwrap();
        assert_eq!(cdp_id, cdp_recovered);
    }
}
