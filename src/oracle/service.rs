//! Async Oracle Service for production price feeds.
//!
//! This module provides a background service that:
//! - Periodically fetches prices from multiple exchanges
//! - Aggregates prices using median calculation
//! - Validates price data against safety thresholds
//! - Publishes updates to subscribers
//!
//! ## Usage
//!
//! ```rust,ignore
//! use zkusd::oracle::service::{OracleService, OracleConfig};
//!
//! let config = OracleConfig::default();
//! let service = OracleService::new(config).await?;
//!
//! // Get current price
//! let price = service.current_price().await;
//!
//! // Subscribe to price updates
//! let mut rx = service.subscribe();
//! while let Some(update) = rx.recv().await {
//!     println!("New price: {} cents", update.price_cents);
//! }
//! ```

#[cfg(feature = "async-oracle")]
use tokio::sync::{broadcast, RwLock};
#[cfg(feature = "async-oracle")]
use tokio::time::{interval, Duration};

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
#[cfg(feature = "async-oracle")]
use crate::oracle::fetchers::{HttpPriceFetcher, FetchResult};
use crate::oracle::fetchers::HttpFetcherConfig;
use crate::oracle::sources::SourceCollection;

// ═══════════════════════════════════════════════════════════════════════════════
// CONFIGURATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Configuration for the Oracle Service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleConfig {
    /// Update interval in seconds
    pub update_interval_secs: u64,
    /// Minimum number of sources required for valid price
    pub min_sources: usize,
    /// Maximum price age in seconds before considered stale
    pub max_price_age_secs: u64,
    /// Maximum allowed deviation between sources (basis points)
    pub max_deviation_bps: u64,
    /// Enable validation checks
    pub enable_validation: bool,
    /// Maximum price change allowed per update (basis points)
    pub max_price_change_bps: u64,
    /// Use major exchanges only (faster)
    pub major_exchanges_only: bool,
    /// HTTP fetcher configuration
    pub http_config: HttpFetcherConfig,
}

impl Default for OracleConfig {
    fn default() -> Self {
        Self {
            update_interval_secs: 30,
            min_sources: 3,
            max_price_age_secs: 120,
            max_deviation_bps: 500, // 5%
            enable_validation: true,
            max_price_change_bps: 1000, // 10% max change per update
            major_exchanges_only: false,
            http_config: HttpFetcherConfig::default(),
        }
    }
}

impl OracleConfig {
    /// Create configuration for high-frequency updates
    pub fn high_frequency() -> Self {
        Self {
            update_interval_secs: 10,
            min_sources: 2,
            max_price_age_secs: 30,
            major_exchanges_only: true,
            ..Default::default()
        }
    }

