//! Timelock queue for delayed execution.
//!
//! Implements a security mechanism that delays execution of approved proposals,
//! giving users time to react to upcoming changes.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use crate::error::{Error, Result};

use super::proposal::ProposalId;

// ═══════════════════════════════════════════════════════════════════════════════
// TIMELOCK ENTRY
// ═══════════════════════════════════════════════════════════════════════════════

/// Entry in the timelock queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelockEntry {
    /// Proposal ID
    pub proposal_id: ProposalId,
    /// Earliest block for execution (ETA)
    pub eta: u64,
    /// Block when queued
    pub queued_at: u64,
    /// Whether entry has been executed
    pub executed: bool,
    /// Whether entry was canceled
    pub canceled: bool,
}

impl TimelockEntry {
    /// Create new timelock entry
    pub fn new(proposal_id: ProposalId, eta: u64, queued_at: u64) -> Self {
        Self {
            proposal_id,
            eta,
            queued_at,
            executed: false,
            canceled: false,
        }
    }

    /// Check if entry can be executed at given block
    pub fn can_execute(&self, block_height: u64) -> bool {
        !self.executed && !self.canceled && block_height >= self.eta
    }

    /// Check if entry is expired
    pub fn is_expired(&self, block_height: u64, grace_period: u64) -> bool {
        block_height > self.eta + grace_period
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TIMELOCK QUEUE
// ═══════════════════════════════════════════════════════════════════════════════

/// Queue of proposals waiting to be executed
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TimelockQueue {
    /// Entries by proposal ID
    entries: HashMap<ProposalId, TimelockEntry>,
    /// Entries ordered by ETA for efficient lookup
    by_eta: BTreeMap<u64, Vec<ProposalId>>,
    /// Minimum delay in blocks
    min_delay: u64,
    /// Maximum delay in blocks
    max_delay: u64,
}

impl TimelockQueue {
    /// Create new timelock queue
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            by_eta: BTreeMap::new(),
            min_delay: 100,      // Minimum ~25 minutes at 15s blocks
            max_delay: 172800,   // Maximum ~30 days
        }
    }

    /// Create with custom delay bounds
    pub fn with_delays(min_delay: u64, max_delay: u64) -> Self {
        Self {
            entries: HashMap::new(),
            by_eta: BTreeMap::new(),
            min_delay,
            max_delay,
        }
    }

    /// Queue a proposal for execution
    pub fn queue(&mut self, proposal_id: ProposalId, eta: u64) -> Result<()> {
        if self.entries.contains_key(&proposal_id) {
            return Err(Error::Internal("Proposal already queued".into()));
        }

        let entry = TimelockEntry::new(proposal_id, eta, 0);
        self.entries.insert(proposal_id, entry);
        self.by_eta.entry(eta).or_default().push(proposal_id);

        Ok(())
    }

    /// Queue with current block height
    pub fn queue_at(&mut self, proposal_id: ProposalId, current_block: u64, delay: u64) -> Result<u64> {
        // Validate delay bounds
        if delay < self.min_delay {
            return Err(Error::InvalidParameter {
                name: "delay".into(),
                reason: format!("delay {} below minimum {}", delay, self.min_delay),
            });
        }
        if delay > self.max_delay {
            return Err(Error::InvalidParameter {
                name: "delay".into(),
                reason: format!("delay {} above maximum {}", delay, self.max_delay),
            });
        }

        let eta = current_block + delay;

        if self.entries.contains_key(&proposal_id) {
            return Err(Error::Internal("Proposal already queued".into()));
        }

        let entry = TimelockEntry::new(proposal_id, eta, current_block);
        self.entries.insert(proposal_id, entry);
        self.by_eta.entry(eta).or_default().push(proposal_id);

        Ok(eta)
    }

    /// Execute a queued proposal
    pub fn execute(&mut self, proposal_id: &ProposalId, block_height: u64) -> Result<()> {
        let entry = self.entries.get_mut(proposal_id)
            .ok_or_else(|| Error::Internal("Proposal not in queue".into()))?;

        if entry.executed {
            return Err(Error::Internal("Proposal already executed".into()));
        }

        if entry.canceled {
            return Err(Error::Internal("Proposal was canceled".into()));
        }

        if block_height < entry.eta {
            return Err(Error::InvalidParameter {
                name: "block_height".into(),
                reason: format!(
                    "timelock not expired, current: {}, eta: {}",
                    block_height, entry.eta
                ),
            });
        }

        entry.executed = true;
        Ok(())
    }

    /// Cancel a queued proposal
    pub fn cancel(&mut self, proposal_id: &ProposalId) {
        if let Some(entry) = self.entries.get_mut(proposal_id) {
            entry.canceled = true;
        }
    }

    /// Get entry for a proposal
    pub fn get(&self, proposal_id: &ProposalId) -> Option<&TimelockEntry> {
        self.entries.get(proposal_id)
    }

