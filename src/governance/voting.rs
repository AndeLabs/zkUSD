//! Voting system for governance proposals.
//!
//! Implements token-weighted voting with support for:
//! - For/Against/Abstain votes
//! - Vote delegation
//! - Vote weight checkpoints

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::error::{Error, Result};
use crate::utils::crypto::PublicKey;

use super::proposal::ProposalId;

// ═══════════════════════════════════════════════════════════════════════════════
// VOTE SUPPORT
// ═══════════════════════════════════════════════════════════════════════════════

/// Type of vote support
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VoteSupport {
    /// Against the proposal
    Against = 0,
    /// For the proposal
    For = 1,
    /// Abstain (counts towards quorum but not for/against)
    Abstain = 2,
}

impl VoteSupport {
    /// Create from u8
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(VoteSupport::Against),
            1 => Some(VoteSupport::For),
            2 => Some(VoteSupport::Abstain),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VOTE RECORD
// ═══════════════════════════════════════════════════════════════════════════════

/// Record of a single vote
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteRecord {
    /// Voter's public key
    pub voter: PublicKey,
    /// Proposal being voted on
    pub proposal_id: ProposalId,
    /// How they voted
    pub support: VoteSupport,
    /// Voting power at time of vote
    pub votes: u64,
    /// Block when vote was cast
    pub block_height: u64,
    /// Optional reason for vote
    pub reason: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// VOTE TALLY
// ═══════════════════════════════════════════════════════════════════════════════

/// Aggregated vote counts for a proposal
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoteTally {
    /// Total votes for
    pub for_votes: u64,
    /// Total votes against
    pub against_votes: u64,
    /// Total abstain votes
    pub abstain_votes: u64,
    /// Total supply at snapshot (for quorum calculation)
    pub total_supply: u64,
    /// Number of unique voters
    pub voter_count: u64,
}

impl VoteTally {
    /// Create new tally with total supply
    pub fn new(total_supply: u64) -> Self {
        Self {
            total_supply,
            ..Default::default()
        }
    }

    /// Add votes to tally
    pub fn add_votes(&mut self, support: VoteSupport, votes: u64) {
        match support {
            VoteSupport::For => self.for_votes = self.for_votes.saturating_add(votes),
            VoteSupport::Against => self.against_votes = self.against_votes.saturating_add(votes),
            VoteSupport::Abstain => self.abstain_votes = self.abstain_votes.saturating_add(votes),
        }
        self.voter_count += 1;
    }

    /// Get total votes cast
    pub fn total_votes(&self) -> u64 {
        self.for_votes + self.against_votes + self.abstain_votes
    }

    /// Calculate participation rate (basis points)
    pub fn participation_bps(&self) -> u64 {
        if self.total_supply == 0 {
            return 0;
        }
        self.total_votes() * 10000 / self.total_supply
    }

    /// Check if quorum was reached
    pub fn has_quorum(&self, quorum_bps: u64) -> bool {
        self.participation_bps() >= quorum_bps
    }

    /// Check if proposal passed
    pub fn passed(&self) -> bool {
        self.for_votes > self.against_votes
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DELEGATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Vote delegation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delegation {
    /// Delegator
    pub from: PublicKey,
    /// Delegate
    pub to: PublicKey,
    /// Amount delegated
    pub amount: u64,
    /// Block when delegated
    pub block_height: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// VOTING SYSTEM
// ═══════════════════════════════════════════════════════════════════════════════

/// Main voting system
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VotingSystem {
    /// Votes by proposal
    proposal_votes: HashMap<ProposalId, VoteTally>,
    /// Individual vote records
    vote_records: HashMap<(ProposalId, PublicKey), VoteRecord>,
    /// Delegations by delegator
    delegations: HashMap<PublicKey, Delegation>,
    /// Voting power checkpoints (voter -> block -> power)
    checkpoints: HashMap<PublicKey, Vec<(u64, u64)>>,
    /// Total votes cast ever
    total_votes_cast: u64,
    /// Unique voters
    unique_voters: HashSet<PublicKey>,
}

impl VotingSystem {
    /// Create new voting system
    pub fn new() -> Self {
        Self::default()
    }

    /// Initialize vote tally for a proposal
    pub fn init_proposal(&mut self, proposal_id: ProposalId, total_supply: u64) {
        self.proposal_votes
            .entry(proposal_id)
            .or_insert_with(|| VoteTally::new(total_supply));
    }

    /// Cast a vote
    pub fn cast_vote(
        &mut self,
        proposal_id: ProposalId,
        voter: PublicKey,
        voting_power: u64,
        support: VoteSupport,
    ) -> Result<()> {
        // Check if already voted
        let key = (proposal_id, voter);
        if self.vote_records.contains_key(&key) {
            return Err(Error::Internal("Already voted on this proposal".into()));
        }

        if voting_power == 0 {
            return Err(Error::ZeroAmount);
        }

        // Get or create tally
        let tally = self.proposal_votes
            .entry(proposal_id)
            .or_insert_with(|| VoteTally::new(0));

        // Add votes
        tally.add_votes(support, voting_power);

        // Record vote
        let record = VoteRecord {
            voter,
            proposal_id,
            support,
            votes: voting_power,
            block_height: 0, // Should be set by caller
            reason: None,
        };

        self.vote_records.insert(key, record);
        self.total_votes_cast += 1;
        self.unique_voters.insert(voter);

        Ok(())
    }

    /// Cast vote with reason
    pub fn cast_vote_with_reason(
        &mut self,
        proposal_id: ProposalId,
        voter: PublicKey,
        voting_power: u64,
        support: VoteSupport,
        reason: String,
    ) -> Result<()> {
        self.cast_vote(proposal_id, voter, voting_power, support)?;

        // Add reason to record
        if let Some(record) = self.vote_records.get_mut(&(proposal_id, voter)) {
            record.reason = Some(reason);
        }

        Ok(())
    }

    /// Get vote tally for a proposal
    pub fn get_votes(&self, proposal_id: &ProposalId) -> VoteTally {
        self.proposal_votes
            .get(proposal_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Get vote record for a specific voter
    pub fn get_vote_record(&self, proposal_id: &ProposalId, voter: &PublicKey) -> Option<&VoteRecord> {
        self.vote_records.get(&(*proposal_id, *voter))
    }

    /// Check if voter has voted on proposal
    pub fn has_voted(&self, proposal_id: &ProposalId, voter: &PublicKey) -> bool {
        self.vote_records.contains_key(&(*proposal_id, *voter))
    }

    /// Get all votes for a proposal
    pub fn get_all_votes(&self, proposal_id: &ProposalId) -> Vec<&VoteRecord> {
        self.vote_records
            .iter()
            .filter(|((pid, _), _)| pid == proposal_id)
            .map(|(_, record)| record)
            .collect()
    }

    /// Delegate votes
    pub fn delegate(&mut self, from: PublicKey, to: PublicKey, amount: u64, block_height: u64) -> Result<()> {
        if from == to {
            return Err(Error::InvalidParameter {
                name: "to".into(),
                reason: "cannot delegate to self".into(),
            });
        }

        if amount == 0 {
            return Err(Error::ZeroAmount);
        }

        let delegation = Delegation {
            from,
            to,
            amount,
            block_height,
        };

        self.delegations.insert(from, delegation);
        Ok(())
    }

    /// Remove delegation
    pub fn undelegate(&mut self, from: &PublicKey) -> Option<Delegation> {
        self.delegations.remove(from)
    }

    /// Get current delegate for an address
    pub fn get_delegate(&self, voter: &PublicKey) -> Option<&PublicKey> {
        self.delegations.get(voter).map(|d| &d.to)
    }

    /// Get all delegators to a delegate
    pub fn get_delegators(&self, delegate: &PublicKey) -> Vec<&Delegation> {
        self.delegations
            .values()
            .filter(|d| &d.to == delegate)
            .collect()
    }

    /// Calculate total voting power for an address (own + delegated)
    pub fn get_voting_power(&self, voter: &PublicKey, own_balance: u64) -> u64 {
        // Start with own balance (unless delegated away)
        let mut power = if self.delegations.contains_key(voter) {
            0
        } else {
            own_balance
        };

        // Add delegated votes
        for delegation in self.delegations.values() {
            if &delegation.to == voter {
                power = power.saturating_add(delegation.amount);
            }
        }

        power
    }

    /// Record a voting power checkpoint
    pub fn checkpoint(&mut self, voter: PublicKey, block_height: u64, power: u64) {
        let checkpoints = self.checkpoints.entry(voter).or_default();

        // Only add if power changed
        if let Some(&(_, last_power)) = checkpoints.last() {
            if last_power == power {
                return;
            }
        }

        checkpoints.push((block_height, power));
    }

    /// Get voting power at a specific block
    pub fn get_prior_votes(&self, voter: &PublicKey, block_height: u64) -> u64 {
        let checkpoints = match self.checkpoints.get(voter) {
            Some(c) => c,
            None => return 0,
        };

        if checkpoints.is_empty() {
            return 0;
        }

        // Binary search for checkpoint at or before block_height
        let idx = checkpoints.partition_point(|(b, _)| *b <= block_height);

        if idx == 0 {
            0
        } else {
            checkpoints[idx - 1].1
        }
    }

    /// Total votes cast
    pub fn total_votes(&self) -> u64 {
        self.total_votes_cast
    }

    /// Unique voter count
    pub fn unique_voters(&self) -> u64 {
        self.unique_voters.len() as u64
    }

    /// Get top voters for a proposal
    pub fn get_top_voters(&self, proposal_id: &ProposalId, limit: usize) -> Vec<&VoteRecord> {
        let mut votes: Vec<_> = self.get_all_votes(proposal_id);
        votes.sort_by(|a, b| b.votes.cmp(&a.votes));
        votes.truncate(limit);
        votes
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
    fn test_vote_tally() {
        let mut tally = VoteTally::new(1_000_000);

        tally.add_votes(VoteSupport::For, 100_000);
        tally.add_votes(VoteSupport::Against, 50_000);
        tally.add_votes(VoteSupport::Abstain, 25_000);

        assert_eq!(tally.total_votes(), 175_000);
        assert_eq!(tally.participation_bps(), 1750); // 17.5%
        assert!(tally.passed()); // For > Against
    }

    #[test]
    fn test_cast_vote() {
        let mut voting = VotingSystem::new();
        let voter = KeyPair::generate();
        let proposal_id = test_proposal_id();

        voting.cast_vote(
            proposal_id,
            *voter.public_key(),
            10_000,
            VoteSupport::For,
        ).unwrap();

        assert!(voting.has_voted(&proposal_id, voter.public_key()));

        let tally = voting.get_votes(&proposal_id);
        assert_eq!(tally.for_votes, 10_000);
    }

    #[test]
    fn test_double_vote_prevented() {
        let mut voting = VotingSystem::new();
        let voter = KeyPair::generate();
        let proposal_id = test_proposal_id();

        voting.cast_vote(proposal_id, *voter.public_key(), 10_000, VoteSupport::For).unwrap();

        // Second vote should fail
        let result = voting.cast_vote(proposal_id, *voter.public_key(), 5_000, VoteSupport::Against);
        assert!(result.is_err());
    }

    #[test]
    fn test_delegation() {
        let mut voting = VotingSystem::new();
        let delegator = KeyPair::generate();
        let delegate = KeyPair::generate();

        voting.delegate(
            *delegator.public_key(),
            *delegate.public_key(),
            10_000,
            100,
        ).unwrap();

        assert_eq!(
            voting.get_delegate(delegator.public_key()),
            Some(delegate.public_key())
        );

        // Delegate's power should include delegated amount
        let power = voting.get_voting_power(delegate.public_key(), 5_000);
        assert_eq!(power, 15_000); // 5000 own + 10000 delegated
    }

    #[test]
    fn test_checkpoints() {
        let mut voting = VotingSystem::new();
        let voter = KeyPair::generate();

        voting.checkpoint(*voter.public_key(), 100, 1000);
        voting.checkpoint(*voter.public_key(), 200, 2000);
        voting.checkpoint(*voter.public_key(), 300, 1500);

        assert_eq!(voting.get_prior_votes(voter.public_key(), 150), 1000);
        assert_eq!(voting.get_prior_votes(voter.public_key(), 250), 2000);
        assert_eq!(voting.get_prior_votes(voter.public_key(), 350), 1500);
    }
}
