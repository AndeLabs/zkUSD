//! Protocol operations - atomic state changes.
//!
//! Operations represent discrete actions that can be executed atomically
//! on the protocol state. Each operation validates inputs, modifies state,
//! and emits appropriate events.

use serde::{Deserialize, Serialize};

use crate::core::cdp::CDPId;
use crate::core::token::TokenAmount;
use crate::core::vault::CollateralAmount;
use crate::utils::crypto::{PublicKey, Signature};

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATION TRAIT
// ═══════════════════════════════════════════════════════════════════════════════

/// Trait for protocol operations
pub trait Operation: Sized + Send + Sync {
    /// The result type of this operation
    type Result;

    /// Get the operation type name
    fn operation_type(&self) -> &'static str;

    /// Get the signer of this operation
    fn signer(&self) -> &PublicKey;

    /// Get the signature
    fn signature(&self) -> &Signature;

    /// Get the nonce for replay protection
    fn nonce(&self) -> u64;
}

// ═══════════════════════════════════════════════════════════════════════════════
// CDP OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Open a new CDP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCDPOp {
    /// Owner of the new CDP
    pub owner: PublicKey,
    /// Initial collateral amount
    pub collateral: CollateralAmount,
    /// Optional initial debt to mint
    pub initial_debt: Option<TokenAmount>,
    /// Nonce for replay protection
    pub nonce: u64,
    /// Signature
    pub signature: Signature,
}

impl Operation for OpenCDPOp {
    type Result = OpenCDPResult;

    fn operation_type(&self) -> &'static str {
        "OpenCDP"
    }

    fn signer(&self) -> &PublicKey {
        &self.owner
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }
}

/// Result of opening a CDP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCDPResult {
    /// The new CDP ID
    pub cdp_id: CDPId,
    /// Initial ratio
    pub ratio: u64,
    /// Debt minted (if any)
    pub debt_minted: TokenAmount,
}

/// Deposit collateral to a CDP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositCollateralOp {
    /// CDP to deposit to
    pub cdp_id: CDPId,
    /// Depositor (may be different from owner)
    pub depositor: PublicKey,
    /// Amount to deposit
    pub amount: CollateralAmount,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Signature,
}

impl Operation for DepositCollateralOp {
    type Result = DepositResult;

    fn operation_type(&self) -> &'static str {
        "DepositCollateral"
    }

    fn signer(&self) -> &PublicKey {
        &self.depositor
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }
}

/// Result of depositing collateral
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositResult {
    /// New total collateral
    pub new_total: CollateralAmount,
    /// New ratio
    pub new_ratio: u64,
}

/// Withdraw collateral from a CDP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawCollateralOp {
    /// CDP to withdraw from
    pub cdp_id: CDPId,
    /// Owner (must be CDP owner)
    pub owner: PublicKey,
    /// Amount to withdraw
    pub amount: CollateralAmount,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Signature,
}

impl Operation for WithdrawCollateralOp {
    type Result = WithdrawResult;

    fn operation_type(&self) -> &'static str {
        "WithdrawCollateral"
    }

    fn signer(&self) -> &PublicKey {
        &self.owner
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }
}

/// Result of withdrawing collateral
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawResult {
    /// Amount withdrawn
    pub withdrawn: CollateralAmount,
    /// Remaining collateral
    pub remaining: CollateralAmount,
    /// New ratio
    pub new_ratio: u64,
}

/// Mint zkUSD from a CDP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintDebtOp {
    /// CDP to mint from
    pub cdp_id: CDPId,
    /// Owner (must be CDP owner)
    pub owner: PublicKey,
    /// Amount to mint
    pub amount: TokenAmount,
    /// Maximum fee willing to pay (in bps)
    pub max_fee_bps: u64,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Signature,
}

impl Operation for MintDebtOp {
    type Result = MintResult;

    fn operation_type(&self) -> &'static str {
        "MintDebt"
    }

    fn signer(&self) -> &PublicKey {
        &self.owner
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }
}

/// Result of minting debt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintResult {
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
}

/// Repay zkUSD debt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepayDebtOp {
    /// CDP to repay
    pub cdp_id: CDPId,
    /// Payer (may be different from owner)
    pub payer: PublicKey,
    /// Amount to repay
    pub amount: TokenAmount,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Signature,
}

impl Operation for RepayDebtOp {
    type Result = RepayResult;

    fn operation_type(&self) -> &'static str {
        "RepayDebt"
    }

    fn signer(&self) -> &PublicKey {
        &self.payer
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }
}

/// Result of repaying debt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepayResult {
    /// Amount repaid
    pub amount_repaid: TokenAmount,
    /// Remaining debt
    pub remaining_debt: TokenAmount,
    /// New ratio (u64::MAX if no debt)
    pub new_ratio: u64,
}

/// Close a CDP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseCDPOp {
    /// CDP to close
    pub cdp_id: CDPId,
    /// Owner (must be CDP owner)
    pub owner: PublicKey,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Signature,
}

impl Operation for CloseCDPOp {
    type Result = CloseResult;

