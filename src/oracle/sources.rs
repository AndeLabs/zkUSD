//! Price source definitions and interfaces.
//!
//! This module defines the various price sources that can be used:
//! - Exchange APIs (Binance, Coinbase, Kraken)
//! - Aggregator sources (Chainlink-style)
//! - Custom oracle sources

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::utils::crypto::{Hash, PublicKey, Signature};

// ═══════════════════════════════════════════════════════════════════════════════
// EXCHANGE DEFINITIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Supported exchanges
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Exchange {
    /// Binance
    Binance,
    /// Coinbase
    Coinbase,
    /// Kraken
    Kraken,
    /// Bitstamp
    Bitstamp,
    /// OKX
    OKX,
    /// Bybit
    Bybit,
    /// Custom oracle
    Custom(u8),
}

impl Exchange {
    /// Get exchange name
    pub fn name(&self) -> &str {
        match self {
            Exchange::Binance => "Binance",
            Exchange::Coinbase => "Coinbase",
            Exchange::Kraken => "Kraken",
            Exchange::Bitstamp => "Bitstamp",
            Exchange::OKX => "OKX",
            Exchange::Bybit => "Bybit",
            Exchange::Custom(_) => "Custom",
        }
    }

    /// Get reliability weight (1-100)
    pub fn weight(&self) -> u8 {
        match self {
            Exchange::Binance => 100,   // Highest volume
            Exchange::Coinbase => 95,   // US-regulated
            Exchange::Kraken => 90,     // Established
            Exchange::Bitstamp => 85,   // Long history
            Exchange::OKX => 80,
            Exchange::Bybit => 75,
            Exchange::Custom(_) => 50,  // Unknown reliability
        }
    }

    /// Get all major exchanges
    pub fn major_exchanges() -> Vec<Exchange> {
        vec![
            Exchange::Binance,
            Exchange::Coinbase,
            Exchange::Kraken,
            Exchange::Bitstamp,
        ]
    }
}

impl std::fmt::Display for Exchange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRICE SOURCE
// ═══════════════════════════════════════════════════════════════════════════════

/// A single price source with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceSource {
    /// Exchange or source identifier
    pub exchange: Exchange,
    /// Price in cents
    pub price_cents: u64,
    /// Timestamp of the price
    pub timestamp: u64,
    /// Volume in the last 24h (for weighting)
    pub volume_24h: Option<u64>,
    /// Signature from the source (if available)
    pub signature: Option<Signature>,
    /// Public key of the signer
    pub signer: Option<PublicKey>,
}

impl PriceSource {
    /// Create a new price source
    pub fn new(exchange: Exchange, price_cents: u64, timestamp: u64) -> Self {
        Self {
            exchange,
            price_cents,
            timestamp,
            volume_24h: None,
            signature: None,
            signer: None,
        }
    }

    /// Add volume data
    pub fn with_volume(mut self, volume: u64) -> Self {
        self.volume_24h = Some(volume);
        self
    }

    /// Add signature
    pub fn with_signature(mut self, signature: Signature, signer: PublicKey) -> Self {
        self.signature = Some(signature);
        self.signer = Some(signer);
        self
    }

    /// Check if source has a valid signature
    pub fn is_signed(&self) -> bool {
        self.signature.is_some() && self.signer.is_some()
    }

    /// Verify signature (placeholder)
    pub fn verify_signature(&self) -> bool {
        // In production, would verify the signature
        self.is_signed()
    }

    /// Get effective weight based on exchange and volume
    pub fn effective_weight(&self) -> u64 {
        let base_weight = self.exchange.weight() as u64;

        // Boost weight based on volume if available
        if let Some(volume) = self.volume_24h {
            // Volume in millions of USD
            let volume_factor = (volume / 1_000_000).min(100);
            base_weight + volume_factor
        } else {
            base_weight
        }
    }

    /// Hash the source data
    pub fn hash(&self) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(self.exchange.name().as_bytes());
        data.extend_from_slice(&self.price_cents.to_be_bytes());
        data.extend_from_slice(&self.timestamp.to_be_bytes());
        Hash::sha256(&data)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SOURCE COLLECTION
// ═══════════════════════════════════════════════════════════════════════════════

/// Collection of price sources for aggregation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceCollection {
    /// All sources
    sources: Vec<PriceSource>,
    /// Timestamp of collection
    pub collected_at: u64,
}

impl SourceCollection {
    /// Create a new empty collection
    pub fn new(timestamp: u64) -> Self {
        Self {
            sources: Vec::new(),
            collected_at: timestamp,
        }
    }

