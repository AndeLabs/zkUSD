//! Protocol parameters that can be changed via governance.
//!
//! This module defines all parameters that can be modified through
//! the governance process.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

// ═══════════════════════════════════════════════════════════════════════════════
// PROTOCOL PARAMETERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Parameters that can be modified via governance
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtocolParameter {
    // ─────────────────────────────────────────────────────────────────────────
    // CDP PARAMETERS
    // ─────────────────────────────────────────────────────────────────────────

    /// Minimum collateralization ratio (percentage, e.g., 110 = 110%)
    MinCollateralRatio,
    /// Critical collateralization ratio for recovery mode (percentage)
    CriticalCollateralRatio,
    /// Minimum debt per CDP (cents)
    MinDebt,
    /// Debt ceiling - maximum total system debt (cents)
    DebtCeiling,

    // ─────────────────────────────────────────────────────────────────────────
    // FEE PARAMETERS
    // ─────────────────────────────────────────────────────────────────────────

    /// Borrowing fee (basis points, e.g., 50 = 0.5%)
    BorrowingFee,
    /// Redemption fee floor (basis points)
    RedemptionFeeFloor,
    /// Redemption fee cap (basis points)
    RedemptionFeeCap,
    /// Liquidation bonus (basis points)
    LiquidationBonus,

    // ─────────────────────────────────────────────────────────────────────────
    // STABILITY POOL PARAMETERS
    // ─────────────────────────────────────────────────────────────────────────

    /// Minimum stability pool deposit (cents)
    MinStabilityDeposit,

    // ─────────────────────────────────────────────────────────────────────────
    // ORACLE PARAMETERS
    // ─────────────────────────────────────────────────────────────────────────

    /// Price staleness threshold (seconds)
    PriceStalenessThreshold,
    /// Minimum price sources required
    MinPriceSources,
    /// Maximum price deviation between sources (basis points)
    MaxPriceDeviation,

    // ─────────────────────────────────────────────────────────────────────────
    // GOVERNANCE PARAMETERS
    // ─────────────────────────────────────────────────────────────────────────

    /// Minimum tokens to create proposal (cents)
    ProposalThreshold,
    /// Quorum required (basis points of total supply)
    GovernanceQuorum,
    /// Voting period (blocks)
    VotingPeriod,
    /// Timelock delay (blocks)
    TimelockDelay,
}

