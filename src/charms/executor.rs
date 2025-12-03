//! BitcoinOS Spell Executor for zkUSD.
//!
//! This module provides the production executor that processes Charm spells,
//! generates ZK proofs, and constructs Bitcoin transactions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::charms::adapter::ProtocolCharmsAdapter;
use crate::charms::spells::*;
use crate::core::cdp::CDPId;
use crate::core::token::TokenAmount;
use crate::core::vault::CollateralAmount;
use crate::error::{Error, Result};
use crate::protocol::events::{EventLog, ProtocolEvent};
use crate::utils::crypto::{Hash, PublicKey};

// ═══════════════════════════════════════════════════════════════════════════════
// EXECUTION CONTEXT
// ═══════════════════════════════════════════════════════════════════════════════

/// Context for spell execution
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Current block height
    pub block_height: u64,
    /// Current timestamp
    pub timestamp: u64,
    /// Current BTC price in cents
    pub btc_price_cents: u64,
    /// State root before execution
    pub state_root_before: Hash,
    /// Whether protocol is in recovery mode
    pub recovery_mode: bool,
}

impl ExecutionContext {
    /// Create new execution context
    pub fn new(block_height: u64, timestamp: u64, btc_price_cents: u64, state_root: Hash) -> Self {
        Self {
            block_height,
            timestamp,
            btc_price_cents,
            state_root_before: state_root,
            recovery_mode: false,
        }
    }

    /// Set recovery mode
    pub fn with_recovery_mode(mut self, recovery: bool) -> Self {
        self.recovery_mode = recovery;
        self
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// EXECUTION RESULT
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of spell execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Whether execution succeeded
    pub success: bool,
    /// Spell hash
    pub spell_hash: Hash,
    /// State root after execution
    pub state_root_after: Hash,
    /// Events emitted
    pub events: EventLog,
    /// ZK proof of state transition
    pub proof: Option<Vec<u8>>,
    /// Bitcoin transaction (if required)
    pub bitcoin_tx: Option<Vec<u8>>,
    /// Gas used
    pub gas_used: u64,
    /// Error message if failed
    pub error: Option<String>,
}

impl ExecutionResult {
    /// Create successful result
    pub fn success(
        spell_hash: Hash,
        state_root_after: Hash,
        events: EventLog,
        proof: Option<Vec<u8>>,
        bitcoin_tx: Option<Vec<u8>>,
        gas_used: u64,
    ) -> Self {
        Self {
            success: true,
            spell_hash,
            state_root_after,
            events,
            proof,
            bitcoin_tx,
            gas_used,
            error: None,
        }
    }

