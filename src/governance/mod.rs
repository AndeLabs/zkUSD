//! On-chain Governance System for zkUSD.
//!
//! This module implements a complete governance system for protocol management:
//! - Proposal creation and lifecycle management
//! - Token-weighted voting
//! - Timelock for security
//! - Automatic execution of approved proposals
//!
//! # Architecture
//!
//! The governance system follows a standard pattern:
//! 1. Proposal Creation - Anyone with minimum tokens can create proposals
//! 2. Voting Period - Token holders vote during the active period
//! 3. Timelock - Approved proposals wait in queue before execution
//! 4. Execution - Proposals are executed after timelock expires
//!
//! # Security Features
//!
//! - Minimum proposal threshold prevents spam
//! - Quorum requirements ensure sufficient participation
//! - Timelock allows users to exit before changes take effect
//! - Guardian can cancel malicious proposals during timelock

pub mod proposal;
pub mod voting;
pub mod timelock;
pub mod executor;
pub mod parameters;

pub use proposal::*;
pub use voting::*;
pub use timelock::*;
pub use executor::*;
pub use parameters::*;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::utils::crypto::{Hash, PublicKey};

// ═══════════════════════════════════════════════════════════════════════════════
// GOVERNANCE CONFIGURATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Configuration for the governance system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceConfig {
    /// Minimum tokens required to create a proposal (in cents)
    pub proposal_threshold: u64,
    /// Minimum votes required for quorum (basis points of total supply)
    pub quorum_bps: u64,
    /// Duration of voting period in blocks
    pub voting_period_blocks: u64,
    /// Duration of timelock in blocks
    pub timelock_blocks: u64,
    /// Grace period after timelock expires (blocks)
    pub grace_period_blocks: u64,
    /// Maximum operations per proposal
    pub max_operations: usize,
    /// Delay before voting starts (blocks)
    pub voting_delay_blocks: u64,
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            proposal_threshold: 100_000_00,    // $100,000 minimum
            quorum_bps: 400,                    // 4% quorum
            voting_period_blocks: 17280,        // ~3 days at 15s blocks
            timelock_blocks: 11520,             // ~2 days
            grace_period_blocks: 5760,          // ~1 day
            max_operations: 10,
            voting_delay_blocks: 1,             // 1 block delay
        }
    }
}

impl GovernanceConfig {
    /// Create configuration for testnet (faster)
    pub fn testnet() -> Self {
        Self {
            proposal_threshold: 1_000_00,      // $1,000 minimum
            quorum_bps: 100,                    // 1% quorum
            voting_period_blocks: 100,          // Fast voting
            timelock_blocks: 50,                // Short timelock
            grace_period_blocks: 50,
            max_operations: 10,
            voting_delay_blocks: 1,
        }
    }

    /// Validate configuration parameters
    pub fn validate(&self) -> Result<()> {
        if self.quorum_bps > 10000 {
            return Err(Error::InvalidParameter {
                name: "quorum_bps".into(),
                reason: "cannot exceed 100%".into(),
            });
        }
        if self.voting_period_blocks == 0 {
            return Err(Error::InvalidParameter {
                name: "voting_period_blocks".into(),
                reason: "must be greater than 0".into(),
            });
        }
        if self.max_operations == 0 {
            return Err(Error::InvalidParameter {
                name: "max_operations".into(),
                reason: "must be greater than 0".into(),
            });
        }
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GOVERNANCE SYSTEM
// ═══════════════════════════════════════════════════════════════════════════════

/// Main governance system coordinating all components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceSystem {
    /// Configuration
    pub config: GovernanceConfig,
    /// Proposal manager
    pub proposals: ProposalManager,
    /// Voting system
    pub voting: VotingSystem,
    /// Timelock queue
    pub timelock: TimelockQueue,
    /// Guardian address (can cancel proposals)
    pub guardian: Option<PublicKey>,
    /// Current block height
    pub block_height: u64,
}

impl GovernanceSystem {
    /// Create new governance system
    pub fn new(config: GovernanceConfig, guardian: Option<PublicKey>) -> Self {
        Self {
            config,
            proposals: ProposalManager::new(),
            voting: VotingSystem::new(),
            timelock: TimelockQueue::new(),
            guardian,
            block_height: 0,
        }
    }

    /// Update block height and process any pending actions
    pub fn advance_block(&mut self, new_height: u64) -> Vec<GovernanceEvent> {
        self.block_height = new_height;

        let mut events = Vec::new();

        // Check for proposals that have finished voting
        let finished = self.proposals.get_proposals_at_voting_end(new_height);
        for proposal_id in finished {
            if let Some(proposal) = self.proposals.get(&proposal_id) {
                let votes = self.voting.get_votes(&proposal_id);
                let passed = self.check_proposal_passed(proposal, &votes);

                if passed {
                    // Queue in timelock
                    let eta = new_height + self.config.timelock_blocks;
                    if let Err(e) = self.timelock.queue(proposal_id, eta) {
                        events.push(GovernanceEvent::ProposalFailed {
                            proposal_id,
                            reason: e.to_string(),
                        });
                    } else {
                        self.proposals.set_status(&proposal_id, ProposalStatus::Queued);
                        events.push(GovernanceEvent::ProposalQueued {
                            proposal_id,
                            eta,
                        });
                    }
                } else {
                    self.proposals.set_status(&proposal_id, ProposalStatus::Defeated);
                    events.push(GovernanceEvent::ProposalDefeated { proposal_id });
                }
            }
        }

        events
    }

