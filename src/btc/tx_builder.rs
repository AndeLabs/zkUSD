//! Bitcoin transaction builder for zkUSD protocol.
//!
//! This module provides transaction construction for all protocol operations:
//! - Collateral deposits
//! - Collateral withdrawals
//! - Liquidations
//! - Redemptions

use bitcoin::{
    absolute::LockTime,
    transaction::Version,
    Amount, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, Witness,
};
use serde::{Deserialize, Serialize};

use crate::btc::scripts::{CollateralScriptBuilder, CollateralScriptConfig, OpReturnBuilder};
use crate::btc::utxo::{SelectionStrategy, Utxo, UtxoSet};
use crate::error::{Error, Result};

/// Estimated virtual size for different input types
pub mod vsize {
    /// P2WPKH input vsize
    pub const P2WPKH_INPUT: u64 = 68;
    /// P2WSH input vsize (estimated)
    pub const P2WSH_INPUT: u64 = 104;
    /// P2TR key path input vsize
    pub const P2TR_KEYPATH_INPUT: u64 = 58;
    /// Standard output vsize
    pub const P2WPKH_OUTPUT: u64 = 31;
    /// OP_RETURN output vsize (estimated)
    pub const OP_RETURN_OUTPUT: u64 = 43;
    /// Transaction overhead
    pub const TX_OVERHEAD: u64 = 11;
}

/// Fee rate in satoshis per virtual byte
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FeeRate(u64);

impl FeeRate {
    /// Minimum relay fee (1 sat/vB)
    pub const MIN: Self = Self(1);

    /// Create from sat/vB
    pub fn from_sat_per_vb(rate: u64) -> Self {
        Self(rate.max(1))
    }

    /// Get rate as sat/vB
    pub fn sat_per_vb(&self) -> u64 {
        self.0
    }

    /// Calculate fee for a given vsize
    pub fn fee_for_vsize(&self, vsize: u64) -> u64 {
        self.0.saturating_mul(vsize)
    }
}

impl Default for FeeRate {
    fn default() -> Self {
        Self(10) // 10 sat/vB default
    }
}

/// Transaction template for building
#[derive(Debug, Clone)]
pub struct TxTemplate {
    /// Inputs to spend
    pub inputs: Vec<TxInput>,
    /// Outputs to create
    pub outputs: Vec<TxOutput>,
    /// Lock time
    pub lock_time: LockTime,
    /// Fee rate
    pub fee_rate: FeeRate,
}

/// Input specification
#[derive(Debug, Clone)]
pub struct TxInput {
    /// UTXO to spend
    pub utxo: Utxo,
    /// Sequence number
    pub sequence: Sequence,
    /// Witness script (for P2WSH)
    pub witness_script: Option<ScriptBuf>,
}

/// Output specification
#[derive(Debug, Clone)]
pub enum TxOutput {
    /// Standard value output
    Value { amount: u64, script_pubkey: ScriptBuf },
    /// Change output (amount computed automatically)
    Change { script_pubkey: ScriptBuf },
    /// OP_RETURN data output
    OpReturn { data: ScriptBuf },
}

