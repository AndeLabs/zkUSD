//! Price feed implementation.
//!
//! This module provides the core price feed functionality:
//! - Price storage and retrieval
//! - Price validation
//! - Historical price tracking

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::utils::constants::*;
use crate::utils::crypto::Hash;
use crate::utils::validation::*;

// ═══════════════════════════════════════════════════════════════════════════════
// PRICE DATA
// ═══════════════════════════════════════════════════════════════════════════════

/// A single price data point
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceData {
    /// Price in cents (e.g., 10000000 = $100,000)
    pub price_cents: u64,
    /// Unix timestamp when price was recorded
    pub timestamp: u64,
    /// Number of sources used to derive this price
    pub source_count: u8,
    /// Confidence score (0-100)
    pub confidence: u8,
}

impl PriceData {
    /// Create a new price data point
    pub fn new(price_cents: u64, timestamp: u64, source_count: u8) -> Self {
        Self {
            price_cents,
            timestamp,
            source_count,
            confidence: Self::calculate_confidence(source_count),
        }
    }

    /// Calculate confidence based on source count
    fn calculate_confidence(source_count: u8) -> u8 {
        match source_count {
            0 => 0,
            1 => 30,
            2 => 50,
            3 => 70,
            4 => 85,
            _ => 95,
        }
    }

    /// Check if price is fresh
    pub fn is_fresh(&self, current_time: u64, max_age: u64) -> bool {
        current_time.saturating_sub(self.timestamp) <= max_age
    }

    /// Check if price has sufficient confidence
    pub fn is_reliable(&self) -> bool {
        self.source_count >= MIN_ORACLE_SOURCES as u8 && self.confidence >= 70
    }

    /// Get age of price in seconds
    pub fn age(&self, current_time: u64) -> u64 {
        current_time.saturating_sub(self.timestamp)
    }

    /// Format price for display
    pub fn format_price(&self) -> String {
        let dollars = self.price_cents / ZKUSD_BASE_UNIT;
        let cents = self.price_cents % ZKUSD_BASE_UNIT;
        format!("${}.{:02}", dollars, cents)
    }
}