    /// Add a source
    pub fn add(&mut self, source: PriceSource) {
        self.sources.push(source);
    }

    /// Get number of sources
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Get all sources
    pub fn sources(&self) -> &[PriceSource] {
        &self.sources
    }

    /// Get all prices
    pub fn prices(&self) -> Vec<u64> {
        self.sources.iter().map(|s| s.price_cents).collect()
    }

    /// Get sources from a specific exchange
    pub fn from_exchange(&self, exchange: Exchange) -> Vec<&PriceSource> {
        self.sources.iter()
            .filter(|s| s.exchange == exchange)
            .collect()
    }

    /// Filter to only signed sources
    pub fn signed_only(&self) -> Vec<&PriceSource> {
        self.sources.iter()
            .filter(|s| s.is_signed())
            .collect()
    }

    /// Calculate simple average price
    pub fn average_price(&self) -> Option<u64> {
        if self.sources.is_empty() {
            return None;
        }
        let sum: u64 = self.sources.iter().map(|s| s.price_cents).sum();
        Some(sum / self.sources.len() as u64)
    }

    /// Calculate median price
    pub fn median_price(&self) -> Option<u64> {
        if self.sources.is_empty() {
            return None;
        }

        let mut prices: Vec<u64> = self.sources.iter().map(|s| s.price_cents).collect();
        prices.sort_unstable();

        let mid = prices.len() / 2;
        if prices.len() % 2 == 0 {
            Some((prices[mid - 1] + prices[mid]) / 2)
        } else {
            Some(prices[mid])
        }
    }

    /// Calculate weighted average price
    pub fn weighted_average_price(&self) -> Option<u64> {
        if self.sources.is_empty() {
            return None;
        }

        let mut weighted_sum: u128 = 0;
        let mut total_weight: u128 = 0;

        for source in &self.sources {
            let weight = source.effective_weight() as u128;
            weighted_sum += (source.price_cents as u128) * weight;
            total_weight += weight;
        }

        if total_weight == 0 {
            return None;
        }

        Some((weighted_sum / total_weight) as u64)
    }

    /// Check if all prices are within acceptable deviation
    pub fn is_consistent(&self, max_deviation_bps: u64) -> bool {
        if self.sources.len() < 2 {
            return true;
        }

        let median = match self.median_price() {
            Some(m) => m,
            None => return false,
        };

        for source in &self.sources {
            let diff = if source.price_cents > median {
                source.price_cents - median
            } else {
                median - source.price_cents
            };

            let deviation_bps = (diff as u128 * 10000 / median as u128) as u64;
            if deviation_bps > max_deviation_bps {
                return false;
            }
        }

        true
    }

    /// Remove outliers (sources too far from median)
    pub fn remove_outliers(&mut self, max_deviation_bps: u64) {
        let median = match self.median_price() {
            Some(m) => m,
            None => return,
        };

        self.sources.retain(|source| {
            let diff = if source.price_cents > median {
                source.price_cents - median
            } else {
                median - source.price_cents
            };

            let deviation_bps = (diff as u128 * 10000 / median as u128) as u64;
            deviation_bps <= max_deviation_bps
        });
    }

    /// Hash all sources
    pub fn hash(&self) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(&self.collected_at.to_be_bytes());

        for source in &self.sources {
            data.extend_from_slice(source.hash().as_bytes());
        }

        Hash::sha256(&data)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ORACLE NODE
// ═══════════════════════════════════════════════════════════════════════════════

/// Configuration for an oracle node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleNode {
    /// Node identifier
    pub id: Hash,
    /// Node public key
    pub pubkey: PublicKey,
    /// Node name/description
    pub name: String,
    /// Exchanges this node provides
    pub exchanges: Vec<Exchange>,
    /// Node reputation score (0-100)
    pub reputation: u8,
    /// Whether node is active
    pub active: bool,
}

impl OracleNode {
    /// Create a new oracle node
    pub fn new(pubkey: PublicKey, name: String, exchanges: Vec<Exchange>) -> Self {
        let id = Hash::sha256(pubkey.as_bytes());
        Self {
            id,
            pubkey,
            name,
            exchanges,
            reputation: 50, // Start with neutral reputation
            active: true,
        }
    }

