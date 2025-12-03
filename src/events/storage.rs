//! Event Storage for persistent event logging.
//!
//! Provides an append-only log of protocol events with efficient querying capabilities.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::core::cdp::CDPId;
use crate::error::{Error, Result};
use crate::protocol::events::ProtocolEvent;
use crate::utils::crypto::{Hash, PublicKey};

// ═══════════════════════════════════════════════════════════════════════════════
// STORED EVENT
// ═══════════════════════════════════════════════════════════════════════════════

/// A stored event with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvent {
    /// Unique event ID (sequential)
    pub id: u64,
    /// Block height when event occurred
    pub block_height: u64,
    /// Transaction index within block
    pub tx_index: u32,
    /// Event index within transaction
    pub event_index: u32,
    /// The event data
    pub event: ProtocolEvent,
    /// Hash of the event for verification
    pub hash: Hash,
}

impl StoredEvent {
    /// Create a new stored event
    pub fn new(
        id: u64,
        block_height: u64,
        tx_index: u32,
        event_index: u32,
        event: ProtocolEvent,
    ) -> Self {
        let hash = event.hash();
        Self {
            id,
            block_height,
            tx_index,
            event_index,
            event,
            hash,
        }
    }

    /// Get the event type
    pub fn event_type(&self) -> &'static str {
        self.event.event_type()
    }

    /// Get timestamp from inner event
    pub fn timestamp(&self) -> u64 {
        self.event.timestamp()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// EVENT FILTER
// ═══════════════════════════════════════════════════════════════════════════════

/// Filter criteria for querying events
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    /// Filter by event types
    pub event_types: Option<Vec<String>>,
    /// Filter by CDP ID
    pub cdp_id: Option<CDPId>,
    /// Filter by account (owner/depositor)
    pub account: Option<PublicKey>,
    /// Filter by block range (start)
    pub from_block: Option<u64>,
    /// Filter by block range (end)
    pub to_block: Option<u64>,
    /// Filter by time range (start)
    pub from_timestamp: Option<u64>,
    /// Filter by time range (end)
    pub to_timestamp: Option<u64>,
}

impl EventFilter {
    /// Create a new empty filter (matches all)
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by event types
    pub fn event_types(mut self, types: Vec<String>) -> Self {
        self.event_types = Some(types);
        self
    }

    /// Filter by single event type
    pub fn event_type(mut self, t: &str) -> Self {
        self.event_types = Some(vec![t.to_string()]);
        self
    }

    /// Filter by CDP ID
    pub fn cdp(mut self, cdp_id: CDPId) -> Self {
        self.cdp_id = Some(cdp_id);
        self
    }

    /// Filter by account
    pub fn account(mut self, account: PublicKey) -> Self {
        self.account = Some(account);
        self
    }

    /// Filter by block range
    pub fn block_range(mut self, from: u64, to: u64) -> Self {
        self.from_block = Some(from);
        self.to_block = Some(to);
        self
    }

    /// Filter from block onwards
    pub fn from_block(mut self, block: u64) -> Self {
        self.from_block = Some(block);
        self
    }

    /// Filter up to block
    pub fn to_block(mut self, block: u64) -> Self {
        self.to_block = Some(block);
        self
    }

    /// Filter by time range
    pub fn time_range(mut self, from: u64, to: u64) -> Self {
        self.from_timestamp = Some(from);
        self.to_timestamp = Some(to);
        self
    }

    /// Check if an event matches this filter
    pub fn matches(&self, event: &StoredEvent) -> bool {
        // Check event type
        if let Some(ref types) = self.event_types {
            if !types.iter().any(|t| t == event.event_type()) {
                return false;
            }
        }

        // Check block range
        if let Some(from) = self.from_block {
            if event.block_height < from {
                return false;
            }
        }
        if let Some(to) = self.to_block {
            if event.block_height > to {
                return false;
            }
        }

        // Check time range
        if let Some(from) = self.from_timestamp {
            if event.timestamp() < from {
                return false;
            }
        }
        if let Some(to) = self.to_timestamp {
            if event.timestamp() > to {
                return false;
            }
        }

        // Check CDP (for CDP-related events)
        if let Some(ref cdp_id) = self.cdp_id {
            if !event_matches_cdp(&event.event, cdp_id) {
                return false;
            }
        }

        // Check account (for account-related events)
        if let Some(ref account) = self.account {
            if !event_matches_account(&event.event, account) {
                return false;
            }
        }

        true
    }
}

