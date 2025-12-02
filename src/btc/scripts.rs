//! Bitcoin script building for zkUSD protocol.
//!
//! This module provides script templates for:
//! - Collateral locking
//! - zkBTC representation
//! - Protocol-specific conditions

use bitcoin::hashes::Hash;
use bitcoin::script::{Builder as ScriptBuilder, PushBytesBuf};
use bitcoin::{opcodes, PubkeyHash, ScriptBuf, WScriptHash};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Script type for collateral
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollateralScriptType {
    /// Simple P2WPKH (Pay-to-Witness-Public-Key-Hash)
    P2WPKH,
    /// P2TR (Pay-to-Taproot) with protocol conditions
    P2TR,
    /// Multisig for protocol-controlled collateral
    Multisig { threshold: u8, total: u8 },
}

/// Configuration for collateral scripts
#[derive(Debug, Clone)]
pub struct CollateralScriptConfig {
    /// Owner's public key (compressed)
    pub owner_pubkey: [u8; 33],
    /// Protocol's public key (for co-signing)
    pub protocol_pubkey: Option<[u8; 33]>,
    /// CDP ID for script path identification
    pub cdp_id: [u8; 32],
    /// Timelock (block height) for emergency recovery
    pub recovery_timelock: Option<u32>,
}

/// Builder for collateral locking scripts
pub struct CollateralScriptBuilder {
    config: CollateralScriptConfig,
}

impl CollateralScriptBuilder {
    /// Create a new builder
    pub fn new(config: CollateralScriptConfig) -> Self {
        Self { config }
    }

    /// Build a P2WPKH script (simplest, owner-only control)
    pub fn build_p2wpkh(&self) -> Result<ScriptBuf> {
        let pubkey_hash = PubkeyHash::hash(&self.config.owner_pubkey);
        let wpkh = bitcoin::WPubkeyHash::from_byte_array(pubkey_hash.to_byte_array());
        Ok(ScriptBuf::new_p2wpkh(&wpkh))
    }

    /// Build witness script for P2WPKH spending
    pub fn build_p2wpkh_witness_script(&self) -> ScriptBuf {
        let pubkey_hash = PubkeyHash::hash(&self.config.owner_pubkey);
        ScriptBuilder::new()
            .push_opcode(opcodes::all::OP_DUP)
            .push_opcode(opcodes::all::OP_HASH160)
            .push_slice(&pubkey_hash.to_byte_array())
            .push_opcode(opcodes::all::OP_EQUALVERIFY)
            .push_opcode(opcodes::all::OP_CHECKSIG)
            .into_script()
    }

    /// Build a 2-of-2 multisig script (owner + protocol)
    pub fn build_multisig_2of2(&self) -> Result<ScriptBuf> {
        let protocol_pubkey = self.config.protocol_pubkey.ok_or_else(|| {
            Error::InvalidParameter {
                name: "protocol_pubkey".into(),
                reason: "Protocol public key required for multisig".into(),
            }
        })?;

        // Sort pubkeys lexicographically for deterministic ordering
        let mut pubkeys = vec![self.config.owner_pubkey, protocol_pubkey];
        pubkeys.sort();

        let script = ScriptBuilder::new()
            .push_opcode(opcodes::all::OP_PUSHNUM_2)
            .push_slice(&pubkeys[0])
            .push_slice(&pubkeys[1])
            .push_opcode(opcodes::all::OP_PUSHNUM_2)
            .push_opcode(opcodes::all::OP_CHECKMULTISIG)
            .into_script();

        Ok(script)
    }

