//! Protocol State Machine - Core orchestration engine.
//!
//! The state machine is the central coordinator for all protocol operations.
//! It ensures atomic execution, state consistency, and invariant preservation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::cdp::{CDP, CDPId, CDPManager};
use crate::core::config::ProtocolConfig;
use crate::core::token::{TokenAmount, ZkUSD};
use crate::core::vault::{CollateralAmount, Vault};
use crate::error::{Error, Result};
use crate::liquidation::stability_pool::StabilityPool;
use crate::protocol::events::*;
use crate::protocol::operations::*;
use crate::storage::backend::StorageBackend;
use crate::storage::state::{ProtocolState, StateManager, TransactionRecord, TransactionType};
use crate::utils::crypto::{verify_signature, Hash, PublicKey};
use crate::utils::math::*;

// ═══════════════════════════════════════════════════════════════════════════════
// STATE MACHINE
// ═══════════════════════════════════════════════════════════════════════════════

/// Protocol state machine - orchestrates all operations
pub struct ProtocolStateMachine<B: StorageBackend> {
    /// State manager for persistence
    state_manager: StateManager<B>,
    /// CDP manager
    cdp_manager: CDPManager,
    /// Token state
    token: ZkUSD,
    /// Vault state
    vault: Vault,
    /// Stability pool
    stability_pool: StabilityPool,
    /// Protocol configuration
    config: ProtocolConfig,
    /// Current BTC price in cents
    current_price: u64,
    /// Current block height
    block_height: u64,
    /// Current timestamp
    timestamp: u64,
    /// Nonces for replay protection (pubkey hash -> nonce)
    nonces: HashMap<[u8; 32], u64>,
    /// Event log for current transaction
    event_log: EventLog,
    /// Whether in recovery mode
    recovery_mode: bool,
}

impl<B: StorageBackend> ProtocolStateMachine<B> {
    /// Create a new state machine with the given storage backend
    pub fn new(backend: B) -> Result<Self> {
        let state_manager = StateManager::new(backend);
        let protocol_state = state_manager.initialize_if_needed()?;

        Ok(Self {
            state_manager,
            cdp_manager: CDPManager::new(),
            token: ZkUSD::new(),
            vault: Vault::new(),
            stability_pool: StabilityPool::new(),
            config: protocol_state.config.clone(),
            current_price: 0,
            block_height: protocol_state.block_height,
            timestamp: protocol_state.last_update,
            nonces: HashMap::new(),
            event_log: EventLog::new(),
            recovery_mode: false,
        })
    }

    /// Load full state from storage
    pub fn load_state(&mut self) -> Result<()> {
        // Load all CDPs
        let cdps = self.state_manager.load_all_cdps()?;
        for cdp in cdps {
            let _ = self.cdp_manager.register(cdp);
        }

        // Load stability pool
        if let Some(pool) = self.state_manager.load_stability_pool()? {
            self.stability_pool = pool;
        }

        // Load price
        if let Some((price, _)) = self.state_manager.load_price()? {
            self.current_price = price;
        }

        // Load protocol state
        let state = self.state_manager.load_protocol_state()?;
        self.config = state.config;
        self.block_height = state.block_height;
        self.timestamp = state.last_update;

        // Check recovery mode
        self.check_recovery_mode()?;

        Ok(())
    }