    /// Get ETA for a proposal
    pub fn get_eta(&self, proposal_id: &ProposalId) -> Option<u64> {
        self.entries.get(proposal_id).map(|e| e.eta)
    }

    /// Check if proposal is in queue
    pub fn is_queued(&self, proposal_id: &ProposalId) -> bool {
        self.entries.get(proposal_id)
            .map(|e| !e.executed && !e.canceled)
            .unwrap_or(false)
    }

    /// Get proposals ready for execution
    pub fn get_ready(&self, block_height: u64) -> Vec<ProposalId> {
        self.entries
            .iter()
            .filter(|(_, e)| e.can_execute(block_height))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Get proposals expiring soon (within grace period)
    pub fn get_expiring(&self, block_height: u64, grace_period: u64) -> Vec<ProposalId> {
        self.entries
            .iter()
            .filter(|(_, e)| {
                let expiry = e.eta + grace_period;
                !e.executed && !e.canceled &&
                block_height >= e.eta &&
                block_height < expiry
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// Clean up expired entries
    pub fn cleanup_expired(&mut self, block_height: u64, grace_period: u64) -> usize {
        let expired: Vec<_> = self.entries
            .iter()
            .filter(|(_, e)| e.is_expired(block_height, grace_period))
            .map(|(id, _)| *id)
            .collect();

        let count = expired.len();

        for id in expired {
            if let Some(entry) = self.entries.remove(&id) {
                if let Some(ids) = self.by_eta.get_mut(&entry.eta) {
                    ids.retain(|i| i != &id);
                }
            }
        }

        count
    }

    /// Number of queued proposals
    pub fn queued_count(&self) -> u64 {
        self.entries
            .values()
            .filter(|e| !e.executed && !e.canceled)
            .count() as u64
    }

    /// Get queue statistics
    pub fn statistics(&self) -> TimelockStats {
        let mut queued = 0;
        let mut executed = 0;
        let mut canceled = 0;

        for entry in self.entries.values() {
            if entry.executed {
                executed += 1;
            } else if entry.canceled {
                canceled += 1;
            } else {
                queued += 1;
            }
        }

        TimelockStats {
            queued,
            executed,
            canceled,
            min_delay: self.min_delay,
            max_delay: self.max_delay,
        }
    }
}

/// Timelock statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelockStats {
    /// Currently queued proposals
    pub queued: u64,
    /// Executed proposals
    pub executed: u64,
    /// Canceled proposals
    pub canceled: u64,
    /// Minimum delay setting
    pub min_delay: u64,
    /// Maximum delay setting
    pub max_delay: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_proposal_id(n: u8) -> ProposalId {
        ProposalId::from_bytes([n; 32])
    }

    #[test]
    fn test_queue_proposal() {
        let mut queue = TimelockQueue::new();
        let proposal = test_proposal_id(1);

        queue.queue(proposal, 1000).unwrap();

        assert!(queue.is_queued(&proposal));
        assert_eq!(queue.get_eta(&proposal), Some(1000));
    }

    #[test]
    fn test_execute_proposal() {
        let mut queue = TimelockQueue::new();
        let proposal = test_proposal_id(1);

        queue.queue(proposal, 1000).unwrap();

        // Too early
        let result = queue.execute(&proposal, 999);
        assert!(result.is_err());

        // On time
        queue.execute(&proposal, 1000).unwrap();

        // Already executed
        let result = queue.execute(&proposal, 1001);
        assert!(result.is_err());
    }

    #[test]
    fn test_cancel_proposal() {
        let mut queue = TimelockQueue::new();
        let proposal = test_proposal_id(1);

        queue.queue(proposal, 1000).unwrap();
        queue.cancel(&proposal);

        assert!(!queue.is_queued(&proposal));

        let result = queue.execute(&proposal, 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_ready() {
        let mut queue = TimelockQueue::new();

        queue.queue(test_proposal_id(1), 100).unwrap();
        queue.queue(test_proposal_id(2), 200).unwrap();
        queue.queue(test_proposal_id(3), 300).unwrap();

        let ready = queue.get_ready(150);
        assert_eq!(ready.len(), 1);

        let ready = queue.get_ready(250);
        assert_eq!(ready.len(), 2);

        let ready = queue.get_ready(350);
        assert_eq!(ready.len(), 3);
    }

    #[test]
    fn test_delay_bounds() {
        let mut queue = TimelockQueue::with_delays(100, 1000);

        // Too short
        let result = queue.queue_at(test_proposal_id(1), 0, 50);
        assert!(result.is_err());

        // Too long
        let result = queue.queue_at(test_proposal_id(2), 0, 2000);
        assert!(result.is_err());

        // Just right
        let result = queue.queue_at(test_proposal_id(3), 0, 500);
        assert!(result.is_ok());
    }
}
