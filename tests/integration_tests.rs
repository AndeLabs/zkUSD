//! Integration tests for zkUSD protocol.
//!
//! These tests verify the complete lifecycle of protocol operations.

use zkusd::core::cdp::{CDP, CDPId, CDPManager, CDPStatus};
use zkusd::core::token::{TokenAmount, ZkUSD};
use zkusd::core::vault::{CollateralAmount, Vault};
use zkusd::liquidation::stability_pool::StabilityPool;
use zkusd::storage::backend::{InMemoryStore, StorageBackend};
use zkusd::utils::crypto::{Hash, KeyPair, Signature};
use zkusd::utils::constants::MIN_COLLATERAL_RATIO;
use zkusd::zkp::{
    CDPTransitionPublicInputs, CDPPrivateInputs, MerkleProof,
    OperationType, DepositCircuit, Circuit,
};

// ═══════════════════════════════════════════════════════════════════════════════
// TEST HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

fn generate_test_users(count: usize) -> Vec<KeyPair> {
    (0..count).map(|_| KeyPair::generate()).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// CDP LIFECYCLE TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_full_cdp_lifecycle() {
    let mut cdp_manager = CDPManager::new();
    let mut token = ZkUSD::new();

    let user = KeyPair::generate();
    let owner = *user.public_key();
    let block_height = 1000;
    let btc_price = 100_000_00u64; // $100,000

    // Step 1: Create and register CDP
    let cdp = CDP::new(owner, 1, block_height);
    let cdp_id = cdp.id;
    cdp_manager.register(cdp).unwrap();

    // Step 2: Deposit collateral (1 BTC = 100,000,000 sats)
    let collateral_sats = 100_000_000u64;
    {
        let cdp = cdp_manager.get_mut(&cdp_id).unwrap();
        cdp.deposit_collateral(collateral_sats, block_height).unwrap();
    }

    // Step 3: Mint debt (50,000 zkUSD = 5,000,000 cents at 200% ratio)
    let debt_cents = 5_000_000u64;
    {
        let cdp = cdp_manager.get_mut(&cdp_id).unwrap();
        let net_mint = cdp.mint_debt(debt_cents, btc_price, MIN_COLLATERAL_RATIO, block_height).unwrap();
        token.mint(owner, TokenAmount::from_cents(net_mint), block_height, Hash::zero()).unwrap();
    }

    // Verify state
    {
        let cdp = cdp_manager.get(&cdp_id).unwrap();
        assert_eq!(cdp.collateral_sats, collateral_sats);
        assert_eq!(cdp.debt_cents, debt_cents);
    }

    // Step 4: Repay partial debt (25,000 zkUSD)
    let repay_amount = 2_500_000u64;
    {
        let cdp = cdp_manager.get_mut(&cdp_id).unwrap();
        cdp.repay_debt(repay_amount, block_height).unwrap();
    }
    token.burn(owner, TokenAmount::from_cents(repay_amount), block_height, Hash::zero()).unwrap();

    // Step 5: Repay remaining debt
    {
        let cdp = cdp_manager.get_mut(&cdp_id).unwrap();
        cdp.repay_debt(2_500_000, block_height).unwrap();
    }

    // Step 6: Close CDP (returns collateral)
    {
        let cdp = cdp_manager.get_mut(&cdp_id).unwrap();
        let returned = cdp.close(block_height).unwrap();
        assert_eq!(returned, collateral_sats);
        assert_eq!(cdp.status, CDPStatus::Closed);
    }
}

#[test]
fn test_cdp_collateral_ratio_enforcement() {
    let user = KeyPair::generate();
    let owner = *user.public_key();
    let block_height = 1000;
    let btc_price = 100_000_00u64; // $100,000

    // Create CDP with 1 BTC
    let mut cdp = CDP::new(owner, 1, block_height);
    cdp.deposit_collateral(100_000_000, block_height).unwrap();

    // 1 BTC at $100k = $100,000 collateral
    // MCR = 110%, so max debt = $100,000 / 1.1 = ~$90,909 = 9,090,900 cents
    // Try to mint $95,000 = 9,500,000 cents - should fail (below MCR)
    let result = cdp.mint_debt(9_500_000, btc_price, MIN_COLLATERAL_RATIO, block_height);
    assert!(result.is_err(), "Should fail due to MCR violation");

    // Mint $80,000 = 8,000,000 cents - should succeed (125% ratio)
    let result = cdp.mint_debt(8_000_000, btc_price, MIN_COLLATERAL_RATIO, block_height);
    assert!(result.is_ok(), "Should succeed at 125% ratio");

    // Check ratio
    let ratio = cdp.calculate_ratio(btc_price);
    assert!(ratio >= MIN_COLLATERAL_RATIO, "Ratio {} should be >= MCR {}", ratio, MIN_COLLATERAL_RATIO);
}

