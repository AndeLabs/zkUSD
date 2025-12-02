//! UTXO (Unspent Transaction Output) management.
//!
//! This module handles tracking and selection of UTXOs for transaction building.

use bitcoin::{Amount, OutPoint, ScriptBuf, TxOut, Txid};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{Error, Result};

/// Represents an unspent transaction output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Utxo {
    /// Transaction ID containing this output
    pub txid: Txid,
    /// Output index within the transaction
    pub vout: u32,
    /// Value in satoshis
    pub value: u64,
    /// The locking script (scriptPubKey)
    pub script_pubkey: ScriptBuf,
    /// Block height when confirmed (None if unconfirmed)
    pub confirmation_height: Option<u32>,
    /// Whether this UTXO is locked for a pending transaction
    pub locked: bool,
    /// Associated CDP ID if this is collateral
    pub cdp_id: Option<[u8; 32]>,
}

impl Utxo {
    /// Create a new UTXO
    pub fn new(txid: Txid, vout: u32, value: u64, script_pubkey: ScriptBuf) -> Self {
        Self {
            txid,
            vout,
            value,
            script_pubkey,
            confirmation_height: None,
            locked: false,
            cdp_id: None,
        }
    }

    /// Get the outpoint for this UTXO
    pub fn outpoint(&self) -> OutPoint {
        OutPoint { txid: self.txid, vout: self.vout }
    }

    /// Convert to TxOut
    pub fn to_tx_out(&self) -> TxOut {
        TxOut {
            value: Amount::from_sat(self.value),
            script_pubkey: self.script_pubkey.clone(),
        }
    }

    /// Check if UTXO is confirmed with sufficient depth
    pub fn is_confirmed(&self, current_height: u32, min_confirmations: u32) -> bool {
        match self.confirmation_height {
            Some(height) => current_height.saturating_sub(height) >= min_confirmations,
            None => false,
        }
    }

    /// Check if UTXO can be spent
    pub fn is_spendable(&self, current_height: u32, min_confirmations: u32) -> bool {
        !self.locked && self.is_confirmed(current_height, min_confirmations)
    }

    /// Mark as confirmed
    pub fn confirm(&mut self, height: u32) {
        self.confirmation_height = Some(height);
    }

    /// Lock for spending
    pub fn lock(&mut self) {
        self.locked = true;
    }

    /// Unlock
    pub fn unlock(&mut self) {
        self.locked = false;
    }
}

/// UTXO selection strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionStrategy {
    /// Select largest UTXOs first (minimizes number of inputs)
    LargestFirst,
    /// Select smallest UTXOs first (consolidates dust)
    SmallestFirst,
    /// Select UTXOs closest to target amount
    BestMatch,
    /// Select oldest UTXOs first (by confirmation height)
    OldestFirst,
}

/// Manages a set of UTXOs
#[derive(Debug, Default)]
pub struct UtxoSet {
    /// All UTXOs indexed by outpoint
    utxos: HashMap<OutPoint, Utxo>,
    /// UTXOs indexed by CDP ID
    cdp_utxos: HashMap<[u8; 32], Vec<OutPoint>>,
}

impl UtxoSet {
    /// Create a new empty UTXO set
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a UTXO to the set
    pub fn add(&mut self, utxo: Utxo) {
        let outpoint = utxo.outpoint();

        if let Some(cdp_id) = utxo.cdp_id {
            self.cdp_utxos.entry(cdp_id).or_default().push(outpoint);
        }

        self.utxos.insert(outpoint, utxo);
    }

    /// Remove a UTXO from the set
    pub fn remove(&mut self, outpoint: &OutPoint) -> Option<Utxo> {
        if let Some(utxo) = self.utxos.remove(outpoint) {
            if let Some(cdp_id) = utxo.cdp_id {
                if let Some(cdp_utxos) = self.cdp_utxos.get_mut(&cdp_id) {
                    cdp_utxos.retain(|op| op != outpoint);
                }
            }
            Some(utxo)
        } else {
            None
        }
    }

    /// Get a UTXO by outpoint
    pub fn get(&self, outpoint: &OutPoint) -> Option<&Utxo> {
        self.utxos.get(outpoint)
    }

    /// Get a mutable UTXO by outpoint
    pub fn get_mut(&mut self, outpoint: &OutPoint) -> Option<&mut Utxo> {
        self.utxos.get_mut(outpoint)
    }

