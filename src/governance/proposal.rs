//! Governance proposal management.
//!
//! Handles proposal lifecycle from creation to execution.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::utils::crypto::{Hash, PublicKey};

use super::parameters::GovernanceOperation;

// ═══════════════════════════════════════════════════════════════════════════════
// PROPOSAL ID
// ═══════════════════════════════════════════════════════════════════════════════

/// Unique proposal identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProposalId(pub [u8; 32]);

impl ProposalId {
    /// Generate proposal ID from components
    pub fn generate(proposer: &PublicKey, nonce: u64, block_height: u64) -> Self {
        let mut data = Vec::new();
        data.extend_from_slice(proposer.as_bytes());
        data.extend_from_slice(&nonce.to_be_bytes());
        data.extend_from_slice(&block_height.to_be_bytes());
        Self(*Hash::sha256(&data).as_bytes())
    }

    /// Create from bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get as bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl std::fmt::Display for ProposalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.to_hex()[..16])
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROPOSAL STATUS
// ═══════════════════════════════════════════════════════════════════════════════

/// Status of a proposal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProposalStatus {
    /// Proposal created, waiting for voting to start
    Pending,
    /// Voting is active
    Active,
    /// Passed and queued in timelock
    Queued,
    /// Successfully executed
    Executed,
    /// Did not pass (quorum not met or more against than for)
    Defeated,
    /// Canceled by guardian or proposer
    Canceled,
    /// Expired before execution
    Expired,
}

impl ProposalStatus {
    /// Check if proposal is finalized
    pub fn is_final(&self) -> bool {
        matches!(
            self,
            ProposalStatus::Executed | ProposalStatus::Defeated |
            ProposalStatus::Canceled | ProposalStatus::Expired
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROPOSAL
// ═══════════════════════════════════════════════════════════════════════════════

/// A governance proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    /// Unique identifier
    pub id: ProposalId,
    /// Proposer's public key
    pub proposer: PublicKey,
    /// Title
    pub title: String,
    /// Description
    pub description: String,
    /// Operations to execute if passed
    pub operations: Vec<GovernanceOperation>,
    /// Block when voting starts
    pub start_block: u64,
    /// Block when voting ends
    pub end_block: u64,
    /// Block when proposal was created
    pub created_at: u64,
    /// Current status
    pub status: ProposalStatus,
    /// ETA for execution (set when queued)
    pub eta: Option<u64>,
}

impl Proposal {
    /// Create a new proposal
    pub fn new(
        proposer: PublicKey,
        title: String,
        description: String,
        operations: Vec<GovernanceOperation>,
        start_block: u64,
        end_block: u64,
        created_at: u64,
    ) -> Self {
        let id = ProposalId::generate(&proposer, created_at, start_block);

        Self {
            id,
            proposer,
            title,
            description,
            operations,
            start_block,
            end_block,
            created_at,
            status: ProposalStatus::Pending,
            eta: None,
        }
    }

    /// Check if voting is active at given block
    pub fn is_voting_active(&self, block_height: u64) -> bool {
        block_height >= self.start_block &&
        block_height <= self.end_block &&
        self.status == ProposalStatus::Active
    }

    /// Compute proposal hash (for signing/verification)
    pub fn hash(&self) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(self.id.as_bytes());
        data.extend_from_slice(self.proposer.as_bytes());

        for op in &self.operations {
            let op_bytes = bincode::serialize(op).unwrap_or_default();
            data.extend_from_slice(&op_bytes);
        }

        Hash::sha256(&data)
    }

    /// Get summary for display
    pub fn summary(&self) -> ProposalSummary {
        ProposalSummary {
            id: self.id,
            title: self.title.clone(),
            proposer: self.proposer,
            status: self.status,
            operations_count: self.operations.len(),
            start_block: self.start_block,
            end_block: self.end_block,
        }
    }
}

/// Brief summary of a proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalSummary {
    /// Proposal ID
    pub id: ProposalId,
    /// Title
    pub title: String,
    /// Proposer
    pub proposer: PublicKey,
    /// Current status
    pub status: ProposalStatus,
    /// Number of operations
    pub operations_count: usize,
    /// Voting start block
    pub start_block: u64,
    /// Voting end block
    pub end_block: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROPOSAL MANAGER
// ═══════════════════════════════════════════════════════════════════════════════

/// Manages all proposals
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProposalManager {
    /// All proposals by ID
    proposals: HashMap<ProposalId, Proposal>,
    /// Proposals by proposer
    by_proposer: HashMap<PublicKey, Vec<ProposalId>>,
    /// Proposal count by status
    status_counts: HashMap<ProposalStatus, u64>,
    /// Next proposal nonce (for unique IDs)
    nonce: u64,
}

impl ProposalManager {
    /// Create new proposal manager
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a new proposal
    pub fn add(&mut self, proposal: Proposal) -> Result<()> {
        if self.proposals.contains_key(&proposal.id) {
            return Err(Error::Internal("Proposal already exists".into()));
        }

        let id = proposal.id;
        let proposer = proposal.proposer;
        let status = proposal.status;

        self.proposals.insert(id, proposal);
        self.by_proposer.entry(proposer).or_default().push(id);
        *self.status_counts.entry(status).or_insert(0) += 1;
        self.nonce += 1;

        Ok(())
    }