#[test]
fn test_multiple_cdps_same_owner() {
    let mut cdp_manager = CDPManager::new();

    let user = KeyPair::generate();
    let owner = *user.public_key();
    let block_height = 1000;

    // Create multiple CDPs with different nonces
    let cdp1 = CDP::new(owner, 1, block_height);
    let cdp2 = CDP::new(owner, 2, block_height);
    let cdp3 = CDP::new(owner, 3, block_height);

    let id1 = cdp1.id;
    let id2 = cdp2.id;
    let id3 = cdp3.id;

    cdp_manager.register(cdp1).unwrap();
    cdp_manager.register(cdp2).unwrap();
    cdp_manager.register(cdp3).unwrap();

    // Verify all are different
    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert_ne!(id1, id3);

    // Verify owner has 3 CDPs
    let owner_cdps = cdp_manager.get_by_owner(&owner);
    assert_eq!(owner_cdps.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════════
// STABILITY POOL TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_stability_pool_deposits_and_withdrawals() {
    let mut pool = StabilityPool::new();

    let users = generate_test_users(3);
    let block_height = 1000;

    // Multiple users deposit
    pool.deposit(*users[0].public_key(), TokenAmount::from_cents(100_000), block_height).unwrap();
    pool.deposit(*users[1].public_key(), TokenAmount::from_cents(200_000), block_height).unwrap();
    pool.deposit(*users[2].public_key(), TokenAmount::from_cents(300_000), block_height).unwrap();

    // Verify total
    assert_eq!(pool.total_deposits().cents(), 600_000);

    // Verify individual balances
    assert_eq!(pool.get_current_value(&users[0].public_key()).cents(), 100_000);
    assert_eq!(pool.get_current_value(&users[1].public_key()).cents(), 200_000);
    assert_eq!(pool.get_current_value(&users[2].public_key()).cents(), 300_000);

    // Partial withdrawal
    let (withdrawn, _gains) = pool.withdraw(&users[1].public_key(), TokenAmount::from_cents(50_000), block_height).unwrap();
    assert_eq!(withdrawn.cents(), 50_000);
    assert_eq!(pool.get_current_value(&users[1].public_key()).cents(), 150_000);

    // Total should be updated
    assert_eq!(pool.total_deposits().cents(), 550_000);
}

#[test]
fn test_stability_pool_liquidation_absorption() {
    let mut pool = StabilityPool::new();

    let users = generate_test_users(2);
    let block_height = 1000;

    // User1 deposits 100,000 (1/3 of pool)
    // User2 deposits 200,000 (2/3 of pool)
    pool.deposit(*users[0].public_key(), TokenAmount::from_cents(100_000), block_height).unwrap();
    pool.deposit(*users[1].public_key(), TokenAmount::from_cents(200_000), block_height).unwrap();

    // Simulate liquidation gain distribution
    let btc_gains = CollateralAmount::from_sats(1_000_000); // 0.01 BTC
    let debt_absorbed = TokenAmount::from_cents(50_000);

    pool.absorb_liquidation(debt_absorbed, btc_gains).unwrap();

    // After absorption, total deposits should be reduced
    assert!(pool.total_deposits().cents() < 300_000);
}

// ═══════════════════════════════════════════════════════════════════════════════
// LIQUIDATION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_liquidation_eligibility() {
    let user = KeyPair::generate();
    let owner = *user.public_key();
    let block_height = 1000;

    // Create CDP with 1 BTC collateral
    let mut cdp = CDP::new(owner, 1, block_height);
    cdp.deposit_collateral(100_000_000, block_height).unwrap();

    // Mint debt at high ratio (safe at $100k)
    let btc_price_high = 100_000_00u64;
    cdp.mint_debt(5_000_000, btc_price_high, MIN_COLLATERAL_RATIO, block_height).unwrap();

    // At $100k BTC: ratio = ($100k / $50k) * 100 = 200% - SAFE
    let ratio_high = cdp.calculate_ratio(btc_price_high);
    assert!(ratio_high >= MIN_COLLATERAL_RATIO, "Should be safe at $100k");
    assert!(!cdp.is_liquidatable(btc_price_high, MIN_COLLATERAL_RATIO));

    // At $50k BTC: ratio = ($50k / $50k) * 100 = 100% - UNSAFE
    let btc_price_low = 50_000_00u64;
    let ratio_low = cdp.calculate_ratio(btc_price_low);
    assert!(ratio_low < MIN_COLLATERAL_RATIO, "Should be unsafe at $50k");
    assert!(cdp.is_liquidatable(btc_price_low, MIN_COLLATERAL_RATIO));
}