    /// Get all UTXOs for a CDP
    pub fn get_cdp_utxos(&self, cdp_id: &[u8; 32]) -> Vec<&Utxo> {
        self.cdp_utxos
            .get(cdp_id)
            .map(|outpoints| {
                outpoints
                    .iter()
                    .filter_map(|op| self.utxos.get(op))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get total value of all UTXOs
    pub fn total_value(&self) -> u64 {
        self.utxos.values().map(|u| u.value).sum()
    }

    /// Get total value of spendable UTXOs
    pub fn spendable_value(&self, current_height: u32, min_confirmations: u32) -> u64 {
        self.utxos
            .values()
            .filter(|u| u.is_spendable(current_height, min_confirmations))
            .map(|u| u.value)
            .sum()
    }

    /// Get total collateral for a CDP
    pub fn cdp_collateral(&self, cdp_id: &[u8; 32]) -> u64 {
        self.get_cdp_utxos(cdp_id).iter().map(|u| u.value).sum()
    }

    /// Select UTXOs to cover a target amount
    pub fn select(
        &self,
        target: u64,
        strategy: SelectionStrategy,
        current_height: u32,
        min_confirmations: u32,
    ) -> Result<Vec<&Utxo>> {
        let mut spendable: Vec<_> = self
            .utxos
            .values()
            .filter(|u| u.is_spendable(current_height, min_confirmations))
            .collect();

        // Sort based on strategy
        match strategy {
            SelectionStrategy::LargestFirst => {
                spendable.sort_by(|a, b| b.value.cmp(&a.value));
            }
            SelectionStrategy::SmallestFirst => {
                spendable.sort_by(|a, b| a.value.cmp(&b.value));
            }
            SelectionStrategy::BestMatch => {
                spendable.sort_by_key(|u| {
                    if u.value >= target {
                        u.value - target
                    } else {
                        u64::MAX - (target - u.value)
                    }
                });
            }
            SelectionStrategy::OldestFirst => {
                spendable.sort_by(|a, b| {
                    let a_height = a.confirmation_height.unwrap_or(u32::MAX);
                    let b_height = b.confirmation_height.unwrap_or(u32::MAX);
                    a_height.cmp(&b_height)
                });
            }
        }

        // Select UTXOs until target is reached
        let mut selected = Vec::new();
        let mut total = 0u64;

        for utxo in spendable {
            if total >= target {
                break;
            }
            selected.push(utxo);
            total += utxo.value;
        }

        if total < target {
            return Err(Error::InsufficientCollateral {
                required: target,
                available: total,
            });
        }

        Ok(selected)
    }

    /// Lock UTXOs for a pending transaction
    pub fn lock_utxos(&mut self, outpoints: &[OutPoint]) {
        for op in outpoints {
            if let Some(utxo) = self.utxos.get_mut(op) {
                utxo.lock();
            }
        }
    }

    /// Unlock UTXOs (e.g., if transaction fails)
    pub fn unlock_utxos(&mut self, outpoints: &[OutPoint]) {
        for op in outpoints {
            if let Some(utxo) = self.utxos.get_mut(op) {
                utxo.unlock();
            }
        }
    }

    /// Update confirmations for all UTXOs
    pub fn update_confirmations(&mut self, txid_heights: &HashMap<Txid, u32>) {
        for utxo in self.utxos.values_mut() {
            if let Some(&height) = txid_heights.get(&utxo.txid) {
                utxo.confirm(height);
            }
        }
    }

    /// Get number of UTXOs
    pub fn len(&self) -> usize {
        self.utxos.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.utxos.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::Hash;
    use std::str::FromStr;

    fn test_txid() -> Txid {
        Txid::from_str("0000000000000000000000000000000000000000000000000000000000000001").unwrap()
    }

    #[test]
    fn test_utxo_creation() {
        let utxo = Utxo::new(
            test_txid(),
            0,
            100_000,
            ScriptBuf::new(),
        );

        assert_eq!(utxo.value, 100_000);
        assert!(!utxo.locked);
        assert!(utxo.confirmation_height.is_none());
    }

    #[test]
    fn test_utxo_set_selection() {
        let mut set = UtxoSet::new();

        // Add some UTXOs
        for i in 0..5 {
            let mut utxo = Utxo::new(
                test_txid(),
                i,
                (i as u64 + 1) * 10_000,
                ScriptBuf::new(),
            );
            utxo.confirm(100);
            set.add(utxo);
        }

        // Select 25000 sats
        let selected = set.select(25_000, SelectionStrategy::SmallestFirst, 110, 1).unwrap();
        assert!(!selected.is_empty());

        let total: u64 = selected.iter().map(|u| u.value).sum();
        assert!(total >= 25_000);
    }
}
