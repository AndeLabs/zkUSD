//! Protocol Health Scoring System.
//!
//! Computes overall protocol health based on multiple factors.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::alerts::{AlertManager, AlertSeverity};
use super::metrics::{MetricType, MetricsCollector};

// ═══════════════════════════════════════════════════════════════════════════════
// HEALTH STATUS
// ═══════════════════════════════════════════════════════════════════════════════

/// Overall health status
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum HealthStatus {
    /// System operating normally
    Healthy,
    /// Minor issues detected
    Degraded,
    /// Significant issues requiring attention
    Warning,
    /// Critical issues affecting operations
    Critical,
    /// System in emergency state
    Emergency,
}

impl HealthStatus {
    /// Get display string
    pub fn as_str(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "HEALTHY",
            HealthStatus::Degraded => "DEGRADED",
            HealthStatus::Warning => "WARNING",
            HealthStatus::Critical => "CRITICAL",
            HealthStatus::Emergency => "EMERGENCY",
        }
    }

    /// Convert from score (0-100)
    pub fn from_score(score: u8) -> Self {
        match score {
            90..=100 => HealthStatus::Healthy,
            70..=89 => HealthStatus::Degraded,
            50..=69 => HealthStatus::Warning,
            25..=49 => HealthStatus::Critical,
            _ => HealthStatus::Emergency,
        }
    }

    /// Get minimum score for this status
    pub fn min_score(&self) -> u8 {
        match self {
            HealthStatus::Healthy => 90,
            HealthStatus::Degraded => 70,
            HealthStatus::Warning => 50,
            HealthStatus::Critical => 25,
            HealthStatus::Emergency => 0,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// HEALTH COMPONENT
// ═══════════════════════════════════════════════════════════════════════════════

/// Components contributing to health score
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HealthComponent {
    /// Collateralization health
    Collateralization,
    /// Oracle/price feed health
    Oracle,
    /// Stability pool health
    StabilityPool,
    /// Liquidation system health
    Liquidation,
    /// Performance health
    Performance,
    /// Governance health
    Governance,
}

impl HealthComponent {
    /// Get all components
    pub fn all() -> &'static [HealthComponent] {
        &[
            HealthComponent::Collateralization,
            HealthComponent::Oracle,
            HealthComponent::StabilityPool,
            HealthComponent::Liquidation,
            HealthComponent::Performance,
            HealthComponent::Governance,
        ]
    }

    /// Get weight for this component (out of 100)
    pub fn weight(&self) -> u8 {
        match self {
            HealthComponent::Collateralization => 30,
            HealthComponent::Oracle => 25,
            HealthComponent::StabilityPool => 15,
            HealthComponent::Liquidation => 15,
            HealthComponent::Performance => 10,
            HealthComponent::Governance => 5,
        }
    }

    /// Get description
    pub fn description(&self) -> &'static str {
        match self {
            HealthComponent::Collateralization => "System collateralization and CDP health",
            HealthComponent::Oracle => "Price feed reliability and freshness",
            HealthComponent::StabilityPool => "Stability pool depth and coverage",
            HealthComponent::Liquidation => "Liquidation system readiness",
            HealthComponent::Performance => "Transaction throughput and latency",
            HealthComponent::Governance => "Governance participation and activity",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMPONENT SCORE
// ═══════════════════════════════════════════════════════════════════════════════

/// Score for a single health component
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentScore {
    /// Component
    pub component: HealthComponent,
    /// Score (0-100)
    pub score: u8,
    /// Status derived from score
    pub status: HealthStatus,
    /// Factors contributing to score
    pub factors: Vec<HealthFactor>,
    /// Recommendations for improvement
    pub recommendations: Vec<String>,
}

impl ComponentScore {
    /// Create new component score
    pub fn new(component: HealthComponent, score: u8, factors: Vec<HealthFactor>) -> Self {
        let status = HealthStatus::from_score(score);
        let recommendations = Self::generate_recommendations(&component, &factors);

        Self {
            component,
            score,
            status,
            factors,
            recommendations,
        }
    }

    /// Generate recommendations based on factors
    fn generate_recommendations(component: &HealthComponent, factors: &[HealthFactor]) -> Vec<String> {
        let mut recommendations = Vec::new();

        for factor in factors {
            if factor.score < 70 {
                let rec = match component {
                    HealthComponent::Collateralization => {
                        match factor.name.as_str() {
                            "system_ratio" => "Consider encouraging debt repayment or additional collateral deposits",
                            "risky_cdps" => "Monitor risky CDPs closely and prepare liquidation bots",
                            "min_ratio" => "Alert users with low ratios to add collateral",
                            _ => "Review collateralization metrics",
                        }
                    }
                    HealthComponent::Oracle => {
                        match factor.name.as_str() {
                            "price_freshness" => "Check oracle connectivity and update frequency",
                            "source_count" => "Add additional price sources for redundancy",
                            "deviation" => "Investigate price source discrepancies",
                            _ => "Review oracle configuration",
                        }
                    }
                    HealthComponent::StabilityPool => {
                        match factor.name.as_str() {
                            "coverage" => "Incentivize stability pool deposits",
                            "utilization" => "Monitor liquidation pressure",
                            _ => "Review stability pool parameters",
                        }
                    }
                    HealthComponent::Liquidation => {
                        match factor.name.as_str() {
                            "backlog" => "Ensure liquidation bots are active",
                            "efficiency" => "Review liquidation incentives",
                            _ => "Check liquidation system",
                        }
                    }
                    HealthComponent::Performance => {
                        match factor.name.as_str() {
                            "latency" => "Investigate transaction processing bottlenecks",
                            "throughput" => "Consider scaling infrastructure",
                            "rate_limits" => "Adjust rate limiting thresholds if legitimate",
                            _ => "Review system performance",
                        }
                    }
                    HealthComponent::Governance => {
                        match factor.name.as_str() {
                            "participation" => "Encourage governance participation",
                            "pending_proposals" => "Review pending proposals",
                            _ => "Review governance activity",
                        }
                    }
                };
                recommendations.push(rec.to_string());
            }
        }

        recommendations
    }
}

/// Individual factor contributing to component score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthFactor {
    /// Factor name
    pub name: String,
    /// Factor score (0-100)
    pub score: u8,
    /// Current value
    pub value: u64,
    /// Optimal/target value
    pub target: u64,
    /// Weight within component
    pub weight: u8,
}

impl HealthFactor {
    /// Create new health factor
    pub fn new(name: &str, value: u64, target: u64, weight: u8) -> Self {
        let score = Self::calculate_score(value, target);
        Self {
            name: name.to_string(),
            score,
            value,
            target,
            weight,
        }
    }

    /// Calculate score based on value vs target
    fn calculate_score(value: u64, target: u64) -> u8 {
        if target == 0 {
            return if value == 0 { 100 } else { 0 };
        }

        let ratio = (value as f64) / (target as f64);

        // Score based on how close to target
        // At target = 100, below target = scaled down, above target = capped at 100
        let score = if ratio >= 1.0 {
            100.0
        } else {
            ratio * 100.0
        };

        score.min(100.0).max(0.0) as u8
    }

    /// Create factor where lower is better (e.g., latency)
    pub fn new_lower_better(name: &str, value: u64, threshold: u64, weight: u8) -> Self {
        let score = if value == 0 {
            100
        } else if value >= threshold {
            0
        } else {
            ((threshold - value) * 100 / threshold) as u8
        };

        Self {
            name: name.to_string(),
            score,
            value,
            target: threshold,
            weight,
        }
    }

    /// Create factor where value should be in range
    pub fn new_range(name: &str, value: u64, min: u64, max: u64, weight: u8) -> Self {
        let score = if value < min || value > max {
            0
        } else {
            // Score based on distance from center of range
            let center = (min + max) / 2;
            let range = max - min;
            let distance = if value > center { value - center } else { center - value };
            let ratio = 1.0 - (distance as f64 / (range as f64 / 2.0));
            (ratio * 100.0) as u8
        };

        Self {
            name: name.to_string(),
            score,
            value,
            target: (min + max) / 2,
            weight,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// HEALTH REPORT
// ═══════════════════════════════════════════════════════════════════════════════

/// Complete health report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    /// Overall score (0-100)
    pub overall_score: u8,
    /// Overall status
    pub status: HealthStatus,
    /// Component scores
    pub components: Vec<ComponentScore>,
    /// Active alert count
    pub active_alerts: u64,
    /// Critical alert count
    pub critical_alerts: u64,
    /// Block height when computed
    pub block_height: u64,
    /// Timestamp when computed
    pub timestamp: u64,
    /// Top recommendations
    pub top_recommendations: Vec<String>,
}

impl HealthReport {
    /// Check if protocol is operational
    pub fn is_operational(&self) -> bool {
        self.status != HealthStatus::Emergency
    }

    /// Get components below threshold
    pub fn degraded_components(&self) -> Vec<&ComponentScore> {
        self.components
            .iter()
            .filter(|c| c.status != HealthStatus::Healthy)
            .collect()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// HEALTH CHECKER
// ═══════════════════════════════════════════════════════════════════════════════

/// Configuration for health checker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckerConfig {
    /// Target system collateralization ratio (basis points)
    pub target_collateral_ratio: u64,
    /// Minimum acceptable collateral ratio
    pub min_collateral_ratio: u64,
    /// Maximum risky CDPs before degradation
    pub max_risky_cdps: u64,
    /// Target stability pool coverage (percentage of debt)
    pub target_stability_coverage: u64,
    /// Maximum acceptable price age (seconds)
    pub max_price_age: u64,
    /// Maximum acceptable price deviation (basis points)
    pub max_price_deviation: u64,
    /// Minimum price sources required
    pub min_price_sources: u64,
    /// Maximum acceptable latency (ms)
    pub max_latency_ms: u64,
}

impl Default for HealthCheckerConfig {
    fn default() -> Self {
        Self {
            target_collateral_ratio: 20000, // 200%
            min_collateral_ratio: 15000,    // 150%
            max_risky_cdps: 50,
            target_stability_coverage: 100, // 100% coverage
            max_price_age: 3600,            // 1 hour
            max_price_deviation: 500,       // 5%
            min_price_sources: 3,
            max_latency_ms: 5000,
        }
    }
}

/// Health checker computes protocol health
#[derive(Debug)]
pub struct HealthChecker {
    /// Configuration
    config: HealthCheckerConfig,
    /// Last report
    last_report: Option<HealthReport>,
    /// Historical scores
    score_history: Vec<(u64, u8)>, // (block_height, score)
    /// Maximum history entries
    max_history: usize,
}

impl HealthChecker {
    /// Create new health checker
    pub fn new(config: HealthCheckerConfig) -> Self {
        Self {
            config,
            last_report: None,
            score_history: Vec::new(),
            max_history: 1000,
        }
    }

    /// Compute health report
    pub fn compute_health(
        &mut self,
        metrics: &MetricsCollector,
        alerts: &AlertManager,
        block_height: u64,
        timestamp: u64,
    ) -> HealthReport {
        let mut components = Vec::new();
        let mut total_weighted_score: u64 = 0;
        let mut total_weight: u64 = 0;

        // Calculate each component
        for component in HealthComponent::all() {
            let score = self.calculate_component_score(*component, metrics);
            let weight = component.weight() as u64;

            total_weighted_score += (score.score as u64) * weight;
            total_weight += weight;

            components.push(score);
        }

        // Calculate overall score
        let overall_score = if total_weight > 0 {
            (total_weighted_score / total_weight) as u8
        } else {
            0
        };

        // Adjust for alerts
        let alert_stats = alerts.statistics();
        let alert_penalty = self.calculate_alert_penalty(&alert_stats, alerts);
        let adjusted_score = overall_score.saturating_sub(alert_penalty);

        let status = HealthStatus::from_score(adjusted_score);

        // Collect top recommendations
        let mut top_recommendations: Vec<String> = components
            .iter()
            .flat_map(|c| c.recommendations.clone())
            .take(5)
            .collect();

        // Add alert-based recommendations
        if alerts.has_emergency() {
            top_recommendations.insert(0, "EMERGENCY: Immediate attention required!".into());
        } else if alerts.has_critical() {
            top_recommendations.insert(0, "Critical alerts active - review immediately".into());
        }

        let report = HealthReport {
            overall_score: adjusted_score,
            status,
            components,
            active_alerts: alert_stats.active_count,
            critical_alerts: alert_stats.by_severity.get(&AlertSeverity::Critical).copied().unwrap_or(0)
                + alert_stats.by_severity.get(&AlertSeverity::Emergency).copied().unwrap_or(0),
            block_height,
            timestamp,
            top_recommendations,
        };

        // Update history
        self.score_history.push((block_height, adjusted_score));
        if self.score_history.len() > self.max_history {
            self.score_history.remove(0);
        }

        self.last_report = Some(report.clone());
        report
    }

    /// Calculate component score
    fn calculate_component_score(
        &self,
        component: HealthComponent,
        metrics: &MetricsCollector,
    ) -> ComponentScore {
        let factors = match component {
            HealthComponent::Collateralization => self.calc_collateralization_factors(metrics),
            HealthComponent::Oracle => self.calc_oracle_factors(metrics),
            HealthComponent::StabilityPool => self.calc_stability_pool_factors(metrics),
            HealthComponent::Liquidation => self.calc_liquidation_factors(metrics),
            HealthComponent::Performance => self.calc_performance_factors(metrics),
            HealthComponent::Governance => self.calc_governance_factors(metrics),
        };

        let score = self.weighted_average(&factors);
        ComponentScore::new(component, score, factors)
    }

    /// Calculate collateralization factors
    fn calc_collateralization_factors(&self, metrics: &MetricsCollector) -> Vec<HealthFactor> {
        let mut factors = Vec::new();

        // System collateral ratio
        let ratio = metrics.get(MetricType::SystemCollateralRatio).unwrap_or(0);
        factors.push(HealthFactor::new(
            "system_ratio",
            ratio,
            self.config.target_collateral_ratio,
            50,
        ));

        // Risky CDP count
        let risky = metrics.get(MetricType::RiskyCDPCount).unwrap_or(0);
        factors.push(HealthFactor::new_lower_better(
            "risky_cdps",
            risky,
            self.config.max_risky_cdps,
            30,
        ));

        // Minimum CDP ratio
        let min_ratio = metrics.get(MetricType::MinimumCDPRatio).unwrap_or(0);
        factors.push(HealthFactor::new(
            "min_ratio",
            min_ratio,
            self.config.min_collateral_ratio,
            20,
        ));

        factors
    }

    /// Calculate oracle factors
    fn calc_oracle_factors(&self, metrics: &MetricsCollector) -> Vec<HealthFactor> {
        let mut factors = Vec::new();

        // Price freshness
        let age = metrics.get(MetricType::PriceUpdateAge).unwrap_or(0);
        factors.push(HealthFactor::new_lower_better(
            "price_freshness",
            age,
            self.config.max_price_age,
            40,
        ));

        // Active price sources
        let sources = metrics.get(MetricType::ActivePriceSources).unwrap_or(0);
        factors.push(HealthFactor::new(
            "source_count",
            sources,
            self.config.min_price_sources,
            35,
        ));

        // Price deviation
        let deviation = metrics.get(MetricType::PriceDeviation).unwrap_or(0);
        factors.push(HealthFactor::new_lower_better(
            "deviation",
            deviation,
            self.config.max_price_deviation,
            25,
        ));

        factors
    }

    /// Calculate stability pool factors
    fn calc_stability_pool_factors(&self, metrics: &MetricsCollector) -> Vec<HealthFactor> {
        let mut factors = Vec::new();

        // Coverage ratio
        let pool_balance = metrics.get(MetricType::StabilityPoolBalance).unwrap_or(0);
        let total_debt = metrics.get(MetricType::TotalDebt).unwrap_or(1);

        let coverage = if total_debt > 0 {
            (pool_balance * 100) / total_debt
        } else {
            100
        };

        factors.push(HealthFactor::new(
            "coverage",
            coverage,
            self.config.target_stability_coverage,
            70,
        ));

        // Pool utilization (inverse - high utilization means pool being used for liquidations)
        // For now, use a placeholder
        factors.push(HealthFactor::new(
            "utilization",
            80, // 80% capacity available
            100,
            30,
        ));

        factors
    }

    /// Calculate liquidation factors
    fn calc_liquidation_factors(&self, metrics: &MetricsCollector) -> Vec<HealthFactor> {
        let mut factors = Vec::new();

        // Liquidation backlog (risky CDPs not yet liquidated)
        let risky = metrics.get(MetricType::RiskyCDPCount).unwrap_or(0);
        let liquidated = metrics.get(MetricType::LiquidatedCDPs).unwrap_or(0);

        // If there are risky CDPs and liquidations are happening, system is healthy
        let backlog_score = if risky == 0 {
            100
        } else if liquidated > 0 {
            70 // Some backlog but liquidations happening
        } else {
            30 // Risky CDPs with no liquidations
        };

        factors.push(HealthFactor {
            name: "backlog".into(),
            score: backlog_score,
            value: risky,
            target: 0,
            weight: 60,
        });

        // Liquidation efficiency (placeholder)
        factors.push(HealthFactor {
            name: "efficiency".into(),
            score: 85,
            value: 85,
            target: 100,
            weight: 40,
        });

        factors
    }

    /// Calculate performance factors
    fn calc_performance_factors(&self, metrics: &MetricsCollector) -> Vec<HealthFactor> {
        let mut factors = Vec::new();

        // Transaction latency
        let latency = metrics.get(MetricType::AvgTransactionLatency).unwrap_or(0);
        factors.push(HealthFactor::new_lower_better(
            "latency",
            latency,
            self.config.max_latency_ms,
            40,
        ));

        // Throughput (TPS)
        let tps = metrics.get(MetricType::TransactionsPerBlock).unwrap_or(0);
        factors.push(HealthFactor::new(
            "throughput",
            tps,
            100, // Target 100 tx/block
            30,
        ));

        // Rate limiting
        let rate_limited = metrics.get(MetricType::RateLimitedRequests).unwrap_or(0);
        factors.push(HealthFactor::new_lower_better(
            "rate_limits",
            rate_limited,
            100, // Concerning if > 100 rate limited
            30,
        ));

        factors
    }

    /// Calculate governance factors
    fn calc_governance_factors(&self, _metrics: &MetricsCollector) -> Vec<HealthFactor> {
        let mut factors = Vec::new();

        // Participation rate (placeholder - would need governance metrics)
        factors.push(HealthFactor {
            name: "participation".into(),
            score: 75,
            value: 75,
            target: 100,
            weight: 50,
        });

        // Pending proposals (placeholder)
        factors.push(HealthFactor {
            name: "pending_proposals".into(),
            score: 90,
            value: 2,
            target: 5, // Target max 5 pending
            weight: 50,
        });

        factors
    }

    /// Calculate weighted average of factors
    fn weighted_average(&self, factors: &[HealthFactor]) -> u8 {
        if factors.is_empty() {
            return 100;
        }

        let total_weight: u64 = factors.iter().map(|f| f.weight as u64).sum();
        if total_weight == 0 {
            return 100;
        }

        let weighted_sum: u64 = factors
            .iter()
            .map(|f| (f.score as u64) * (f.weight as u64))
            .sum();

        (weighted_sum / total_weight) as u8
    }

    /// Calculate penalty from active alerts
    fn calculate_alert_penalty(
        &self,
        stats: &super::alerts::AlertStatistics,
        alerts: &AlertManager,
    ) -> u8 {
        let mut penalty: u8 = 0;

        // Penalty per severity
        if let Some(&count) = stats.by_severity.get(&AlertSeverity::Emergency) {
            penalty = penalty.saturating_add((count * 30).min(50) as u8);
        }
        if let Some(&count) = stats.by_severity.get(&AlertSeverity::Critical) {
            penalty = penalty.saturating_add((count * 15).min(30) as u8);
        }
        if let Some(&count) = stats.by_severity.get(&AlertSeverity::Warning) {
            penalty = penalty.saturating_add((count * 5).min(15) as u8);
        }

        // Extra penalty for unacknowledged
        let unacked = alerts.get_unacknowledged().len() as u8;
        penalty = penalty.saturating_add((unacked * 2).min(10));

        penalty.min(50) // Max 50 point penalty from alerts
    }

    /// Get last report
    pub fn last_report(&self) -> Option<&HealthReport> {
        self.last_report.as_ref()
    }

    /// Get score trend (positive = improving, negative = declining)
    pub fn score_trend(&self, lookback: usize) -> i8 {
        if self.score_history.len() < 2 {
            return 0;
        }

        let recent: Vec<_> = self.score_history.iter().rev().take(lookback).collect();
        if recent.len() < 2 {
            return 0;
        }

        let newest = recent[0].1 as i16;
        let oldest = recent[recent.len() - 1].1 as i16;

        (newest - oldest).clamp(-100, 100) as i8
    }

    /// Get average score over period
    pub fn average_score(&self, lookback: usize) -> u8 {
        if self.score_history.is_empty() {
            return 0;
        }

        let scores: Vec<_> = self.score_history.iter().rev().take(lookback).collect();
        let sum: u64 = scores.iter().map(|(_, s)| *s as u64).sum();
        (sum / scores.len() as u64) as u8
    }
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new(HealthCheckerConfig::default())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// HEALTH ENDPOINT DATA
// ═══════════════════════════════════════════════════════════════════════════════

/// Simplified health status for external endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthEndpointResponse {
    /// Status string (healthy, degraded, warning, critical, emergency)
    pub status: String,
    /// Overall score
    pub score: u8,
    /// Whether system is accepting transactions
    pub operational: bool,
    /// Timestamp
    pub timestamp: u64,
    /// Component summary
    pub components: HashMap<String, ComponentSummary>,
}

/// Simplified component status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentSummary {
    /// Status string
    pub status: String,
    /// Score
    pub score: u8,
}

impl From<&HealthReport> for HealthEndpointResponse {
    fn from(report: &HealthReport) -> Self {
        let mut components = HashMap::new();

        for comp in &report.components {
            components.insert(
                format!("{:?}", comp.component).to_lowercase(),
                ComponentSummary {
                    status: comp.status.as_str().to_lowercase(),
                    score: comp.score,
                },
            );
        }

        Self {
            status: report.status.as_str().to_lowercase(),
            score: report.overall_score,
            operational: report.is_operational(),
            timestamp: report.timestamp,
            components,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status_from_score() {
        assert_eq!(HealthStatus::from_score(95), HealthStatus::Healthy);
        assert_eq!(HealthStatus::from_score(80), HealthStatus::Degraded);
        assert_eq!(HealthStatus::from_score(60), HealthStatus::Warning);
        assert_eq!(HealthStatus::from_score(40), HealthStatus::Critical);
        assert_eq!(HealthStatus::from_score(10), HealthStatus::Emergency);
    }

    #[test]
    fn test_health_factor_calculation() {
        // Value at target = 100
        let factor = HealthFactor::new("test", 100, 100, 50);
        assert_eq!(factor.score, 100);

        // Value below target
        let factor = HealthFactor::new("test", 50, 100, 50);
        assert_eq!(factor.score, 50);

        // Value above target (capped at 100)
        let factor = HealthFactor::new("test", 150, 100, 50);
        assert_eq!(factor.score, 100);
    }

    #[test]
    fn test_health_factor_lower_better() {
        // Zero is perfect
        let factor = HealthFactor::new_lower_better("test", 0, 100, 50);
        assert_eq!(factor.score, 100);

        // At threshold is zero
        let factor = HealthFactor::new_lower_better("test", 100, 100, 50);
        assert_eq!(factor.score, 0);

        // Halfway
        let factor = HealthFactor::new_lower_better("test", 50, 100, 50);
        assert_eq!(factor.score, 50);
    }

    #[test]
    fn test_component_weights() {
        let total: u8 = HealthComponent::all().iter().map(|c| c.weight()).sum();
        assert_eq!(total, 100);
    }

    #[test]
    fn test_health_checker() {
        let checker = HealthChecker::default();
        assert!(checker.last_report().is_none());
    }

    #[test]
    fn test_health_status_ordering() {
        // Derived Ord uses variant order: Healthy=0 < Degraded=1 < Warning=2...
        // Lower is better (healthier state)
        assert!(HealthStatus::Healthy < HealthStatus::Degraded);
        assert!(HealthStatus::Degraded < HealthStatus::Warning);
        assert!(HealthStatus::Warning < HealthStatus::Critical);
        assert!(HealthStatus::Critical < HealthStatus::Emergency);
    }
}
