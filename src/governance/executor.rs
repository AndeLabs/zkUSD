//! Governance executor for applying approved operations.
//!
//! This module handles the execution of governance operations after
//! they pass through the proposal and timelock system.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::utils::crypto::PublicKey;

use super::parameters::{GovernanceOperation, ParameterStore, ProtocolParameter};
use super::proposal::ProposalId;

// ═══════════════════════════════════════════════════════════════════════════════
// EXECUTION RESULT
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of executing a governance operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Proposal that was executed
    pub proposal_id: ProposalId,
    /// Whether execution succeeded
    pub success: bool,
    /// Individual operation results
    pub operation_results: Vec<OperationResult>,
    /// Block when executed
    pub block_height: u64,
    /// Error if failed
    pub error: Option<String>,
}

/// Result of a single operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResult {
    /// Index of operation in proposal
    pub index: usize,
    /// Operation that was executed
    pub operation: GovernanceOperation,
    /// Whether this operation succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// EXECUTION CONTEXT
// ═══════════════════════════════════════════════════════════════════════════════

/// Context for governance execution
pub struct GovernanceExecutionContext<'a> {
    /// Parameter store to modify
    pub parameters: &'a mut ParameterStore,
    /// Current block height
    pub block_height: u64,
    /// Executor's public key
    pub executor: PublicKey,
}

// ═══════════════════════════════════════════════════════════════════════════════
// GOVERNANCE EXECUTOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Executes approved governance operations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GovernanceExecutor {
    /// Execution history
    history: Vec<ExecutionResult>,
    /// Protocol paused flag
    paused: bool,
    /// Emergency shutdown flag
    shutdown: bool,
    /// Current guardian
    guardian: Option<PublicKey>,
}

impl GovernanceExecutor {
    /// Create new executor
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with guardian
    pub fn with_guardian(guardian: PublicKey) -> Self {
        Self {
            guardian: Some(guardian),
            ..Default::default()
        }
    }

    /// Check if protocol is paused
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Check if emergency shutdown is active
    pub fn is_shutdown(&self) -> bool {
        self.shutdown
    }

    /// Get current guardian
    pub fn guardian(&self) -> Option<&PublicKey> {
        self.guardian.as_ref()
    }

    /// Execute a list of operations from an approved proposal
    pub fn execute(
        &mut self,
        proposal_id: ProposalId,
        operations: &[GovernanceOperation],
        ctx: &mut GovernanceExecutionContext,
    ) -> ExecutionResult {
        // Check if shutdown
        if self.shutdown {
            return ExecutionResult {
                proposal_id,
                success: false,
                operation_results: vec![],
                block_height: ctx.block_height,
                error: Some("Protocol is in emergency shutdown".into()),
            };
        }

        let mut results = Vec::with_capacity(operations.len());
        let mut all_success = true;

        for (index, operation) in operations.iter().enumerate() {
            let result = self.execute_operation(operation, ctx);

            let op_result = OperationResult {
                index,
                operation: operation.clone(),
                success: result.is_ok(),
                error: result.err().map(|e| e.to_string()),
            };

            if !op_result.success {
                all_success = false;
            }

            results.push(op_result);
        }

        let result = ExecutionResult {
            proposal_id,
            success: all_success,
            operation_results: results,
            block_height: ctx.block_height,
            error: if all_success { None } else { Some("One or more operations failed".into()) },
        };

        self.history.push(result.clone());
        result
    }