#[test]
fn test_liquidation_execution() {
    let user = KeyPair::generate();
    let owner = *user.public_key();
    let block_height = 1000;

    // Create undercollateralized CDP
    let mut cdp = CDP::new(owner, 1, block_height);
    cdp.deposit_collateral(100_000_000, block_height).unwrap();

    // Mint at high price
    let btc_price_high = 100_000_00u64; // $100k
    cdp.mint_debt(9_000_000, btc_price_high, MIN_COLLATERAL_RATIO, block_height).unwrap(); // $90k debt

    // Price crashes
    let btc_price_crash = 90_000_00u64; // $90k - ratio now ~100%
    assert!(cdp.is_liquidatable(btc_price_crash, MIN_COLLATERAL_RATIO));

    // Execute liquidation
    let result = cdp.liquidate(btc_price_crash, MIN_COLLATERAL_RATIO, block_height);
    assert!(result.is_ok(), "Liquidation should succeed");

    let liq_result = result.unwrap();
    assert!(liq_result.collateral_seized > 0);
    assert!(liq_result.debt_covered > 0);
}

// ═══════════════════════════════════════════════════════════════════════════════
// TOKEN TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_token_supply_tracking() {
    let mut token = ZkUSD::new();
    let block_height = 1000;

    let users = generate_test_users(3);

    // Initial supply is 0
    assert_eq!(token.total_supply().cents(), 0);

    // Mint to users
    token.mint(*users[0].public_key(), TokenAmount::from_cents(100_000), block_height, Hash::zero()).unwrap();
    token.mint(*users[1].public_key(), TokenAmount::from_cents(200_000), block_height, Hash::zero()).unwrap();
    token.mint(*users[2].public_key(), TokenAmount::from_cents(300_000), block_height, Hash::zero()).unwrap();

    // Total supply should be 600,000
    assert_eq!(token.total_supply().cents(), 600_000);

    // Burn from user1
    token.burn(*users[0].public_key(), TokenAmount::from_cents(50_000), block_height, Hash::zero()).unwrap();

    // Total supply should be 550,000
    assert_eq!(token.total_supply().cents(), 550_000);

    // User1 balance should be 50,000
    assert_eq!(token.balance_of(&users[0].public_key()).cents(), 50_000);
}

#[test]
fn test_token_transfer() {
    let mut token = ZkUSD::new();
    let block_height = 1000;

    let sender = KeyPair::generate();
    let receiver = KeyPair::generate();

    // Mint to sender
    token.mint(*sender.public_key(), TokenAmount::from_cents(100_000), block_height, Hash::zero()).unwrap();

    // Transfer
    let transfer_amount = TokenAmount::from_cents(30_000);
    token.transfer(
        *sender.public_key(),
        *receiver.public_key(),
        transfer_amount,
        block_height,
        Hash::zero(),
    ).unwrap();

    // Check balances
    assert_eq!(token.balance_of(&sender.public_key()).cents(), 70_000);
    assert_eq!(token.balance_of(&receiver.public_key()).cents(), 30_000);

    // Total supply unchanged
    assert_eq!(token.total_supply().cents(), 100_000);
}