    fn operation_type(&self) -> &'static str {
        "CloseCDP"
    }

    fn signer(&self) -> &PublicKey {
        &self.owner
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }
}

/// Result of closing a CDP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseResult {
    /// Collateral returned
    pub collateral_returned: CollateralAmount,
}

/// Liquidate a CDP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidateCDPOp {
    /// CDP to liquidate
    pub cdp_id: CDPId,
    /// Liquidator
    pub liquidator: PublicKey,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Signature,
}

impl Operation for LiquidateCDPOp {
    type Result = LiquidateResult;

    fn operation_type(&self) -> &'static str {
        "LiquidateCDP"
    }

    fn signer(&self) -> &PublicKey {
        &self.liquidator
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }
}

/// Result of liquidating a CDP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidateResult {
    /// Debt covered
    pub debt_covered: TokenAmount,
    /// Collateral seized
    pub collateral_seized: CollateralAmount,
    /// Liquidator bonus
    pub liquidator_bonus: CollateralAmount,
    /// Ratio at liquidation
    pub ratio_at_liquidation: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// TOKEN OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Transfer zkUSD
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferOp {
    /// Sender
    pub from: PublicKey,
    /// Recipient
    pub to: PublicKey,
    /// Amount
    pub amount: TokenAmount,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Signature,
}

impl Operation for TransferOp {
    type Result = TransferResult;

    fn operation_type(&self) -> &'static str {
        "Transfer"
    }

    fn signer(&self) -> &PublicKey {
        &self.from
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }
}

/// Result of transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferResult {
    /// New sender balance
    pub from_balance: TokenAmount,
    /// New recipient balance
    pub to_balance: TokenAmount,
}

// ═══════════════════════════════════════════════════════════════════════════════
// STABILITY POOL OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Deposit to stability pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityDepositOp {
    /// Depositor
    pub depositor: PublicKey,
    /// Amount to deposit
    pub amount: TokenAmount,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Signature,
}

impl Operation for StabilityDepositOp {
    type Result = StabilityDepositResult;

    fn operation_type(&self) -> &'static str {
        "StabilityDeposit"
    }

    fn signer(&self) -> &PublicKey {
        &self.depositor
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }
}

/// Result of stability pool deposit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityDepositResult {
    /// New total deposit for depositor
    pub new_total: TokenAmount,
}

/// Withdraw from stability pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityWithdrawOp {
    /// Depositor
    pub depositor: PublicKey,
    /// Amount to withdraw
    pub amount: TokenAmount,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Signature,
}

impl Operation for StabilityWithdrawOp {
    type Result = StabilityWithdrawResult;

    fn operation_type(&self) -> &'static str {
        "StabilityWithdraw"
    }

    fn signer(&self) -> &PublicKey {
        &self.depositor
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }
}

/// Result of stability pool withdrawal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityWithdrawResult {
    /// Amount withdrawn
    pub withdrawn: TokenAmount,
    /// Remaining deposit
    pub remaining: TokenAmount,
}

/// Claim BTC gains from stability pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimGainsOp {
    /// Depositor
    pub depositor: PublicKey,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Signature,
}

impl Operation for ClaimGainsOp {
    type Result = ClaimGainsResult;

    fn operation_type(&self) -> &'static str {
        "ClaimGains"
    }

    fn signer(&self) -> &PublicKey {
        &self.depositor
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }
}

/// Result of claiming gains
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimGainsResult {
    /// BTC amount claimed
    pub btc_claimed: CollateralAmount,
}

// ═══════════════════════════════════════════════════════════════════════════════
// REDEMPTION OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Redeem zkUSD for collateral
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedeemOp {
    /// Redeemer
    pub redeemer: PublicKey,
    /// Amount of zkUSD to redeem
    pub amount: TokenAmount,
    /// Maximum fee willing to pay (in bps)
    pub max_fee_bps: u64,
    /// Hint for first CDP (optimization)
    pub first_cdp_hint: Option<CDPId>,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Signature,
}

impl Operation for RedeemOp {
    type Result = RedeemResult;

    fn operation_type(&self) -> &'static str {
        "Redeem"
    }

    fn signer(&self) -> &PublicKey {
        &self.redeemer
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }
}

/// Result of redemption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedeemResult {
    /// zkUSD redeemed
    pub zkusd_redeemed: TokenAmount,
    /// Collateral received
    pub collateral_received: CollateralAmount,
    /// Fee paid
    pub fee: TokenAmount,
    /// CDPs affected count
    pub cdps_affected: u32,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ORACLE OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Update price (oracle operation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePriceOp {
    /// Oracle operator
    pub operator: PublicKey,
    /// New price in cents
    pub price_cents: u64,
    /// Source count
    pub source_count: u8,
    /// Confidence (0-100)
    pub confidence: u8,
    /// Proof data (ZK proof of price aggregation)
    pub proof: Vec<u8>,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Signature,
}

impl Operation for UpdatePriceOp {
    type Result = UpdatePriceResult;

    fn operation_type(&self) -> &'static str {
        "UpdatePrice"
    }

    fn signer(&self) -> &PublicKey {
        &self.operator
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }
}

