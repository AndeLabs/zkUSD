//! zkUSD Token implementation.
//!
//! This module implements the zkUSD stablecoin token:
//! - Token minting and burning
//! - Balance tracking
//! - Transfer operations
//! - Supply management

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::utils::constants::*;
use crate::utils::crypto::{Hash, PublicKey};
use crate::utils::math::*;

// ═══════════════════════════════════════════════════════════════════════════════
// TOKEN AMOUNT
// ═══════════════════════════════════════════════════════════════════════════════

/// Strongly-typed token amount (prevents mixing sats and cents)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TokenAmount(u64);

impl TokenAmount {
    /// Zero amount
    pub const ZERO: Self = Self(0);

    /// Create from cents
    pub const fn from_cents(cents: u64) -> Self {
        Self(cents)
    }

    /// Create from dollars (for convenience)
    pub fn from_dollars(dollars: u64) -> Self {
        Self(dollars * ZKUSD_BASE_UNIT)
    }

    /// Get raw cents value
    pub fn cents(&self) -> u64 {
        self.0
    }

    /// Get value in dollars (truncated)
    pub fn dollars(&self) -> u64 {
        self.0 / ZKUSD_BASE_UNIT
    }

    /// Get formatted string representation
    pub fn to_string_formatted(&self) -> String {
        let dollars = self.0 / ZKUSD_BASE_UNIT;
        let cents = self.0 % ZKUSD_BASE_UNIT;
        format!("${}.{:02}", dollars, cents)
    }

    /// Check if zero
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Saturating addition
    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// Saturating subtraction
    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    /// Checked addition
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    /// Checked subtraction
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }
}

impl std::fmt::Display for TokenAmount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string_formatted())
    }
}

impl From<u64> for TokenAmount {
    fn from(cents: u64) -> Self {
        Self(cents)
    }
}