    /// Save current state to storage
    pub fn save_state(&self) -> Result<()> {
        // Save protocol state
        let state = ProtocolState {
            config: self.config.clone(),
            total_supply: self.token.total_supply().cents(),
            total_collateral: self.vault.total_collateral().sats(),
            total_debt: self.total_debt(),
            active_cdps: self.cdp_manager.active_count(),
            block_height: self.block_height,
            last_update: self.timestamp,
            version: 1,
        };
        self.state_manager.save_protocol_state(&state)?;

        // Save stability pool
        self.state_manager.save_stability_pool(&self.stability_pool)?;

        // Save price
        self.state_manager.save_price(self.current_price, self.timestamp)?;

        // Flush
        self.state_manager.flush()?;

        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // BLOCK PROCESSING
    // ═══════════════════════════════════════════════════════════════════════════

    /// Begin a new block
    pub fn begin_block(&mut self, height: u64, timestamp: u64) -> Result<()> {
        self.block_height = height;
        self.timestamp = timestamp;
        self.event_log.clear();
        Ok(())
    }

    /// End the current block
    pub fn end_block(&mut self) -> Result<EventLog> {
        // Save state
        self.save_state()?;

        // Return events
        let events = std::mem::take(&mut self.event_log);
        Ok(events)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // OPERATION EXECUTION
    // ═══════════════════════════════════════════════════════════════════════════

    /// Execute a protocol operation
    pub fn execute(&mut self, op: ProtocolOperation) -> Result<OperationResult> {
        // Verify nonce
        self.verify_nonce(op.signer(), op.nonce())?;

        // Execute based on operation type
        let result = match op {
            ProtocolOperation::OpenCDP(op) => self.execute_open_cdp(op),
            ProtocolOperation::DepositCollateral(op) => self.execute_deposit(op),
            ProtocolOperation::WithdrawCollateral(op) => self.execute_withdraw(op),
            ProtocolOperation::MintDebt(op) => self.execute_mint(op),
            ProtocolOperation::RepayDebt(op) => self.execute_repay(op),
            ProtocolOperation::CloseCDP(op) => self.execute_close(op),
            ProtocolOperation::LiquidateCDP(op) => self.execute_liquidate(op),
            ProtocolOperation::Transfer(op) => self.execute_transfer(op),
            ProtocolOperation::StabilityDeposit(op) => self.execute_sp_deposit(op),
            ProtocolOperation::StabilityWithdraw(op) => self.execute_sp_withdraw(op),
            ProtocolOperation::ClaimGains(op) => self.execute_claim_gains(op),
            ProtocolOperation::Redeem(op) => self.execute_redeem(op),
            ProtocolOperation::UpdatePrice(op) => self.execute_update_price(op),
        };

        // Check recovery mode after any state change
        if result.is_ok() {
            self.check_recovery_mode()?;
        }

        result
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // CDP OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════════

    fn execute_open_cdp(&mut self, op: OpenCDPOp) -> Result<OperationResult> {
        // Verify signature
        self.verify_operation_signature(&op)?;

        // Check protocol not paused
        if self.config.paused {
            return Err(Error::ProtocolPaused);
        }

        // Create CDP
        let cdp = CDP::with_collateral(
            op.owner,
            op.collateral.sats(),
            self.block_height,
            self.timestamp,
        )?;
        let cdp_id = cdp.id;

        // Mint initial debt if requested
        let mut debt_minted = TokenAmount::from_cents(0);
        let mut ratio = u64::MAX;

        if let Some(initial_debt) = op.initial_debt {
            if initial_debt.cents() > 0 {
                // Calculate ratio
                ratio = calculate_collateral_ratio(
                    op.collateral.sats(),
                    initial_debt.cents(),
                    self.current_price,
                )?;

                // Check MCR
                let min_ratio = if self.recovery_mode {
                    self.config.params.critical_collateral_ratio
                } else {
                    self.config.effective_mcr()
                };

                if ratio < min_ratio {
                    return Err(Error::CollateralizationRatioTooLow {
                        current: ratio,
                        minimum: min_ratio,
                    });
                }

                debt_minted = initial_debt;
            }
        }

        // Add CDP to manager
        let mut cdp = cdp;
        if debt_minted.cents() > 0 {
            cdp.debt_cents = debt_minted.cents();
        }
        self.cdp_manager.register(cdp.clone())?;

        // Update vault
        let tx_hash = Hash::sha256(&bincode::serialize(&op).unwrap_or_default());
        self.vault.deposit(cdp_id, op.collateral, self.block_height, tx_hash)?;

        // Mint tokens if debt was created
        if debt_minted.cents() > 0 {
            self.token.mint(op.owner, debt_minted, self.block_height, tx_hash)?;
        }

        // Update config
        self.config.add_position(op.collateral.sats(), debt_minted.cents());

        // Save CDP
        self.state_manager.save_cdp(&cdp)?;

        // Emit event
        self.event_log.push(ProtocolEvent::CDPOpened(CDPOpenedEvent {
            cdp_id,
            owner: op.owner,
            collateral: op.collateral,
            initial_debt: debt_minted,
            ratio,
            block_height: self.block_height,
            timestamp: self.timestamp,
        }));

        // Record transaction
        let tx = TransactionRecord::new(
            TransactionType::OpenCDP,
            op.owner,
            op.collateral.sats(),
            self.timestamp,
            self.block_height,
        ).with_cdp(cdp_id);
        self.state_manager.save_transaction(&tx)?;

        Ok(OperationResult::OpenCDP(OpenCDPResult {
            cdp_id,
            ratio,
            debt_minted,
        }))
    }

    fn execute_deposit(&mut self, op: DepositCollateralOp) -> Result<OperationResult> {
        self.verify_operation_signature(&op)?;

        // Get CDP
        let cdp = self.cdp_manager.get_mut(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;

        // Deposit
        cdp.deposit_collateral(op.amount.sats(), self.block_height)?;
        let new_total = CollateralAmount::from_sats(cdp.collateral_sats);
        let new_ratio = cdp.calculate_ratio(self.current_price);

        // Update vault
        let tx_hash = Hash::sha256(&bincode::serialize(&op).unwrap_or_default());
        self.vault.deposit(op.cdp_id, op.amount, self.block_height, tx_hash)?;

        // Update config
        self.config.add_position(op.amount.sats(), 0);

        // Save CDP
        let cdp = self.cdp_manager.get(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;
        self.state_manager.save_cdp(cdp)?;

        // Emit event
        self.event_log.push(ProtocolEvent::CollateralDeposited(CollateralDepositedEvent {
            cdp_id: op.cdp_id,
            depositor: op.depositor,
            amount: op.amount,
            new_total,
            new_ratio,
            block_height: self.block_height,
            timestamp: self.timestamp,
        }));

        Ok(OperationResult::Deposit(DepositResult {
            new_total,
            new_ratio,
        }))
    }

    fn execute_withdraw(&mut self, op: WithdrawCollateralOp) -> Result<OperationResult> {
        self.verify_operation_signature(&op)?;

        // Get CDP and verify owner
        let cdp = self.cdp_manager.get(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;

        if cdp.owner != op.owner {
            return Err(Error::Unauthorized("Not CDP owner".into()));
        }

        // Calculate new ratio after withdrawal
        let new_collateral = cdp.collateral_sats.checked_sub(op.amount.sats())
            .ok_or(Error::InsufficientCollateral {
                required: op.amount.sats(),
                available: cdp.collateral_sats,
            })?;

        if cdp.debt_cents > 0 {
            let new_ratio = calculate_collateral_ratio(
                new_collateral,
                cdp.debt_cents,
                self.current_price,
            )?;

            let min_ratio = if self.recovery_mode {
                self.config.params.critical_collateral_ratio
            } else {
                self.config.effective_mcr()
            };

            if new_ratio < min_ratio {
                return Err(Error::WithdrawalWouldUndercollateralize);
            }
        }

        // Execute withdrawal
        let cdp = self.cdp_manager.get_mut(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;
        cdp.withdraw_collateral(op.amount.sats(), self.current_price, self.config.effective_mcr(), self.block_height)?;

        let remaining = CollateralAmount::from_sats(cdp.collateral_sats);
        let new_ratio = cdp.calculate_ratio(self.current_price);

        // Update vault
        let tx_hash = Hash::sha256(&bincode::serialize(&op).unwrap_or_default());
        self.vault.withdraw(op.cdp_id, op.amount, self.block_height, tx_hash)?;

        // Update config
        self.config.remove_position(op.amount.sats(), 0);

        // Save CDP
        let cdp = self.cdp_manager.get(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;
        self.state_manager.save_cdp(cdp)?;

        // Emit event
        self.event_log.push(ProtocolEvent::CollateralWithdrawn(CollateralWithdrawnEvent {
            cdp_id: op.cdp_id,
            owner: op.owner,
            amount: op.amount,
            new_total: remaining,
            new_ratio,
            block_height: self.block_height,
            timestamp: self.timestamp,
        }));

        Ok(OperationResult::Withdraw(WithdrawResult {
            withdrawn: op.amount,
            remaining,
            new_ratio,
        }))
    }

    fn execute_mint(&mut self, op: MintDebtOp) -> Result<OperationResult> {
        self.verify_operation_signature(&op)?;

        if self.config.paused {
            return Err(Error::ProtocolPaused);
        }

        // Get CDP and verify owner
        let cdp = self.cdp_manager.get(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;

        if cdp.owner != op.owner {
            return Err(Error::Unauthorized("Not CDP owner".into()));
        }

        // Calculate borrowing fee
        let fee_bps = self.config.params.borrowing_fee_bps;
        if fee_bps > op.max_fee_bps {
            return Err(Error::InvalidParameter {
                name: "fee".into(),
                reason: format!("Fee {}bps exceeds max {}bps", fee_bps, op.max_fee_bps),
            });
        }

        let fee_amount = calculate_fee_bps(op.amount.cents(), fee_bps)?;
        let gross_amount = op.amount.cents();
        let net_amount = gross_amount.saturating_sub(fee_amount);

        // Calculate new ratio
        let new_debt = cdp.debt_cents + gross_amount;
        let new_ratio = calculate_collateral_ratio(
            cdp.collateral_sats,
            new_debt,
            self.current_price,
        )?;

        let min_ratio = if self.recovery_mode {
            self.config.params.critical_collateral_ratio
        } else {
            self.config.effective_mcr()
        };

        if new_ratio < min_ratio {
            return Err(Error::CollateralizationRatioTooLow {
                current: new_ratio,
                minimum: min_ratio,
            });
        }

        // Check debt ceiling
        let new_system_debt = self.total_debt() + gross_amount;
        if new_system_debt > self.config.debt_ceiling {
            return Err(Error::DebtCeilingReached {
                current: new_system_debt,
                max: self.config.debt_ceiling,
            });
        }

        // Execute mint
        let cdp = self.cdp_manager.get_mut(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;
        let _net_mint = cdp.mint_debt(gross_amount, self.current_price, min_ratio, self.timestamp)?;

        // Mint tokens
        let tx_hash = Hash::sha256(&bincode::serialize(&op).unwrap_or_default());
        self.token.mint(op.owner, TokenAmount::from_cents(net_amount), self.block_height, tx_hash)?;

        // Update config
        self.config.add_position(0, gross_amount);

        // Save CDP
        let cdp = self.cdp_manager.get(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;
        self.state_manager.save_cdp(cdp)?;

        // Emit event
        self.event_log.push(ProtocolEvent::DebtMinted(DebtMintedEvent {
            cdp_id: op.cdp_id,
            owner: op.owner,
            gross_amount: TokenAmount::from_cents(gross_amount),
            fee: TokenAmount::from_cents(fee_amount),
            net_amount: TokenAmount::from_cents(net_amount),
            new_debt: TokenAmount::from_cents(new_debt),
            new_ratio,
            block_height: self.block_height,
            timestamp: self.timestamp,
        }));

        Ok(OperationResult::Mint(MintResult {
            gross_amount: TokenAmount::from_cents(gross_amount),
            fee: TokenAmount::from_cents(fee_amount),
            net_amount: TokenAmount::from_cents(net_amount),
            new_debt: TokenAmount::from_cents(new_debt),
            new_ratio,
        }))
    }

    fn execute_repay(&mut self, op: RepayDebtOp) -> Result<OperationResult> {
        self.verify_operation_signature(&op)?;

        // Get CDP
        let cdp = self.cdp_manager.get(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;
        let current_debt = cdp.debt_cents;

        // Calculate repayment
        let repay_amount = op.amount.cents().min(current_debt);
        let remaining_debt = current_debt - repay_amount;

        // Burn tokens from payer
        let tx_hash = Hash::sha256(&bincode::serialize(&op).unwrap_or_default());
        self.token.burn(op.payer, TokenAmount::from_cents(repay_amount), self.block_height, tx_hash)?;

        // Execute repayment
        let cdp = self.cdp_manager.get_mut(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;
        cdp.repay_debt(repay_amount, self.block_height)?;

        let new_ratio = if remaining_debt == 0 {
            u64::MAX
        } else {
            cdp.calculate_ratio(self.current_price)
        };

        // Update config
        self.config.remove_position(0, repay_amount);

        // Save CDP
        let cdp = self.cdp_manager.get(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;
        self.state_manager.save_cdp(cdp)?;

        // Emit event
        self.event_log.push(ProtocolEvent::DebtRepaid(DebtRepaidEvent {
            cdp_id: op.cdp_id,
            payer: op.payer,
            amount: TokenAmount::from_cents(repay_amount),
            remaining_debt: TokenAmount::from_cents(remaining_debt),
            new_ratio,
            block_height: self.block_height,
            timestamp: self.timestamp,
        }));

        Ok(OperationResult::Repay(RepayResult {
            amount_repaid: TokenAmount::from_cents(repay_amount),
            remaining_debt: TokenAmount::from_cents(remaining_debt),
            new_ratio,
        }))
    }

    fn execute_close(&mut self, op: CloseCDPOp) -> Result<OperationResult> {
        self.verify_operation_signature(&op)?;

        // Get CDP and verify owner
        let cdp = self.cdp_manager.get(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;

        if cdp.owner != op.owner {
            return Err(Error::Unauthorized("Not CDP owner".into()));
        }

        // Check no outstanding debt
        if cdp.debt_cents > 0 {
            return Err(Error::InvalidParameter {
                name: "debt".into(),
                reason: "Cannot close CDP with outstanding debt".into(),
            });
        }

        let collateral = CollateralAmount::from_sats(cdp.collateral_sats);

        // Close CDP
        let cdp = self.cdp_manager.get_mut(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;
        cdp.close(self.block_height)?;

        // Update vault
        let tx_hash = Hash::sha256(&bincode::serialize(&op).unwrap_or_default());
        self.vault.withdraw(op.cdp_id, collateral, self.block_height, tx_hash)?;

        // Update config
        self.config.remove_position(collateral.sats(), 0);

        // Save CDP
        let cdp = self.cdp_manager.get(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;
        self.state_manager.save_cdp(cdp)?;

        // Emit event
        self.event_log.push(ProtocolEvent::CDPClosed(CDPClosedEvent {
            cdp_id: op.cdp_id,
            owner: op.owner,
            collateral_returned: collateral,
            block_height: self.block_height,
            timestamp: self.timestamp,
        }));

        Ok(OperationResult::Close(CloseResult {
            collateral_returned: collateral,
        }))
    }

    fn execute_liquidate(&mut self, op: LiquidateCDPOp) -> Result<OperationResult> {
        self.verify_operation_signature(&op)?;

        // Get CDP
        let cdp = self.cdp_manager.get(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;

        // Check liquidatable
        if !cdp.is_liquidatable(self.current_price, self.config.effective_mcr()) {
            return Err(Error::CDPHealthy(op.cdp_id.to_hex()));
        }

        let owner = cdp.owner;
        let ratio_at_liquidation = cdp.calculate_ratio(self.current_price);
        let debt = cdp.debt_cents;
        let collateral = cdp.collateral_sats;

        // Execute liquidation
        let cdp = self.cdp_manager.get_mut(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;
        let liq_result = cdp.liquidate(
            self.current_price,
            self.config.effective_mcr(),
            self.block_height,
        )?;

        // Determine liquidation mode
        let (mode, bonus) = if self.stability_pool.can_absorb(TokenAmount::from_cents(debt)) {
            // Absorb through stability pool
            self.stability_pool.absorb_liquidation(
                TokenAmount::from_cents(debt),
                CollateralAmount::from_sats(collateral),
            )?;
            (LiquidationMode::StabilityPool, CollateralAmount::from_sats(0))
        } else {
            // Direct liquidation
            (LiquidationMode::Direct, CollateralAmount::from_sats(liq_result.liquidator_bonus))
        };

        // Update vault
        let tx_hash = Hash::sha256(&bincode::serialize(&op).unwrap_or_default());
        self.vault.seize(op.cdp_id, CollateralAmount::from_sats(liq_result.collateral_seized), self.block_height, tx_hash)?;

        // Update config
        self.config.remove_position(collateral, debt);

        // Save CDP
        let cdp = self.cdp_manager.get(&op.cdp_id)
            .ok_or_else(|| Error::CDPNotFound(op.cdp_id.to_hex()))?;
        self.state_manager.save_cdp(cdp)?;

        // Emit event
        self.event_log.push(ProtocolEvent::CDPLiquidated(CDPLiquidatedEvent {
            cdp_id: op.cdp_id,
            owner,
            liquidator: op.liquidator,
            debt_covered: TokenAmount::from_cents(liq_result.debt_covered),
            collateral_seized: CollateralAmount::from_sats(liq_result.collateral_seized),
            liquidator_bonus: bonus,
            ratio_at_liquidation,
            btc_price: self.current_price,
            mode,
            block_height: self.block_height,
            timestamp: self.timestamp,
        }));

        Ok(OperationResult::Liquidate(LiquidateResult {
            debt_covered: TokenAmount::from_cents(liq_result.debt_covered),
            collateral_seized: CollateralAmount::from_sats(liq_result.collateral_seized),
            liquidator_bonus: bonus,
            ratio_at_liquidation,
        }))
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TOKEN OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════════

    fn execute_transfer(&mut self, op: TransferOp) -> Result<OperationResult> {
        self.verify_operation_signature(&op)?;

        // Execute transfer
        let tx_hash = Hash::sha256(&bincode::serialize(&op).unwrap_or_default());
        self.token.transfer(op.from, op.to, op.amount, self.block_height, tx_hash)?;

        let from_balance = self.token.balance_of(&op.from);
        let to_balance = self.token.balance_of(&op.to);

        // Emit event
        self.event_log.push(ProtocolEvent::TokenTransfer(TokenTransferEvent {
            from: op.from,
            to: op.to,
            amount: op.amount,
            block_height: self.block_height,
            timestamp: self.timestamp,
        }));

        Ok(OperationResult::Transfer(TransferResult {
            from_balance,
            to_balance,
        }))
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // STABILITY POOL OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════════

    fn execute_sp_deposit(&mut self, op: StabilityDepositOp) -> Result<OperationResult> {
        self.verify_operation_signature(&op)?;

        // Burn tokens from depositor (transfer to pool)
        let tx_hash = Hash::sha256(&bincode::serialize(&op).unwrap_or_default());
        self.token.burn(op.depositor, op.amount, self.block_height, tx_hash)?;

        // Deposit to stability pool
        self.stability_pool.deposit(op.depositor, op.amount, self.block_height)?;

        let new_total = self.stability_pool.get_current_value(&op.depositor);

        // Emit event
        self.event_log.push(ProtocolEvent::StabilityDeposit(StabilityDepositEvent {
            depositor: op.depositor,
            amount: op.amount,
            new_total,
            block_height: self.block_height,
            timestamp: self.timestamp,
        }));

        Ok(OperationResult::StabilityDeposit(StabilityDepositResult { new_total }))
    }

    fn execute_sp_withdraw(&mut self, op: StabilityWithdrawOp) -> Result<OperationResult> {
        self.verify_operation_signature(&op)?;

        // Withdraw from stability pool
        let (withdrawn_amount, btc_claimed) = self.stability_pool.withdraw(&op.depositor, op.amount, self.block_height)?;

        // Mint tokens back to depositor
        let tx_hash = Hash::sha256(&bincode::serialize(&op).unwrap_or_default());
        self.token.mint(op.depositor, withdrawn_amount, self.block_height, tx_hash)?;

        let remaining = self.stability_pool.get_current_value(&op.depositor);

        // Emit event
        self.event_log.push(ProtocolEvent::StabilityWithdraw(StabilityWithdrawEvent {
            depositor: op.depositor,
            amount: withdrawn_amount,
            remaining,
            block_height: self.block_height,
            timestamp: self.timestamp,
        }));

        Ok(OperationResult::StabilityWithdraw(StabilityWithdrawResult {
            withdrawn: withdrawn_amount,
            remaining,
        }))
    }

    fn execute_claim_gains(&mut self, op: ClaimGainsOp) -> Result<OperationResult> {
        self.verify_operation_signature(&op)?;

        // Claim BTC gains
        let btc_claimed = self.stability_pool.claim_btc(&op.depositor)?;

        // Emit event
        self.event_log.push(ProtocolEvent::GainsClaimed(GainsClaimedEvent {
            depositor: op.depositor,
            btc_amount: btc_claimed,
            block_height: self.block_height,
            timestamp: self.timestamp,
        }));

        Ok(OperationResult::ClaimGains(ClaimGainsResult { btc_claimed }))
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // REDEMPTION OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════════

    fn execute_redeem(&mut self, op: RedeemOp) -> Result<OperationResult> {
        self.verify_operation_signature(&op)?;

        // Calculate fee
        let fee_bps = self.config.calculate_redemption_fee(self.timestamp);
        if fee_bps > op.max_fee_bps {
            return Err(Error::InvalidParameter {
                name: "fee".into(),
                reason: format!("Fee {}bps exceeds max {}bps", fee_bps, op.max_fee_bps),
            });
        }

        let fee_amount = calculate_fee_bps(op.amount.cents(), fee_bps)?;
        let net_redemption = op.amount.cents() - fee_amount;

        // Get sorted CDPs (by ratio, ascending)
        let sorted_cdps = self.cdp_manager.get_sorted_by_ratio(self.current_price);

        let mut remaining = net_redemption;
        let mut total_collateral = 0u64;
        let mut cdps_affected = 0u32;
        let mut cdp_updates: Vec<(CDPId, u64, u64)> = Vec::new();

        for (cdp, _ratio) in sorted_cdps {
            if remaining == 0 {
                break;
            }
            if cdp.debt_cents == 0 {
                continue;
            }

            let redeem_from_this = remaining.min(cdp.debt_cents);
            let coll_to_take = safe_mul_div(
                redeem_from_this,
                crate::utils::constants::SATS_PER_BTC,
                self.current_price,
            )?;

            cdp_updates.push((
                cdp.id,
                cdp.debt_cents.saturating_sub(redeem_from_this),
                cdp.collateral_sats.saturating_sub(coll_to_take),
            ));

            remaining -= redeem_from_this;
            total_collateral += coll_to_take;
            cdps_affected += 1;
        }

        // Apply CDP updates
        for (id, new_debt, new_coll) in cdp_updates {
            let cdp = self.cdp_manager.get_mut(&id)
                .ok_or_else(|| Error::CDPNotFound(id.to_hex()))?;
            cdp.debt_cents = new_debt;
            cdp.collateral_sats = new_coll;
            // Status is computed from debt/collateral values automatically

            let cdp = self.cdp_manager.get(&id)
                .ok_or_else(|| Error::CDPNotFound(id.to_hex()))?;
            self.state_manager.save_cdp(cdp)?;
        }

        let redeemed = op.amount.cents() - remaining;

        // Burn redeemed tokens
        let tx_hash = Hash::sha256(&bincode::serialize(&op).unwrap_or_default());
        self.token.burn(op.redeemer, TokenAmount::from_cents(redeemed), self.block_height, tx_hash)?;

        // Update base rate
        self.config.update_base_rate(redeemed, self.timestamp);

        // Emit event
        self.event_log.push(ProtocolEvent::Redemption(RedemptionEvent {
            redeemer: op.redeemer,
            zkusd_amount: TokenAmount::from_cents(redeemed),
            collateral_received: CollateralAmount::from_sats(total_collateral),
            fee: TokenAmount::from_cents(fee_amount),
            cdps_affected,
            btc_price: self.current_price,
            block_height: self.block_height,
            timestamp: self.timestamp,
        }));

        Ok(OperationResult::Redeem(RedeemResult {
            zkusd_redeemed: TokenAmount::from_cents(redeemed),
            collateral_received: CollateralAmount::from_sats(total_collateral),
            fee: TokenAmount::from_cents(fee_amount),
            cdps_affected,
        }))
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // ORACLE OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════════

    fn execute_update_price(&mut self, op: UpdatePriceOp) -> Result<OperationResult> {
        self.verify_operation_signature(&op)?;

        let previous_price = self.current_price;
        let was_recovery_mode = self.recovery_mode;

        // Update price
        self.current_price = op.price_cents;

        // Save price
        self.state_manager.save_price(op.price_cents, self.timestamp)?;
        self.state_manager.save_price_history(self.timestamp, op.price_cents)?;

        // Check recovery mode
        self.check_recovery_mode()?;
        let recovery_mode_changed = was_recovery_mode != self.recovery_mode;

        // Emit event
        self.event_log.push(ProtocolEvent::PriceUpdated(PriceUpdatedEvent {
            price_cents: op.price_cents,
            previous_price,
            source_count: op.source_count,
            confidence: op.confidence,
            block_height: self.block_height,
            timestamp: self.timestamp,
        }));

        if recovery_mode_changed {
            if self.recovery_mode {
                self.event_log.push(ProtocolEvent::RecoveryModeEntered(RecoveryModeEvent {
                    tcr: self.calculate_tcr()?,
                    block_height: self.block_height,
                    timestamp: self.timestamp,
                }));
            } else {
                self.event_log.push(ProtocolEvent::RecoveryModeExited(RecoveryModeEvent {
                    tcr: self.calculate_tcr()?,
                    block_height: self.block_height,
                    timestamp: self.timestamp,
                }));
            }
        }

        Ok(OperationResult::UpdatePrice(UpdatePriceResult {
            previous_price,
            new_price: op.price_cents,
            recovery_mode_changed,
        }))
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // HELPER METHODS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Verify operation signature
    fn verify_operation_signature<O: Operation + Serialize>(&self, op: &O) -> Result<()> {
        let data = bincode::serialize(op).map_err(|e| {
            Error::Serialization(format!("Failed to serialize operation: {}", e))
        })?;
        let hash = Hash::sha256(&data);

        if !verify_signature(op.signer(), &hash, op.signature()) {
            return Err(Error::InvalidSignature);
        }

        Ok(())
    }

    /// Verify nonce
    fn verify_nonce(&mut self, signer: &PublicKey, nonce: u64) -> Result<()> {
        let key: [u8; 32] = *Hash::sha256(signer.as_bytes()).as_bytes();
        let current = self.nonces.get(&key).copied().unwrap_or(0);

        if nonce <= current {
            return Err(Error::InvalidParameter {
                name: "nonce".into(),
                reason: format!("Nonce {} already used, expected > {}", nonce, current),
            });
        }

        self.nonces.insert(key, nonce);
        Ok(())
    }

    /// Check and update recovery mode
    fn check_recovery_mode(&mut self) -> Result<()> {
        let tcr = self.calculate_tcr()?;
        self.recovery_mode = tcr < self.config.params.critical_collateral_ratio;
        Ok(())
    }

    /// Calculate Total Collateralization Ratio
    fn calculate_tcr(&self) -> Result<u64> {
        let total_debt = self.total_debt();
        if total_debt == 0 {
            return Ok(u64::MAX);
        }
        calculate_collateral_ratio(
            self.vault.total_collateral().sats(),
            total_debt,
            self.current_price,
        )
    }

    /// Get total system debt
    fn total_debt(&self) -> u64 {
        self.token.total_supply().cents()
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // QUERY METHODS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Get current BTC price
    pub fn price(&self) -> u64 {
        self.current_price
    }

    /// Get protocol configuration
    pub fn config(&self) -> &ProtocolConfig {
        &self.config
    }

    /// Check if in recovery mode
    pub fn is_recovery_mode(&self) -> bool {
        self.recovery_mode
    }

    /// Get current block height
    pub fn block_height(&self) -> u64 {
        self.block_height
    }

    /// Get a CDP by ID
    pub fn get_cdp(&self, id: &CDPId) -> Option<&CDP> {
        self.cdp_manager.get(id)
    }

    /// Get token balance
    pub fn balance(&self, account: &PublicKey) -> TokenAmount {
        self.token.balance_of(account)
    }

    /// Get stability pool deposit
    pub fn stability_deposit(&self, depositor: &PublicKey) -> Option<TokenAmount> {
        let value = self.stability_pool.get_current_value(depositor);
        if value.is_zero() && self.stability_pool.get_deposit(depositor).is_none() {
            None
        } else {
            Some(value)
        }
    }

    /// Get total supply
    pub fn total_supply(&self) -> TokenAmount {
        self.token.total_supply()
    }

    /// Get total collateral
    pub fn total_collateral(&self) -> CollateralAmount {
        self.vault.total_collateral()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATION RESULT
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of any protocol operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationResult {
    /// Open CDP result
    OpenCDP(OpenCDPResult),
    /// Deposit result
    Deposit(DepositResult),
    /// Withdraw result
    Withdraw(WithdrawResult),
    /// Mint result
    Mint(MintResult),
    /// Repay result
    Repay(RepayResult),
    /// Close result
    Close(CloseResult),
    /// Liquidate result
    Liquidate(LiquidateResult),
    /// Transfer result
    Transfer(TransferResult),
    /// Stability deposit result
    StabilityDeposit(StabilityDepositResult),
    /// Stability withdraw result
    StabilityWithdraw(StabilityWithdrawResult),
    /// Claim gains result
    ClaimGains(ClaimGainsResult),
    /// Redeem result
    Redeem(RedeemResult),
    /// Update price result
    UpdatePrice(UpdatePriceResult),
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::backend::InMemoryStore;

    fn create_test_machine() -> ProtocolStateMachine<InMemoryStore> {
        ProtocolStateMachine::new(InMemoryStore::new()).unwrap()
    }

    #[test]
    fn test_state_machine_creation() {
        let machine = create_test_machine();
        assert_eq!(machine.price(), 0);
        assert!(!machine.is_recovery_mode());
    }

    #[test]
    fn test_begin_end_block() {
        let mut machine = create_test_machine();

        machine.begin_block(100, 1234567890).unwrap();
        assert_eq!(machine.block_height(), 100);

        let events = machine.end_block().unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_total_supply_and_collateral() {
        let machine = create_test_machine();

        assert_eq!(machine.total_supply().cents(), 0);
        assert_eq!(machine.total_collateral().sats(), 0);
    }
}