/// Result of price update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePriceResult {
    /// Previous price
    pub previous_price: u64,
    /// New price
    pub new_price: u64,
    /// Whether recovery mode was triggered
    pub recovery_mode_changed: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
// BATCH OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Batch of operations to execute atomically
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchOp {
    /// Operations in the batch
    pub operations: Vec<ProtocolOperation>,
    /// Batch submitter
    pub submitter: PublicKey,
    /// Nonce
    pub nonce: u64,
    /// Signature over the batch
    pub signature: Signature,
}

/// All possible protocol operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProtocolOperation {
    /// Open CDP
    OpenCDP(OpenCDPOp),
    /// Deposit collateral
    DepositCollateral(DepositCollateralOp),
    /// Withdraw collateral
    WithdrawCollateral(WithdrawCollateralOp),
    /// Mint debt
    MintDebt(MintDebtOp),
    /// Repay debt
    RepayDebt(RepayDebtOp),
    /// Close CDP
    CloseCDP(CloseCDPOp),
    /// Liquidate CDP
    LiquidateCDP(LiquidateCDPOp),
    /// Transfer tokens
    Transfer(TransferOp),
    /// Stability pool deposit
    StabilityDeposit(StabilityDepositOp),
    /// Stability pool withdraw
    StabilityWithdraw(StabilityWithdrawOp),
    /// Claim gains
    ClaimGains(ClaimGainsOp),
    /// Redeem
    Redeem(RedeemOp),
    /// Update price
    UpdatePrice(UpdatePriceOp),
}

impl ProtocolOperation {
    /// Get the operation type name
    pub fn operation_type(&self) -> &'static str {
        match self {
            Self::OpenCDP(_) => "OpenCDP",
            Self::DepositCollateral(_) => "DepositCollateral",
            Self::WithdrawCollateral(_) => "WithdrawCollateral",
            Self::MintDebt(_) => "MintDebt",
            Self::RepayDebt(_) => "RepayDebt",
            Self::CloseCDP(_) => "CloseCDP",
            Self::LiquidateCDP(_) => "LiquidateCDP",
            Self::Transfer(_) => "Transfer",
            Self::StabilityDeposit(_) => "StabilityDeposit",
            Self::StabilityWithdraw(_) => "StabilityWithdraw",
            Self::ClaimGains(_) => "ClaimGains",
            Self::Redeem(_) => "Redeem",
            Self::UpdatePrice(_) => "UpdatePrice",
        }
    }

    /// Get the signer
    pub fn signer(&self) -> &PublicKey {
        match self {
            Self::OpenCDP(op) => &op.owner,
            Self::DepositCollateral(op) => &op.depositor,
            Self::WithdrawCollateral(op) => &op.owner,
            Self::MintDebt(op) => &op.owner,
            Self::RepayDebt(op) => &op.payer,
            Self::CloseCDP(op) => &op.owner,
            Self::LiquidateCDP(op) => &op.liquidator,
            Self::Transfer(op) => &op.from,
            Self::StabilityDeposit(op) => &op.depositor,
            Self::StabilityWithdraw(op) => &op.depositor,
            Self::ClaimGains(op) => &op.depositor,
            Self::Redeem(op) => &op.redeemer,
            Self::UpdatePrice(op) => &op.operator,
        }
    }

    /// Get the nonce
    pub fn nonce(&self) -> u64 {
        match self {
            Self::OpenCDP(op) => op.nonce,
            Self::DepositCollateral(op) => op.nonce,
            Self::WithdrawCollateral(op) => op.nonce,
            Self::MintDebt(op) => op.nonce,
            Self::RepayDebt(op) => op.nonce,
            Self::CloseCDP(op) => op.nonce,
            Self::LiquidateCDP(op) => op.nonce,
            Self::Transfer(op) => op.nonce,
            Self::StabilityDeposit(op) => op.nonce,
            Self::StabilityWithdraw(op) => op.nonce,
            Self::ClaimGains(op) => op.nonce,
            Self::Redeem(op) => op.nonce,
            Self::UpdatePrice(op) => op.nonce,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::crypto::KeyPair;

    #[test]
    fn test_operation_types() {
        let keypair = KeyPair::generate();

        let op = OpenCDPOp {
            owner: *keypair.public_key(),
            collateral: CollateralAmount::from_sats(100_000_000),
            initial_debt: None,
            nonce: 1,
            signature: Signature::new([0u8; 64]),
        };

        assert_eq!(op.operation_type(), "OpenCDP");
        assert_eq!(op.nonce(), 1);
    }

    #[test]
    fn test_protocol_operation_enum() {
        let keypair = KeyPair::generate();

        let op = ProtocolOperation::Transfer(TransferOp {
            from: *keypair.public_key(),
            to: *keypair.public_key(),
            amount: TokenAmount::from_cents(1000),
            nonce: 5,
            signature: Signature::new([0u8; 64]),
        });

        assert_eq!(op.operation_type(), "Transfer");
        assert_eq!(op.nonce(), 5);
    }
}
