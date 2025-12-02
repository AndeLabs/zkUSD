//! Protocol events for state change notifications.
//!
//! Events are emitted for all significant state changes in the protocol,
//! enabling clients to track activity and react accordingly.

use serde::{Deserialize, Serialize};

use crate::core::cdp::CDPId;
use crate::core::token::TokenAmount;
use crate::core::vault::CollateralAmount;
use crate::utils::crypto::{Hash, PublicKey};

// ═══════════════════════════════════════════════════════════════════════════════
// EVENT TYPES
// ═══════════════════════════════════════════════════════════════════════════════

/// All protocol event types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProtocolEvent {
    // CDP Events
    /// CDP was opened
    CDPOpened(CDPOpenedEvent),
    /// Collateral was deposited to CDP
    CollateralDeposited(CollateralDepositedEvent),
    /// Collateral was withdrawn from CDP
    CollateralWithdrawn(CollateralWithdrawnEvent),
    /// zkUSD was minted from CDP
    DebtMinted(DebtMintedEvent),
    /// zkUSD was repaid to CDP
    DebtRepaid(DebtRepaidEvent),
    /// CDP was closed
    CDPClosed(CDPClosedEvent),
    /// CDP was liquidated
    CDPLiquidated(CDPLiquidatedEvent),

    // Token Events
    /// zkUSD was transferred
    TokenTransfer(TokenTransferEvent),

    // Stability Pool Events
    /// Deposit to stability pool
    StabilityDeposit(StabilityDepositEvent),
    /// Withdrawal from stability pool
    StabilityWithdraw(StabilityWithdrawEvent),
    /// BTC gains claimed
    GainsClaimed(GainsClaimedEvent),
    /// Liquidation absorbed by pool
    LiquidationAbsorbed(LiquidationAbsorbedEvent),

    // Redemption Events
    /// zkUSD redeemed for collateral
    Redemption(RedemptionEvent),

    // Oracle Events
    /// Price updated
    PriceUpdated(PriceUpdatedEvent),

    // Protocol Events
    /// Protocol configuration changed
    ConfigChanged(ConfigChangedEvent),
    /// Recovery mode entered
    RecoveryModeEntered(RecoveryModeEvent),
    /// Recovery mode exited
    RecoveryModeExited(RecoveryModeEvent),
}