impl TxTemplate {
    /// Create a new empty template
    pub fn new() -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            lock_time: LockTime::ZERO,
            fee_rate: FeeRate::default(),
        }
    }

    /// Add an input
    pub fn add_input(&mut self, utxo: Utxo) -> &mut Self {
        self.inputs.push(TxInput {
            utxo,
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness_script: None,
        });
        self
    }

    /// Add an output
    pub fn add_output(&mut self, amount: u64, script_pubkey: ScriptBuf) -> &mut Self {
        self.outputs.push(TxOutput::Value { amount, script_pubkey });
        self
    }

    /// Add a change output
    pub fn add_change(&mut self, script_pubkey: ScriptBuf) -> &mut Self {
        self.outputs.push(TxOutput::Change { script_pubkey });
        self
    }

    /// Add an OP_RETURN output
    pub fn add_op_return(&mut self, data: ScriptBuf) -> &mut Self {
        self.outputs.push(TxOutput::OpReturn { data });
        self
    }

    /// Set fee rate
    pub fn with_fee_rate(&mut self, rate: FeeRate) -> &mut Self {
        self.fee_rate = rate;
        self
    }

    /// Set lock time
    pub fn with_lock_time(&mut self, lock_time: LockTime) -> &mut Self {
        self.lock_time = lock_time;
        self
    }

    /// Estimate virtual size
    pub fn estimate_vsize(&self) -> u64 {
        let input_vsize: u64 = self.inputs.iter().map(|_| vsize::P2WPKH_INPUT).sum();

        let output_vsize: u64 = self.outputs.iter().map(|o| match o {
            TxOutput::Value { .. } | TxOutput::Change { .. } => vsize::P2WPKH_OUTPUT,
            TxOutput::OpReturn { .. } => vsize::OP_RETURN_OUTPUT,
        }).sum();

        vsize::TX_OVERHEAD + input_vsize + output_vsize
    }

    /// Calculate required fee
    pub fn required_fee(&self) -> u64 {
        self.fee_rate.fee_for_vsize(self.estimate_vsize())
    }

    /// Calculate total input value
    pub fn total_input(&self) -> u64 {
        self.inputs.iter().map(|i| i.utxo.value).sum()
    }

    /// Calculate total output value (excluding change)
    pub fn total_output(&self) -> u64 {
        self.outputs.iter().filter_map(|o| match o {
            TxOutput::Value { amount, .. } => Some(*amount),
            _ => None,
        }).sum()
    }

    /// Build the unsigned transaction
    pub fn build_unsigned(&self) -> Result<Transaction> {
        let total_input = self.total_input();
        let total_output = self.total_output();
        let fee = self.required_fee();

        if total_input < total_output + fee {
            return Err(Error::InsufficientCollateral {
                required: total_output + fee,
                available: total_input,
            });
        }

        let change = total_input - total_output - fee;

        let inputs: Vec<TxIn> = self.inputs.iter().map(|i| TxIn {
            previous_output: i.utxo.outpoint(),
            script_sig: ScriptBuf::new(),
            sequence: i.sequence,
            witness: Witness::default(),
        }).collect();

        let mut outputs: Vec<TxOut> = Vec::new();

        for o in &self.outputs {
            match o {
                TxOutput::Value { amount, script_pubkey } => {
                    outputs.push(TxOut {
                        value: Amount::from_sat(*amount),
                        script_pubkey: script_pubkey.clone(),
                    });
                }
                TxOutput::Change { script_pubkey } if change > 546 => {
                    // Only add change if above dust threshold
                    outputs.push(TxOut {
                        value: Amount::from_sat(change),
                        script_pubkey: script_pubkey.clone(),
                    });
                }
                TxOutput::OpReturn { data } => {
                    outputs.push(TxOut {
                        value: Amount::ZERO,
                        script_pubkey: data.clone(),
                    });
                }
                _ => {}
            }
        }

        Ok(Transaction {
            version: Version::TWO,
            lock_time: self.lock_time,
            input: inputs,
            output: outputs,
        })
    }
}

impl Default for TxTemplate {
    fn default() -> Self {
        Self::new()
    }
}

/// High-level transaction builder for protocol operations
pub struct ProtocolTxBuilder {
    /// UTXO set for input selection
    utxo_set: UtxoSet,
    /// Current block height
    current_height: u32,
    /// Required confirmations for UTXOs
    min_confirmations: u32,
    /// Fee rate
    fee_rate: FeeRate,
    /// Protocol's public key (for co-signed scripts)
    protocol_pubkey: Option<[u8; 33]>,
}

impl ProtocolTxBuilder {
    /// Create a new builder
    pub fn new(utxo_set: UtxoSet, current_height: u32) -> Self {
        Self {
            utxo_set,
            current_height,
            min_confirmations: 1,
            fee_rate: FeeRate::default(),
            protocol_pubkey: None,
        }
    }

    /// Set fee rate
    pub fn with_fee_rate(mut self, rate: FeeRate) -> Self {
        self.fee_rate = rate;
        self
    }

    /// Set minimum confirmations
    pub fn with_min_confirmations(mut self, confirmations: u32) -> Self {
        self.min_confirmations = confirmations;
        self
    }

    /// Set protocol pubkey
    pub fn with_protocol_pubkey(mut self, pubkey: [u8; 33]) -> Self {
        self.protocol_pubkey = Some(pubkey);
        self
    }

    /// Build a collateral deposit transaction
    pub fn build_deposit(
        &self,
        cdp_id: [u8; 32],
        owner_pubkey: [u8; 33],
        amount: u64,
        change_script: ScriptBuf,
    ) -> Result<Transaction> {
        // Select UTXOs to cover amount + estimated fee
        let estimated_fee = self.fee_rate.fee_for_vsize(200);
        let utxos = self.utxo_set.select(
            amount + estimated_fee,
            SelectionStrategy::SmallestFirst,
            self.current_height,
            self.min_confirmations,
        )?;

        // Build collateral output script
        let config = CollateralScriptConfig {
            owner_pubkey,
            protocol_pubkey: self.protocol_pubkey,
            cdp_id,
            recovery_timelock: None,
        };
        let script_builder = CollateralScriptBuilder::new(config);
        let collateral_script = script_builder.build_p2wpkh()?;

        // Build OP_RETURN
        let op_return = OpReturnBuilder::collateral_deposit(&cdp_id, amount);

        // Build transaction template
        let mut template = TxTemplate::new();
        template.with_fee_rate(self.fee_rate);

        for utxo in utxos {
            template.add_input(utxo.clone());
        }

        template.add_output(amount, collateral_script);
        template.add_op_return(op_return);
        template.add_change(change_script);

        template.build_unsigned()
    }

