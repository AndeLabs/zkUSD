//! Event Indexer - High-level API for event management.
//!
//! Provides a unified interface for storing, querying, and subscribing to events.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

#[cfg(feature = "async-oracle")]
use tokio::sync::broadcast;

use crate::core::cdp::CDPId;
use crate::error::{Error, Result};
use crate::events::storage::{EventFilter, EventQuery, InMemoryEventStore, QueryResult, StoredEvent};
use crate::protocol::events::{EventLog, ProtocolEvent};
use crate::utils::crypto::PublicKey;

// ═══════════════════════════════════════════════════════════════════════════════
// EVENT SUBSCRIPTION
// ═══════════════════════════════════════════════════════════════════════════════

/// Subscription filter for real-time events
#[derive(Debug, Clone, Default)]
pub struct SubscriptionFilter {
    /// Event types to subscribe to (None = all)
    pub event_types: Option<Vec<String>>,
    /// CDP to filter by
    pub cdp_id: Option<CDPId>,
    /// Account to filter by
    pub account: Option<PublicKey>,
}

impl SubscriptionFilter {
    /// Create a filter that matches all events
    pub fn all() -> Self {
        Self::default()
    }

    /// Filter by event types
    pub fn event_types(mut self, types: Vec<String>) -> Self {
        self.event_types = Some(types);
        self
    }

    /// Filter by CDP
    pub fn cdp(mut self, cdp_id: CDPId) -> Self {
        self.cdp_id = Some(cdp_id);
        self
    }

    /// Filter by account
    pub fn account(mut self, account: PublicKey) -> Self {
        self.account = Some(account);
        self
    }