    /// Execute a single operation
    fn execute_operation(
        &mut self,
        operation: &GovernanceOperation,
        ctx: &mut GovernanceExecutionContext,
    ) -> Result<()> {
        // Validate operation first
        operation.validate()?;

        match operation {
            GovernanceOperation::UpdateParameter { parameter, new_value } => {
                self.execute_update_parameter(ctx.parameters, *parameter, *new_value)
            }

            GovernanceOperation::AddOracleSource { source_id, weight } => {
                self.execute_add_oracle_source(source_id, *weight)
            }

            GovernanceOperation::RemoveOracleSource { source_id } => {
                self.execute_remove_oracle_source(source_id)
            }

            GovernanceOperation::UpdateOracleWeight { source_id, new_weight } => {
                self.execute_update_oracle_weight(source_id, *new_weight)
            }

            GovernanceOperation::PauseProtocol => {
                self.execute_pause()
            }

            GovernanceOperation::UnpauseProtocol => {
                self.execute_unpause()
            }

            GovernanceOperation::UpdateGuardian { new_guardian } => {
                self.execute_update_guardian(new_guardian)
            }

            GovernanceOperation::EmergencyShutdown => {
                self.execute_emergency_shutdown()
            }

            GovernanceOperation::Custom { operation_type, data } => {
                self.execute_custom(operation_type, data)
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // OPERATION IMPLEMENTATIONS
    // ─────────────────────────────────────────────────────────────────────────

    fn execute_update_parameter(
        &self,
        parameters: &mut ParameterStore,
        parameter: ProtocolParameter,
        new_value: u64,
    ) -> Result<()> {
        parameters.update(parameter, new_value)?;
        Ok(())
    }

    fn execute_add_oracle_source(&self, _source_id: &str, _weight: u64) -> Result<()> {
        // In production, this would interact with the oracle module
        // For now, we just validate and record the operation
        Ok(())
    }

    fn execute_remove_oracle_source(&self, _source_id: &str) -> Result<()> {
        Ok(())
    }

    fn execute_update_oracle_weight(&self, _source_id: &str, _new_weight: u64) -> Result<()> {
        Ok(())
    }

    fn execute_pause(&mut self) -> Result<()> {
        if self.paused {
            return Err(Error::Internal("Protocol already paused".into()));
        }
        self.paused = true;
        Ok(())
    }

    fn execute_unpause(&mut self) -> Result<()> {
        if !self.paused {
            return Err(Error::Internal("Protocol not paused".into()));
        }
        if self.shutdown {
            return Err(Error::Internal("Cannot unpause during shutdown".into()));
        }
        self.paused = false;
        Ok(())
    }

    fn execute_update_guardian(&mut self, new_guardian: &Option<Vec<u8>>) -> Result<()> {
        self.guardian = match new_guardian {
            Some(bytes) if bytes.len() == 33 => {
                let mut arr = [0u8; 33];
                arr.copy_from_slice(bytes);
                Some(PublicKey::new(arr))
            }
            Some(_) => return Err(Error::InvalidParameter {
                name: "new_guardian".into(),
                reason: "must be 33 bytes".into(),
            }),
            None => None,
        };
        Ok(())
    }

    fn execute_emergency_shutdown(&mut self) -> Result<()> {
        self.shutdown = true;
        self.paused = true;
        Ok(())
    }

    fn execute_custom(&self, operation_type: &str, _data: &[u8]) -> Result<()> {
        // Custom operations are logged but no-op for now
        // In production, this would dispatch to registered handlers
        Err(Error::Internal(format!(
            "Custom operation '{}' not implemented",
            operation_type
        )))
    }

    // ─────────────────────────────────────────────────────────────────────────
    // QUERIES
    // ─────────────────────────────────────────────────────────────────────────

    /// Get execution history
    pub fn history(&self) -> &[ExecutionResult] {
        &self.history
    }

    /// Get recent executions
    pub fn recent_executions(&self, limit: usize) -> Vec<&ExecutionResult> {
        self.history.iter().rev().take(limit).collect()
    }

    /// Get execution by proposal ID
    pub fn get_execution(&self, proposal_id: &ProposalId) -> Option<&ExecutionResult> {
        self.history.iter().find(|e| &e.proposal_id == proposal_id)
    }

    /// Count successful executions
    pub fn successful_count(&self) -> usize {
        self.history.iter().filter(|e| e.success).count()
    }

    /// Count failed executions
    pub fn failed_count(&self) -> usize {
        self.history.iter().filter(|e| !e.success).count()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BATCH EXECUTOR (for multiple proposals)
// ═══════════════════════════════════════════════════════════════════════════════

/// Batch executor for processing multiple proposals
pub struct BatchExecutor<'a> {
    executor: &'a mut GovernanceExecutor,
    parameters: &'a mut ParameterStore,
}

impl<'a> BatchExecutor<'a> {
    /// Create new batch executor
    pub fn new(executor: &'a mut GovernanceExecutor, parameters: &'a mut ParameterStore) -> Self {
        Self { executor, parameters }
    }

    /// Execute multiple proposals
    pub fn execute_batch(
        &mut self,
        proposals: Vec<(ProposalId, Vec<GovernanceOperation>)>,
        executor_key: PublicKey,
        block_height: u64,
    ) -> Vec<ExecutionResult> {
        let mut results = Vec::with_capacity(proposals.len());

        for (proposal_id, operations) in proposals {
            let mut ctx = GovernanceExecutionContext {
                parameters: self.parameters,
                block_height,
                executor: executor_key,
            };

            let result = self.executor.execute(proposal_id, &operations, &mut ctx);
            results.push(result);
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::crypto::KeyPair;

    fn test_proposal_id() -> ProposalId {
        ProposalId::from_bytes([1u8; 32])
    }

    #[test]
    fn test_execute_parameter_update() {
        let mut executor = GovernanceExecutor::new();
        let mut params = ParameterStore::new();
        let keypair = KeyPair::generate();

        let operations = vec![
            GovernanceOperation::UpdateParameter {
                parameter: ProtocolParameter::MinCollateralRatio,
                new_value: 120,
            },
        ];

        let mut ctx = GovernanceExecutionContext {
            parameters: &mut params,
            block_height: 100,
            executor: *keypair.public_key(),
        };

        let result = executor.execute(test_proposal_id(), &operations, &mut ctx);

        assert!(result.success);
        assert_eq!(params.get(ProtocolParameter::MinCollateralRatio), 120);
    }

    #[test]
    fn test_pause_unpause() {
        let mut executor = GovernanceExecutor::new();
        let mut params = ParameterStore::new();
        let keypair = KeyPair::generate();

        // Pause
        let mut ctx = GovernanceExecutionContext {
            parameters: &mut params,
            block_height: 100,
            executor: *keypair.public_key(),
        };

        let result = executor.execute(
            test_proposal_id(),
            &[GovernanceOperation::PauseProtocol],
            &mut ctx,
        );

        assert!(result.success);
        assert!(executor.is_paused());

        // Unpause
        let result = executor.execute(
            ProposalId::from_bytes([2u8; 32]),
            &[GovernanceOperation::UnpauseProtocol],
            &mut ctx,
        );

        assert!(result.success);
        assert!(!executor.is_paused());
    }

    #[test]
    fn test_emergency_shutdown() {
        let mut executor = GovernanceExecutor::new();
        let mut params = ParameterStore::new();
        let keypair = KeyPair::generate();

        let mut ctx = GovernanceExecutionContext {
            parameters: &mut params,
            block_height: 100,
            executor: *keypair.public_key(),
        };

        let result = executor.execute(
            test_proposal_id(),
            &[GovernanceOperation::EmergencyShutdown],
            &mut ctx,
        );

        assert!(result.success);
        assert!(executor.is_shutdown());
        assert!(executor.is_paused());

        // Cannot execute after shutdown
        let result = executor.execute(
            ProposalId::from_bytes([2u8; 32]),
            &[GovernanceOperation::UpdateParameter {
                parameter: ProtocolParameter::MinCollateralRatio,
                new_value: 130,
            }],
            &mut ctx,
        );

        assert!(!result.success);
    }

    #[test]
    fn test_multiple_operations() {
        let mut executor = GovernanceExecutor::new();
        let mut params = ParameterStore::new();
        let keypair = KeyPair::generate();

        let operations = vec![
            GovernanceOperation::UpdateParameter {
                parameter: ProtocolParameter::MinCollateralRatio,
                new_value: 115,
            },
            GovernanceOperation::UpdateParameter {
                parameter: ProtocolParameter::BorrowingFee,
                new_value: 75, // 0.75%
            },
        ];

        let mut ctx = GovernanceExecutionContext {
            parameters: &mut params,
            block_height: 100,
            executor: *keypair.public_key(),
        };

        let result = executor.execute(test_proposal_id(), &operations, &mut ctx);

        assert!(result.success);
        assert_eq!(result.operation_results.len(), 2);
        assert_eq!(params.get(ProtocolParameter::MinCollateralRatio), 115);
        assert_eq!(params.get(ProtocolParameter::BorrowingFee), 75);
    }
}