impl ProtocolEvent {
    /// Get the event type as a string
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::CDPOpened(_) => "CDPOpened",
            Self::CollateralDeposited(_) => "CollateralDeposited",
            Self::CollateralWithdrawn(_) => "CollateralWithdrawn",
            Self::DebtMinted(_) => "DebtMinted",
            Self::DebtRepaid(_) => "DebtRepaid",
            Self::CDPClosed(_) => "CDPClosed",
            Self::CDPLiquidated(_) => "CDPLiquidated",
            Self::TokenTransfer(_) => "TokenTransfer",
            Self::StabilityDeposit(_) => "StabilityDeposit",
            Self::StabilityWithdraw(_) => "StabilityWithdraw",
            Self::GainsClaimed(_) => "GainsClaimed",
            Self::LiquidationAbsorbed(_) => "LiquidationAbsorbed",
            Self::Redemption(_) => "Redemption",
            Self::PriceUpdated(_) => "PriceUpdated",
            Self::ConfigChanged(_) => "ConfigChanged",
            Self::RecoveryModeEntered(_) => "RecoveryModeEntered",
            Self::RecoveryModeExited(_) => "RecoveryModeExited",
        }
    }

    /// Get the timestamp of the event
    pub fn timestamp(&self) -> u64 {
        match self {
            Self::CDPOpened(e) => e.timestamp,
            Self::CollateralDeposited(e) => e.timestamp,
            Self::CollateralWithdrawn(e) => e.timestamp,
            Self::DebtMinted(e) => e.timestamp,
            Self::DebtRepaid(e) => e.timestamp,
            Self::CDPClosed(e) => e.timestamp,
            Self::CDPLiquidated(e) => e.timestamp,
            Self::TokenTransfer(e) => e.timestamp,
            Self::StabilityDeposit(e) => e.timestamp,
            Self::StabilityWithdraw(e) => e.timestamp,
            Self::GainsClaimed(e) => e.timestamp,
            Self::LiquidationAbsorbed(e) => e.timestamp,
            Self::Redemption(e) => e.timestamp,
            Self::PriceUpdated(e) => e.timestamp,
            Self::ConfigChanged(e) => e.timestamp,
            Self::RecoveryModeEntered(e) => e.timestamp,
            Self::RecoveryModeExited(e) => e.timestamp,
        }
    }

    /// Get the block height of the event
    pub fn block_height(&self) -> u64 {
        match self {
            Self::CDPOpened(e) => e.block_height,
            Self::CollateralDeposited(e) => e.block_height,
            Self::CollateralWithdrawn(e) => e.block_height,
            Self::DebtMinted(e) => e.block_height,
            Self::DebtRepaid(e) => e.block_height,
            Self::CDPClosed(e) => e.block_height,
            Self::CDPLiquidated(e) => e.block_height,
            Self::TokenTransfer(e) => e.block_height,
            Self::StabilityDeposit(e) => e.block_height,
            Self::StabilityWithdraw(e) => e.block_height,
            Self::GainsClaimed(e) => e.block_height,
            Self::LiquidationAbsorbed(e) => e.block_height,
            Self::Redemption(e) => e.block_height,
            Self::PriceUpdated(e) => e.block_height,
            Self::ConfigChanged(e) => e.block_height,
            Self::RecoveryModeEntered(e) => e.block_height,
            Self::RecoveryModeExited(e) => e.block_height,
        }
    }

    /// Compute event hash
    pub fn hash(&self) -> Hash {
        let data = bincode::serialize(self).unwrap_or_default();
        Hash::sha256(&data)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CDP EVENTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Event emitted when a CDP is opened
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CDPOpenedEvent {
    /// CDP identifier
    pub cdp_id: CDPId,
    /// Owner public key
    pub owner: PublicKey,
    /// Initial collateral amount
    pub collateral: CollateralAmount,
    /// Initial debt amount (may be zero)
    pub initial_debt: TokenAmount,
    /// Collateralization ratio
    pub ratio: u64,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

/// Event emitted when collateral is deposited
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollateralDepositedEvent {
    /// CDP identifier
    pub cdp_id: CDPId,
    /// Depositor (may be different from owner)
    pub depositor: PublicKey,
    /// Amount deposited
    pub amount: CollateralAmount,
    /// New total collateral
    pub new_total: CollateralAmount,
    /// New ratio
    pub new_ratio: u64,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

/// Event emitted when collateral is withdrawn
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollateralWithdrawnEvent {
    /// CDP identifier
    pub cdp_id: CDPId,
    /// Owner who withdrew
    pub owner: PublicKey,
    /// Amount withdrawn
    pub amount: CollateralAmount,
    /// New total collateral
    pub new_total: CollateralAmount,
    /// New ratio
    pub new_ratio: u64,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

/// Event emitted when debt is minted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebtMintedEvent {
    /// CDP identifier
    pub cdp_id: CDPId,
    /// Owner who minted
    pub owner: PublicKey,
    /// Gross amount minted
    pub gross_amount: TokenAmount,
    /// Fee paid
    pub fee: TokenAmount,
    /// Net amount received
    pub net_amount: TokenAmount,
    /// New total debt
    pub new_debt: TokenAmount,
    /// New ratio
    pub new_ratio: u64,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

/// Event emitted when debt is repaid
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebtRepaidEvent {
    /// CDP identifier
    pub cdp_id: CDPId,
    /// Payer (may be different from owner)
    pub payer: PublicKey,
    /// Amount repaid
    pub amount: TokenAmount,
    /// Remaining debt
    pub remaining_debt: TokenAmount,
    /// New ratio (u64::MAX if no debt)
    pub new_ratio: u64,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

/// Event emitted when a CDP is closed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CDPClosedEvent {
    /// CDP identifier
    pub cdp_id: CDPId,
    /// Owner
    pub owner: PublicKey,
    /// Collateral returned
    pub collateral_returned: CollateralAmount,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

/// Event emitted when a CDP is liquidated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CDPLiquidatedEvent {
    /// CDP identifier
    pub cdp_id: CDPId,
    /// Previous owner
    pub owner: PublicKey,
    /// Liquidator
    pub liquidator: PublicKey,
    /// Debt that was covered
    pub debt_covered: TokenAmount,
    /// Collateral seized
    pub collateral_seized: CollateralAmount,
    /// Liquidator bonus
    pub liquidator_bonus: CollateralAmount,
    /// Collateralization ratio at liquidation
    pub ratio_at_liquidation: u64,
    /// BTC price at liquidation
    pub btc_price: u64,
    /// Liquidation mode (SP, redistribution, or direct)
    pub mode: LiquidationMode,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

/// Liquidation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiquidationMode {
    /// Absorbed by stability pool
    StabilityPool,
    /// Redistributed to other CDPs
    Redistribution,
    /// Direct liquidation by liquidator
    Direct,
}

// ═══════════════════════════════════════════════════════════════════════════════
// TOKEN EVENTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Event emitted when tokens are transferred
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenTransferEvent {
    /// Sender
    pub from: PublicKey,
    /// Recipient
    pub to: PublicKey,
    /// Amount transferred
    pub amount: TokenAmount,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// STABILITY POOL EVENTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Event emitted when depositing to stability pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityDepositEvent {
    /// Depositor
    pub depositor: PublicKey,
    /// Amount deposited
    pub amount: TokenAmount,
    /// New total deposit for this depositor
    pub new_total: TokenAmount,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

/// Event emitted when withdrawing from stability pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityWithdrawEvent {
    /// Depositor
    pub depositor: PublicKey,
    /// Amount withdrawn
    pub amount: TokenAmount,
    /// Remaining deposit
    pub remaining: TokenAmount,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

/// Event emitted when claiming BTC gains
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GainsClaimedEvent {
    /// Depositor
    pub depositor: PublicKey,
    /// BTC amount claimed
    pub btc_amount: CollateralAmount,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

/// Event emitted when stability pool absorbs liquidation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationAbsorbedEvent {
    /// CDP that was liquidated
    pub cdp_id: CDPId,
    /// Debt absorbed
    pub debt_absorbed: TokenAmount,
    /// Collateral distributed to depositors
    pub collateral_distributed: CollateralAmount,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// REDEMPTION EVENTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Event emitted when zkUSD is redeemed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedemptionEvent {
    /// Redeemer
    pub redeemer: PublicKey,
    /// zkUSD redeemed
    pub zkusd_amount: TokenAmount,
    /// Collateral received
    pub collateral_received: CollateralAmount,
    /// Fee paid
    pub fee: TokenAmount,
    /// Number of CDPs affected
    pub cdps_affected: u32,
    /// BTC price at redemption
    pub btc_price: u64,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ORACLE EVENTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Event emitted when price is updated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceUpdatedEvent {
    /// New price in cents
    pub price_cents: u64,
    /// Previous price in cents
    pub previous_price: u64,
    /// Number of sources used
    pub source_count: u8,
    /// Confidence level (0-100)
    pub confidence: u8,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROTOCOL EVENTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Event emitted when protocol configuration changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChangedEvent {
    /// Parameter that changed
    pub parameter: String,
    /// Old value (as string for flexibility)
    pub old_value: String,
    /// New value
    pub new_value: String,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

/// Event emitted for recovery mode changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryModeEvent {
    /// Total Collateralization Ratio that triggered the change
    pub tcr: u64,
    /// Block height
    pub block_height: u64,
    /// Timestamp
    pub timestamp: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// EVENT LOG
// ═══════════════════════════════════════════════════════════════════════════════

/// Collection of events from a transaction or block
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventLog {
    events: Vec<ProtocolEvent>,
}

impl EventLog {
    /// Create a new empty event log
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Add an event to the log
    pub fn push(&mut self, event: ProtocolEvent) {
        self.events.push(event);
    }

    /// Get all events
    pub fn events(&self) -> &[ProtocolEvent] {
        &self.events
    }

    /// Get events of a specific type
    pub fn filter_by_type(&self, event_type: &str) -> Vec<&ProtocolEvent> {
        self.events
            .iter()
            .filter(|e| e.event_type() == event_type)
            .collect()
    }

    /// Get the number of events
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Merge another event log into this one
    pub fn merge(&mut self, other: EventLog) {
        self.events.extend(other.events);
    }

    /// Clear all events
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Compute merkle root of all events
    pub fn merkle_root(&self) -> Hash {
        use crate::utils::crypto::merkle_root;
        let hashes: Vec<Hash> = self.events.iter().map(|e| e.hash()).collect();
        merkle_root(&hashes)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::crypto::KeyPair;

    fn test_keypair() -> KeyPair {
        KeyPair::generate()
    }

    #[test]
    fn test_event_types() {
        let keypair = test_keypair();
        let cdp_id = CDPId::generate(keypair.public_key(), 1);

        let event = ProtocolEvent::CDPOpened(CDPOpenedEvent {
            cdp_id,
            owner: *keypair.public_key(),
            collateral: CollateralAmount::from_sats(100_000_000),
            initial_debt: TokenAmount::from_cents(0),
            ratio: u64::MAX,
            block_height: 100,
            timestamp: 1234567890,
        });

        assert_eq!(event.event_type(), "CDPOpened");
        assert_eq!(event.timestamp(), 1234567890);
        assert_eq!(event.block_height(), 100);
    }

    #[test]
    fn test_event_log() {
        let keypair = test_keypair();
        let cdp_id = CDPId::generate(keypair.public_key(), 1);

        let mut log = EventLog::new();
        assert!(log.is_empty());

        log.push(ProtocolEvent::CDPOpened(CDPOpenedEvent {
            cdp_id,
            owner: *keypair.public_key(),
            collateral: CollateralAmount::from_sats(100_000_000),
            initial_debt: TokenAmount::from_cents(0),
            ratio: u64::MAX,
            block_height: 100,
            timestamp: 1234567890,
        }));

        log.push(ProtocolEvent::CollateralDeposited(CollateralDepositedEvent {
            cdp_id,
            depositor: *keypair.public_key(),
            amount: CollateralAmount::from_sats(50_000_000),
            new_total: CollateralAmount::from_sats(150_000_000),
            new_ratio: u64::MAX,
            block_height: 101,
            timestamp: 1234567900,
        }));

        assert_eq!(log.len(), 2);

        let cdp_events = log.filter_by_type("CDPOpened");
        assert_eq!(cdp_events.len(), 1);

        let deposit_events = log.filter_by_type("CollateralDeposited");
        assert_eq!(deposit_events.len(), 1);
    }

    #[test]
    fn test_event_hash() {
        let keypair = test_keypair();
        let cdp_id = CDPId::generate(keypair.public_key(), 1);

        let event = ProtocolEvent::CDPOpened(CDPOpenedEvent {
            cdp_id,
            owner: *keypair.public_key(),
            collateral: CollateralAmount::from_sats(100_000_000),
            initial_debt: TokenAmount::from_cents(0),
            ratio: u64::MAX,
            block_height: 100,
            timestamp: 1234567890,
        });

        let hash1 = event.hash();
        let hash2 = event.hash();
        assert_eq!(hash1, hash2);
        assert!(!hash1.is_zero());
    }

    #[test]
    fn test_event_log_merkle_root() {
        let keypair = test_keypair();

        let mut log = EventLog::new();

        // Empty log has zero merkle root (deterministic)
        let empty_root = log.merkle_root();
        assert!(empty_root.is_zero());

        // Add events and verify root is now non-zero
        log.push(ProtocolEvent::TokenTransfer(TokenTransferEvent {
            from: *keypair.public_key(),
            to: *keypair.public_key(),
            amount: TokenAmount::from_cents(1000),
            block_height: 100,
            timestamp: 1234567890,
        }));

        let root_with_one = log.merkle_root();
        assert!(!root_with_one.is_zero());
        assert_ne!(empty_root, root_with_one);
    }
}