#[test]
fn test_insufficient_balance_transfer() {
    let mut token = ZkUSD::new();
    let block_height = 1000;

    let sender = KeyPair::generate();
    let receiver = KeyPair::generate();

    // Mint small amount
    token.mint(*sender.public_key(), TokenAmount::from_cents(100), block_height, Hash::zero()).unwrap();

    // Try to transfer more than balance
    let result = token.transfer(
        *sender.public_key(),
        *receiver.public_key(),
        TokenAmount::from_cents(1000),
        block_height,
        Hash::zero(),
    );

    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// VAULT TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_vault_collateral_tracking() {
    let mut vault = Vault::new();

    let users = generate_test_users(3);
    let block_height = 1000;

    // Create CDP IDs
    let cdp1 = CDPId::generate(&users[0].public_key(), 1);
    let cdp2 = CDPId::generate(&users[1].public_key(), 1);
    let cdp3 = CDPId::generate(&users[2].public_key(), 1);

    // Deposit collateral (cdp_id as value, not reference; includes tx_hash)
    vault.deposit(cdp1, CollateralAmount::from_sats(100_000_000), block_height, Hash::zero()).unwrap();
    vault.deposit(cdp2, CollateralAmount::from_sats(200_000_000), block_height, Hash::zero()).unwrap();
    vault.deposit(cdp3, CollateralAmount::from_sats(300_000_000), block_height, Hash::zero()).unwrap();

    // Check total collateral
    assert_eq!(vault.total_collateral().sats(), 600_000_000);

    // Check individual collateral
    assert_eq!(vault.collateral_of(&cdp1).sats(), 100_000_000);
    assert_eq!(vault.collateral_of(&cdp2).sats(), 200_000_000);
    assert_eq!(vault.collateral_of(&cdp3).sats(), 300_000_000);

    // Withdraw from cdp2
    vault.withdraw(cdp2, CollateralAmount::from_sats(50_000_000), block_height, Hash::zero()).unwrap();

    assert_eq!(vault.collateral_of(&cdp2).sats(), 150_000_000);
    assert_eq!(vault.total_collateral().sats(), 550_000_000);
}

// ═══════════════════════════════════════════════════════════════════════════════
// CRYPTO TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_keypair_operations() {
    let keypair = KeyPair::generate();

    // Sign and verify
    let message = Hash::sha256(b"test message");
    let signature = keypair.sign(&message);

    assert!(zkusd::utils::crypto::verify_signature(
        keypair.public_key(),
        &message,
        &signature
    ));
}

#[test]
fn test_signature_verification_fails_wrong_key() {
    let keypair1 = KeyPair::generate();
    let keypair2 = KeyPair::generate();

    let message = Hash::sha256(b"test message");
    let signature = keypair1.sign(&message);

    // Verify with wrong key should fail
    assert!(!zkusd::utils::crypto::verify_signature(
        keypair2.public_key(),
        &message,
        &signature
    ));
}

#[test]
fn test_signature_verification_fails_wrong_message() {
    let keypair = KeyPair::generate();

    let message1 = Hash::sha256(b"message 1");
    let message2 = Hash::sha256(b"message 2");

    let signature = keypair.sign(&message1);

    // Verify with wrong message should fail
    assert!(!zkusd::utils::crypto::verify_signature(
        keypair.public_key(),
        &message2,
        &signature
    ));
}

// ═══════════════════════════════════════════════════════════════════════════════
// CHARMS INTEGRATION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_charms_adapter_creation() {
    use zkusd::charms::CharmsAdapter;

    let creator = KeyPair::generate();
    let adapter = CharmsAdapter::new(*creator.public_key(), 1000);

    let metadata = adapter.get_metadata();
    assert!(metadata.is_some());

    let meta = metadata.unwrap();
    assert_eq!(meta.name, "zkUSD");
    assert_eq!(meta.symbol, "zkUSD");
    assert_eq!(meta.decimals, 2);
}

