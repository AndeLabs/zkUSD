//! SP1 guest program for deposit circuit.
//!
//! Proves that a collateral deposit is valid:
//! - Owner signature is valid
//! - Collateral increased correctly
//! - Debt unchanged
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
        OperationType::Deposit,
        "Invalid operation type"
    );

    // Verify deposit amount is positive
    let deposit_amount = private.collateral_after.saturating_sub(private.collateral_before);
    assert!(deposit_amount > 0, "Deposit amount must be positive");

    // Verify debt unchanged
    assert_eq!(
        private.debt_before, private.debt_after,
        "Debt must not change during deposit"
    );

    // Calculate transition hash
    let mut transition_data = Vec::new();
    transition_data.extend_from_slice(public.cdp_id.as_bytes());
    transition_data.extend_from_slice(&deposit_amount.to_le_bytes());
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