/// Check if an event is related to a specific CDP
fn event_matches_cdp(event: &ProtocolEvent, cdp_id: &CDPId) -> bool {
    match event {
        ProtocolEvent::CDPOpened(e) => &e.cdp_id == cdp_id,
        ProtocolEvent::CollateralDeposited(e) => &e.cdp_id == cdp_id,
        ProtocolEvent::CollateralWithdrawn(e) => &e.cdp_id == cdp_id,
        ProtocolEvent::DebtMinted(e) => &e.cdp_id == cdp_id,
        ProtocolEvent::DebtRepaid(e) => &e.cdp_id == cdp_id,
        ProtocolEvent::CDPClosed(e) => &e.cdp_id == cdp_id,
        ProtocolEvent::CDPLiquidated(e) => &e.cdp_id == cdp_id,
        ProtocolEvent::LiquidationAbsorbed(e) => &e.cdp_id == cdp_id,
        _ => false,
    }
}

/// Check if an event is related to a specific account
fn event_matches_account(event: &ProtocolEvent, account: &PublicKey) -> bool {
    match event {
        ProtocolEvent::CDPOpened(e) => &e.owner == account,
        ProtocolEvent::CollateralDeposited(e) => &e.depositor == account,
        ProtocolEvent::CollateralWithdrawn(e) => &e.owner == account,
        ProtocolEvent::DebtMinted(e) => &e.owner == account,
        ProtocolEvent::DebtRepaid(e) => &e.payer == account,
        ProtocolEvent::CDPClosed(e) => &e.owner == account,
        ProtocolEvent::CDPLiquidated(e) => &e.owner == account || &e.liquidator == account,
        ProtocolEvent::TokenTransfer(e) => &e.from == account || &e.to == account,
        ProtocolEvent::StabilityDeposit(e) => &e.depositor == account,
        ProtocolEvent::StabilityWithdraw(e) => &e.depositor == account,
        ProtocolEvent::GainsClaimed(e) => &e.depositor == account,
        ProtocolEvent::Redemption(e) => &e.redeemer == account,
        _ => false,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// EVENT QUERY
// ═══════════════════════════════════════════════════════════════════════════════

/// Query parameters for retrieving events
#[derive(Debug, Clone)]
pub struct EventQuery {
    /// Filter criteria
    pub filter: EventFilter,
    /// Maximum number of results
    pub limit: usize,
    /// Offset for pagination
    pub offset: usize,
    /// Sort order (true = newest first)
    pub descending: bool,
}

impl Default for EventQuery {
    fn default() -> Self {
        Self {
            filter: EventFilter::default(),
            limit: 100,
            offset: 0,
            descending: true,
        }
    }
}

impl EventQuery {
    /// Create a new query
    pub fn new() -> Self {
        Self::default()
    }

    /// Set filter
    pub fn filter(mut self, filter: EventFilter) -> Self {
        self.filter = filter;
        self
    }

    /// Set event type filter
    pub fn event_type(mut self, t: &str) -> Self {
        self.filter = self.filter.event_type(t);
        self
    }

    /// Set CDP filter
    pub fn cdp(mut self, cdp_id: CDPId) -> Self {
        self.filter = self.filter.cdp(cdp_id);
        self
    }

    /// Set account filter
    pub fn account(mut self, account: PublicKey) -> Self {
        self.filter = self.filter.account(account);
        self
    }

    /// Set from_block filter
    pub fn from_block(mut self, block: u64) -> Self {
        self.filter = self.filter.from_block(block);
        self
    }

    /// Set to_block filter
    pub fn to_block(mut self, block: u64) -> Self {
        self.filter = self.filter.to_block(block);
        self
    }

    /// Set limit
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Set offset
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Sort oldest first
    pub fn ascending(mut self) -> Self {
        self.descending = false;
        self
    }

    /// Sort newest first (default)
    pub fn descending(mut self) -> Self {
        self.descending = true;
        self
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// QUERY RESULT
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of an event query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    /// Matched events
    pub events: Vec<StoredEvent>,
    /// Total matching events (before pagination)
    pub total_count: u64,
    /// Whether there are more results
    pub has_more: bool,
    /// Next offset for pagination
    pub next_offset: usize,
}

impl QueryResult {
    /// Create a new query result
    pub fn new(events: Vec<StoredEvent>, total_count: u64, query: &EventQuery) -> Self {
        let has_more = (query.offset + events.len()) < total_count as usize;
        let next_offset = query.offset + events.len();

        Self {
            events,
            total_count,
            has_more,
            next_offset,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// IN-MEMORY EVENT STORE
// ═══════════════════════════════════════════════════════════════════════════════

/// In-memory event store for testing and light usage
#[derive(Debug, Default)]
pub struct InMemoryEventStore {
    /// All events by ID
    events: Vec<StoredEvent>,
    /// Index by block height
    block_index: BTreeMap<u64, Vec<usize>>,
    /// Index by event type
    type_index: BTreeMap<String, Vec<usize>>,
    /// Next event ID
    next_id: u64,
}

impl InMemoryEventStore {
    /// Create a new in-memory store
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a single event
    pub fn store(&mut self, block_height: u64, tx_index: u32, event_index: u32, event: ProtocolEvent) -> StoredEvent {
        let id = self.next_id;
        self.next_id += 1;

        let stored = StoredEvent::new(id, block_height, tx_index, event_index, event);
        let idx = self.events.len();

        // Update indexes
        self.block_index
            .entry(block_height)
            .or_default()
            .push(idx);
        self.type_index
            .entry(stored.event_type().to_string())
            .or_default()
            .push(idx);

        self.events.push(stored.clone());
        stored
    }

    /// Store multiple events from a block
    pub fn store_block_events(&mut self, block_height: u64, events: Vec<(u32, Vec<ProtocolEvent>)>) -> Vec<StoredEvent> {
        let mut stored = Vec::new();
        for (tx_index, tx_events) in events {
            for (event_index, event) in tx_events.into_iter().enumerate() {
                stored.push(self.store(block_height, tx_index, event_index as u32, event));
            }
        }
        stored
    }

    /// Query events
    pub fn query(&self, query: &EventQuery) -> QueryResult {
        let mut matching: Vec<&StoredEvent> = self
            .events
            .iter()
            .filter(|e| query.filter.matches(e))
            .collect();

        let total_count = matching.len() as u64;

        // Sort
        if query.descending {
            matching.sort_by(|a, b| b.id.cmp(&a.id));
        } else {
            matching.sort_by(|a, b| a.id.cmp(&b.id));
        }

        // Paginate
        let events: Vec<StoredEvent> = matching
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .cloned()
            .collect();

        QueryResult::new(events, total_count, query)
    }

    /// Get event by ID
    pub fn get(&self, id: u64) -> Option<&StoredEvent> {
        self.events.get(id as usize)
    }

    /// Get events by block height
    pub fn get_by_block(&self, block_height: u64) -> Vec<&StoredEvent> {
        self.block_index
            .get(&block_height)
            .map(|indices| indices.iter().filter_map(|&i| self.events.get(i)).collect())
            .unwrap_or_default()
    }

    /// Get total event count
    pub fn count(&self) -> u64 {
        self.events.len() as u64
    }

    /// Get latest block height
    pub fn latest_block(&self) -> Option<u64> {
        self.block_index.keys().max().copied()
    }

    /// Get event count by type
    pub fn count_by_type(&self, event_type: &str) -> u64 {
        self.type_index
            .get(event_type)
            .map(|v| v.len() as u64)
            .unwrap_or(0)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PERSISTENT EVENT STORE (RocksDB)
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(feature = "rocksdb-storage")]
pub mod persistent {
    use super::*;
    use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, Options, DB};
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Column families for event storage
    const CF_EVENTS: &str = "events";
    const CF_BLOCK_INDEX: &str = "block_index";
    const CF_TYPE_INDEX: &str = "type_index";
    const CF_CDP_INDEX: &str = "cdp_index";
    const CF_ACCOUNT_INDEX: &str = "account_index";
    const CF_META: &str = "meta";

    /// Persistent event store using RocksDB
    pub struct PersistentEventStore {
        db: Arc<DB>,
        path: PathBuf,
    }

    impl PersistentEventStore {
        /// Open or create a persistent event store
        pub fn open(path: impl AsRef<Path>) -> Result<Self> {
            let path = path.as_ref().to_path_buf();

            let mut opts = Options::default();
            opts.create_if_missing(true);
            opts.create_missing_column_families(true);
            opts.set_max_open_files(256);
            opts.set_compression_type(rocksdb::DBCompressionType::Lz4);

            let cf_descriptors = vec![
                ColumnFamilyDescriptor::new(CF_EVENTS, Options::default()),
                ColumnFamilyDescriptor::new(CF_BLOCK_INDEX, Options::default()),
                ColumnFamilyDescriptor::new(CF_TYPE_INDEX, Options::default()),
                ColumnFamilyDescriptor::new(CF_CDP_INDEX, Options::default()),
                ColumnFamilyDescriptor::new(CF_ACCOUNT_INDEX, Options::default()),
                ColumnFamilyDescriptor::new(CF_META, Options::default()),
            ];

            let db = DB::open_cf_descriptors(&opts, &path, cf_descriptors)
                .map_err(|e| Error::Storage(format!("Failed to open event store: {}", e)))?;

            Ok(Self {
                db: Arc::new(db),
                path,
            })
        }

        /// Store a single event
        pub fn store(&self, block_height: u64, tx_index: u32, event_index: u32, event: ProtocolEvent) -> Result<StoredEvent> {
            let id = self.next_id()?;
            let stored = StoredEvent::new(id, block_height, tx_index, event_index, event);

            // Serialize event
            let event_data = bincode::serialize(&stored)
                .map_err(|e| Error::Storage(format!("Serialization failed: {}", e)))?;

            // Store event
            let cf_events = self.cf(CF_EVENTS)?;
            self.db.put_cf(cf_events, id.to_be_bytes(), event_data)
                .map_err(|e| Error::Storage(format!("Write failed: {}", e)))?;

            // Update block index
            self.add_to_index(CF_BLOCK_INDEX, &block_height.to_be_bytes(), id)?;

            // Update type index
            self.add_to_index(CF_TYPE_INDEX, stored.event_type().as_bytes(), id)?;

            // Update CDP index if applicable
            if let Some(cdp_id) = get_cdp_id(&stored.event) {
                self.add_to_index(CF_CDP_INDEX, cdp_id.as_bytes(), id)?;
            }

            // Update account index if applicable
            for account in get_accounts(&stored.event) {
                self.add_to_index(CF_ACCOUNT_INDEX, account.as_bytes(), id)?;
            }

            // Update next ID
            self.set_next_id(id + 1)?;

            Ok(stored)
        }

        /// Store multiple events from a block
        pub fn store_block_events(&self, block_height: u64, events: Vec<(u32, Vec<ProtocolEvent>)>) -> Result<Vec<StoredEvent>> {
            let mut stored = Vec::new();
            for (tx_index, tx_events) in events {
                for (event_index, event) in tx_events.into_iter().enumerate() {
                    stored.push(self.store(block_height, tx_index, event_index as u32, event)?);
                }
            }
            Ok(stored)
        }

        /// Query events
        pub fn query(&self, query: &EventQuery) -> Result<QueryResult> {
            let cf_events = self.cf(CF_EVENTS)?;

            // Get all events (in production, use index for efficiency)
            let mut matching = Vec::new();
            let iter = self.db.iterator_cf(cf_events, rocksdb::IteratorMode::Start);

            for item in iter {
                let (_, value) = item.map_err(|e| Error::Storage(format!("Iterator error: {}", e)))?;
                let event: StoredEvent = bincode::deserialize(&value)
                    .map_err(|e| Error::Storage(format!("Deserialization failed: {}", e)))?;

                if query.filter.matches(&event) {
                    matching.push(event);
                }
            }

            let total_count = matching.len() as u64;

            // Sort
            if query.descending {
                matching.sort_by(|a, b| b.id.cmp(&a.id));
            } else {
                matching.sort_by(|a, b| a.id.cmp(&b.id));
            }

            // Paginate
            let events: Vec<StoredEvent> = matching
                .into_iter()
                .skip(query.offset)
                .take(query.limit)
                .collect();

            Ok(QueryResult::new(events, total_count, query))
        }

        /// Get event by ID
        pub fn get(&self, id: u64) -> Result<Option<StoredEvent>> {
            let cf_events = self.cf(CF_EVENTS)?;
            match self.db.get_cf(cf_events, id.to_be_bytes()) {
                Ok(Some(data)) => {
                    let event = bincode::deserialize(&data)
                        .map_err(|e| Error::Storage(format!("Deserialization failed: {}", e)))?;
                    Ok(Some(event))
                }
                Ok(None) => Ok(None),
                Err(e) => Err(Error::Storage(format!("Read failed: {}", e))),
            }
        }

        /// Get total event count
        pub fn count(&self) -> Result<u64> {
            self.next_id()
        }

        /// Get next event ID
        fn next_id(&self) -> Result<u64> {
            let cf_meta = self.cf(CF_META)?;
            match self.db.get_cf(cf_meta, b"next_id") {
                Ok(Some(data)) => {
                    let mut bytes = [0u8; 8];
                    bytes.copy_from_slice(&data);
                    Ok(u64::from_be_bytes(bytes))
                }
                Ok(None) => Ok(0),
                Err(e) => Err(Error::Storage(format!("Read failed: {}", e))),
            }
        }

        /// Set next event ID
        fn set_next_id(&self, id: u64) -> Result<()> {
            let cf_meta = self.cf(CF_META)?;
            self.db.put_cf(cf_meta, b"next_id", id.to_be_bytes())
                .map_err(|e| Error::Storage(format!("Write failed: {}", e)))
        }

        /// Add an event ID to an index
        fn add_to_index(&self, cf_name: &str, key: &[u8], event_id: u64) -> Result<()> {
            let cf = self.cf(cf_name)?;

            // Get existing IDs
            let mut ids: Vec<u64> = match self.db.get_cf(cf, key) {
                Ok(Some(data)) => bincode::deserialize(&data).unwrap_or_default(),
                _ => Vec::new(),
            };

            ids.push(event_id);

            // Store updated IDs
            let data = bincode::serialize(&ids)
                .map_err(|e| Error::Storage(format!("Serialization failed: {}", e)))?;
            self.db.put_cf(cf, key, data)
                .map_err(|e| Error::Storage(format!("Write failed: {}", e)))
        }

        /// Get column family handle
        fn cf(&self, name: &str) -> Result<&ColumnFamily> {
            self.db.cf_handle(name)
                .ok_or_else(|| Error::Storage(format!("Column family not found: {}", name)))
        }
    }

    /// Extract CDP ID from event if applicable
    fn get_cdp_id(event: &ProtocolEvent) -> Option<CDPId> {
        match event {
            ProtocolEvent::CDPOpened(e) => Some(e.cdp_id),
            ProtocolEvent::CollateralDeposited(e) => Some(e.cdp_id),
            ProtocolEvent::CollateralWithdrawn(e) => Some(e.cdp_id),
            ProtocolEvent::DebtMinted(e) => Some(e.cdp_id),
            ProtocolEvent::DebtRepaid(e) => Some(e.cdp_id),
            ProtocolEvent::CDPClosed(e) => Some(e.cdp_id),
            ProtocolEvent::CDPLiquidated(e) => Some(e.cdp_id),
            ProtocolEvent::LiquidationAbsorbed(e) => Some(e.cdp_id),
            _ => None,
        }
    }

    /// Extract accounts from event
    fn get_accounts(event: &ProtocolEvent) -> Vec<PublicKey> {
        match event {
            ProtocolEvent::CDPOpened(e) => vec![e.owner],
            ProtocolEvent::CollateralDeposited(e) => vec![e.depositor],
            ProtocolEvent::CollateralWithdrawn(e) => vec![e.owner],
            ProtocolEvent::DebtMinted(e) => vec![e.owner],
            ProtocolEvent::DebtRepaid(e) => vec![e.payer],
            ProtocolEvent::CDPClosed(e) => vec![e.owner],
            ProtocolEvent::CDPLiquidated(e) => vec![e.owner, e.liquidator],
            ProtocolEvent::TokenTransfer(e) => vec![e.from, e.to],
            ProtocolEvent::StabilityDeposit(e) => vec![e.depositor],
            ProtocolEvent::StabilityWithdraw(e) => vec![e.depositor],
            ProtocolEvent::GainsClaimed(e) => vec![e.depositor],
            ProtocolEvent::Redemption(e) => vec![e.redeemer],
            _ => Vec::new(),
        }
    }
}

#[cfg(feature = "rocksdb-storage")]
pub use persistent::PersistentEventStore;

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::token::TokenAmount;
    use crate::core::vault::CollateralAmount;
    use crate::protocol::events::CDPOpenedEvent;
    use crate::utils::crypto::KeyPair;

    fn test_keypair() -> KeyPair {
        KeyPair::generate()
    }

    #[test]
    fn test_in_memory_store() {
        let mut store = InMemoryEventStore::new();
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

        let stored = store.store(100, 0, 0, event);
        assert_eq!(stored.id, 0);
        assert_eq!(stored.block_height, 100);
        assert_eq!(store.count(), 1);
    }

    #[test]
    fn test_event_filter() {
        let mut store = InMemoryEventStore::new();
        let keypair = test_keypair();
        let cdp_id = CDPId::generate(keypair.public_key(), 1);

        // Store some events
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
            store.store(100 + i, 0, 0, event);
        }

        // Query all
        let result = store.query(&EventQuery::new().limit(10));
        assert_eq!(result.events.len(), 5);

        // Query with block filter
        let result = store.query(&EventQuery::new().from_block(102));
        assert_eq!(result.events.len(), 3);

        // Query with type filter
        let result = store.query(&EventQuery::new().event_type("CDPOpened"));
        assert_eq!(result.events.len(), 5);

        // Query with pagination
        let result = store.query(&EventQuery::new().limit(2));
        assert_eq!(result.events.len(), 2);
        assert!(result.has_more);
    }

    #[test]
    fn test_event_query_pagination() {
        let mut store = InMemoryEventStore::new();
        let keypair = test_keypair();
        let cdp_id = CDPId::generate(keypair.public_key(), 1);

        for i in 0..10 {
            let event = ProtocolEvent::CDPOpened(CDPOpenedEvent {
                cdp_id,
                owner: *keypair.public_key(),
                collateral: CollateralAmount::from_sats(100_000_000),
                initial_debt: TokenAmount::from_cents(0),
                ratio: u64::MAX,
                block_height: 100 + i,
                timestamp: 1234567890 + i,
            });
            store.store(100 + i, 0, 0, event);
        }

        // First page
        let result = store.query(&EventQuery::new().limit(3).offset(0));
        assert_eq!(result.events.len(), 3);
        assert_eq!(result.total_count, 10);
        assert!(result.has_more);
        assert_eq!(result.next_offset, 3);

        // Second page
        let result = store.query(&EventQuery::new().limit(3).offset(3));
        assert_eq!(result.events.len(), 3);
        assert!(result.has_more);
        assert_eq!(result.next_offset, 6);

        // Last page
        let result = store.query(&EventQuery::new().limit(3).offset(9));
        assert_eq!(result.events.len(), 1);
        assert!(!result.has_more);
    }
}