#[test]
fn test_protocol_charms_adapter() {
    use zkusd::charms::ProtocolCharmsAdapter;

    let creator = KeyPair::generate();
    let btc_price = 100_000_00; // $100k

    let adapter = ProtocolCharmsAdapter::new(*creator.public_key(), 1000, btc_price);

    let stats = adapter.statistics();
    assert_eq!(stats.total_supply, 0);
    assert_eq!(stats.btc_price, btc_price);
    assert_eq!(stats.block_height, 1000);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZKP TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_zkp_circuit_execution() {
    let keypair = KeyPair::generate();

    // Create valid deposit transition inputs
    let public = CDPTransitionPublicInputs {
        state_root_before: Hash::sha256(b"before"),
        state_root_after: Hash::sha256(b"after"),
        cdp_id: CDPId::generate(keypair.public_key(), 1),
        operation_type: OperationType::Deposit as u8,
        block_height: 1000,
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
        btc_price: 100_000_00,
        merkle_proof: MerkleProof::empty(),
    };

    // Execute circuit
    let result = DepositCircuit::execute(&public, &private);
    assert!(result.is_ok(), "Circuit execution should succeed: {:?}", result);
}

// ═══════════════════════════════════════════════════════════════════════════════
// STATE MACHINE TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_state_machine_creation() {
    use zkusd::protocol::state_machine::ProtocolStateMachine;

    let backend = InMemoryStore::new();
    let result = ProtocolStateMachine::new(backend);

    assert!(result.is_ok(), "State machine creation should succeed");
}

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASE TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_cdp_close_with_outstanding_debt() {
    let user = KeyPair::generate();
    let block_height = 1000;
    let btc_price = 100_000_00u64;

    let mut cdp = CDP::new(*user.public_key(), 1, block_height);
    cdp.deposit_collateral(100_000_000, block_height).unwrap();
    cdp.mint_debt(1_000_000, btc_price, MIN_COLLATERAL_RATIO, block_height).unwrap();

    // Try to close - should fail due to outstanding debt
    let result = cdp.close(block_height);
    assert!(result.is_err(), "Should not close CDP with debt");
}

#[test]
fn test_withdraw_would_undercollateralize() {
    let user = KeyPair::generate();
    let block_height = 1000;
    let btc_price = 100_000_00u64; // $100k

    let mut cdp = CDP::new(*user.public_key(), 1, block_height);
    cdp.deposit_collateral(100_000_000, block_height).unwrap(); // 1 BTC
    cdp.mint_debt(8_000_000, btc_price, MIN_COLLATERAL_RATIO, block_height).unwrap(); // $80k debt

    // Try to withdraw most collateral - should fail
    let result = cdp.withdraw_collateral(90_000_000, btc_price, MIN_COLLATERAL_RATIO, block_height);
    assert!(result.is_err(), "Should not allow withdrawal that undercollateralizes");
}

#[test]
fn test_hash_determinism() {
    let data = b"test data for hashing";

    let hash1 = Hash::sha256(data);
    let hash2 = Hash::sha256(data);

    assert_eq!(hash1, hash2, "Same data should produce same hash");

    let different_hash = Hash::sha256(b"different data");
    assert_ne!(hash1, different_hash, "Different data should produce different hash");
}

#[test]
fn test_cdp_id_generation() {
    let user = KeyPair::generate();

    let id1 = CDPId::generate(&user.public_key(), 1);
    let id2 = CDPId::generate(&user.public_key(), 2);
    let id3 = CDPId::generate(&user.public_key(), 1); // Same nonce

    assert_ne!(id1, id2, "Different nonces should produce different IDs");
    assert_eq!(id1, id3, "Same owner and nonce should produce same ID");
}

// ═══════════════════════════════════════════════════════════════════════════════
// STORAGE BACKEND TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_in_memory_store() {
    let store = InMemoryStore::new();

    // Set and get
    store.set(b"key1", b"value1").unwrap();
    let value = store.get(b"key1").unwrap();
    assert_eq!(value, Some(b"value1".to_vec()));

    // Check exists
    assert!(store.exists(b"key1").unwrap());
    assert!(!store.exists(b"nonexistent").unwrap());

    // Delete
    store.delete(b"key1").unwrap();
    assert!(!store.exists(b"key1").unwrap());
}

#[test]
fn test_storage_prefix_listing() {
    let store = InMemoryStore::new();

    store.set(b"cdp:1", b"data1").unwrap();
    store.set(b"cdp:2", b"data2").unwrap();
    store.set(b"cdp:3", b"data3").unwrap();
    store.set(b"user:1", b"user1").unwrap();

    let cdp_keys = store.list_prefix(b"cdp:").unwrap();
    assert_eq!(cdp_keys.len(), 3);

    let user_keys = store.list_prefix(b"user:").unwrap();
    assert_eq!(user_keys.len(), 1);
}