    /// Build a HTLC-like script with timelock recovery
    pub fn build_with_timelock(&self) -> Result<ScriptBuf> {
        let timelock = self.config.recovery_timelock.ok_or_else(|| {
            Error::InvalidParameter {
                name: "recovery_timelock".into(),
                reason: "Timelock required".into(),
            }
        })?;

        let protocol_pubkey = self.config.protocol_pubkey.ok_or_else(|| {
            Error::InvalidParameter {
                name: "protocol_pubkey".into(),
                reason: "Protocol public key required".into(),
            }
        })?;

        // Script: IF <protocol_pubkey> CHECKSIG ELSE <timelock> CHECKLOCKTIMEVERIFY DROP <owner_pubkey> CHECKSIG ENDIF
        let script = ScriptBuilder::new()
            .push_opcode(opcodes::all::OP_IF)
            .push_slice(&protocol_pubkey)
            .push_opcode(opcodes::all::OP_CHECKSIG)
            .push_opcode(opcodes::all::OP_ELSE)
            .push_int(timelock as i64)
            .push_opcode(opcodes::all::OP_CLTV)
            .push_opcode(opcodes::all::OP_DROP)
            .push_slice(&self.config.owner_pubkey)
            .push_opcode(opcodes::all::OP_CHECKSIG)
            .push_opcode(opcodes::all::OP_ENDIF)
            .into_script();

        Ok(script)
    }

    /// Build P2WSH (Pay-to-Witness-Script-Hash) from a witness script
    pub fn build_p2wsh(&self, witness_script: &ScriptBuf) -> ScriptBuf {
        let script_hash = WScriptHash::hash(witness_script.as_bytes());
        ScriptBuf::new_p2wsh(&script_hash)
    }
}

/// OP_RETURN data builder for embedding protocol data
pub struct OpReturnBuilder;

impl OpReturnBuilder {
    /// Maximum OP_RETURN data size
    pub const MAX_DATA_SIZE: usize = 80;

    /// Protocol identifier prefix
    pub const PROTOCOL_PREFIX: &'static [u8] = b"ZKUSD";

    /// Build an OP_RETURN script for CDP creation
    pub fn cdp_create(cdp_id: &[u8; 32]) -> ScriptBuf {
        let mut data = Vec::with_capacity(37);
        data.extend_from_slice(Self::PROTOCOL_PREFIX);
        data.push(0x01); // Operation: Create CDP
        data.extend_from_slice(cdp_id);

        let push_bytes = PushBytesBuf::try_from(data).expect("OP_RETURN data within limits");

        ScriptBuilder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_slice(push_bytes)
            .into_script()
    }

    /// Build an OP_RETURN script for collateral deposit
    pub fn collateral_deposit(cdp_id: &[u8; 32], amount: u64) -> ScriptBuf {
        let mut data = Vec::with_capacity(45);
        data.extend_from_slice(Self::PROTOCOL_PREFIX);
        data.push(0x02); // Operation: Deposit
        data.extend_from_slice(cdp_id);
        data.extend_from_slice(&amount.to_le_bytes());

        let push_bytes = PushBytesBuf::try_from(data).expect("OP_RETURN data within limits");

        ScriptBuilder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_slice(push_bytes)
            .into_script()
    }

    /// Build an OP_RETURN script for collateral withdrawal
    pub fn collateral_withdraw(cdp_id: &[u8; 32], amount: u64) -> ScriptBuf {
        let mut data = Vec::with_capacity(45);
        data.extend_from_slice(Self::PROTOCOL_PREFIX);
        data.push(0x03); // Operation: Withdraw
        data.extend_from_slice(cdp_id);
        data.extend_from_slice(&amount.to_le_bytes());

        let push_bytes = PushBytesBuf::try_from(data).expect("OP_RETURN data within limits");

        ScriptBuilder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_slice(push_bytes)
            .into_script()
    }

    /// Build an OP_RETURN script for liquidation
    pub fn liquidation(cdp_id: &[u8; 32], debt_repaid: u64, collateral_seized: u64) -> ScriptBuf {
        let mut data = Vec::with_capacity(53);
        data.extend_from_slice(Self::PROTOCOL_PREFIX);
        data.push(0x10); // Operation: Liquidation
        data.extend_from_slice(cdp_id);
        data.extend_from_slice(&debt_repaid.to_le_bytes());
        data.extend_from_slice(&collateral_seized.to_le_bytes());

        let push_bytes = PushBytesBuf::try_from(data).expect("OP_RETURN data within limits");

        ScriptBuilder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_slice(push_bytes)
            .into_script()
    }