    /// Create a new proposal
    pub fn create_proposal(
        &mut self,
        proposer: PublicKey,
        proposer_votes: u64,
        title: String,
        description: String,
        operations: Vec<GovernanceOperation>,
    ) -> Result<ProposalId> {
        // Check proposer has enough tokens
        if proposer_votes < self.config.proposal_threshold {
            return Err(Error::InsufficientCollateral {
                required: self.config.proposal_threshold,
                available: proposer_votes,
            });
        }

        // Check operations count
        if operations.is_empty() {
            return Err(Error::InvalidParameter {
                name: "operations".into(),
                reason: "must have at least one operation".into(),
            });
        }
        if operations.len() > self.config.max_operations {
            return Err(Error::InvalidParameter {
                name: "operations".into(),
                reason: format!("exceeds max of {}", self.config.max_operations),
            });
        }

        // Calculate voting period
        let start_block = self.block_height + self.config.voting_delay_blocks;
        let end_block = start_block + self.config.voting_period_blocks;

        let proposal = Proposal::new(
            proposer,
            title,
            description,
            operations,
            start_block,
            end_block,
            self.block_height,
        );

        let id = proposal.id;
        self.proposals.add(proposal)?;

        Ok(id)
    }

    /// Cast a vote on a proposal
    pub fn cast_vote(
        &mut self,
        proposal_id: ProposalId,
        voter: PublicKey,
        voting_power: u64,
        support: VoteSupport,
    ) -> Result<()> {
        let proposal = self.proposals.get(&proposal_id)
            .ok_or_else(|| Error::Internal("Proposal not found".into()))?;

        // Check voting is active
        if self.block_height < proposal.start_block {
            return Err(Error::InvalidParameter {
                name: "block_height".into(),
                reason: "voting has not started".into(),
            });
        }
        if self.block_height > proposal.end_block {
            return Err(Error::InvalidParameter {
                name: "block_height".into(),
                reason: "voting has ended".into(),
            });
        }

        self.voting.cast_vote(proposal_id, voter, voting_power, support)
    }

    /// Execute a queued proposal
    pub fn execute_proposal(&mut self, proposal_id: ProposalId) -> Result<Vec<GovernanceOperation>> {
        let proposal = self.proposals.get(&proposal_id)
            .ok_or_else(|| Error::Internal("Proposal not found".into()))?;

        if proposal.status != ProposalStatus::Queued {
            return Err(Error::InvalidParameter {
                name: "status".into(),
                reason: "proposal not queued".into(),
            });
        }

        // Check timelock has passed
        self.timelock.execute(&proposal_id, self.block_height)?;

        let operations = proposal.operations.clone();
        self.proposals.set_status(&proposal_id, ProposalStatus::Executed);

        Ok(operations)
    }

    /// Cancel a proposal (guardian only or proposer if defeated)
    pub fn cancel_proposal(&mut self, proposal_id: ProposalId, caller: &PublicKey) -> Result<()> {
        let proposal = self.proposals.get(&proposal_id)
            .ok_or_else(|| Error::Internal("Proposal not found".into()))?;

        let is_guardian = self.guardian.as_ref() == Some(caller);
        let is_proposer = proposal.proposer == *caller;

        if !is_guardian && !is_proposer {
            return Err(Error::Unauthorized("only guardian or proposer can cancel".into()));
        }

        if proposal.status == ProposalStatus::Executed {
            return Err(Error::InvalidParameter {
                name: "status".into(),
                reason: "cannot cancel executed proposal".into(),
            });
        }

        self.proposals.set_status(&proposal_id, ProposalStatus::Canceled);
        self.timelock.cancel(&proposal_id);

        Ok(())
    }

    /// Check if a proposal passed based on votes
    fn check_proposal_passed(&self, proposal: &Proposal, votes: &VoteTally) -> bool {
        // Must have quorum
        let total_votes = votes.for_votes + votes.against_votes + votes.abstain_votes;
        let required_quorum = self.calculate_quorum(votes.total_supply);

        if total_votes < required_quorum {
            return false;
        }

        // For votes must exceed against votes
        votes.for_votes > votes.against_votes
    }

    /// Calculate required quorum based on total supply
    fn calculate_quorum(&self, total_supply: u64) -> u64 {
        total_supply * self.config.quorum_bps / 10000
    }