impl ProtocolParameter {
    /// Get human-readable name
    pub fn name(&self) -> &'static str {
        match self {
            Self::MinCollateralRatio => "Minimum Collateral Ratio",
            Self::CriticalCollateralRatio => "Critical Collateral Ratio",
            Self::MinDebt => "Minimum Debt",
            Self::DebtCeiling => "Debt Ceiling",
            Self::BorrowingFee => "Borrowing Fee",
            Self::RedemptionFeeFloor => "Redemption Fee Floor",
            Self::RedemptionFeeCap => "Redemption Fee Cap",
            Self::LiquidationBonus => "Liquidation Bonus",
            Self::MinStabilityDeposit => "Minimum Stability Deposit",
            Self::PriceStalenessThreshold => "Price Staleness Threshold",
            Self::MinPriceSources => "Minimum Price Sources",
            Self::MaxPriceDeviation => "Maximum Price Deviation",
            Self::ProposalThreshold => "Proposal Threshold",
            Self::GovernanceQuorum => "Governance Quorum",
            Self::VotingPeriod => "Voting Period",
            Self::TimelockDelay => "Timelock Delay",
        }
    }

    /// Get description
    pub fn description(&self) -> &'static str {
        match self {
            Self::MinCollateralRatio => "Minimum collateralization ratio for CDPs",
            Self::CriticalCollateralRatio => "Ratio below which recovery mode activates",
            Self::MinDebt => "Minimum debt allowed per CDP",
            Self::DebtCeiling => "Maximum total zkUSD that can be minted",
            Self::BorrowingFee => "One-time fee charged when minting zkUSD",
            Self::RedemptionFeeFloor => "Minimum redemption fee",
            Self::RedemptionFeeCap => "Maximum redemption fee",
            Self::LiquidationBonus => "Bonus given to liquidators",
            Self::MinStabilityDeposit => "Minimum deposit in stability pool",
            Self::PriceStalenessThreshold => "Time before price is considered stale",
            Self::MinPriceSources => "Minimum oracle sources for valid price",
            Self::MaxPriceDeviation => "Maximum deviation between oracle sources",
            Self::ProposalThreshold => "Minimum tokens to create governance proposal",
            Self::GovernanceQuorum => "Minimum participation for valid vote",
            Self::VotingPeriod => "Duration of voting on proposals",
            Self::TimelockDelay => "Delay before executing approved proposals",
        }
    }

    /// Get validation bounds (min, max)
    pub fn bounds(&self) -> (u64, u64) {
        match self {
            Self::MinCollateralRatio => (100, 500),         // 100% to 500%
            Self::CriticalCollateralRatio => (100, 200),    // 100% to 200%
            Self::MinDebt => (100, 1_000_000_00),           // $1 to $1M
            Self::DebtCeiling => (0, u64::MAX),             // No upper limit
            Self::BorrowingFee => (0, 1000),                // 0% to 10%
            Self::RedemptionFeeFloor => (0, 500),           // 0% to 5%
            Self::RedemptionFeeCap => (0, 1000),            // 0% to 10%
            Self::LiquidationBonus => (0, 2000),            // 0% to 20%
            Self::MinStabilityDeposit => (0, 100_000_00),   // $0 to $1000
            Self::PriceStalenessThreshold => (60, 86400),   // 1 minute to 1 day
            Self::MinPriceSources => (1, 10),               // 1 to 10 sources
            Self::MaxPriceDeviation => (10, 1000),          // 0.1% to 10%
            Self::ProposalThreshold => (100_00, 10_000_000_00), // $100 to $100M
            Self::GovernanceQuorum => (100, 5000),          // 1% to 50%
            Self::VotingPeriod => (100, 100_000),           // ~25 min to ~17 days
            Self::TimelockDelay => (10, 100_000),           // ~2.5 min to ~17 days
        }
    }

    /// Validate a value for this parameter
    pub fn validate(&self, value: u64) -> Result<()> {
        let (min, max) = self.bounds();

        if value < min || value > max {
            return Err(Error::InvalidParameter {
                name: self.name().into(),
                reason: format!("value {} outside bounds [{}, {}]", value, min, max),
            });
        }

        Ok(())
    }

    /// Check if this is a critical parameter (requires extra caution)
    pub fn is_critical(&self) -> bool {
        matches!(
            self,
            Self::MinCollateralRatio |
            Self::DebtCeiling |
            Self::TimelockDelay |
            Self::GovernanceQuorum
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GOVERNANCE OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Operations that can be executed through governance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GovernanceOperation {
    /// Update a protocol parameter
    UpdateParameter {
        parameter: ProtocolParameter,
        new_value: u64,
    },

    /// Add a new oracle source
    AddOracleSource {
        source_id: String,
        weight: u64,
    },

    /// Remove an oracle source
    RemoveOracleSource {
        source_id: String,
    },

    /// Update oracle source weight
    UpdateOracleWeight {
        source_id: String,
        new_weight: u64,
    },

    /// Pause the protocol
    PauseProtocol,

    /// Unpause the protocol
    UnpauseProtocol,

    /// Update the guardian address
    UpdateGuardian {
        /// New guardian public key bytes (33 bytes), None to remove
        new_guardian: Option<Vec<u8>>,
    },

    /// Emergency shutdown
    EmergencyShutdown,

    /// Custom operation (for future extensions)
    Custom {
        operation_type: String,
        data: Vec<u8>,
    },
}

impl GovernanceOperation {
    /// Get operation name
    pub fn name(&self) -> &'static str {
        match self {
            Self::UpdateParameter { .. } => "Update Parameter",
            Self::AddOracleSource { .. } => "Add Oracle Source",
            Self::RemoveOracleSource { .. } => "Remove Oracle Source",
            Self::UpdateOracleWeight { .. } => "Update Oracle Weight",
            Self::PauseProtocol => "Pause Protocol",
            Self::UnpauseProtocol => "Unpause Protocol",
            Self::UpdateGuardian { .. } => "Update Guardian",
            Self::EmergencyShutdown => "Emergency Shutdown",
            Self::Custom { .. } => "Custom Operation",
        }
    }

    /// Check if this is a critical operation
    pub fn is_critical(&self) -> bool {
        matches!(
            self,
            Self::EmergencyShutdown |
            Self::UpdateGuardian { .. } |
            Self::PauseProtocol |
            Self::UpdateParameter { parameter: ProtocolParameter::DebtCeiling | ProtocolParameter::MinCollateralRatio, .. }
        )
    }

    /// Validate the operation
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::UpdateParameter { parameter, new_value } => {
                parameter.validate(*new_value)?;
            }
            Self::AddOracleSource { weight, .. } => {
                if *weight == 0 || *weight > 100 {
                    return Err(Error::InvalidParameter {
                        name: "weight".into(),
                        reason: "must be between 1 and 100".into(),
                    });
                }
            }
            Self::UpdateOracleWeight { new_weight, .. } => {
                if *new_weight == 0 || *new_weight > 100 {
                    return Err(Error::InvalidParameter {
                        name: "new_weight".into(),
                        reason: "must be between 1 and 100".into(),
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Get description of the operation
    pub fn describe(&self) -> String {
        match self {
            Self::UpdateParameter { parameter, new_value } => {
                format!("Update {} to {}", parameter.name(), new_value)
            }
            Self::AddOracleSource { source_id, weight } => {
                format!("Add oracle source {} with weight {}", source_id, weight)
            }
            Self::RemoveOracleSource { source_id } => {
                format!("Remove oracle source {}", source_id)
            }
            Self::UpdateOracleWeight { source_id, new_weight } => {
                format!("Update oracle {} weight to {}", source_id, new_weight)
            }
            Self::PauseProtocol => "Pause protocol".into(),
            Self::UnpauseProtocol => "Unpause protocol".into(),
            Self::UpdateGuardian { new_guardian } => {
                if new_guardian.is_some() {
                    "Update guardian address".into()
                } else {
                    "Remove guardian".into()
                }
            }
            Self::EmergencyShutdown => "EMERGENCY SHUTDOWN".into(),
            Self::Custom { operation_type, .. } => {
                format!("Custom: {}", operation_type)
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PARAMETER STORE
// ═══════════════════════════════════════════════════════════════════════════════

/// Storage for current parameter values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterStore {
    /// Current parameter values
    values: std::collections::HashMap<ProtocolParameter, u64>,
}

impl Default for ParameterStore {
    fn default() -> Self {
        let mut store = Self {
            values: std::collections::HashMap::new(),
        };

        // Set default values
        store.set(ProtocolParameter::MinCollateralRatio, 110);
        store.set(ProtocolParameter::CriticalCollateralRatio, 150);
        store.set(ProtocolParameter::MinDebt, 200_000);          // $2000
        store.set(ProtocolParameter::DebtCeiling, 100_000_000_000_00); // $100B
        store.set(ProtocolParameter::BorrowingFee, 50);          // 0.5%
        store.set(ProtocolParameter::RedemptionFeeFloor, 50);    // 0.5%
        store.set(ProtocolParameter::RedemptionFeeCap, 500);     // 5%
        store.set(ProtocolParameter::LiquidationBonus, 1000);    // 10%
        store.set(ProtocolParameter::MinStabilityDeposit, 10_000); // $100
        store.set(ProtocolParameter::PriceStalenessThreshold, 3600); // 1 hour
        store.set(ProtocolParameter::MinPriceSources, 3);
        store.set(ProtocolParameter::MaxPriceDeviation, 200);    // 2%
        store.set(ProtocolParameter::ProposalThreshold, 100_000_00); // $100k
        store.set(ProtocolParameter::GovernanceQuorum, 400);     // 4%
        store.set(ProtocolParameter::VotingPeriod, 17280);       // ~3 days
        store.set(ProtocolParameter::TimelockDelay, 11520);      // ~2 days

        store
    }
}

impl ParameterStore {
    /// Create new parameter store with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Get parameter value
    pub fn get(&self, param: ProtocolParameter) -> u64 {
        *self.values.get(&param).unwrap_or(&0)
    }

    /// Set parameter value
    pub fn set(&mut self, param: ProtocolParameter, value: u64) {
        self.values.insert(param, value);
    }

    /// Update parameter with validation
    pub fn update(&mut self, param: ProtocolParameter, value: u64) -> Result<()> {
        param.validate(value)?;
        self.set(param, value);
        Ok(())
    }

    /// Get all parameters
    pub fn all(&self) -> Vec<(ProtocolParameter, u64)> {
        self.values.iter().map(|(k, v)| (*k, *v)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parameter_validation() {
        let param = ProtocolParameter::MinCollateralRatio;

        assert!(param.validate(110).is_ok());
        assert!(param.validate(200).is_ok());
        assert!(param.validate(50).is_err());   // Below 100
        assert!(param.validate(600).is_err());  // Above 500
    }

    #[test]
    fn test_operation_validation() {
        let op = GovernanceOperation::UpdateParameter {
            parameter: ProtocolParameter::BorrowingFee,
            new_value: 100, // 1%
        };
        assert!(op.validate().is_ok());

        let bad_op = GovernanceOperation::AddOracleSource {
            source_id: "test".into(),
            weight: 0, // Invalid
        };
        assert!(bad_op.validate().is_err());
    }

    #[test]
    fn test_parameter_store() {
        let mut store = ParameterStore::new();

        assert_eq!(store.get(ProtocolParameter::MinCollateralRatio), 110);

        store.update(ProtocolParameter::MinCollateralRatio, 120).unwrap();
        assert_eq!(store.get(ProtocolParameter::MinCollateralRatio), 120);
    }
}