impl From<TokenAmount> for u64 {
    fn from(amount: TokenAmount) -> Self {
        amount.0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TOKEN OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Type of token operation for event logging
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenOperation {
    /// Minting new tokens (from CDP)
    Mint,
    /// Burning tokens (repaying debt)
    Burn,
    /// Transfer between accounts
    Transfer,
    /// Redemption for collateral
    Redeem,
}

/// Record of a token operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenEvent {
    /// Type of operation
    pub operation: TokenOperation,
    /// Sender (None for mint)
    pub from: Option<PublicKey>,
    /// Recipient (None for burn)
    pub to: Option<PublicKey>,
    /// Amount in cents
    pub amount: TokenAmount,
    /// Block height
    pub block_height: u64,
    /// Hash of the transaction/spell
    pub tx_hash: Hash,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZKUSD TOKEN
// ═══════════════════════════════════════════════════════════════════════════════

/// The zkUSD stablecoin token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZkUSD {
    /// Token name
    pub name: String,
    /// Token symbol
    pub symbol: String,
    /// Decimal places
    pub decimals: u8,
    /// Total supply in cents
    total_supply: TokenAmount,
    /// Balances by public key
    balances: HashMap<PublicKey, TokenAmount>,
    /// Recent events (for client-side tracking)
    events: Vec<TokenEvent>,
    /// Maximum events to keep in memory
    max_events: usize,
}

impl Default for ZkUSD {
    fn default() -> Self {
        Self::new()
    }
}

impl ZkUSD {
    /// Create a new zkUSD token instance
    pub fn new() -> Self {
        Self {
            name: "zkUSD".to_string(),
            symbol: "zkUSD".to_string(),
            decimals: ZKUSD_DECIMALS,
            total_supply: TokenAmount::ZERO,
            balances: HashMap::new(),
            events: Vec::new(),
            max_events: 1000,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // SUPPLY MANAGEMENT
    // ═══════════════════════════════════════════════════════════════════════════

    /// Get total supply
    pub fn total_supply(&self) -> TokenAmount {
        self.total_supply
    }

    /// Get balance of an address
    pub fn balance_of(&self, owner: &PublicKey) -> TokenAmount {
        self.balances.get(owner).copied().unwrap_or(TokenAmount::ZERO)
    }

    /// Mint new tokens (only called from CDP operations)
    pub fn mint(
        &mut self,
        to: PublicKey,
        amount: TokenAmount,
        block_height: u64,
        tx_hash: Hash,
    ) -> Result<()> {
        if amount.is_zero() {
            return Err(Error::ZeroAmount);
        }

        // Check supply cap
        let new_supply = self.total_supply.checked_add(amount).ok_or(Error::Overflow {
            operation: "mint total supply".into(),
        })?;

        if new_supply.cents() > MAX_ZKUSD_SUPPLY {
            return Err(Error::DebtCeilingReached {
                current: new_supply.cents(),
                max: MAX_ZKUSD_SUPPLY,
            });
        }

        // Update balances
        let current_balance = self.balance_of(&to);
        let new_balance = current_balance.checked_add(amount).ok_or(Error::Overflow {
            operation: "mint balance".into(),
        })?;

        self.balances.insert(to, new_balance);
        self.total_supply = new_supply;

        // Record event
        self.add_event(TokenEvent {
            operation: TokenOperation::Mint,
            from: None,
            to: Some(to),
            amount,
            block_height,
            tx_hash,
        });

        Ok(())
    }

    /// Burn tokens (only called from CDP operations)
    pub fn burn(
        &mut self,
        from: PublicKey,
        amount: TokenAmount,
        block_height: u64,
        tx_hash: Hash,
    ) -> Result<()> {
        if amount.is_zero() {
            return Err(Error::ZeroAmount);
        }

        let current_balance = self.balance_of(&from);
        if current_balance < amount {
            return Err(Error::InsufficientCollateral {
                required: amount.cents(),
                available: current_balance.cents(),
            });
        }

        // Update balances
        let new_balance = current_balance.saturating_sub(amount);
        if new_balance.is_zero() {
            self.balances.remove(&from);
        } else {
            self.balances.insert(from, new_balance);
        }

        self.total_supply = self.total_supply.saturating_sub(amount);

        // Record event
        self.add_event(TokenEvent {
            operation: TokenOperation::Burn,
            from: Some(from),
            to: None,
            amount,
            block_height,
            tx_hash,
        });

        Ok(())
    }

    /// Transfer tokens between accounts
    pub fn transfer(
        &mut self,
        from: PublicKey,
        to: PublicKey,
        amount: TokenAmount,
        block_height: u64,
        tx_hash: Hash,
    ) -> Result<()> {
        if amount.is_zero() {
            return Err(Error::ZeroAmount);
        }

        if from == to {
            return Ok(()); // No-op for self-transfer
        }

        let from_balance = self.balance_of(&from);
        if from_balance < amount {
            return Err(Error::InsufficientCollateral {
                required: amount.cents(),
                available: from_balance.cents(),
            });
        }

        // Update sender balance
        let new_from_balance = from_balance.saturating_sub(amount);
        if new_from_balance.is_zero() {
            self.balances.remove(&from);
        } else {
            self.balances.insert(from, new_from_balance);
        }

        // Update recipient balance
        let to_balance = self.balance_of(&to);
        let new_to_balance = to_balance.checked_add(amount).ok_or(Error::Overflow {
            operation: "transfer balance".into(),
        })?;
        self.balances.insert(to, new_to_balance);

        // Record event
        self.add_event(TokenEvent {
            operation: TokenOperation::Transfer,
            from: Some(from),
            to: Some(to),
            amount,
            block_height,
            tx_hash,
        });

        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // QUERIES
    // ═══════════════════════════════════════════════════════════════════════════

    /// Get number of token holders
    pub fn holder_count(&self) -> usize {
        self.balances.len()
    }

    /// Get all balances (for auditing)
    pub fn all_balances(&self) -> &HashMap<PublicKey, TokenAmount> {
        &self.balances
    }

    /// Verify supply invariant (total_supply == sum of all balances)
    pub fn verify_supply_invariant(&self) -> bool {
        let sum: u64 = self.balances.values().map(|b| b.cents()).sum();
        sum == self.total_supply.cents()
    }

    /// Get recent events
    pub fn recent_events(&self) -> &[TokenEvent] {
        &self.events
    }

    /// Get events for a specific address
    pub fn events_for_address(&self, address: &PublicKey) -> Vec<&TokenEvent> {
        self.events
            .iter()
            .filter(|e| e.from.as_ref() == Some(address) || e.to.as_ref() == Some(address))
            .collect()
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // INTERNAL
    // ═══════════════════════════════════════════════════════════════════════════

    /// Add an event (with pruning)
    fn add_event(&mut self, event: TokenEvent) {
        self.events.push(event);

        // Prune old events if needed
        if self.events.len() > self.max_events {
            self.events.drain(0..self.events.len() - self.max_events);
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(|e| Error::Serialization(e.to_string()))
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes).map_err(|e| Error::Deserialization(e.to_string()))
    }

    /// Compute state hash (for ZK proofs)
    pub fn state_hash(&self) -> Hash {
        // Hash only the essential state (supply + balances)
        let mut data = Vec::new();
        data.extend_from_slice(&self.total_supply.cents().to_be_bytes());

        // Sort balances for deterministic hashing
        let mut sorted_balances: Vec<_> = self.balances.iter().collect();
        sorted_balances.sort_by_key(|(k, _)| k.as_bytes());

        for (pubkey, balance) in sorted_balances {
            data.extend_from_slice(pubkey.as_bytes());
            data.extend_from_slice(&balance.cents().to_be_bytes());
        }

        Hash::sha256(&data)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TOKEN METADATA
// ═══════════════════════════════════════════════════════════════════════════════

/// Token metadata for display purposes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenMetadata {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub description: String,
    pub website: String,
    pub logo_uri: Option<String>,
}

impl Default for TokenMetadata {
    fn default() -> Self {
        Self {
            name: "zkUSD".to_string(),
            symbol: "zkUSD".to_string(),
            decimals: ZKUSD_DECIMALS,
            description: "A decentralized stablecoin backed by Bitcoin".to_string(),
            website: "https://zkusd.io".to_string(),
            logo_uri: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pubkey() -> PublicKey {
        PublicKey::new([0x02; PUBKEY_LENGTH])
    }

    fn test_pubkey_2() -> PublicKey {
        PublicKey::new([0x03; PUBKEY_LENGTH])
    }

    fn test_hash() -> Hash {
        Hash::sha256(b"test")
    }

    #[test]
    fn test_token_amount() {
        let amount = TokenAmount::from_dollars(100);
        assert_eq!(amount.cents(), 10000);
        assert_eq!(amount.dollars(), 100);
        assert_eq!(amount.to_string_formatted(), "$100.00");
    }

    #[test]
    fn test_token_amount_arithmetic() {
        let a = TokenAmount::from_cents(100);
        let b = TokenAmount::from_cents(50);

        assert_eq!(a.saturating_add(b), TokenAmount::from_cents(150));
        assert_eq!(a.saturating_sub(b), TokenAmount::from_cents(50));
        assert_eq!(b.saturating_sub(a), TokenAmount::ZERO);
    }

    #[test]
    fn test_mint() {
        let mut token = ZkUSD::new();
        let owner = test_pubkey();

        token.mint(owner, TokenAmount::from_dollars(1000), 1, test_hash()).unwrap();

        assert_eq!(token.balance_of(&owner), TokenAmount::from_dollars(1000));
        assert_eq!(token.total_supply(), TokenAmount::from_dollars(1000));
    }

    #[test]
    fn test_burn() {
        let mut token = ZkUSD::new();
        let owner = test_pubkey();

        token.mint(owner, TokenAmount::from_dollars(1000), 1, test_hash()).unwrap();
        token.burn(owner, TokenAmount::from_dollars(400), 2, test_hash()).unwrap();

        assert_eq!(token.balance_of(&owner), TokenAmount::from_dollars(600));
        assert_eq!(token.total_supply(), TokenAmount::from_dollars(600));
    }

    #[test]
    fn test_burn_insufficient_balance() {
        let mut token = ZkUSD::new();
        let owner = test_pubkey();

        token.mint(owner, TokenAmount::from_dollars(100), 1, test_hash()).unwrap();
        let result = token.burn(owner, TokenAmount::from_dollars(200), 2, test_hash());

        assert!(result.is_err());
    }

    #[test]
    fn test_transfer() {
        let mut token = ZkUSD::new();
        let from = test_pubkey();
        let to = test_pubkey_2();

        token.mint(from, TokenAmount::from_dollars(1000), 1, test_hash()).unwrap();
        token.transfer(from, to, TokenAmount::from_dollars(300), 2, test_hash()).unwrap();

        assert_eq!(token.balance_of(&from), TokenAmount::from_dollars(700));
        assert_eq!(token.balance_of(&to), TokenAmount::from_dollars(300));
        assert_eq!(token.total_supply(), TokenAmount::from_dollars(1000));
    }

    #[test]
    fn test_supply_invariant() {
        let mut token = ZkUSD::new();
        let owner1 = test_pubkey();
        let owner2 = test_pubkey_2();

        token.mint(owner1, TokenAmount::from_dollars(1000), 1, test_hash()).unwrap();
        token.mint(owner2, TokenAmount::from_dollars(500), 2, test_hash()).unwrap();
        token.transfer(owner1, owner2, TokenAmount::from_dollars(200), 3, test_hash()).unwrap();
        token.burn(owner2, TokenAmount::from_dollars(100), 4, test_hash()).unwrap();

        assert!(token.verify_supply_invariant());
    }

    #[test]
    fn test_holder_count() {
        let mut token = ZkUSD::new();
        let owner1 = test_pubkey();
        let owner2 = test_pubkey_2();

        assert_eq!(token.holder_count(), 0);

        token.mint(owner1, TokenAmount::from_dollars(100), 1, test_hash()).unwrap();
        assert_eq!(token.holder_count(), 1);

        token.mint(owner2, TokenAmount::from_dollars(100), 2, test_hash()).unwrap();
        assert_eq!(token.holder_count(), 2);

        // Burning entire balance removes holder
        token.burn(owner1, TokenAmount::from_dollars(100), 3, test_hash()).unwrap();
        assert_eq!(token.holder_count(), 1);
    }

    #[test]
    fn test_state_hash_deterministic() {
        let mut token1 = ZkUSD::new();
        let mut token2 = ZkUSD::new();
        let owner = test_pubkey();

        token1.mint(owner, TokenAmount::from_dollars(100), 1, test_hash()).unwrap();
        token2.mint(owner, TokenAmount::from_dollars(100), 1, test_hash()).unwrap();

        assert_eq!(token1.state_hash(), token2.state_hash());
    }
}
