//! Protocol Metrics Collection.
//!
//! Collects and tracks key protocol metrics for monitoring and alerting.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

// ═══════════════════════════════════════════════════════════════════════════════
// METRIC TYPES
// ═══════════════════════════════════════════════════════════════════════════════

/// Types of protocol metrics
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetricType {
    // Protocol State
    /// Total CDPs in system
    TotalCDPs,
    /// Active CDPs
    ActiveCDPs,
    /// Liquidated CDPs
    LiquidatedCDPs,
    /// Total collateral locked (sats)
    TotalCollateral,
    /// Total debt issued (cents)
    TotalDebt,
    /// System collateralization ratio
    SystemCollateralRatio,

    // Financial Metrics
    /// BTC price (cents)
    BTCPrice,
    /// zkUSD total supply (cents)
    TotalSupply,
    /// Stability pool balance (cents)
    StabilityPoolBalance,
    /// Total fees collected (cents)
    TotalFeesCollected,

    // Performance Metrics
    /// Transactions per block
    TransactionsPerBlock,
    /// Average transaction latency (ms)
    AvgTransactionLatency,
    /// RPC requests per second
    RPCRequestsPerSecond,
    /// Rate limited requests
    RateLimitedRequests,

    // Risk Metrics
    /// CDPs below 120% ratio
    RiskyCDPCount,
    /// Average CDP ratio
    AverageCDPRatio,
    /// Minimum CDP ratio in system
    MinimumCDPRatio,
    /// Recovery mode active
    RecoveryModeActive,

    // Oracle Metrics
    /// Price sources active
    ActivePriceSources,
    /// Price update age (seconds)
    PriceUpdateAge,
    /// Price deviation between sources
    PriceDeviation,
}

// ═══════════════════════════════════════════════════════════════════════════════
// METRIC VALUE
// ═══════════════════════════════════════════════════════════════════════════════

/// A single metric data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricValue {
    /// Metric type
    pub metric: MetricType,
    /// Value
    pub value: u64,
    /// Block height when recorded
    pub block_height: u64,
    /// Timestamp when recorded
    pub timestamp: u64,
}

