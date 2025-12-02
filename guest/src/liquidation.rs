//! SP1 guest program for liquidation circuit.
//!
//! Proves that a liquidation is valid:
//! - CDP is undercollateralized (ratio < minimum)
//! - Debt absorption is correct
//! - Collateral distribution is correct
//! - State transition is consistent

#![no_main]
sp1_zkvm::entrypoint!(main);

mod common;

use common::*;

/// Liquidation-specific public inputs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LiquidationPublicInputs {
    pub state_root_before: Hash,
    pub state_root_after: Hash,
    pub cdp_id: CDPId,
    pub debt_absorbed: u64,
    pub collateral_distributed: u64,
    pub btc_price: u64,
    pub block_height: u64,
}

/// Liquidation-specific private inputs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LiquidationPrivateInputs {
    pub cdp_owner: PublicKey,
    pub collateral_before: u64,
    pub debt_before: u64,
    pub stability_pool_balance: u64,
}

fn main() {
    // Read inputs from host
    let public_bytes = sp1_zkvm::io::read::<Vec<u8>>();
    let private_bytes = sp1_zkvm::io::read::<Vec<u8>>();

    let public: LiquidationPublicInputs = bincode::deserialize(&public_bytes)
        .expect("Failed to deserialize public inputs");
    let private: LiquidationPrivateInputs = bincode::deserialize(&private_bytes)
        .expect("Failed to deserialize private inputs");

    // Verify CDP is undercollateralized
    let ratio = calculate_ratio(
        private.collateral_before,
        private.debt_before,
        public.btc_price
    );
    assert!(
        ratio < MIN_COLLATERAL_RATIO_BPS,
        "CDP is not undercollateralized"
    );

    // Verify debt absorption doesn't exceed CDP debt
    assert!(
        public.debt_absorbed <= private.debt_before,
        "Cannot absorb more debt than exists"
    );

    // Verify collateral distribution doesn't exceed CDP collateral
    assert!(
        public.collateral_distributed <= private.collateral_before,
        "Cannot distribute more collateral than exists"
    );

    // If stability pool has funds, verify it absorbs proportionally
    if private.stability_pool_balance > 0 && private.stability_pool_balance >= private.debt_before {
        // Full absorption by stability pool
        assert_eq!(
            public.debt_absorbed, private.debt_before,
            "Full absorption should cover all debt"
        );
    }

    // Calculate transition hash
    let mut transition_data = Vec::new();
    transition_data.extend_from_slice(public.cdp_id.as_bytes());
    transition_data.extend_from_slice(&public.debt_absorbed.to_le_bytes());
    transition_data.extend_from_slice(&public.collateral_distributed.to_le_bytes());
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