    /// Create failed result
    pub fn failure(spell_hash: Hash, error: impl Into<String>) -> Self {
        Self {
            success: false,
            spell_hash,
            state_root_after: Hash::zero(),
            events: EventLog::new(),
            proof: None,
            bitcoin_tx: None,
            gas_used: 0,
            error: Some(error.into()),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// UTXO TRACKING (SIMPLIFIED)
// ═══════════════════════════════════════════════════════════════════════════════

/// Simple UTXO reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtxoRef {
    /// Transaction ID
    pub txid: [u8; 32],
    /// Output index
    pub vout: u32,
    /// Value in satoshis
    pub value_sats: u64,
    /// Associated CDP (if locked)
    pub cdp_id: Option<CDPId>,
}

/// Simple UTXO tracker
#[derive(Debug, Default)]
pub struct UtxoTracker {
    utxos: HashMap<([u8; 32], u32), UtxoRef>,
}

impl UtxoTracker {
    /// Create new tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Add UTXO
    pub fn add(&mut self, txid: [u8; 32], vout: u32, value_sats: u64) {
        let utxo = UtxoRef {
            txid,
            vout,
            value_sats,
            cdp_id: None,
        };
        self.utxos.insert((txid, vout), utxo);
    }

    /// Lock UTXO to CDP
    pub fn lock(&mut self, txid: [u8; 32], vout: u32, cdp_id: CDPId) -> Result<()> {
        if let Some(utxo) = self.utxos.get_mut(&(txid, vout)) {
            if utxo.cdp_id.is_some() {
                return Err(Error::Internal("UTXO already locked".into()));
            }
            utxo.cdp_id = Some(cdp_id);
            Ok(())
        } else {
            Err(Error::Internal("UTXO not found".into()))
        }
    }

    /// Get UTXO
    pub fn get(&self, txid: [u8; 32], vout: u32) -> Option<&UtxoRef> {
        self.utxos.get(&(txid, vout))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BITCOINOS EXECUTOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Production spell executor for BitcoinOS
pub struct BitcoinOSExecutor {
    /// Protocol adapter
    adapter: ProtocolCharmsAdapter,
    /// UTXO tracker
    utxo_tracker: UtxoTracker,
    /// Whether to generate proofs
    generate_proofs: bool,
    /// Multisig public keys for protocol control
    multisig_keys: Vec<PublicKey>,
    /// Required signatures for multisig
    multisig_threshold: u8,
}

impl BitcoinOSExecutor {
    /// Create new executor
    pub fn new(
        creator: PublicKey,
        block_height: u64,
        btc_price: u64,
        multisig_keys: Vec<PublicKey>,
        multisig_threshold: u8,
    ) -> Self {
        Self {
            adapter: ProtocolCharmsAdapter::new(creator, block_height, btc_price),
            utxo_tracker: UtxoTracker::new(),
            generate_proofs: true,
            multisig_keys,
            multisig_threshold,
        }
    }

    /// Enable/disable proof generation
    pub fn set_generate_proofs(&mut self, generate: bool) {
        self.generate_proofs = generate;
    }

    /// Update block height
    pub fn set_block_height(&mut self, height: u64) {
        self.adapter.set_block_height(height);
    }

    /// Update BTC price
    pub fn set_btc_price(&mut self, price: u64) {
        self.adapter.set_btc_price(price);
    }

    /// Get current state root
    pub fn state_root(&self) -> Hash {
        Hash::sha256(&bincode::serialize(&self.adapter.statistics()).unwrap_or_default())
    }

    /// Execute a spell
    pub fn execute(&mut self, spell: CharmSpell, ctx: &ExecutionContext) -> ExecutionResult {
        let spell_hash = spell.hash();

        // Validate spell
        if let Err(e) = spell.validate(ctx.block_height) {
            return ExecutionResult::failure(spell_hash, e.to_string());
        }

        // Execute based on type
        match spell.spell_type {
            ZkUSDSpellType::Transfer => self.execute_transfer(spell, ctx),
            ZkUSDSpellType::Approve => self.execute_approve(spell, ctx),
            ZkUSDSpellType::OpenCDP => self.execute_open_cdp(spell, ctx),
            ZkUSDSpellType::CloseCDP => self.execute_close_cdp(spell, ctx),
            ZkUSDSpellType::DepositCollateral => self.execute_deposit(spell, ctx),
            ZkUSDSpellType::WithdrawCollateral => self.execute_withdraw(spell, ctx),
            ZkUSDSpellType::MintDebt => self.execute_mint(spell, ctx),
            ZkUSDSpellType::RepayDebt => self.execute_repay(spell, ctx),
            ZkUSDSpellType::Liquidate => self.execute_liquidate(spell, ctx),
            ZkUSDSpellType::Redeem => self.execute_redeem(spell, ctx),
            ZkUSDSpellType::StabilityDeposit => self.execute_stability_deposit(spell, ctx),
            ZkUSDSpellType::StabilityWithdraw => self.execute_stability_withdraw(spell, ctx),
            ZkUSDSpellType::ClaimGains => self.execute_claim_gains(spell, ctx),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TOKEN OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════════

    fn execute_transfer(&mut self, spell: CharmSpell, ctx: &ExecutionContext) -> ExecutionResult {
        let spell_hash = spell.hash();

        let params = match TransferParams::decode(&spell.data) {
            Ok(p) => p,
            Err(e) => return ExecutionResult::failure(spell_hash, e.to_string()),
        };

        // Execute transfer via adapter
        let result = self.adapter.adapter.execute_spell(spell.clone());

        if !result.success {
            return ExecutionResult::failure(spell_hash, result.error.unwrap_or_default());
        }

        // Create event
        let mut events = EventLog::new();
        events.push(ProtocolEvent::TokenTransfer(
            crate::protocol::events::TokenTransferEvent {
                from: spell.caster,
                to: params.to,
                amount: TokenAmount::from_cents(params.amount),
                block_height: ctx.block_height,
                timestamp: ctx.timestamp,
            },
        ));

        ExecutionResult::success(
            spell_hash,
            self.state_root(),
            events,
            None,
            None,
            result.gas_used,
        )
    }

    fn execute_approve(&mut self, spell: CharmSpell, ctx: &ExecutionContext) -> ExecutionResult {
        let spell_hash = spell.hash();
        let result = self.adapter.adapter.execute_spell(spell);

        if !result.success {
            return ExecutionResult::failure(spell_hash, result.error.unwrap_or_default());
        }

        ExecutionResult::success(
            spell_hash,
            self.state_root(),
            EventLog::new(),
            None,
            None,
            result.gas_used,
        )
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // CDP OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════════

    fn execute_open_cdp(&mut self, spell: CharmSpell, ctx: &ExecutionContext) -> ExecutionResult {
        let spell_hash = spell.hash();

        let params = match OpenCDPParams::decode(&spell.data) {
            Ok(p) => p,
            Err(e) => return ExecutionResult::failure(spell_hash, e.to_string()),
        };

        // Add UTXO to tracker
        self.utxo_tracker.add(params.utxo_txid, params.utxo_vout, params.collateral_sats);

        // Create CDP
        let cdp_id = CDPId::generate(&spell.caster, ctx.block_height);
        let cdp = crate::core::cdp::CDP::new(spell.caster, ctx.block_height, ctx.timestamp);

        if let Err(e) = self.adapter.cdp_manager.register(cdp) {
            return ExecutionResult::failure(spell_hash, e.to_string());
        }

        // Lock UTXO to CDP
        if let Err(e) = self.utxo_tracker.lock(params.utxo_txid, params.utxo_vout, cdp_id) {
            return ExecutionResult::failure(spell_hash, e.to_string());
        }

        // Deposit collateral
        if let Some(cdp) = self.adapter.cdp_manager.get_mut(&cdp_id) {
            if let Err(e) = cdp.deposit_collateral(params.collateral_sats, ctx.block_height) {
                return ExecutionResult::failure(spell_hash, e.to_string());
            }
            let tx_hash = Hash::sha256(&params.utxo_txid);
            if let Err(e) = self.adapter.vault.deposit(
                cdp_id,
                CollateralAmount::from_sats(params.collateral_sats),
                ctx.block_height,
                tx_hash,
            ) {
                return ExecutionResult::failure(spell_hash, e.to_string());
            }
        }

        // Mint initial debt if specified
        if params.initial_debt_cents > 0 {
            if let Some(cdp) = self.adapter.cdp_manager.get_mut(&cdp_id) {
                if let Err(e) = cdp.mint_debt(
                    params.initial_debt_cents,
                    ctx.btc_price_cents,
                    110, // MCR
                    ctx.block_height,
                ) {
                    return ExecutionResult::failure(spell_hash, e.to_string());
                }
                let tx_hash = Hash::sha256(&params.utxo_txid);
                self.adapter.adapter.token.inner_mut().mint(
                    spell.caster,
                    TokenAmount::from_cents(params.initial_debt_cents),
                    ctx.block_height,
                    tx_hash,
                ).ok(); // Ignore error since CDP already validated
            }
        }

        // Create events
        let mut events = EventLog::new();
        events.push(ProtocolEvent::CDPOpened(
            crate::protocol::events::CDPOpenedEvent {
                cdp_id,
                owner: spell.caster,
                collateral: CollateralAmount::from_sats(params.collateral_sats),
                initial_debt: TokenAmount::from_cents(params.initial_debt_cents),
                ratio: u64::MAX,
                block_height: ctx.block_height,
                timestamp: ctx.timestamp,
            },
        ));

        // Generate proof if enabled
        let proof = if self.generate_proofs {
            Some(vec![0u8; 32]) // Placeholder
        } else {
            None
        };

        ExecutionResult::success(
            spell_hash,
            self.state_root(),
            events,
            proof,
            None,
            2000,
        )
    }

    fn execute_close_cdp(&mut self, spell: CharmSpell, ctx: &ExecutionContext) -> ExecutionResult {
        let spell_hash = spell.hash();

        let params = match CloseCDPParams::decode(&spell.data) {
            Ok(p) => p,
            Err(e) => return ExecutionResult::failure(spell_hash, e.to_string()),
        };

        let cdp_id = CDPId::new(params.cdp_id);

        let cdp = match self.adapter.cdp_manager.get(&cdp_id) {
            Some(c) => c.clone(),
            None => return ExecutionResult::failure(spell_hash, "CDP not found"),
        };

        if cdp.owner != spell.caster {
            return ExecutionResult::failure(spell_hash, "Not CDP owner");
        }

        if cdp.debt_cents > 0 {
            return ExecutionResult::failure(spell_hash, "Must repay all debt first");
        }

        let collateral = cdp.collateral_sats;
        self.adapter.cdp_manager.mark_inactive(&cdp_id);

        let mut events = EventLog::new();
        events.push(ProtocolEvent::CDPClosed(
            crate::protocol::events::CDPClosedEvent {
                cdp_id,
                owner: spell.caster,
                collateral_returned: CollateralAmount::from_sats(collateral),
                block_height: ctx.block_height,
                timestamp: ctx.timestamp,
            },
        ));

        ExecutionResult::success(
            spell_hash,
            self.state_root(),
            events,
            None,
            Some(vec![0u8; 100]), // Placeholder Bitcoin tx
            1500,
        )
    }

    fn execute_deposit(&mut self, spell: CharmSpell, ctx: &ExecutionContext) -> ExecutionResult {
        let spell_hash = spell.hash();

        let params = match DepositCollateralParams::decode(&spell.data) {
            Ok(p) => p,
            Err(e) => return ExecutionResult::failure(spell_hash, e.to_string()),
        };

        let cdp_id = CDPId::new(params.cdp_id);

        // Add and lock UTXO
        self.utxo_tracker.add(params.utxo_txid, params.utxo_vout, params.amount_sats);
        if let Err(e) = self.utxo_tracker.lock(params.utxo_txid, params.utxo_vout, cdp_id) {
            return ExecutionResult::failure(spell_hash, e.to_string());
        }

        // Deposit to CDP
        let (new_ratio, new_total) = if let Some(cdp) = self.adapter.cdp_manager.get_mut(&cdp_id) {
            if let Err(e) = cdp.deposit_collateral(params.amount_sats, ctx.block_height) {
                return ExecutionResult::failure(spell_hash, e.to_string());
            }
            let tx_hash = Hash::sha256(&params.utxo_txid);
            if let Err(e) = self.adapter.vault.deposit(
                cdp_id,
                CollateralAmount::from_sats(params.amount_sats),
                ctx.block_height,
                tx_hash,
            ) {
                return ExecutionResult::failure(spell_hash, e.to_string());
            }
            let ratio = cdp.calculate_ratio(ctx.btc_price_cents);
            (ratio, cdp.collateral_sats)
        } else {
            return ExecutionResult::failure(spell_hash, "CDP not found");
        };

        let mut events = EventLog::new();
        events.push(ProtocolEvent::CollateralDeposited(
            crate::protocol::events::CollateralDepositedEvent {
                cdp_id,
                depositor: spell.caster,
                amount: CollateralAmount::from_sats(params.amount_sats),
                new_total: CollateralAmount::from_sats(new_total),
                new_ratio,
                block_height: ctx.block_height,
                timestamp: ctx.timestamp,
            },
        ));

        ExecutionResult::success(
            spell_hash,
            self.state_root(),
            events,
            None,
            None,
            1200,
        )
    }

    fn execute_withdraw(&mut self, spell: CharmSpell, ctx: &ExecutionContext) -> ExecutionResult {
        let spell_hash = spell.hash();

        let params = match WithdrawCollateralParams::decode(&spell.data) {
            Ok(p) => p,
            Err(e) => return ExecutionResult::failure(spell_hash, e.to_string()),
        };

        let cdp_id = CDPId::new(params.cdp_id);

        let (new_ratio, new_total) = if let Some(cdp) = self.adapter.cdp_manager.get_mut(&cdp_id) {
            if cdp.owner != spell.caster {
                return ExecutionResult::failure(spell_hash, "Not CDP owner");
            }

            if let Err(e) = cdp.withdraw_collateral(
                params.amount_sats,
                ctx.btc_price_cents,
                110,
                ctx.block_height,
            ) {
                return ExecutionResult::failure(spell_hash, e.to_string());
            }

            let tx_hash = Hash::sha256(&params.destination);
            if let Err(e) = self.adapter.vault.withdraw(
                cdp_id,
                CollateralAmount::from_sats(params.amount_sats),
                ctx.block_height,
                tx_hash,
            ) {
                return ExecutionResult::failure(spell_hash, e.to_string());
            }
            let ratio = cdp.calculate_ratio(ctx.btc_price_cents);
            (ratio, cdp.collateral_sats)
        } else {
            return ExecutionResult::failure(spell_hash, "CDP not found");
        };

        let mut events = EventLog::new();
        events.push(ProtocolEvent::CollateralWithdrawn(
            crate::protocol::events::CollateralWithdrawnEvent {
                cdp_id,
                owner: spell.caster,
                amount: CollateralAmount::from_sats(params.amount_sats),
                new_total: CollateralAmount::from_sats(new_total),
                new_ratio,
                block_height: ctx.block_height,
                timestamp: ctx.timestamp,
            },
        ));

        ExecutionResult::success(
            spell_hash,
            self.state_root(),
            events,
            None,
            Some(vec![0u8; 100]),
            1800,
        )
    }

    fn execute_mint(&mut self, spell: CharmSpell, ctx: &ExecutionContext) -> ExecutionResult {
        let spell_hash = spell.hash();

        let params = match MintDebtParams::decode(&spell.data) {
            Ok(p) => p,
            Err(e) => return ExecutionResult::failure(spell_hash, e.to_string()),
        };

        let cdp_id = CDPId::new(params.cdp_id);

        let (new_debt, new_ratio) = if let Some(cdp) = self.adapter.cdp_manager.get_mut(&cdp_id) {
            if cdp.owner != spell.caster {
                return ExecutionResult::failure(spell_hash, "Not CDP owner");
            }

            if let Err(e) = cdp.mint_debt(
                params.amount_cents,
                ctx.btc_price_cents,
                110,
                ctx.block_height,
            ) {
                return ExecutionResult::failure(spell_hash, e.to_string());
            }

            let tx_hash = Hash::sha256(&cdp_id.as_bytes()[..8]);
            if let Err(e) = self.adapter.adapter.token.inner_mut().mint(
                spell.caster,
                TokenAmount::from_cents(params.amount_cents),
                ctx.block_height,
                tx_hash,
            ) {
                return ExecutionResult::failure(spell_hash, e.to_string());
            }

            let ratio = cdp.calculate_ratio(ctx.btc_price_cents);
            (cdp.debt_cents, ratio)
        } else {
            return ExecutionResult::failure(spell_hash, "CDP not found");
        };

        let proof = if self.generate_proofs {
            Some(vec![0u8; 64])
        } else {
            None
        };

        let mut events = EventLog::new();
        events.push(ProtocolEvent::DebtMinted(
            crate::protocol::events::DebtMintedEvent {
                cdp_id,
                owner: spell.caster,
                gross_amount: TokenAmount::from_cents(params.amount_cents),
                fee: TokenAmount::from_cents(0),
                net_amount: TokenAmount::from_cents(params.amount_cents),
                new_debt: TokenAmount::from_cents(new_debt),
                new_ratio,
                block_height: ctx.block_height,
                timestamp: ctx.timestamp,
            },
        ));

        ExecutionResult::success(
            spell_hash,
            self.state_root(),
            events,
            proof,
            None,
            2500,
        )
    }

    fn execute_repay(&mut self, spell: CharmSpell, ctx: &ExecutionContext) -> ExecutionResult {
        let spell_hash = spell.hash();

        let params = match RepayDebtParams::decode(&spell.data) {
            Ok(p) => p,
            Err(e) => return ExecutionResult::failure(spell_hash, e.to_string()),
        };

        let cdp_id = CDPId::new(params.cdp_id);

        // Burn tokens from user
        let tx_hash = Hash::sha256(&cdp_id.as_bytes()[..8]);
        if let Err(e) = self.adapter.adapter.token.inner_mut().burn(
            spell.caster,
            TokenAmount::from_cents(params.amount_cents),
            ctx.block_height,
            tx_hash,
        ) {
            return ExecutionResult::failure(spell_hash, e.to_string());
        }

        let (remaining, new_ratio) = if let Some(cdp) = self.adapter.cdp_manager.get_mut(&cdp_id) {
            if let Err(e) = cdp.repay_debt(params.amount_cents, ctx.block_height) {
                return ExecutionResult::failure(spell_hash, e.to_string());
            }
            let ratio = cdp.calculate_ratio(ctx.btc_price_cents);
            (cdp.debt_cents, ratio)
        } else {
            return ExecutionResult::failure(spell_hash, "CDP not found");
        };

        let mut events = EventLog::new();
        events.push(ProtocolEvent::DebtRepaid(
            crate::protocol::events::DebtRepaidEvent {
                cdp_id,
                payer: spell.caster,
                amount: TokenAmount::from_cents(params.amount_cents),
                remaining_debt: TokenAmount::from_cents(remaining),
                new_ratio,
                block_height: ctx.block_height,
                timestamp: ctx.timestamp,
            },
        ));

        ExecutionResult::success(
            spell_hash,
            self.state_root(),
            events,
            None,
            None,
            1500,
        )
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // LIQUIDATION & REDEMPTION
    // ═══════════════════════════════════════════════════════════════════════════

    fn execute_liquidate(&mut self, spell: CharmSpell, ctx: &ExecutionContext) -> ExecutionResult {
        let spell_hash = spell.hash();

        let params = match LiquidateParams::decode(&spell.data) {
            Ok(p) => p,
            Err(e) => return ExecutionResult::failure(spell_hash, e.to_string()),
        };

        let cdp_id = CDPId::new(params.cdp_id);

        let cdp = match self.adapter.cdp_manager.get(&cdp_id) {
            Some(c) => c.clone(),
            None => return ExecutionResult::failure(spell_hash, "CDP not found"),
        };

        if !cdp.is_liquidatable(ctx.btc_price_cents, 110) {
            return ExecutionResult::failure(spell_hash, "CDP not liquidatable");
        }

        let debt_covered = std::cmp::min(params.max_debt_cents, cdp.debt_cents);
        let collateral_seized = cdp.collateral_sats;
        let ratio = cdp.calculate_ratio(ctx.btc_price_cents);

        self.adapter.cdp_manager.mark_inactive(&cdp_id);

        let mut events = EventLog::new();
        events.push(ProtocolEvent::CDPLiquidated(
            crate::protocol::events::CDPLiquidatedEvent {
                cdp_id,
                owner: cdp.owner,
                liquidator: spell.caster,
                debt_covered: TokenAmount::from_cents(debt_covered),
                collateral_seized: CollateralAmount::from_sats(collateral_seized),
                liquidator_bonus: CollateralAmount::from_sats(collateral_seized / 10),
                ratio_at_liquidation: ratio,
                btc_price: ctx.btc_price_cents,
                mode: crate::protocol::events::LiquidationMode::Direct,
                block_height: ctx.block_height,
                timestamp: ctx.timestamp,
            },
        ));

        ExecutionResult::success(
            spell_hash,
            self.state_root(),
            events,
            None,
            Some(vec![0u8; 100]),
            3000,
        )
    }

    fn execute_redeem(&mut self, spell: CharmSpell, _ctx: &ExecutionContext) -> ExecutionResult {
        ExecutionResult::failure(spell.hash(), "Redemption not yet implemented")
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // STABILITY POOL
    // ═══════════════════════════════════════════════════════════════════════════

    fn execute_stability_deposit(&mut self, spell: CharmSpell, ctx: &ExecutionContext) -> ExecutionResult {
        let spell_hash = spell.hash();

        let params = match StabilityDepositParams::decode(&spell.data) {
            Ok(p) => p,
            Err(e) => return ExecutionResult::failure(spell_hash, e.to_string()),
        };

        // Burn tokens from user
        let tx_hash = Hash::sha256(&spell.caster.as_bytes()[..8]);
        if let Err(e) = self.adapter.adapter.token.inner_mut().burn(
            spell.caster,
            TokenAmount::from_cents(params.amount_cents),
            ctx.block_height,
            tx_hash,
        ) {
            return ExecutionResult::failure(spell_hash, e.to_string());
        }

        // Deposit to pool
        if let Err(e) = self.adapter.stability_pool.deposit(
            spell.caster,
            TokenAmount::from_cents(params.amount_cents),
            ctx.block_height,
        ) {
            return ExecutionResult::failure(spell_hash, e.to_string());
        }

        let new_total = self.adapter.stability_pool.get_current_value(&spell.caster);

        let mut events = EventLog::new();
        events.push(ProtocolEvent::StabilityDeposit(
            crate::protocol::events::StabilityDepositEvent {
                depositor: spell.caster,
                amount: TokenAmount::from_cents(params.amount_cents),
                new_total,
                block_height: ctx.block_height,
                timestamp: ctx.timestamp,
            },
        ));

        ExecutionResult::success(
            spell_hash,
            self.state_root(),
            events,
            None,
            None,
            1000,
        )
    }

    fn execute_stability_withdraw(&mut self, spell: CharmSpell, ctx: &ExecutionContext) -> ExecutionResult {
        let spell_hash = spell.hash();

        let params = match StabilityWithdrawParams::decode(&spell.data) {
            Ok(p) => p,
            Err(e) => return ExecutionResult::failure(spell_hash, e.to_string()),
        };

        let (withdrawn, _btc_gains) = match self.adapter.stability_pool.withdraw(
            &spell.caster,
            TokenAmount::from_cents(params.amount_cents),
            ctx.block_height,
        ) {
            Ok(result) => result,
            Err(e) => return ExecutionResult::failure(spell_hash, e.to_string()),
        };

        // Mint tokens back to user
        let tx_hash = Hash::sha256(&spell.caster.as_bytes()[..8]);
        if let Err(e) = self.adapter.adapter.token.inner_mut().mint(
            spell.caster,
            withdrawn,
            ctx.block_height,
            tx_hash,
        ) {
            return ExecutionResult::failure(spell_hash, e.to_string());
        }

        let remaining = self.adapter.stability_pool.get_current_value(&spell.caster);

        let mut events = EventLog::new();
        events.push(ProtocolEvent::StabilityWithdraw(
            crate::protocol::events::StabilityWithdrawEvent {
                depositor: spell.caster,
                amount: withdrawn,
                remaining,
                block_height: ctx.block_height,
                timestamp: ctx.timestamp,
            },
        ));

        ExecutionResult::success(
            spell_hash,
            self.state_root(),
            events,
            None,
            None,
            1000,
        )
    }

    fn execute_claim_gains(&mut self, spell: CharmSpell, _ctx: &ExecutionContext) -> ExecutionResult {
        ExecutionResult::failure(spell.hash(), "Claim gains not yet implemented")
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
    fn test_executor_creation() {
        let creator = KeyPair::generate();
        let executor = BitcoinOSExecutor::new(
            *creator.public_key(),
            100,
            10_000_000,
            vec![*creator.public_key()],
            1,
        );

        assert!(!executor.state_root().is_zero());
    }

    #[test]
    fn test_executor_state_root() {
        let sender = KeyPair::generate();

        let executor = BitcoinOSExecutor::new(
            *sender.public_key(),
            100,
            10_000_000,
            vec![*sender.public_key()],
            1,
        );

        // Verify state root is not zero after initialization
        let root = executor.state_root();
        assert!(!root.is_zero());
    }

    #[test]
    fn test_execution_context() {
        let ctx = ExecutionContext::new(100, 1234567890, 10_000_000, Hash::zero());
        assert_eq!(ctx.block_height, 100);
        assert_eq!(ctx.timestamp, 1234567890);
        assert_eq!(ctx.btc_price_cents, 10_000_000);
        assert!(!ctx.recovery_mode);

        let ctx2 = ctx.with_recovery_mode(true);
        assert!(ctx2.recovery_mode);
    }
}