impl MetricValue {
    /// Create new metric value
    pub fn new(metric: MetricType, value: u64, block_height: u64, timestamp: u64) -> Self {
        Self {
            metric,
            value,
            block_height,
            timestamp,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TIME SERIES
// ═══════════════════════════════════════════════════════════════════════════════

/// Time series of metric values
#[derive(Debug, Clone)]
pub struct MetricTimeSeries {
    /// Metric type
    metric: MetricType,
    /// Historical values
    values: VecDeque<MetricValue>,
    /// Maximum history size
    max_size: usize,
    /// Current value
    current: u64,
    /// Min value seen
    min: u64,
    /// Max value seen
    max: u64,
    /// Running sum for average calculation
    sum: u64,
    /// Count for average
    count: u64,
}

impl MetricTimeSeries {
    /// Create new time series
    pub fn new(metric: MetricType, max_size: usize) -> Self {
        Self {
            metric,
            values: VecDeque::with_capacity(max_size),
            max_size,
            current: 0,
            min: u64::MAX,
            max: 0,
            sum: 0,
            count: 0,
        }
    }

    /// Add a value
    pub fn add(&mut self, value: u64, block_height: u64, timestamp: u64) {
        // Update current
        self.current = value;

        // Update min/max
        self.min = self.min.min(value);
        self.max = self.max.max(value);

        // Update running average
        self.sum = self.sum.saturating_add(value);
        self.count += 1;

        // Add to history
        let metric_value = MetricValue::new(self.metric, value, block_height, timestamp);

        if self.values.len() >= self.max_size {
            self.values.pop_front();
        }
        self.values.push_back(metric_value);
    }

    /// Get current value
    pub fn current(&self) -> u64 {
        self.current
    }

    /// Get minimum value
    pub fn min(&self) -> u64 {
        self.min
    }

    /// Get maximum value
    pub fn max(&self) -> u64 {
        self.max
    }

    /// Get average value
    pub fn average(&self) -> u64 {
        if self.count == 0 {
            return 0;
        }
        self.sum / self.count
    }

    /// Get values in range
    pub fn values_since(&self, since_block: u64) -> Vec<&MetricValue> {
        self.values
            .iter()
            .filter(|v| v.block_height >= since_block)
            .collect()
    }

    /// Get last n values
    pub fn last_n(&self, n: usize) -> Vec<&MetricValue> {
        self.values.iter().rev().take(n).collect()
    }

    /// Calculate rate of change (per block)
    pub fn rate_of_change(&self, lookback: usize) -> i64 {
        if self.values.len() < 2 {
            return 0;
        }

        let recent: Vec<_> = self.values.iter().rev().take(lookback).collect();
        if recent.len() < 2 {
            return 0;
        }

        let newest = recent[0].value as i64;
        let oldest = recent[recent.len() - 1].value as i64;
        let blocks = (recent[0].block_height - recent[recent.len() - 1].block_height) as i64;

        if blocks == 0 {
            return 0;
        }

        (newest - oldest) / blocks
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// METRICS COLLECTOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Main metrics collector
#[derive(Debug)]
pub struct MetricsCollector {
    /// All metric time series
    series: std::collections::HashMap<MetricType, MetricTimeSeries>,
    /// Collection start time
    start_time: Instant,
    /// Total collections
    collection_count: AtomicU64,
    /// History size per metric
    history_size: usize,
}

impl MetricsCollector {
    /// Create new collector
    pub fn new(history_size: usize) -> Self {
        Self {
            series: std::collections::HashMap::new(),
            start_time: Instant::now(),
            collection_count: AtomicU64::new(0),
            history_size,
        }
    }

    /// Record a metric value
    pub fn record(&mut self, metric: MetricType, value: u64, block_height: u64, timestamp: u64) {
        let series = self.series
            .entry(metric)
            .or_insert_with(|| MetricTimeSeries::new(metric, self.history_size));

        series.add(value, block_height, timestamp);
        self.collection_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record multiple metrics at once
    pub fn record_batch(&mut self, metrics: &[(MetricType, u64)], block_height: u64, timestamp: u64) {
        for &(metric, value) in metrics {
            self.record(metric, value, block_height, timestamp);
        }
    }

    /// Get current value for a metric
    pub fn get(&self, metric: MetricType) -> Option<u64> {
        self.series.get(&metric).map(|s| s.current())
    }

    /// Get time series for a metric
    pub fn get_series(&self, metric: MetricType) -> Option<&MetricTimeSeries> {
        self.series.get(&metric)
    }

    /// Get all current metric values
    pub fn snapshot(&self) -> MetricsSnapshot {
        let mut values = Vec::new();

        for (metric, series) in &self.series {
            values.push((*metric, series.current()));
        }

        MetricsSnapshot {
            values,
            collection_count: self.collection_count.load(Ordering::Relaxed),
            uptime_seconds: self.start_time.elapsed().as_secs(),
        }
    }

    /// Get rate of change for a metric
    pub fn rate_of_change(&self, metric: MetricType, lookback: usize) -> Option<i64> {
        self.series.get(&metric).map(|s| s.rate_of_change(lookback))
    }

    /// Get all metrics in a category
    pub fn get_category(&self, category: MetricCategory) -> Vec<(MetricType, u64)> {
        self.series
            .iter()
            .filter(|(m, _)| get_metric_category(**m) == category)
            .map(|(m, s)| (*m, s.current()))
            .collect()
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new(1000)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// METRICS SNAPSHOT
// ═══════════════════════════════════════════════════════════════════════════════

/// Snapshot of all current metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// All metric values
    pub values: Vec<(MetricType, u64)>,
    /// Total collections made
    pub collection_count: u64,
    /// Collector uptime in seconds
    pub uptime_seconds: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// METRIC CATEGORIES
// ═══════════════════════════════════════════════════════════════════════════════

/// Categories of metrics
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricCategory {
    /// Protocol state metrics
    ProtocolState,
    /// Financial metrics
    Financial,
    /// Performance metrics
    Performance,
    /// Risk metrics
    Risk,
    /// Oracle metrics
    Oracle,
}

/// Get category for a metric type
pub fn get_metric_category(metric: MetricType) -> MetricCategory {
    match metric {
        MetricType::TotalCDPs
        | MetricType::ActiveCDPs
        | MetricType::LiquidatedCDPs
        | MetricType::TotalCollateral
        | MetricType::TotalDebt
        | MetricType::SystemCollateralRatio => MetricCategory::ProtocolState,

        MetricType::BTCPrice
        | MetricType::TotalSupply
        | MetricType::StabilityPoolBalance
        | MetricType::TotalFeesCollected => MetricCategory::Financial,

        MetricType::TransactionsPerBlock
        | MetricType::AvgTransactionLatency
        | MetricType::RPCRequestsPerSecond
        | MetricType::RateLimitedRequests => MetricCategory::Performance,

        MetricType::RiskyCDPCount
        | MetricType::AverageCDPRatio
        | MetricType::MinimumCDPRatio
        | MetricType::RecoveryModeActive => MetricCategory::Risk,

        MetricType::ActivePriceSources
        | MetricType::PriceUpdateAge
        | MetricType::PriceDeviation => MetricCategory::Oracle,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTER METRIC
// ═══════════════════════════════════════════════════════════════════════════════

/// Simple atomic counter for high-frequency metrics
#[derive(Debug)]
pub struct Counter {
    value: AtomicU64,
    name: &'static str,
}

impl Counter {
    /// Create new counter
    pub const fn new(name: &'static str) -> Self {
        Self {
            value: AtomicU64::new(0),
            name,
        }
    }

    /// Increment counter
    pub fn increment(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Add to counter
    pub fn add(&self, n: u64) {
        self.value.fetch_add(n, Ordering::Relaxed);
    }

    /// Get current value
    pub fn value(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Reset counter
    pub fn reset(&self) -> u64 {
        self.value.swap(0, Ordering::Relaxed)
    }

    /// Get name
    pub fn name(&self) -> &'static str {
        self.name
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GAUGE METRIC
// ═══════════════════════════════════════════════════════════════════════════════

/// Gauge for values that can go up or down
#[derive(Debug)]
pub struct Gauge {
    value: AtomicU64,
    name: &'static str,
}

impl Gauge {
    /// Create new gauge
    pub const fn new(name: &'static str) -> Self {
        Self {
            value: AtomicU64::new(0),
            name,
        }
    }

    /// Set gauge value
    pub fn set(&self, value: u64) {
        self.value.store(value, Ordering::Relaxed);
    }

    /// Get current value
    pub fn value(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Increment gauge
    pub fn increment(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement gauge
    pub fn decrement(&self) {
        self.value.fetch_sub(1, Ordering::Relaxed);
    }

    /// Get name
    pub fn name(&self) -> &'static str {
        self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_time_series() {
        let mut series = MetricTimeSeries::new(MetricType::TotalCDPs, 100);

        series.add(10, 100, 1000);
        series.add(20, 101, 1001);
        series.add(15, 102, 1002);

        assert_eq!(series.current(), 15);
        assert_eq!(series.min(), 10);
        assert_eq!(series.max(), 20);
        assert_eq!(series.average(), 15); // (10+20+15)/3 = 15
    }

    #[test]
    fn test_metrics_collector() {
        let mut collector = MetricsCollector::new(100);

        collector.record(MetricType::TotalCDPs, 100, 1, 1000);
        collector.record(MetricType::TotalCDPs, 150, 2, 1001);
        collector.record(MetricType::BTCPrice, 10_000_000, 2, 1001);

        assert_eq!(collector.get(MetricType::TotalCDPs), Some(150));
        assert_eq!(collector.get(MetricType::BTCPrice), Some(10_000_000));
    }

    #[test]
    fn test_counter() {
        let counter = Counter::new("test_counter");

        counter.increment();
        counter.increment();
        counter.add(5);

        assert_eq!(counter.value(), 7);
        assert_eq!(counter.reset(), 7);
        assert_eq!(counter.value(), 0);
    }

    #[test]
    fn test_gauge() {
        let gauge = Gauge::new("test_gauge");

        gauge.set(100);
        assert_eq!(gauge.value(), 100);

        gauge.increment();
        assert_eq!(gauge.value(), 101);

        gauge.decrement();
        assert_eq!(gauge.value(), 100);
    }
}