    /// Check if an event matches this filter
    pub fn matches(&self, event: &StoredEvent) -> bool {
        // Convert to EventFilter and use its matching logic
        let filter = EventFilter {
            event_types: self.event_types.clone(),
            cdp_id: self.cdp_id,
            account: self.account,
            from_block: None,
            to_block: None,
            from_timestamp: None,
            to_timestamp: None,
        };
        filter.matches(event)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INDEXER STATISTICS
// ═══════════════════════════════════════════════════════════════════════════════

/// Statistics about the event indexer
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexerStatistics {
    /// Total events indexed
    pub total_events: u64,
    /// Events by type
    pub events_by_type: HashMap<String, u64>,
    /// Latest block indexed
    pub latest_block: u64,
    /// Number of active subscribers
    pub subscriber_count: u32,
    /// Index uptime in seconds
    pub uptime_seconds: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// EVENT INDEXER
// ═══════════════════════════════════════════════════════════════════════════════

/// Main event indexer interface
pub struct EventIndexer {
    /// Event storage
    store: Arc<RwLock<InMemoryEventStore>>,
    /// Event type counts
    type_counts: Arc<RwLock<HashMap<String, u64>>>,
    /// Latest indexed block
    latest_block: Arc<RwLock<u64>>,
    /// Broadcast channel for subscriptions (async feature)
    #[cfg(feature = "async-oracle")]
    broadcaster: broadcast::Sender<StoredEvent>,
    /// Start time
    start_time: std::time::Instant,
}

impl EventIndexer {
    /// Create a new in-memory event indexer
    pub fn new() -> Self {
        #[cfg(feature = "async-oracle")]
        let (broadcaster, _) = broadcast::channel(1024);

        Self {
            store: Arc::new(RwLock::new(InMemoryEventStore::new())),
            type_counts: Arc::new(RwLock::new(HashMap::new())),
            latest_block: Arc::new(RwLock::new(0)),
            #[cfg(feature = "async-oracle")]
            broadcaster,
            start_time: std::time::Instant::now(),
        }
    }

    /// Create an indexer with persistent storage
    #[cfg(feature = "rocksdb-storage")]
    pub fn with_path(path: impl AsRef<Path>) -> Result<Self> {
        use crate::events::storage::PersistentEventStore;

        // For now, still use in-memory but initialize from persistent if exists
        // Full persistent implementation would replace InMemoryEventStore
        Ok(Self::new())
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // EVENT STORAGE
    // ═══════════════════════════════════════════════════════════════════════════

    /// Store a single event
    pub fn store_event(&self, block_height: u64, tx_index: u32, event_index: u32, event: ProtocolEvent) -> Result<StoredEvent> {
        let stored = {
            let mut store = self.store.write()
                .map_err(|_| Error::Lock)?;
            store.store(block_height, tx_index, event_index, event)
        };

        // Update type count
        {
            let mut counts = self.type_counts.write()
                .map_err(|_| Error::Lock)?;
            *counts.entry(stored.event_type().to_string()).or_insert(0) += 1;
        }

        // Update latest block
        {
            let mut latest = self.latest_block.write()
                .map_err(|_| Error::Lock)?;
            if block_height > *latest {
                *latest = block_height;
            }
        }

        // Broadcast to subscribers
        #[cfg(feature = "async-oracle")]
        {
            let _ = self.broadcaster.send(stored.clone());
        }

        Ok(stored)
    }

    /// Store events from an EventLog
    pub fn store_event_log(&self, block_height: u64, tx_index: u32, log: EventLog) -> Result<Vec<StoredEvent>> {
        let mut stored = Vec::new();
        for (idx, event) in log.events().iter().enumerate() {
            stored.push(self.store_event(block_height, tx_index, idx as u32, event.clone())?);
        }
        Ok(stored)
    }

    /// Store all events from a block
    pub fn store_block_events(&self, block_height: u64, events: Vec<(u32, EventLog)>) -> Result<Vec<StoredEvent>> {
        let mut all_stored = Vec::new();
        for (tx_index, log) in events {
            all_stored.extend(self.store_event_log(block_height, tx_index, log)?);
        }
        Ok(all_stored)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // EVENT QUERYING
    // ═══════════════════════════════════════════════════════════════════════════

    /// Query events with filter
    pub fn query(&self, query: EventQuery) -> Result<QueryResult> {
        let store = self.store.read()
            .map_err(|_| Error::Lock)?;
        Ok(store.query(&query))
    }

    /// Get event by ID
    pub fn get_event(&self, id: u64) -> Result<Option<StoredEvent>> {
        let store = self.store.read()
            .map_err(|_| Error::Lock)?;
        Ok(store.get(id).cloned())
    }

    /// Get events by block
    pub fn get_block_events(&self, block_height: u64) -> Result<Vec<StoredEvent>> {
        let store = self.store.read()
            .map_err(|_| Error::Lock)?;
        Ok(store.get_by_block(block_height).into_iter().cloned().collect())
    }

    /// Get events for a CDP
    pub fn get_cdp_events(&self, cdp_id: CDPId) -> Result<QueryResult> {
        self.query(EventQuery::new().cdp(cdp_id))
    }

    /// Get events for an account
    pub fn get_account_events(&self, account: PublicKey) -> Result<QueryResult> {
        self.query(EventQuery::new().account(account))
    }

    /// Get events by type
    pub fn get_events_by_type(&self, event_type: &str) -> Result<QueryResult> {
        self.query(EventQuery::new().event_type(event_type))
    }

    /// Get latest events
    pub fn get_latest_events(&self, limit: usize) -> Result<QueryResult> {
        self.query(EventQuery::new().limit(limit).descending())
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // SUBSCRIPTIONS (async feature)
    // ═══════════════════════════════════════════════════════════════════════════

    /// Subscribe to all events
    #[cfg(feature = "async-oracle")]
    pub fn subscribe(&self) -> broadcast::Receiver<StoredEvent> {
        self.broadcaster.subscribe()
    }

    /// Subscribe to filtered events
    #[cfg(feature = "async-oracle")]
    pub fn subscribe_filtered(&self, filter: SubscriptionFilter) -> FilteredReceiver {
        FilteredReceiver {
            receiver: self.broadcaster.subscribe(),
            filter,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // STATISTICS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Get indexer statistics
    pub fn statistics(&self) -> Result<IndexerStatistics> {
        let store = self.store.read()
            .map_err(|_| Error::Lock)?;
        let counts = self.type_counts.read()
            .map_err(|_| Error::Lock)?;
        let latest = *self.latest_block.read()
            .map_err(|_| Error::Lock)?;

        #[cfg(feature = "async-oracle")]
        let subscriber_count = self.broadcaster.receiver_count() as u32;
        #[cfg(not(feature = "async-oracle"))]
        let subscriber_count = 0;

        Ok(IndexerStatistics {
            total_events: store.count(),
            events_by_type: counts.clone(),
            latest_block: latest,
            subscriber_count,
            uptime_seconds: self.start_time.elapsed().as_secs(),
        })
    }

    /// Get total event count
    pub fn count(&self) -> Result<u64> {
        let store = self.store.read()
            .map_err(|_| Error::Lock)?;
        Ok(store.count())
    }

    /// Get latest indexed block
    pub fn latest_block(&self) -> Result<u64> {
        let latest = self.latest_block.read()
            .map_err(|_| Error::Lock)?;
        Ok(*latest)
    }
}

impl Default for EventIndexer {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// FILTERED RECEIVER
// ═══════════════════════════════════════════════════════════════════════════════

/// Filtered event receiver for subscriptions
#[cfg(feature = "async-oracle")]
pub struct FilteredReceiver {
    receiver: broadcast::Receiver<StoredEvent>,
    filter: SubscriptionFilter,
}

#[cfg(feature = "async-oracle")]
impl FilteredReceiver {
    /// Receive the next matching event
    pub async fn recv(&mut self) -> Option<StoredEvent> {
        loop {
            match self.receiver.recv().await {
                Ok(event) => {
                    if self.filter.matches(&event) {
                        return Some(event);
                    }
                    // Event didn't match, continue waiting
                }
                Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Missed some events, continue
                    continue;
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::token::TokenAmount;
    use crate::core::vault::CollateralAmount;
    use crate::protocol::events::{CDPOpenedEvent, CollateralDepositedEvent};
    use crate::utils::crypto::KeyPair;

    fn test_keypair() -> KeyPair {
        KeyPair::generate()
    }

    #[test]
    fn test_event_indexer_basic() {
        let indexer = EventIndexer::new();
        let keypair = test_keypair();
        let cdp_id = CDPId::generate(keypair.public_key(), 1);

        let event = ProtocolEvent::CDPOpened(CDPOpenedEvent {
            cdp_id,
            owner: *keypair.public_key(),
            collateral: CollateralAmount::from_sats(100_000_000),
            initial_debt: TokenAmount::from_cents(0),
            ratio: u64::MAX,
            block_height: 100,
            timestamp: 1234567890,
        });

        let stored = indexer.store_event(100, 0, 0, event).unwrap();
        assert_eq!(stored.id, 0);

        assert_eq!(indexer.count().unwrap(), 1);
        assert_eq!(indexer.latest_block().unwrap(), 100);
    }

    #[test]
    fn test_event_indexer_query() {
        let indexer = EventIndexer::new();
        let keypair = test_keypair();
        let cdp_id = CDPId::generate(keypair.public_key(), 1);

        // Store multiple events
        for i in 0..5 {
            let event = ProtocolEvent::CDPOpened(CDPOpenedEvent {
                cdp_id,
                owner: *keypair.public_key(),
                collateral: CollateralAmount::from_sats(100_000_000),
                initial_debt: TokenAmount::from_cents(0),
                ratio: u64::MAX,
                block_height: 100 + i,
                timestamp: 1234567890 + i,
            });
            indexer.store_event(100 + i, 0, 0, event).unwrap();
        }

        // Store a different type
        let deposit_event = ProtocolEvent::CollateralDeposited(CollateralDepositedEvent {
            cdp_id,
            depositor: *keypair.public_key(),
            amount: CollateralAmount::from_sats(50_000_000),
            new_total: CollateralAmount::from_sats(150_000_000),
            new_ratio: u64::MAX,
            block_height: 105,
            timestamp: 1234567895,
        });
        indexer.store_event(105, 0, 0, deposit_event).unwrap();

        // Query all
        let result = indexer.query(EventQuery::new().limit(10)).unwrap();
        assert_eq!(result.events.len(), 6);

        // Query by type
        let result = indexer.get_events_by_type("CDPOpened").unwrap();
        assert_eq!(result.events.len(), 5);

        let result = indexer.get_events_by_type("CollateralDeposited").unwrap();
        assert_eq!(result.events.len(), 1);

        // Query by CDP
        let result = indexer.get_cdp_events(cdp_id).unwrap();
        assert_eq!(result.events.len(), 6);
    }

    #[test]
    fn test_event_indexer_statistics() {
        let indexer = EventIndexer::new();
        let keypair = test_keypair();
        let cdp_id = CDPId::generate(keypair.public_key(), 1);

        for i in 0..3 {
            let event = ProtocolEvent::CDPOpened(CDPOpenedEvent {
                cdp_id,
                owner: *keypair.public_key(),
                collateral: CollateralAmount::from_sats(100_000_000),
                initial_debt: TokenAmount::from_cents(0),
                ratio: u64::MAX,
                block_height: 100 + i,
                timestamp: 1234567890 + i,
            });
            indexer.store_event(100 + i, 0, 0, event).unwrap();
        }

        let stats = indexer.statistics().unwrap();
        assert_eq!(stats.total_events, 3);
        assert_eq!(stats.latest_block, 102);
        assert_eq!(stats.events_by_type.get("CDPOpened"), Some(&3));
    }

    #[test]
    fn test_event_log_storage() {
        let indexer = EventIndexer::new();
        let keypair = test_keypair();
        let cdp_id = CDPId::generate(keypair.public_key(), 1);

        let mut log = EventLog::new();
        log.push(ProtocolEvent::CDPOpened(CDPOpenedEvent {
            cdp_id,
            owner: *keypair.public_key(),
            collateral: CollateralAmount::from_sats(100_000_000),
            initial_debt: TokenAmount::from_cents(0),
            ratio: u64::MAX,
            block_height: 100,
            timestamp: 1234567890,
        }));
        log.push(ProtocolEvent::CollateralDeposited(CollateralDepositedEvent {
            cdp_id,
            depositor: *keypair.public_key(),
            amount: CollateralAmount::from_sats(50_000_000),
            new_total: CollateralAmount::from_sats(150_000_000),
            new_ratio: u64::MAX,
            block_height: 100,
            timestamp: 1234567890,
        }));

        let stored = indexer.store_event_log(100, 0, log).unwrap();
        assert_eq!(stored.len(), 2);
        assert_eq!(indexer.count().unwrap(), 2);
    }
}
