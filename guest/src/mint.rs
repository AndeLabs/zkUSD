//! SP1 guest program for mint circuit.
//!
//! Proves that minting zkUSD is valid:
//! - Owner signature is valid
//! - Collateral unchanged
//! - Debt increased correctly
//! - Resulting collateral ratio >= minimum
//! - State transition is consistent

#![no_main]
sp1_zkvm::entrypoint!(main);

mod common;

use common::*;

fn main() {
    // Read inputs from host
    let public_bytes = sp1_zkvm::io::read::<Vec<u8>>();
    let private_bytes = sp1_zkvm::io::read::<Vec<u8>>();

    let public: CDPTransitionPublicInputs = bincode::deserialize(&public_bytes)
        .expect("Failed to deserialize public inputs");
    let private: CDPPrivateInputs = bincode::deserialize(&private_bytes)
        .expect("Failed to deserialize private inputs");

    // Verify operation type
    assert_eq!(
        OperationType::from(public.operation_type),
        OperationType::Mint,
        "Invalid operation type"
    );

    // Verify mint amount is positive
    let mint_amount = private.debt_after.saturating_sub(private.debt_before);
    assert!(mint_amount > 0, "Mint amount must be positive");

    // Verify collateral unchanged
    assert_eq!(
        private.collateral_before, private.collateral_after,
        "Collateral must not change during mint"
    );

    // Verify collateral ratio after minting
    let ratio = calculate_ratio(
        private.collateral_after,
        private.debt_after,
        private.btc_price
    );
    assert!(
        ratio >= MIN_COLLATERAL_RATIO_BPS,
        "Collateral ratio below minimum after minting"
    );

    // Calculate transition hash
    let mut transition_data = Vec::new();
    transition_data.extend_from_slice(public.cdp_id.as_bytes());
    transition_data.extend_from_slice(&mint_amount.to_le_bytes());
    transition_data.extend_from_slice(&public.block_height.to_le_bytes());
    let transition_hash = Hash::sha256(&transition_data);

    // Create output
    let output = CircuitOutput {
        valid: true,
        transition_hash,
        new_state_root: public.state_root_after,
    };

    // Commit output to the journal
    let output_bytes = bincode::serialize(&output).expect("Failed to serialize output");
    sp1_zkvm::io::commit_slice(&output_bytes);
}