    /// Update reputation based on accuracy
    pub fn update_reputation(&mut self, accurate: bool) {
        if accurate {
            self.reputation = self.reputation.saturating_add(1).min(100);
        } else {
            self.reputation = self.reputation.saturating_sub(5).max(0);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SOURCE FETCHER TRAIT
// ═══════════════════════════════════════════════════════════════════════════════

/// Trait for fetching prices from sources
pub trait PriceSourceFetcher {
    /// Fetch price from an exchange
    fn fetch_price(&self, exchange: Exchange) -> Result<PriceSource>;

    /// Fetch prices from all configured exchanges
    fn fetch_all(&self) -> Result<SourceCollection>;

    /// Get supported exchanges
    fn supported_exchanges(&self) -> Vec<Exchange>;
}

/// Mock implementation for testing
#[derive(Debug, Default)]
pub struct MockPriceFetcher {
    pub base_price: u64,
    pub timestamp: u64,
}

impl MockPriceFetcher {
    pub fn new(base_price: u64, timestamp: u64) -> Self {
        Self { base_price, timestamp }
    }
}

impl PriceSourceFetcher for MockPriceFetcher {
    fn fetch_price(&self, exchange: Exchange) -> Result<PriceSource> {
        // Add small random variation per exchange
        let variation = match exchange {
            Exchange::Binance => 0,
            Exchange::Coinbase => 10000,
            Exchange::Kraken => -5000,
            Exchange::Bitstamp => 7500,
            _ => 0,
        };

        let price = (self.base_price as i64 + variation) as u64;
        Ok(PriceSource::new(exchange, price, self.timestamp))
    }

    fn fetch_all(&self) -> Result<SourceCollection> {
        let mut collection = SourceCollection::new(self.timestamp);

        for exchange in self.supported_exchanges() {
            if let Ok(source) = self.fetch_price(exchange) {
                collection.add(source);
            }
        }

        Ok(collection)
    }

    fn supported_exchanges(&self) -> Vec<Exchange> {
        Exchange::major_exchanges()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exchange_properties() {
        assert_eq!(Exchange::Binance.name(), "Binance");
        assert_eq!(Exchange::Binance.weight(), 100);
        assert!(Exchange::major_exchanges().len() >= 4);
    }

    #[test]
    fn test_price_source_creation() {
        let source = PriceSource::new(Exchange::Binance, 10_000_000, 1000);
        assert_eq!(source.exchange, Exchange::Binance);
        assert_eq!(source.price_cents, 10_000_000);
    }

    #[test]
    fn test_source_collection_median() {
        let mut collection = SourceCollection::new(1000);
        collection.add(PriceSource::new(Exchange::Binance, 10_000_000, 1000));
        collection.add(PriceSource::new(Exchange::Coinbase, 10_100_000, 1000));
        collection.add(PriceSource::new(Exchange::Kraken, 10_050_000, 1000));

        let median = collection.median_price().unwrap();
        assert_eq!(median, 10_050_000);
    }

    #[test]
    fn test_source_collection_consistency() {
        let mut collection = SourceCollection::new(1000);
        collection.add(PriceSource::new(Exchange::Binance, 10_000_000, 1000));
        collection.add(PriceSource::new(Exchange::Coinbase, 10_100_000, 1000));
        collection.add(PriceSource::new(Exchange::Kraken, 10_050_000, 1000));

        // 1% deviation - should be consistent with 5% limit
        assert!(collection.is_consistent(500));

        // Add outlier
        collection.add(PriceSource::new(Exchange::Custom(1), 11_000_000, 1000));

        // 10% deviation - not consistent with 5% limit
        assert!(!collection.is_consistent(500));
    }

    #[test]
    fn test_remove_outliers() {
        let mut collection = SourceCollection::new(1000);
        collection.add(PriceSource::new(Exchange::Binance, 10_000_000, 1000));
        collection.add(PriceSource::new(Exchange::Coinbase, 10_050_000, 1000));
        collection.add(PriceSource::new(Exchange::Kraken, 10_025_000, 1000));
        collection.add(PriceSource::new(Exchange::Custom(1), 11_000_000, 1000)); // Outlier

        assert_eq!(collection.len(), 4);

        collection.remove_outliers(500); // 5% max deviation

        assert_eq!(collection.len(), 3);
    }

    #[test]
    fn test_weighted_average() {
        let mut collection = SourceCollection::new(1000);
        collection.add(PriceSource::new(Exchange::Binance, 10_000_000, 1000)); // weight 100
        collection.add(PriceSource::new(Exchange::Custom(1), 10_100_000, 1000)); // weight 50

        let weighted = collection.weighted_average_price().unwrap();
        let simple = collection.average_price().unwrap();

        // Weighted should favor Binance (higher weight)
        assert!(weighted < simple);
    }

    #[test]
    fn test_mock_fetcher() {
        let fetcher = MockPriceFetcher::new(10_000_000, 1000);
        let collection = fetcher.fetch_all().unwrap();

        assert_eq!(collection.len(), 4);
        assert!(collection.average_price().is_some());
    }
}