    /// Build a collateral withdrawal transaction
    pub fn build_withdrawal(
        &self,
        cdp_id: [u8; 32],
        owner_pubkey: [u8; 33],
        amount: u64,
        destination: ScriptBuf,
    ) -> Result<Transaction> {
        // Get collateral UTXOs for this CDP
        let cdp_utxos = self.utxo_set.get_cdp_utxos(&cdp_id);
        let total_collateral: u64 = cdp_utxos.iter().map(|u| u.value).sum();

        if total_collateral < amount {
            return Err(Error::InsufficientCollateral {
                required: amount,
                available: total_collateral,
            });
        }

        // Build OP_RETURN
        let op_return = OpReturnBuilder::collateral_withdraw(&cdp_id, amount);

        // Build transaction template
        let mut template = TxTemplate::new();
        template.with_fee_rate(self.fee_rate);

        for utxo in cdp_utxos {
            template.add_input(utxo.clone());
        }

        template.add_output(amount, destination);
        template.add_op_return(op_return);

        // Remaining collateral goes back to CDP script
        let config = CollateralScriptConfig {
            owner_pubkey,
            protocol_pubkey: self.protocol_pubkey,
            cdp_id,
            recovery_timelock: None,
        };
        let script_builder = CollateralScriptBuilder::new(config);
        let collateral_script = script_builder.build_p2wpkh()?;
        template.add_change(collateral_script);

        template.build_unsigned()
    }

    /// Build a liquidation transaction
    pub fn build_liquidation(
        &self,
        cdp_id: [u8; 32],
        debt_repaid: u64,
        collateral_seized: u64,
        liquidator_script: ScriptBuf,
        stability_pool_script: ScriptBuf,
    ) -> Result<Transaction> {
        // Get collateral UTXOs for this CDP
        let cdp_utxos = self.utxo_set.get_cdp_utxos(&cdp_id);

        // Build OP_RETURN
        let op_return = OpReturnBuilder::liquidation(&cdp_id, debt_repaid, collateral_seized);

        // Build transaction template
        let mut template = TxTemplate::new();
        template.with_fee_rate(self.fee_rate);

        for utxo in cdp_utxos {
            template.add_input(utxo.clone());
        }

        // Collateral to liquidator (with bonus)
        template.add_output(collateral_seized, liquidator_script);

        // Any remainder to stability pool
        template.add_change(stability_pool_script);

        template.add_op_return(op_return);

        template.build_unsigned()
    }

    /// Get mutable access to UTXO set
    pub fn utxo_set_mut(&mut self) -> &mut UtxoSet {
        &mut self.utxo_set
    }
}

/// Result of a built transaction
#[derive(Debug, Clone)]
pub struct BuiltTransaction {
    /// The unsigned transaction
    pub tx: Transaction,
    /// Inputs that need to be signed
    pub signing_inputs: Vec<SigningInput>,
    /// Transaction ID (after signing)
    pub txid: Option<Txid>,
}

/// Information needed to sign an input
#[derive(Debug, Clone)]
pub struct SigningInput {
    /// Input index
    pub index: usize,
    /// Previous output
    pub prev_output: TxOut,
    /// Signing script (witness script for P2WSH)
    pub signing_script: ScriptBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn test_txid() -> Txid {
        Txid::from_str("0000000000000000000000000000000000000000000000000000000000000001").unwrap()
    }

    #[test]
    fn test_tx_template_creation() {
        let mut template = TxTemplate::new();

        let utxo = Utxo::new(test_txid(), 0, 100_000, ScriptBuf::new());
        template.add_input(utxo);
        template.add_output(50_000, ScriptBuf::new());
        template.add_change(ScriptBuf::new());

        assert_eq!(template.inputs.len(), 1);
        assert_eq!(template.outputs.len(), 2);
        assert_eq!(template.total_input(), 100_000);
        assert_eq!(template.total_output(), 50_000);
    }

    #[test]
    fn test_fee_calculation() {
        let rate = FeeRate::from_sat_per_vb(10);
        assert_eq!(rate.fee_for_vsize(100), 1000);
    }

    #[test]
    fn test_vsize_estimation() {
        let mut template = TxTemplate::new();

        let utxo = Utxo::new(test_txid(), 0, 100_000, ScriptBuf::new());
        template.add_input(utxo);
        template.add_output(50_000, ScriptBuf::new());

        let vsize = template.estimate_vsize();
        assert!(vsize > 0);
        assert!(vsize < 500); // Reasonable range for 1-in, 1-out
    }
}