    /// Get a proposal by ID
    pub fn get(&self, id: &ProposalId) -> Option<&Proposal> {
        self.proposals.get(id)
    }

    /// Get mutable proposal by ID
    pub fn get_mut(&mut self, id: &ProposalId) -> Option<&mut Proposal> {
        self.proposals.get_mut(id)
    }

    /// Set proposal status
    pub fn set_status(&mut self, id: &ProposalId, new_status: ProposalStatus) {
        if let Some(proposal) = self.proposals.get_mut(id) {
            let old_status = proposal.status;
            proposal.status = new_status;

            // Update counts
            if let Some(count) = self.status_counts.get_mut(&old_status) {
                *count = count.saturating_sub(1);
            }
            *self.status_counts.entry(new_status).or_insert(0) += 1;
        }
    }

    /// Set ETA for execution
    pub fn set_eta(&mut self, id: &ProposalId, eta: u64) {
        if let Some(proposal) = self.proposals.get_mut(id) {
            proposal.eta = Some(eta);
        }
    }

    /// Get proposals by proposer
    pub fn get_by_proposer(&self, proposer: &PublicKey) -> Vec<&Proposal> {
        self.by_proposer
            .get(proposer)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.proposals.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get proposals that end voting at a specific block
    pub fn get_proposals_at_voting_end(&self, block_height: u64) -> Vec<ProposalId> {
        self.proposals
            .values()
            .filter(|p| p.end_block == block_height && p.status == ProposalStatus::Active)
            .map(|p| p.id)
            .collect()
    }

    /// Get all active proposals
    pub fn get_active(&self) -> Vec<&Proposal> {
        self.proposals
            .values()
            .filter(|p| matches!(p.status, ProposalStatus::Pending | ProposalStatus::Active))
            .collect()
    }

    /// Get all queued proposals
    pub fn get_queued(&self) -> Vec<&Proposal> {
        self.proposals
            .values()
            .filter(|p| p.status == ProposalStatus::Queued)
            .collect()
    }

    /// Total proposal count
    pub fn total_count(&self) -> u64 {
        self.proposals.len() as u64
    }

    /// Active proposal count
    pub fn active_count(&self) -> u64 {
        *self.status_counts.get(&ProposalStatus::Active).unwrap_or(&0) +
        *self.status_counts.get(&ProposalStatus::Pending).unwrap_or(&0)
    }

    /// Executed proposal count
    pub fn executed_count(&self) -> u64 {
        *self.status_counts.get(&ProposalStatus::Executed).unwrap_or(&0)
    }

    /// Get all proposals (paginated)
    pub fn list(&self, offset: usize, limit: usize) -> Vec<ProposalSummary> {
        let mut proposals: Vec<_> = self.proposals.values().collect();
        proposals.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        proposals
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|p| p.summary())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::crypto::KeyPair;

    #[test]
    fn test_proposal_id_generation() {
        let proposer = KeyPair::generate();
        let id1 = ProposalId::generate(proposer.public_key(), 1, 100);
        let id2 = ProposalId::generate(proposer.public_key(), 2, 100);

        assert_ne!(id1, id2);
    }

    #[test]
    fn test_proposal_manager() {
        let mut manager = ProposalManager::new();
        let proposer = KeyPair::generate();

        let proposal = Proposal::new(
            *proposer.public_key(),
            "Test".into(),
            "Test proposal".into(),
            vec![],
            100,
            200,
            1,
        );

        let id = proposal.id;
        manager.add(proposal).unwrap();

        assert!(manager.get(&id).is_some());
        assert_eq!(manager.total_count(), 1);
    }

    #[test]
    fn test_status_tracking() {
        let mut manager = ProposalManager::new();
        let proposer = KeyPair::generate();

        let proposal = Proposal::new(
            *proposer.public_key(),
            "Test".into(),
            "Test".into(),
            vec![],
            100,
            200,
            1,
        );

        let id = proposal.id;
        manager.add(proposal).unwrap();

        assert_eq!(manager.active_count(), 1);

        manager.set_status(&id, ProposalStatus::Executed);
        assert_eq!(manager.active_count(), 0);
        assert_eq!(manager.executed_count(), 1);
    }
}