    /// Create configuration for low-frequency, high-reliability updates
    pub fn conservative() -> Self {
        Self {
            update_interval_secs: 60,
            min_sources: 4,
            max_price_age_secs: 300,
            max_deviation_bps: 300, // 3%
            ..Default::default()
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRICE UPDATE
// ═══════════════════════════════════════════════════════════════════════════════

/// Price update event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceUpdate {
    /// Price in cents (USD)
    pub price_cents: u64,
    /// Timestamp of the update
    pub timestamp: u64,
    /// Number of sources used
    pub source_count: usize,
    /// Standard deviation of sources (cents)
    pub std_deviation: u64,
    /// Minimum price from sources
    pub min_price: u64,
    /// Maximum price from sources
    pub max_price: u64,
    /// Update sequence number
    pub sequence: u64,
}

impl PriceUpdate {
    /// Create from source collection
    pub fn from_collection(collection: &SourceCollection, sequence: u64) -> Option<Self> {
        let median = collection.median_price()?;
        let prices: Vec<u64> = collection.sources().iter().map(|s| s.price_cents).collect();

        let min_price = *prices.iter().min()?;
        let max_price = *prices.iter().max()?;

        // Calculate standard deviation
        let mean = prices.iter().sum::<u64>() as f64 / prices.len() as f64;
        let variance = prices.iter()
            .map(|&p| (p as f64 - mean).powi(2))
            .sum::<f64>() / prices.len() as f64;
        let std_deviation = variance.sqrt() as u64;

        Some(Self {
            price_cents: median,
            timestamp: collection.collected_at,
            source_count: collection.len(),
            std_deviation,
            min_price,
            max_price,
            sequence,
        })
    }

    /// Check if price is stale
    pub fn is_stale(&self, max_age_secs: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        now.saturating_sub(self.timestamp) > max_age_secs
    }

    /// Calculate spread in basis points
    pub fn spread_bps(&self) -> u64 {
        if self.price_cents == 0 {
            return 0;
        }
        ((self.max_price - self.min_price) as u128 * 10000 / self.price_cents as u128) as u64
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ORACLE STATE
// ═══════════════════════════════════════════════════════════════════════════════

/// Current oracle state
#[derive(Debug, Clone)]
pub struct OracleState {
    /// Last successful price update
    pub last_update: Option<PriceUpdate>,
    /// Update sequence counter
    pub sequence: u64,
    /// Total successful updates
    pub total_updates: u64,
    /// Total failed updates
    pub failed_updates: u64,
    /// Average latency in milliseconds
    pub avg_latency_ms: u64,
    /// Service start timestamp
    pub started_at: u64,
    /// Is service running
    pub is_running: bool,
}

impl Default for OracleState {
    fn default() -> Self {
        Self {
            last_update: None,
            sequence: 0,
            total_updates: 0,
            failed_updates: 0,
            avg_latency_ms: 0,
            started_at: 0,
            is_running: false,
        }
    }
}

impl OracleState {
    /// Get current price if available and not stale
    pub fn current_price(&self, max_age_secs: u64) -> Option<u64> {
        self.last_update.as_ref().and_then(|u| {
            if u.is_stale(max_age_secs) {
                None
            } else {
                Some(u.price_cents)
            }
        })
    }

    /// Get success rate as percentage
    pub fn success_rate(&self) -> f64 {
        let total = self.total_updates + self.failed_updates;
        if total == 0 {
            0.0
        } else {
            self.total_updates as f64 / total as f64 * 100.0
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ORACLE SERVICE
// ═══════════════════════════════════════════════════════════════════════════════

/// Background oracle service for production price feeds
#[cfg(feature = "async-oracle")]
pub struct OracleService {
    /// Configuration
    config: OracleConfig,
    /// Current state
    state: Arc<RwLock<OracleState>>,
    /// Price update broadcaster
    tx: broadcast::Sender<PriceUpdate>,
    /// HTTP price fetcher
    fetcher: Arc<HttpPriceFetcher>,
    /// Shutdown signal
    shutdown: Arc<RwLock<bool>>,
}

#[cfg(feature = "async-oracle")]
impl OracleService {
    /// Create a new oracle service
    pub async fn new(config: OracleConfig) -> Result<Self> {
        let fetcher = HttpPriceFetcher::new(config.http_config.clone())?;
        let (tx, _) = broadcast::channel(100);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let state = OracleState {
            started_at: now,
            ..Default::default()
        };

        Ok(Self {
            config,
            state: Arc::new(RwLock::new(state)),
            tx,
            fetcher: Arc::new(fetcher),
            shutdown: Arc::new(RwLock::new(false)),
        })
    }

    /// Create with default configuration
    pub async fn with_defaults() -> Result<Self> {
        Self::new(OracleConfig::default()).await
    }

    /// Start the oracle service (returns immediately, runs in background)
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let config = self.config.clone();
        let state = Arc::clone(&self.state);
        let tx = self.tx.clone();
        let fetcher = Arc::clone(&self.fetcher);
        let shutdown = Arc::clone(&self.shutdown);

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(config.update_interval_secs));

            // Mark as running
            {
                let mut s = state.write().await;
                s.is_running = true;
            }

            loop {
                interval.tick().await;

                // Check for shutdown
                if *shutdown.read().await {
                    break;
                }

                // Fetch prices
                let start = std::time::Instant::now();

                let collection = if config.major_exchanges_only {
                    fetcher.fetch_major().await
                } else {
                    fetcher.fetch_all().await
                };

                let duration_ms = start.elapsed().as_millis() as u64;

                // Update state
                let mut s = state.write().await;

                // Validate collection
                if collection.len() < config.min_sources {
                    s.failed_updates += 1;
                    tracing::warn!(
                        "Insufficient price sources: {} < {}",
                        collection.len(),
                        config.min_sources
                    );
                    continue;
                }

                // Check deviation
                if config.enable_validation {
                    if let Some(median) = collection.median_price() {
                        let prices: Vec<u64> = collection.sources().iter()
                            .map(|s| s.price_cents)
                            .collect();

                        let max_dev = prices.iter()
                            .map(|&p| {
                                if median > 0 {
                                    ((p as i64 - median as i64).abs() as u64 * 10000) / median
                                } else {
                                    0
                                }
                            })
                            .max()
                            .unwrap_or(0);

                        if max_dev > config.max_deviation_bps {
                            s.failed_updates += 1;
                            tracing::warn!(
                                "Price deviation too high: {} bps > {} bps",
                                max_dev,
                                config.max_deviation_bps
                            );
                            continue;
                        }

                        // Check for sudden price changes
                        if let Some(ref last) = s.last_update {
                            let change = if median > last.price_cents {
                                ((median - last.price_cents) as u128 * 10000) / last.price_cents as u128
                            } else {
                                ((last.price_cents - median) as u128 * 10000) / last.price_cents as u128
                            };

                            if change as u64 > config.max_price_change_bps {
                                s.failed_updates += 1;
                                tracing::warn!(
                                    "Price change too large: {} bps > {} bps",
                                    change,
                                    config.max_price_change_bps
                                );
                                continue;
                            }
                        }
                    }
                }

                // Create price update
                s.sequence += 1;
                if let Some(update) = PriceUpdate::from_collection(&collection, s.sequence) {
                    // Update average latency
                    let total = s.total_updates;
                    if total > 0 {
                        s.avg_latency_ms = (s.avg_latency_ms * total + duration_ms) / (total + 1);
                    } else {
                        s.avg_latency_ms = duration_ms;
                    }

                    s.total_updates += 1;
                    s.last_update = Some(update.clone());

                    // Broadcast update
                    let _ = tx.send(update);

                    tracing::info!(
                        "Price updated: {} cents from {} sources in {}ms",
                        s.last_update.as_ref().unwrap().price_cents,
                        collection.len(),
                        duration_ms
                    );
                }
            }

            // Mark as stopped
            let mut s = state.write().await;
            s.is_running = false;
        })
    }

    /// Stop the oracle service
    pub async fn stop(&self) {
        *self.shutdown.write().await = true;
    }

    /// Get current price
    pub async fn current_price(&self) -> Option<u64> {
        let state = self.state.read().await;
        state.current_price(self.config.max_price_age_secs)
    }

    /// Get current state
    pub async fn state(&self) -> OracleState {
        self.state.read().await.clone()
    }

    /// Get last update
    pub async fn last_update(&self) -> Option<PriceUpdate> {
        self.state.read().await.last_update.clone()
    }

    /// Subscribe to price updates
    pub fn subscribe(&self) -> broadcast::Receiver<PriceUpdate> {
        self.tx.subscribe()
    }

    /// Check if service is running
    pub async fn is_running(&self) -> bool {
        self.state.read().await.is_running
    }

    /// Trigger a manual price fetch
    pub async fn fetch_now(&self) -> Result<PriceUpdate> {
        let collection = if self.config.major_exchanges_only {
            self.fetcher.fetch_major().await
        } else {
            self.fetcher.fetch_all().await
        };

        if collection.len() < self.config.min_sources {
            return Err(Error::Internal(format!(
                "Insufficient sources: {} < {}",
                collection.len(),
                self.config.min_sources
            )));
        }

        let mut state = self.state.write().await;
        state.sequence += 1;

        PriceUpdate::from_collection(&collection, state.sequence).ok_or_else(|| {
            Error::Internal("Failed to create price update".into())
        })
    }

    /// Get statistics
    pub async fn statistics(&self) -> OracleStatistics {
        let state = self.state.read().await;
        OracleStatistics {
            total_updates: state.total_updates,
            failed_updates: state.failed_updates,
            success_rate: state.success_rate(),
            avg_latency_ms: state.avg_latency_ms,
            uptime_secs: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .saturating_sub(state.started_at),
            is_running: state.is_running,
        }
    }
}

/// Oracle service statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleStatistics {
    /// Total successful updates
    pub total_updates: u64,
    /// Total failed updates
    pub failed_updates: u64,
    /// Success rate percentage
    pub success_rate: f64,
    /// Average latency in milliseconds
    pub avg_latency_ms: u64,
    /// Uptime in seconds
    pub uptime_secs: u64,
    /// Is service running
    pub is_running: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
// STUB (WHEN FEATURE DISABLED)
// ═══════════════════════════════════════════════════════════════════════════════

/// Stub implementation when async-oracle is disabled
#[cfg(not(feature = "async-oracle"))]
pub struct OracleService;

#[cfg(not(feature = "async-oracle"))]
impl OracleService {
    /// Create (stub)
    pub async fn new(_config: OracleConfig) -> Result<Self> {
        Err(Error::Internal(
            "async-oracle feature not enabled. Rebuild with --features async-oracle".into(),
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::sources::{Exchange, PriceSource};

    #[test]
    fn test_config_default() {
        let config = OracleConfig::default();
        assert_eq!(config.update_interval_secs, 30);
        assert_eq!(config.min_sources, 3);
    }

    #[test]
    fn test_config_high_frequency() {
        let config = OracleConfig::high_frequency();
        assert_eq!(config.update_interval_secs, 10);
        assert!(config.major_exchanges_only);
    }

    #[test]
    fn test_price_update_from_collection() {
        let mut collection = SourceCollection::new(1000);
        collection.add(PriceSource::new(Exchange::Binance, 10000000, 1000));
        collection.add(PriceSource::new(Exchange::Coinbase, 10001000, 1000));
        collection.add(PriceSource::new(Exchange::Kraken, 9999000, 1000));

        let update = PriceUpdate::from_collection(&collection, 1).unwrap();
        assert_eq!(update.source_count, 3);
        assert_eq!(update.min_price, 9999000);
        assert_eq!(update.max_price, 10001000);
    }

    #[test]
    fn test_price_update_spread() {
        let update = PriceUpdate {
            price_cents: 10000000,
            timestamp: 1000,
            source_count: 3,
            std_deviation: 1000,
            min_price: 9900000,
            max_price: 10100000,
            sequence: 1,
        };

        let spread = update.spread_bps();
        assert_eq!(spread, 200); // 2%
    }

    #[test]
    fn test_oracle_state() {
        let mut state = OracleState::default();
        state.total_updates = 90;
        state.failed_updates = 10;

        assert_eq!(state.success_rate(), 90.0);
    }
}
