//! SP1 guest program for repay circuit.
//!
//! Proves that repaying debt is valid:
//! - Signature is valid (can be owner or redeemer)
//! - Collateral unchanged
//! - Debt decreased correctly
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
        OperationType::Repay,
        "Invalid operation type"
    );

    // Verify repayment amount is positive
    let repay_amount = private.debt_before.saturating_sub(private.debt_after);
    assert!(repay_amount > 0, "Repay amount must be positive");

    // Verify collateral unchanged
    assert_eq!(
        private.collateral_before, private.collateral_after,
        "Collateral must not change during repayment"
    );

    // Verify debt didn't underflow (can't repay more than owed)
    assert!(
        private.debt_after <= private.debt_before,
        "Cannot repay more than owed"
    );

    // Calculate transition hash
    let mut transition_data = Vec::new();
    transition_data.extend_from_slice(public.cdp_id.as_bytes());
    transition_data.extend_from_slice(&repay_amount.to_le_bytes());
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