    /// Parse protocol data from an OP_RETURN script
    pub fn parse(script: &ScriptBuf) -> Option<ProtocolOp> {
        let bytes = script.as_bytes();

        // Check for OP_RETURN
        if bytes.is_empty() || bytes[0] != opcodes::all::OP_RETURN.to_u8() {
            return None;
        }

        // Skip OP_RETURN and push opcode
        let data = if bytes.len() > 2 { &bytes[2..] } else { return None };

        // Check prefix
        if data.len() < 6 || &data[..5] != Self::PROTOCOL_PREFIX {
            return None;
        }

        let op_code = data[5];
        let payload = &data[6..];

        match op_code {
            0x01 if payload.len() >= 32 => {
                let mut cdp_id = [0u8; 32];
                cdp_id.copy_from_slice(&payload[..32]);
                Some(ProtocolOp::CreateCDP { cdp_id })
            }
            0x02 if payload.len() >= 40 => {
                let mut cdp_id = [0u8; 32];
                cdp_id.copy_from_slice(&payload[..32]);
                let amount = u64::from_le_bytes(payload[32..40].try_into().ok()?);
                Some(ProtocolOp::Deposit { cdp_id, amount })
            }
            0x03 if payload.len() >= 40 => {
                let mut cdp_id = [0u8; 32];
                cdp_id.copy_from_slice(&payload[..32]);
                let amount = u64::from_le_bytes(payload[32..40].try_into().ok()?);
                Some(ProtocolOp::Withdraw { cdp_id, amount })
            }
            0x10 if payload.len() >= 48 => {
                let mut cdp_id = [0u8; 32];
                cdp_id.copy_from_slice(&payload[..32]);
                let debt_repaid = u64::from_le_bytes(payload[32..40].try_into().ok()?);
                let collateral_seized = u64::from_le_bytes(payload[40..48].try_into().ok()?);
                Some(ProtocolOp::Liquidation { cdp_id, debt_repaid, collateral_seized })
            }
            _ => None,
        }
    }
}

/// Protocol operation parsed from OP_RETURN
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolOp {
    /// Create a new CDP
    CreateCDP { cdp_id: [u8; 32] },
    /// Deposit collateral
    Deposit { cdp_id: [u8; 32], amount: u64 },
    /// Withdraw collateral
    Withdraw { cdp_id: [u8; 32], amount: u64 },
    /// Liquidation event
    Liquidation { cdp_id: [u8; 32], debt_repaid: u64, collateral_seized: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_p2wpkh_script() {
        let config = CollateralScriptConfig {
            owner_pubkey: [2u8; 33], // Compressed pubkey starting with 02
            protocol_pubkey: None,
            cdp_id: [0u8; 32],
            recovery_timelock: None,
        };

        let builder = CollateralScriptBuilder::new(config);
        let script = builder.build_p2wpkh().unwrap();

        // P2WPKH script should be 22 bytes
        assert_eq!(script.len(), 22);
    }

    #[test]
    fn test_op_return_roundtrip() {
        let cdp_id = [42u8; 32];
        let amount = 1_000_000u64;

        let script = OpReturnBuilder::collateral_deposit(&cdp_id, amount);
        let parsed = OpReturnBuilder::parse(&script);

        assert!(parsed.is_some());
        match parsed.unwrap() {
            ProtocolOp::Deposit { cdp_id: parsed_id, amount: parsed_amount } => {
                assert_eq!(parsed_id, cdp_id);
                assert_eq!(parsed_amount, amount);
            }
            _ => panic!("Wrong operation type"),
        }
    }
}