    /// Get proposal state
    pub fn get_proposal_state(&self, proposal_id: &ProposalId) -> Option<ProposalState> {
        let proposal = self.proposals.get(proposal_id)?;
        let votes = self.voting.get_votes(proposal_id);

        let state = match proposal.status {
            ProposalStatus::Pending => {
                if self.block_height < proposal.start_block {
                    ProposalState::Pending
                } else if self.block_height <= proposal.end_block {
                    ProposalState::Active
                } else {
                    ProposalState::Expired
                }
            }
            ProposalStatus::Active => ProposalState::Active,
            ProposalStatus::Queued => {
                if let Some(eta) = self.timelock.get_eta(proposal_id) {
                    if self.block_height >= eta + self.config.grace_period_blocks {
                        ProposalState::Expired
                    } else if self.block_height >= eta {
                        ProposalState::Ready
                    } else {
                        ProposalState::Queued
                    }
                } else {
                    ProposalState::Queued
                }
            }
            ProposalStatus::Executed => ProposalState::Executed,
            ProposalStatus::Defeated => ProposalState::Defeated,
            ProposalStatus::Canceled => ProposalState::Canceled,
            ProposalStatus::Expired => ProposalState::Expired,
        };

        Some(state)
    }

    /// Get statistics
    pub fn statistics(&self) -> GovernanceStats {
        GovernanceStats {
            total_proposals: self.proposals.total_count(),
            active_proposals: self.proposals.active_count(),
            queued_proposals: self.timelock.queued_count(),
            executed_proposals: self.proposals.executed_count(),
            total_votes_cast: self.voting.total_votes(),
            unique_voters: self.voting.unique_voters(),
        }
    }
}

/// Governance events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GovernanceEvent {
    /// Proposal was created
    ProposalCreated {
        proposal_id: ProposalId,
        proposer: PublicKey,
        start_block: u64,
        end_block: u64,
    },
    /// Vote was cast
    VoteCast {
        proposal_id: ProposalId,
        voter: PublicKey,
        support: VoteSupport,
        votes: u64,
    },
    /// Proposal was queued for execution
    ProposalQueued {
        proposal_id: ProposalId,
        eta: u64,
    },
    /// Proposal was executed
    ProposalExecuted {
        proposal_id: ProposalId,
    },
    /// Proposal was defeated
    ProposalDefeated {
        proposal_id: ProposalId,
    },
    /// Proposal failed
    ProposalFailed {
        proposal_id: ProposalId,
        reason: String,
    },
    /// Proposal was canceled
    ProposalCanceled {
        proposal_id: ProposalId,
    },
}

/// Current state of a proposal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalState {
    /// Waiting for voting to start
    Pending,
    /// Voting is active
    Active,
    /// Voting ended, waiting in timelock
    Queued,
    /// Ready to execute
    Ready,
    /// Successfully executed
    Executed,
    /// Did not reach quorum or was voted down
    Defeated,
    /// Canceled by guardian or proposer
    Canceled,
    /// Expired before execution
    Expired,
}

/// Governance statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceStats {
    /// Total proposals created
    pub total_proposals: u64,
    /// Currently active proposals
    pub active_proposals: u64,
    /// Proposals in timelock queue
    pub queued_proposals: u64,
    /// Successfully executed proposals
    pub executed_proposals: u64,
    /// Total votes cast across all proposals
    pub total_votes_cast: u64,
    /// Unique voters who have participated
    pub unique_voters: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::crypto::KeyPair;

    #[test]
    fn test_governance_config_validation() {
        let config = GovernanceConfig::default();
        assert!(config.validate().is_ok());

        let mut bad_config = config.clone();
        bad_config.quorum_bps = 20000;
        assert!(bad_config.validate().is_err());
    }

    #[test]
    fn test_create_proposal() {
        let guardian = KeyPair::generate();
        let mut gov = GovernanceSystem::new(
            GovernanceConfig::testnet(),
            Some(*guardian.public_key()),
        );

        let proposer = KeyPair::generate();
        let operations = vec![
            GovernanceOperation::UpdateParameter {
                parameter: ProtocolParameter::MinCollateralRatio,
                new_value: 120,
            },
        ];

        let result = gov.create_proposal(
            *proposer.public_key(),
            10_000_00, // $10,000
            "Test Proposal".into(),
            "Update MCR to 120%".into(),
            operations,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_proposal_threshold() {
        let mut gov = GovernanceSystem::new(GovernanceConfig::testnet(), None);

        let proposer = KeyPair::generate();
        let operations = vec![
            GovernanceOperation::UpdateParameter {
                parameter: ProtocolParameter::MinCollateralRatio,
                new_value: 120,
            },
        ];

        // Below threshold should fail
        let result = gov.create_proposal(
            *proposer.public_key(),
            100, // Only $1, below $1000 threshold
            "Test".into(),
            "Test".into(),
            operations,
        );

        assert!(result.is_err());
    }
}
