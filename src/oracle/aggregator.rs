//! Price aggregator for combining multiple sources.
//!
//! This module provides price aggregation functionality:
//! - Median-based aggregation (resistant to outliers)
//! - Weighted aggregation based on source reliability
//! - ZK proof generation for aggregated prices

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::oracle::price_feed::{PriceData, PriceFeed, PriceProof};
use crate::oracle::sources::{PriceSource, PriceSourceFetcher, SourceCollection};
use crate::utils::constants::*;
use crate::utils::crypto::Hash;
use crate::utils::validation::*;

// ═══════════════════════════════════════════════════════════════════════════════
// AGGREGATION STRATEGY
// ═══════════════════════════════════════════════════════════════════════════════

/// Strategy for aggregating prices
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggregationStrategy {
    /// Simple median of all prices
    Median,
    /// Weighted average based on source reliability
    WeightedAverage,
    /// Median after removing outliers
    TrimmedMedian,
    /// Weighted median
    WeightedMedian,
}

impl Default for AggregationStrategy {
    fn default() -> Self {
        Self::TrimmedMedian
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AGGREGATION RESULT
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of price aggregation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationResult {
    /// Final aggregated price in cents
    pub price_cents: u64,
    /// Number of sources used
    pub source_count: usize,
    /// Strategy used
    pub strategy: AggregationStrategy,
    /// Timestamp of aggregation
    pub timestamp: u64,
    /// Hash of source data
    pub sources_hash: Hash,
    /// Individual source prices (for verification)
    pub source_prices: Vec<u64>,
    /// Confidence score
    pub confidence: u8,
}

impl AggregationResult {
    /// Convert to PriceData
    pub fn to_price_data(&self) -> PriceData {
        PriceData::new(
            self.price_cents,
            self.timestamp,
            self.source_count as u8,
        )
    }

    /// Create a price proof from this result
    pub fn to_proof(&self) -> PriceProof {
        // In production, this would generate an actual ZK proof
        let proof_data = self.generate_proof_data();

        PriceProof::new(
            self.to_price_data(),
            self.sources_hash,
            proof_data,
            self.timestamp,
        )
    }

    /// Generate proof data (placeholder for ZK proof)
    fn generate_proof_data(&self) -> Vec<u8> {
        let mut data = Vec::new();

        // Include all inputs to the aggregation
        data.extend_from_slice(&self.price_cents.to_be_bytes());
        data.extend_from_slice(&(self.source_count as u64).to_be_bytes());
        data.extend_from_slice(&(self.strategy as u8).to_be_bytes()[..1]);
        data.extend_from_slice(&self.timestamp.to_be_bytes());

        // Include source prices
        for price in &self.source_prices {
            data.extend_from_slice(&price.to_be_bytes());
        }

        // Hash the data (in production, would be ZK proof)
        Hash::sha256(&data).as_bytes().to_vec()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRICE AGGREGATOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Aggregator for combining multiple price sources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceAggregator {
    /// Price feed to update
    price_feed: PriceFeed,
    /// Aggregation strategy
    strategy: AggregationStrategy,
    /// Minimum sources required
    min_sources: usize,
    /// Maximum price deviation for outlier detection
    max_deviation_bps: u64,
    /// Last successful aggregation
    last_aggregation: Option<AggregationResult>,
}

impl Default for PriceAggregator {
    fn default() -> Self {
        Self::new()
    }
}

impl PriceAggregator {
    /// Create a new price aggregator
    pub fn new() -> Self {
        Self {
            price_feed: PriceFeed::new(),
            strategy: AggregationStrategy::default(),
            min_sources: MIN_ORACLE_SOURCES,
            max_deviation_bps: MAX_PRICE_DEVIATION_BPS,
            last_aggregation: None,
        }
    }

    /// Create with custom parameters
    pub fn with_params(
        strategy: AggregationStrategy,
        min_sources: usize,
        max_deviation_bps: u64,
    ) -> Self {
        Self {
            price_feed: PriceFeed::with_params(min_sources, MAX_PRICE_STALENESS_SECS, max_deviation_bps),
            strategy,
            min_sources,
            max_deviation_bps,
            last_aggregation: None,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // AGGREGATION
    // ═══════════════════════════════════════════════════════════════════════════

    /// Aggregate prices from a collection of sources
    pub fn aggregate(&mut self, sources: &SourceCollection) -> Result<AggregationResult> {
        // Validate minimum sources
        if sources.len() < self.min_sources {
            return Err(Error::InsufficientOracleSources {
                got: sources.len(),
                need: self.min_sources,
            });
        }

        // Get prices and apply strategy
        let mut prices: Vec<u64> = sources.prices();

        // Sort for median calculations
        prices.sort_unstable();

        // Calculate aggregated price based on strategy
        let price_cents = match self.strategy {
            AggregationStrategy::Median => {
                self.calculate_median(&prices)
            }
            AggregationStrategy::WeightedAverage => {
                sources.weighted_average_price()
                    .ok_or(Error::InsufficientOracleSources {
                        got: 0,
                        need: self.min_sources,
                    })?
            }
            AggregationStrategy::TrimmedMedian => {
                let trimmed = self.trim_outliers(&prices);
                self.calculate_median(&trimmed)
            }
            AggregationStrategy::WeightedMedian => {
                self.calculate_weighted_median(sources)
            }
        };

        // Validate resulting price
        validate_btc_price(price_cents)?;

        // Calculate confidence
        let confidence = self.calculate_confidence(&prices, price_cents);

        let result = AggregationResult {
            price_cents,
            source_count: sources.len(),
            strategy: self.strategy,
            timestamp: sources.collected_at,
            sources_hash: sources.hash(),
            source_prices: prices,
            confidence,
        };

        // Update price feed
        self.price_feed.update(result.to_price_data())?;

        // Store result
        self.last_aggregation = Some(result.clone());

        Ok(result)
    }

    /// Aggregate and generate proof
    pub fn aggregate_with_proof(&mut self, sources: &SourceCollection) -> Result<(AggregationResult, PriceProof)> {
        let result = self.aggregate(sources)?;
        let proof = result.to_proof();
        Ok((result, proof))
    }

    /// Fetch and aggregate using a price fetcher
    pub fn fetch_and_aggregate<F: PriceSourceFetcher>(
        &mut self,
        fetcher: &F,
    ) -> Result<AggregationResult> {
        let sources = fetcher.fetch_all()?;
        self.aggregate(&sources)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // CALCULATION HELPERS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Calculate median of sorted prices
    fn calculate_median(&self, sorted_prices: &[u64]) -> u64 {
        if sorted_prices.is_empty() {
            return 0;
        }

        let mid = sorted_prices.len() / 2;
        if sorted_prices.len() % 2 == 0 {
            (sorted_prices[mid - 1] + sorted_prices[mid]) / 2
        } else {
            sorted_prices[mid]
        }
    }

    /// Trim outliers from prices
    fn trim_outliers(&self, sorted_prices: &[u64]) -> Vec<u64> {
        if sorted_prices.len() <= 2 {
            return sorted_prices.to_vec();
        }

        let median = self.calculate_median(sorted_prices);

        sorted_prices
            .iter()
            .filter(|&&price| {
                let diff = if price > median {
                    price - median
                } else {
                    median - price
                };
                let deviation_bps = (diff as u128 * BPS_DIVISOR as u128 / median as u128) as u64;
                deviation_bps <= self.max_deviation_bps
            })
            .copied()
            .collect()
    }

    /// Calculate weighted median
    fn calculate_weighted_median(&self, sources: &SourceCollection) -> u64 {
        let mut weighted: Vec<(u64, u64)> = sources
            .sources()
            .iter()
            .map(|s| (s.price_cents, s.effective_weight()))
            .collect();

        // Sort by price
        weighted.sort_by_key(|(price, _)| *price);

        // Find weighted median
        let total_weight: u64 = weighted.iter().map(|(_, w)| w).sum();
        let half_weight = total_weight / 2;

        let mut cumulative_weight = 0u64;
        for (price, weight) in weighted {
            cumulative_weight += weight;
            if cumulative_weight >= half_weight {
                return price;
            }
        }

        // Fallback to simple median
        sources.median_price().unwrap_or(0)
    }

    /// Calculate confidence score
    fn calculate_confidence(&self, prices: &[u64], aggregated: u64) -> u8 {
        if prices.is_empty() || aggregated == 0 {
            return 0;
        }

        // Base confidence from source count
        let source_confidence: u8 = match prices.len() {
            1 => 30,
            2 => 50,
            3 => 70,
            4 => 85,
            _ => 95,
        };

        // Reduce confidence based on deviation
        let max_deviation = prices.iter().map(|&p| {
            let diff = if p > aggregated { p - aggregated } else { aggregated - p };
            (diff as u128 * 10000 / aggregated as u128) as u64
        }).max().unwrap_or(0);

        let deviation_penalty: u8 = (max_deviation / 100).min(30) as u8;

        source_confidence.saturating_sub(deviation_penalty)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // QUERIES
    // ═══════════════════════════════════════════════════════════════════════════

    /// Get current price feed
    pub fn price_feed(&self) -> &PriceFeed {
        &self.price_feed
    }

    /// Get current price
    pub fn current_price(&self) -> Option<u64> {
        if self.price_feed.price_cents() > 0 {
            Some(self.price_feed.price_cents())
        } else {
            None
        }
    }

    /// Get last aggregation result
    pub fn last_aggregation(&self) -> Option<&AggregationResult> {
        self.last_aggregation.as_ref()
    }

    /// Check if price is valid for use
    pub fn is_price_valid(&self, current_time: u64) -> bool {
        self.price_feed.is_valid(current_time)
    }

    /// Get validated price or error
    pub fn get_validated_price(&self, current_time: u64) -> Result<u64> {
        self.price_feed.get_validated_price(current_time)
    }

    /// Get TWAP
    pub fn twap(&self, period_secs: u64, current_time: u64) -> Option<u64> {
        self.price_feed.twap(period_secs, current_time)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ORACLE SERVICE
// ═══════════════════════════════════════════════════════════════════════════════

/// High-level oracle service combining fetching and aggregation
pub struct OracleService<F: PriceSourceFetcher> {
    /// Price fetcher
    fetcher: F,
    /// Price aggregator
    aggregator: PriceAggregator,
    /// Update interval in seconds
    update_interval: u64,
    /// Last update timestamp
    last_update: u64,
}

impl<F: PriceSourceFetcher> OracleService<F> {
    /// Create a new oracle service
    pub fn new(fetcher: F, update_interval: u64) -> Self {
        Self {
            fetcher,
            aggregator: PriceAggregator::new(),
            update_interval,
            last_update: 0,
        }
    }

    /// Update price if needed
    pub fn update_if_needed(&mut self, current_time: u64) -> Result<Option<AggregationResult>> {
        if current_time < self.last_update + self.update_interval {
            return Ok(None);
        }

        let result = self.aggregator.fetch_and_aggregate(&self.fetcher)?;
        self.last_update = current_time;
        Ok(Some(result))
    }

    /// Force update
    pub fn force_update(&mut self) -> Result<AggregationResult> {
        let result = self.aggregator.fetch_and_aggregate(&self.fetcher)?;
        self.last_update = result.timestamp;
        Ok(result)
    }

    /// Get current price
    pub fn current_price(&self) -> Option<u64> {
        self.aggregator.current_price()
    }

    /// Get aggregator
    pub fn aggregator(&self) -> &PriceAggregator {
        &self.aggregator
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::sources::{Exchange, MockPriceFetcher, PriceSource};

    fn make_collection(prices: &[u64], timestamp: u64) -> SourceCollection {
        let mut collection = SourceCollection::new(timestamp);
        let exchanges = [Exchange::Binance, Exchange::Coinbase, Exchange::Kraken, Exchange::Bitstamp];

        for (i, &price) in prices.iter().enumerate() {
            let exchange = exchanges[i % exchanges.len()];
            collection.add(PriceSource::new(exchange, price, timestamp));
        }

        collection
    }

    #[test]
    fn test_median_aggregation() {
        let mut aggregator = PriceAggregator::with_params(
            AggregationStrategy::Median,
            3,
            500,
        );

        let collection = make_collection(&[10_000_000, 10_100_000, 10_050_000], 1000);
        let result = aggregator.aggregate(&collection).unwrap();

        assert_eq!(result.price_cents, 10_050_000);
        assert_eq!(result.source_count, 3);
    }

    #[test]
    fn test_trimmed_median() {
        let mut aggregator = PriceAggregator::with_params(
            AggregationStrategy::TrimmedMedian,
            3,
            500, // 5% max deviation
        );

        // Include an outlier
        let collection = make_collection(
            &[10_000_000, 10_050_000, 10_025_000, 11_000_000], // Last is outlier
            1000,
        );
        let result = aggregator.aggregate(&collection).unwrap();

        // Outlier should be trimmed, median of remaining
        assert!(result.price_cents >= 10_000_000 && result.price_cents <= 10_100_000);
    }

    #[test]
    fn test_insufficient_sources() {
        let mut aggregator = PriceAggregator::new();
        let collection = make_collection(&[10_000_000, 10_100_000], 1000);

        let result = aggregator.aggregate(&collection);
        assert!(result.is_err());
    }

    #[test]
    fn test_confidence_calculation() {
        let mut aggregator = PriceAggregator::new();

        // Tight prices = high confidence
        let collection = make_collection(&[10_000_000, 10_010_000, 10_005_000], 1000);
        let result = aggregator.aggregate(&collection).unwrap();
        assert!(result.confidence >= 70);

        // Spread prices = lower confidence
        let collection2 = make_collection(&[10_000_000, 10_400_000, 10_200_000], 2000);
        let result2 = aggregator.aggregate(&collection2).unwrap();
        assert!(result2.confidence < result.confidence);
    }

    #[test]
    fn test_proof_generation() {
        let mut aggregator = PriceAggregator::new();
        let collection = make_collection(&[10_000_000, 10_050_000, 10_025_000], 1000);

        let (result, proof) = aggregator.aggregate_with_proof(&collection).unwrap();

        assert!(proof.verify());
        assert_eq!(proof.price.price_cents, result.price_cents);
    }

    #[test]
    fn test_oracle_service() {
        let fetcher = MockPriceFetcher::new(10_000_000, 1000);
        let mut service = OracleService::new(fetcher, 60);

        // First update should work
        let result = service.update_if_needed(1000).unwrap();
        assert!(result.is_some());

        // Second update within interval should return None
        let result2 = service.update_if_needed(1030).unwrap();
        assert!(result2.is_none());

        // Update after interval should work
        let result3 = service.update_if_needed(1061).unwrap();
        assert!(result3.is_some());
    }

    #[test]
    fn test_price_feed_update() {
        let mut aggregator = PriceAggregator::new();

        let collection1 = make_collection(&[10_000_000, 10_050_000, 10_025_000], 1000);
        aggregator.aggregate(&collection1).unwrap();

        assert!(aggregator.is_price_valid(1000));
        assert_eq!(aggregator.current_price(), Some(10_025_000));

        // Price should be stale after max staleness
        assert!(!aggregator.is_price_valid(1000 + MAX_PRICE_STALENESS_SECS + 1));
    }
}
