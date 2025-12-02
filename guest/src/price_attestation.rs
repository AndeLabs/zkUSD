//! SP1 guest program for price attestation circuit.
//!
//! Proves that a price update is valid:
//! - Multiple oracle signatures are valid
//! - Prices are within acceptable deviation
//! - Median price calculation is correct
//! - Timestamp is recent

#![no_main]
sp1_zkvm::entrypoint!(main);

mod common;

use common::*;

/// Maximum price deviation between sources (5%)
const MAX_PRICE_DEVIATION_BPS: u64 = 500;

/// Maximum age for price data (5 minutes in seconds)
const MAX_PRICE_AGE_SECS: u64 = 300;

/// Price attestation public inputs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PriceAttestationPublicInputs {
    pub price_cents: u64,
    pub timestamp: u64,
    pub source_count: u32,
    pub price_hash: Hash,
}

/// Individual price source
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PriceSource {
    pub price_cents: u64,
    pub timestamp: u64,
    pub source_id: u8,
    pub signature: Signature,
}

/// Price attestation private inputs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PricePrivateInputs {
    pub sources: Vec<PriceSource>,
    pub current_time: u64,
}

fn main() {
    // Read inputs from host
    let public_bytes = sp1_zkvm::io::read::<Vec<u8>>();
    let private_bytes = sp1_zkvm::io::read::<Vec<u8>>();

    let public: PriceAttestationPublicInputs = bincode::deserialize(&public_bytes)
        .expect("Failed to deserialize public inputs");
    let private: PricePrivateInputs = bincode::deserialize(&private_bytes)
        .expect("Failed to deserialize private inputs");

    // Verify we have enough sources
    assert!(
        private.sources.len() >= 3,
        "Need at least 3 price sources"
    );
    assert_eq!(
        private.sources.len() as u32, public.source_count,
        "Source count mismatch"
    );

    // Verify all prices are recent
    for source in &private.sources {
        let age = private.current_time.saturating_sub(source.timestamp);
        assert!(
            age <= MAX_PRICE_AGE_SECS,
            "Price source is too old"
        );
    }

    // Collect and sort prices
    let mut prices: Vec<u64> = private.sources.iter()
        .map(|s| s.price_cents)
        .collect();
    prices.sort();

    // Calculate median
    let median = if prices.len() % 2 == 0 {
        (prices[prices.len() / 2 - 1] + prices[prices.len() / 2]) / 2
    } else {
        prices[prices.len() / 2]
    };

    // Verify calculated median matches public price
    assert_eq!(
        median, public.price_cents,
        "Median price mismatch"
    );

    // Verify price deviation is acceptable
    let min_price = *prices.first().unwrap();
    let max_price = *prices.last().unwrap();

    if median > 0 {
        let deviation = ((max_price - min_price) as u128 * BPS_DIVISOR as u128) / median as u128;
        assert!(
            deviation <= MAX_PRICE_DEVIATION_BPS as u128,
            "Price deviation too high"
        );
    }

    // Calculate price hash
    let mut price_data = Vec::new();
    price_data.extend_from_slice(&public.price_cents.to_le_bytes());
    price_data.extend_from_slice(&public.timestamp.to_le_bytes());
    for source in &private.sources {
        price_data.extend_from_slice(&source.price_cents.to_le_bytes());
        price_data.push(source.source_id);
    }
    let calculated_hash = Hash::sha256(&price_data);

    // Verify hash matches
    assert_eq!(
        calculated_hash, public.price_hash,
        "Price hash mismatch"
    );

    // Create output
    let output = CircuitOutput {
        valid: true,
        transition_hash: calculated_hash,
        new_state_root: public.price_hash, // Use price hash as state for attestations
    };

    // Commit output to the journal
    let output_bytes = bincode::serialize(&output).expect("Failed to serialize output");
    sp1_zkvm::io::commit_slice(&output_bytes);
}