impl Default for PriceData {
    fn default() -> Self {
        Self {
            price_cents: 0,
            timestamp: 0,
            source_count: 0,
            confidence: 0,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRICE FEED
// ═══════════════════════════════════════════════════════════════════════════════

/// Price feed for BTC/USD
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceFeed {
    /// Current price
    current: PriceData,
    /// Previous price (for change detection)
    previous: PriceData,
    /// Price history (for TWAP calculations)
    history: Vec<PriceData>,
    /// Maximum history size
    max_history: usize,
    /// Minimum sources required
    min_sources: usize,
    /// Maximum price staleness in seconds
    max_staleness: u64,
    /// Maximum allowed deviation between updates
    max_deviation_bps: u64,
}

impl Default for PriceFeed {
    fn default() -> Self {
        Self::new()
    }
}

impl PriceFeed {
    /// Create a new price feed
    pub fn new() -> Self {
        Self {
            current: PriceData::default(),
            previous: PriceData::default(),
            history: Vec::new(),
            max_history: 100,
            min_sources: MIN_ORACLE_SOURCES,
            max_staleness: MAX_PRICE_STALENESS_SECS,
            max_deviation_bps: MAX_PRICE_DEVIATION_BPS,
        }
    }

    /// Create with custom parameters
    pub fn with_params(
        min_sources: usize,
        max_staleness: u64,
        max_deviation_bps: u64,
    ) -> Self {
        Self {
            min_sources,
            max_staleness,
            max_deviation_bps,
            ..Self::new()
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // PRICE UPDATES
    // ═══════════════════════════════════════════════════════════════════════════

    /// Update price with validation
    pub fn update(&mut self, price: PriceData) -> Result<()> {
        // Validate price bounds
        validate_btc_price(price.price_cents)?;

        // Validate source count
        if (price.source_count as usize) < self.min_sources {
            return Err(Error::InsufficientOracleSources {
                got: price.source_count as usize,
                need: self.min_sources,
            });
        }

        // Validate timestamp
        if self.current.timestamp > 0 && price.timestamp < self.current.timestamp {
            return Err(Error::InvalidParameter {
                name: "timestamp".into(),
                reason: "price timestamp is older than current".into(),
            });
        }

        // Validate deviation from current price (if we have one)
        if self.current.price_cents > 0 {
            let deviation = self.calculate_deviation(self.current.price_cents, price.price_cents);
            if deviation > self.max_deviation_bps {
                return Err(Error::PriceDeviationTooHigh {
                    deviation: deviation / 100,
                    max_deviation: self.max_deviation_bps / 100,
                });
            }
        }

        // Store previous price
        self.previous = self.current;

        // Update current price
        self.current = price;

        // Add to history
        self.history.push(price);
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }

        Ok(())
    }

    /// Force update (bypass validation, for emergency/admin use)
    pub fn force_update(&mut self, price: PriceData) {
        self.previous = self.current;
        self.current = price;
        self.history.push(price);
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // QUERIES
    // ═══════════════════════════════════════════════════════════════════════════

    /// Get current price
    pub fn current_price(&self) -> &PriceData {
        &self.current
    }

    /// Get current price in cents
    pub fn price_cents(&self) -> u64 {
        self.current.price_cents
    }

    /// Get previous price
    pub fn previous_price(&self) -> &PriceData {
        &self.previous
    }

    /// Check if current price is valid for use
    pub fn is_valid(&self, current_time: u64) -> bool {
        self.current.price_cents > 0
            && self.current.is_fresh(current_time, self.max_staleness)
            && self.current.is_reliable()
    }

    /// Get validated price or error
    pub fn get_validated_price(&self, current_time: u64) -> Result<u64> {
        if self.current.price_cents == 0 {
            return Err(Error::InvalidParameter {
                name: "price".into(),
                reason: "no price available".into(),
            });
        }

        validate_price_freshness(self.current.timestamp, current_time)?;

        if !self.current.is_reliable() {
            return Err(Error::InsufficientOracleSources {
                got: self.current.source_count as usize,
                need: self.min_sources,
            });
        }

        Ok(self.current.price_cents)
    }

    /// Calculate Time-Weighted Average Price (TWAP)
    pub fn twap(&self, period_secs: u64, current_time: u64) -> Option<u64> {
        let cutoff = current_time.saturating_sub(period_secs);

        let relevant_prices: Vec<_> = self.history
            .iter()
            .filter(|p| p.timestamp >= cutoff)
            .collect();

        if relevant_prices.is_empty() {
            return None;
        }

        // Simple average (in production, would be time-weighted)
        let sum: u64 = relevant_prices.iter().map(|p| p.price_cents).sum();
        Some(sum / relevant_prices.len() as u64)
    }

    /// Get price change percentage (in basis points)
    pub fn price_change_bps(&self) -> i64 {
        if self.previous.price_cents == 0 {
            return 0;
        }

        let current = self.current.price_cents as i64;
        let previous = self.previous.price_cents as i64;

        ((current - previous) * BPS_DIVISOR as i64) / previous
    }

    /// Get price volatility (standard deviation of recent prices)
    pub fn volatility(&self, window: usize) -> Option<f64> {
        let prices: Vec<_> = self.history
            .iter()
            .rev()
            .take(window)
            .map(|p| p.price_cents as f64)
            .collect();

        if prices.len() < 2 {
            return None;
        }

        let mean = prices.iter().sum::<f64>() / prices.len() as f64;
        let variance = prices.iter()
            .map(|p| (p - mean).powi(2))
            .sum::<f64>() / (prices.len() - 1) as f64;

        Some(variance.sqrt())
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // INTERNAL
    // ═══════════════════════════════════════════════════════════════════════════

    /// Calculate deviation between two prices in basis points
    fn calculate_deviation(&self, old_price: u64, new_price: u64) -> u64 {
        let diff = if new_price > old_price {
            new_price - old_price
        } else {
            old_price - new_price
        };

        (diff as u128 * BPS_DIVISOR as u128 / old_price as u128) as u64
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(|e| Error::Serialization(e.to_string()))
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes).map_err(|e| Error::Deserialization(e.to_string()))
    }

    /// Compute state hash
    pub fn state_hash(&self) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(&self.current.price_cents.to_be_bytes());
        data.extend_from_slice(&self.current.timestamp.to_be_bytes());
        data.extend_from_slice(&[self.current.source_count]);
        Hash::sha256(&data)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRICE PROOF
// ═══════════════════════════════════════════════════════════════════════════════

/// ZK proof of price validity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceProof {
    /// The proven price
    pub price: PriceData,
    /// Hash of source data
    pub sources_hash: Hash,
    /// Proof data (would be actual ZK proof in production)
    pub proof_data: Vec<u8>,
    /// Timestamp of proof generation
    pub generated_at: u64,
}

impl PriceProof {
    /// Create a new price proof
    pub fn new(price: PriceData, sources_hash: Hash, proof_data: Vec<u8>, timestamp: u64) -> Self {
        Self {
            price,
            sources_hash,
            proof_data,
            generated_at: timestamp,
        }
    }

    /// Verify the proof (placeholder - would use actual ZK verification)
    pub fn verify(&self) -> bool {
        // In production, this would verify the ZK proof
        // For now, basic sanity checks
        self.price.price_cents >= MIN_SANE_BTC_PRICE
            && self.price.price_cents <= MAX_SANE_BTC_PRICE
            && !self.sources_hash.is_zero()
            && !self.proof_data.is_empty()
    }

    /// Get proof hash
    pub fn hash(&self) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(&self.price.price_cents.to_be_bytes());
        data.extend_from_slice(&self.price.timestamp.to_be_bytes());
        data.extend_from_slice(self.sources_hash.as_bytes());
        data.extend_from_slice(&self.proof_data);
        Hash::sha256(&data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_price(price: u64, timestamp: u64, sources: u8) -> PriceData {
        PriceData::new(price, timestamp, sources)
    }

    #[test]
    fn test_price_data_creation() {
        let price = make_price(10_000_000, 1000, 3);
        assert_eq!(price.price_cents, 10_000_000);
        assert_eq!(price.timestamp, 1000);
        assert_eq!(price.source_count, 3);
        assert!(price.confidence >= 70);
    }

    #[test]
    fn test_price_freshness() {
        let price = make_price(10_000_000, 1000, 3);

        // Fresh (10 seconds old)
        assert!(price.is_fresh(1010, 3600));

        // Stale (2 hours old)
        assert!(!price.is_fresh(8200, 3600));
    }

    #[test]
    fn test_price_feed_update() {
        let mut feed = PriceFeed::new();

        let price = make_price(10_000_000, 1000, 3);
        feed.update(price).unwrap();

        assert_eq!(feed.price_cents(), 10_000_000);
    }

    #[test]
    fn test_price_feed_deviation_check() {
        let mut feed = PriceFeed::new();

        // First update (baseline)
        let price1 = make_price(10_000_000, 1000, 3);
        feed.update(price1).unwrap();

        // Small update (2% increase - within bounds)
        let price2 = make_price(10_200_000, 1100, 3);
        assert!(feed.update(price2).is_ok());

        // Large update (10% increase - exceeds 5% limit)
        let price3 = make_price(11_200_000, 1200, 3);
        assert!(feed.update(price3).is_err());
    }

    #[test]
    fn test_price_feed_insufficient_sources() {
        let mut feed = PriceFeed::new();

        // Only 2 sources (need 3)
        let price = make_price(10_000_000, 1000, 2);
        assert!(feed.update(price).is_err());
    }

    #[test]
    fn test_price_validation() {
        let mut feed = PriceFeed::new();

        let price = make_price(10_000_000, 1000, 3);
        feed.update(price).unwrap();

        // Valid at timestamp 1010
        assert!(feed.is_valid(1010));

        // Valid at timestamp just before staleness
        assert!(feed.is_valid(1000 + MAX_PRICE_STALENESS_SECS));

        // Invalid when stale
        assert!(!feed.is_valid(1000 + MAX_PRICE_STALENESS_SECS + 1));
    }

    #[test]
    fn test_twap() {
        let mut feed = PriceFeed::new();

        // Add several prices
        feed.force_update(make_price(10_000_000, 100, 3));
        feed.force_update(make_price(10_200_000, 200, 3));
        feed.force_update(make_price(10_100_000, 300, 3));

        let twap = feed.twap(300, 300).unwrap();
        // Average of 10M, 10.2M, 10.1M = 10.1M
        assert_eq!(twap, 10_100_000);
    }

    #[test]
    fn test_price_change() {
        let mut feed = PriceFeed::new();

        feed.force_update(make_price(10_000_000, 100, 3));
        feed.force_update(make_price(10_500_000, 200, 3));

        // 5% increase = 500 basis points
        assert_eq!(feed.price_change_bps(), 500);
    }

    #[test]
    fn test_price_proof() {
        let price = make_price(10_000_000, 1000, 3);
        let proof = PriceProof::new(
            price,
            Hash::sha256(b"sources"),
            vec![1, 2, 3, 4],
            1000,
        );

        assert!(proof.verify());
    }
}
