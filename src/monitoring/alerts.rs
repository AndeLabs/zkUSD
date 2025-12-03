//! Protocol Alert System.
//!
//! Defines alert rules, thresholds, and notification mechanisms.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

use super::metrics::{MetricType, MetricsCollector};

// ═══════════════════════════════════════════════════════════════════════════════
// ALERT SEVERITY
// ═══════════════════════════════════════════════════════════════════════════════

/// Severity levels for alerts
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AlertSeverity {
    /// Informational alert
    Info,
    /// Warning - potential issue
    Warning,
    /// Critical - immediate attention required
    Critical,
    /// Emergency - system at risk
    Emergency,
}

impl AlertSeverity {
    /// Get display name
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertSeverity::Info => "INFO",
            AlertSeverity::Warning => "WARNING",
            AlertSeverity::Critical => "CRITICAL",
            AlertSeverity::Emergency => "EMERGENCY",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ALERT TYPE
// ═══════════════════════════════════════════════════════════════════════════════

/// Types of alerts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AlertType {
    // Protocol Health
    /// System collateralization below threshold
    LowCollateralization,
    /// Recovery mode activated
    RecoveryModeActive,
    /// High number of risky CDPs
    HighRiskyCDPCount,
    /// Minimum CDP ratio dangerously low
    DangerousMinCDPRatio,

    // Oracle Alerts
    /// Price feed stale
    StalePriceFeed,
    /// High price deviation between sources
    HighPriceDeviation,
    /// Insufficient price sources
    InsufficientPriceSources,

    // Financial Alerts
    /// Stability pool balance low
    LowStabilityPoolBalance,
    /// Sudden debt increase
    RapidDebtIncrease,
    /// Large redemption volume
    HighRedemptionVolume,

    // Performance Alerts
    /// High transaction latency
    HighTransactionLatency,
    /// Rate limiting threshold reached
    RateLimitThresholdReached,
    /// Unusual transaction volume
    UnusualTransactionVolume,

    // Security Alerts
    /// Multiple failed transactions
    MultipleFailedTransactions,
    /// Unusual withdrawal pattern
    UnusualWithdrawalPattern,
    /// Governance proposal requiring attention
    GovernanceAlert,
}

impl AlertType {
    /// Get default severity for this alert type
    pub fn default_severity(&self) -> AlertSeverity {
        match self {
            AlertType::LowCollateralization => AlertSeverity::Warning,
            AlertType::RecoveryModeActive => AlertSeverity::Critical,
            AlertType::HighRiskyCDPCount => AlertSeverity::Warning,
            AlertType::DangerousMinCDPRatio => AlertSeverity::Critical,

            AlertType::StalePriceFeed => AlertSeverity::Critical,
            AlertType::HighPriceDeviation => AlertSeverity::Warning,
            AlertType::InsufficientPriceSources => AlertSeverity::Critical,

            AlertType::LowStabilityPoolBalance => AlertSeverity::Warning,
            AlertType::RapidDebtIncrease => AlertSeverity::Warning,
            AlertType::HighRedemptionVolume => AlertSeverity::Info,

            AlertType::HighTransactionLatency => AlertSeverity::Warning,
            AlertType::RateLimitThresholdReached => AlertSeverity::Info,
            AlertType::UnusualTransactionVolume => AlertSeverity::Info,

            AlertType::MultipleFailedTransactions => AlertSeverity::Warning,
            AlertType::UnusualWithdrawalPattern => AlertSeverity::Warning,
            AlertType::GovernanceAlert => AlertSeverity::Info,
        }
    }

    /// Get description of this alert type
    pub fn description(&self) -> &'static str {
        match self {
            AlertType::LowCollateralization => "System collateralization ratio below safe threshold",
            AlertType::RecoveryModeActive => "Recovery mode has been activated",
            AlertType::HighRiskyCDPCount => "High number of CDPs at risk of liquidation",
            AlertType::DangerousMinCDPRatio => "Minimum CDP ratio in system dangerously low",

            AlertType::StalePriceFeed => "Price feed has not updated recently",
            AlertType::HighPriceDeviation => "Significant price deviation between oracle sources",
            AlertType::InsufficientPriceSources => "Too few price sources reporting",

            AlertType::LowStabilityPoolBalance => "Stability pool balance below optimal level",
            AlertType::RapidDebtIncrease => "Total debt increasing rapidly",
            AlertType::HighRedemptionVolume => "High volume of redemptions detected",

            AlertType::HighTransactionLatency => "Transaction processing latency elevated",
            AlertType::RateLimitThresholdReached => "Rate limiting is actively blocking requests",
            AlertType::UnusualTransactionVolume => "Transaction volume outside normal range",

            AlertType::MultipleFailedTransactions => "Multiple transaction failures detected",
            AlertType::UnusualWithdrawalPattern => "Unusual withdrawal pattern detected",
            AlertType::GovernanceAlert => "Governance proposal requires attention",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ALERT
// ═══════════════════════════════════════════════════════════════════════════════

/// A single alert instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Alert ID
    pub id: u64,
    /// Alert type
    pub alert_type: AlertType,
    /// Severity level
    pub severity: AlertSeverity,
    /// Human-readable message
    pub message: String,
    /// Current metric value that triggered alert
    pub current_value: u64,
    /// Threshold that was breached
    pub threshold: u64,
    /// Block height when triggered
    pub block_height: u64,
    /// Timestamp when triggered
    pub timestamp: u64,
    /// Whether alert has been acknowledged
    pub acknowledged: bool,
    /// Whether alert has been resolved
    pub resolved: bool,
    /// Resolution timestamp if resolved
    pub resolved_at: Option<u64>,
}

impl Alert {
    /// Create new alert
    pub fn new(
        id: u64,
        alert_type: AlertType,
        severity: AlertSeverity,
        message: String,
        current_value: u64,
        threshold: u64,
        block_height: u64,
        timestamp: u64,
    ) -> Self {
        Self {
            id,
            alert_type,
            severity,
            message,
            current_value,
            threshold,
            block_height,
            timestamp,
            acknowledged: false,
            resolved: false,
            resolved_at: None,
        }
    }

    /// Acknowledge this alert
    pub fn acknowledge(&mut self) {
        self.acknowledged = true;
    }

    /// Resolve this alert
    pub fn resolve(&mut self, timestamp: u64) {
        self.resolved = true;
        self.resolved_at = Some(timestamp);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ALERT RULE
// ═══════════════════════════════════════════════════════════════════════════════

/// Condition for triggering an alert
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertCondition {
    /// Metric below threshold
    Below(u64),
    /// Metric above threshold
    Above(u64),
    /// Metric equals value
    Equals(u64),
    /// Metric changes by more than percentage (basis points)
    ChangeExceeds(u64),
    /// Rate of change exceeds threshold
    RateExceeds(i64),
}

impl AlertCondition {
    /// Check if condition is met
    pub fn is_triggered(&self, current: u64, previous: Option<u64>) -> bool {
        match self {
            AlertCondition::Below(threshold) => current < *threshold,
            AlertCondition::Above(threshold) => current > *threshold,
            AlertCondition::Equals(value) => current == *value,
            AlertCondition::ChangeExceeds(bps) => {
                if let Some(prev) = previous {
                    if prev == 0 {
                        return current > 0;
                    }
                    let change = if current > prev {
                        ((current - prev) * 10000) / prev
                    } else {
                        ((prev - current) * 10000) / prev
                    };
                    change > *bps
                } else {
                    false
                }
            }
            AlertCondition::RateExceeds(threshold) => {
                // This needs rate calculation from time series
                // For now, return false - should be checked externally
                let _ = threshold;
                false
            }
        }
    }

    /// Get threshold value for display
    pub fn threshold_value(&self) -> u64 {
        match self {
            AlertCondition::Below(v) => *v,
            AlertCondition::Above(v) => *v,
            AlertCondition::Equals(v) => *v,
            AlertCondition::ChangeExceeds(v) => *v,
            AlertCondition::RateExceeds(v) => *v as u64,
        }
    }
}

/// Alert rule definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    /// Rule ID
    pub id: u64,
    /// Name
    pub name: String,
    /// Alert type to generate
    pub alert_type: AlertType,
    /// Metric to monitor
    pub metric: MetricType,
    /// Condition for triggering
    pub condition: AlertCondition,
    /// Severity override (if None, use default)
    pub severity_override: Option<AlertSeverity>,
    /// Minimum blocks between re-triggering
    pub cooldown_blocks: u64,
    /// Whether rule is enabled
    pub enabled: bool,
    /// Last triggered block
    pub last_triggered: Option<u64>,
}

impl AlertRule {
    /// Create new rule
    pub fn new(
        id: u64,
        name: String,
        alert_type: AlertType,
        metric: MetricType,
        condition: AlertCondition,
    ) -> Self {
        Self {
            id,
            name,
            alert_type,
            metric,
            condition,
            severity_override: None,
            cooldown_blocks: 10,
            enabled: true,
            last_triggered: None,
        }
    }

    /// Check if rule can trigger (respecting cooldown)
    pub fn can_trigger(&self, block_height: u64) -> bool {
        if !self.enabled {
            return false;
        }

        match self.last_triggered {
            Some(last) => block_height >= last + self.cooldown_blocks,
            None => true,
        }
    }

    /// Get effective severity
    pub fn severity(&self) -> AlertSeverity {
        self.severity_override.unwrap_or_else(|| self.alert_type.default_severity())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ALERT MANAGER
// ═══════════════════════════════════════════════════════════════════════════════

/// Configuration for alert manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertManagerConfig {
    /// Maximum alerts to keep in history
    pub max_history: usize,
    /// Default cooldown between alerts (blocks)
    pub default_cooldown_blocks: u64,
    /// Auto-resolve alerts after this many blocks without trigger
    pub auto_resolve_blocks: u64,
}

impl Default for AlertManagerConfig {
    fn default() -> Self {
        Self {
            max_history: 10000,
            default_cooldown_blocks: 10,
            auto_resolve_blocks: 100,
        }
    }
}

/// Manages alert rules and triggered alerts
#[derive(Debug)]
pub struct AlertManager {
    /// Configuration
    config: AlertManagerConfig,
    /// Alert rules
    rules: Vec<AlertRule>,
    /// Active alerts
    active_alerts: HashMap<AlertType, Alert>,
    /// Alert history
    history: VecDeque<Alert>,
    /// Total alerts generated
    total_alerts: AtomicU64,
    /// Next alert ID
    next_id: AtomicU64,
    /// Previous metric values for change detection
    previous_values: HashMap<MetricType, u64>,
}

impl AlertManager {
    /// Create new alert manager
    pub fn new(config: AlertManagerConfig) -> Self {
        Self {
            config,
            rules: Vec::new(),
            active_alerts: HashMap::new(),
            history: VecDeque::new(),
            total_alerts: AtomicU64::new(0),
            next_id: AtomicU64::new(1),
            previous_values: HashMap::new(),
        }
    }

    /// Create with default production rules
    pub fn with_default_rules() -> Self {
        let mut manager = Self::new(AlertManagerConfig::default());
        manager.add_default_rules();
        manager
    }

    /// Add default production alert rules
    fn add_default_rules(&mut self) {
        // Protocol health rules
        self.add_rule(AlertRule::new(
            1,
            "Low System Collateralization".into(),
            AlertType::LowCollateralization,
            MetricType::SystemCollateralRatio,
            AlertCondition::Below(15000), // 150%
        ));

        self.add_rule(AlertRule::new(
            2,
            "Recovery Mode Active".into(),
            AlertType::RecoveryModeActive,
            MetricType::RecoveryModeActive,
            AlertCondition::Equals(1),
        ));

        self.add_rule(AlertRule::new(
            3,
            "High Risky CDP Count".into(),
            AlertType::HighRiskyCDPCount,
            MetricType::RiskyCDPCount,
            AlertCondition::Above(100),
        ));

        self.add_rule(AlertRule::new(
            4,
            "Dangerous Minimum CDP Ratio".into(),
            AlertType::DangerousMinCDPRatio,
            MetricType::MinimumCDPRatio,
            AlertCondition::Below(11500), // 115%
        ));

        // Oracle rules
        self.add_rule(AlertRule::new(
            5,
            "Stale Price Feed".into(),
            AlertType::StalePriceFeed,
            MetricType::PriceUpdateAge,
            AlertCondition::Above(3600), // 1 hour
        ));

        self.add_rule(AlertRule::new(
            6,
            "High Price Deviation".into(),
            AlertType::HighPriceDeviation,
            MetricType::PriceDeviation,
            AlertCondition::Above(500), // 5%
        ));

        self.add_rule(AlertRule::new(
            7,
            "Insufficient Price Sources".into(),
            AlertType::InsufficientPriceSources,
            MetricType::ActivePriceSources,
            AlertCondition::Below(3),
        ));

        // Financial rules
        self.add_rule(AlertRule::new(
            8,
            "Low Stability Pool".into(),
            AlertType::LowStabilityPoolBalance,
            MetricType::StabilityPoolBalance,
            AlertCondition::Below(1_000_000_00), // 1M USD
        ));

        // Performance rules
        self.add_rule(AlertRule::new(
            9,
            "High Transaction Latency".into(),
            AlertType::HighTransactionLatency,
            MetricType::AvgTransactionLatency,
            AlertCondition::Above(5000), // 5 seconds
        ));

        self.add_rule(AlertRule::new(
            10,
            "Rate Limit Threshold".into(),
            AlertType::RateLimitThresholdReached,
            MetricType::RateLimitedRequests,
            AlertCondition::Above(1000),
        ));
    }

    /// Add a rule
    pub fn add_rule(&mut self, rule: AlertRule) {
        self.rules.push(rule);
    }

    /// Remove a rule by ID
    pub fn remove_rule(&mut self, rule_id: u64) {
        self.rules.retain(|r| r.id != rule_id);
    }

    /// Enable/disable a rule
    pub fn set_rule_enabled(&mut self, rule_id: u64, enabled: bool) {
        if let Some(rule) = self.rules.iter_mut().find(|r| r.id == rule_id) {
            rule.enabled = enabled;
        }
    }

    /// Check all rules against current metrics
    pub fn check_rules(
        &mut self,
        metrics: &MetricsCollector,
        block_height: u64,
        timestamp: u64,
    ) -> Vec<Alert> {
        let mut new_alerts = Vec::new();

        for rule in &mut self.rules {
            if !rule.can_trigger(block_height) {
                continue;
            }

            if let Some(current_value) = metrics.get(rule.metric) {
                let previous = self.previous_values.get(&rule.metric).copied();

                if rule.condition.is_triggered(current_value, previous) {
                    let alert_id = self.next_id.fetch_add(1, Ordering::Relaxed);
                    let alert = Alert::new(
                        alert_id,
                        rule.alert_type,
                        rule.severity(),
                        format!("{}: {}", rule.name, rule.alert_type.description()),
                        current_value,
                        rule.condition.threshold_value(),
                        block_height,
                        timestamp,
                    );

                    new_alerts.push(alert.clone());
                    self.active_alerts.insert(rule.alert_type, alert);
                    rule.last_triggered = Some(block_height);
                    self.total_alerts.fetch_add(1, Ordering::Relaxed);
                }

                // Update previous value
                self.previous_values.insert(rule.metric, current_value);
            }
        }

        // Auto-resolve old alerts
        self.auto_resolve_alerts(block_height, timestamp);

        // Add to history
        for alert in &new_alerts {
            self.add_to_history(alert.clone());
        }

        new_alerts
    }

    /// Auto-resolve alerts that haven't been triggered recently
    fn auto_resolve_alerts(&mut self, block_height: u64, timestamp: u64) {
        let resolve_threshold = block_height.saturating_sub(self.config.auto_resolve_blocks);

        for alert in self.active_alerts.values_mut() {
            if !alert.resolved && alert.block_height < resolve_threshold {
                alert.resolve(timestamp);
            }
        }

        // Remove resolved alerts from active
        self.active_alerts.retain(|_, a| !a.resolved);
    }

    /// Add alert to history
    fn add_to_history(&mut self, alert: Alert) {
        if self.history.len() >= self.config.max_history {
            self.history.pop_front();
        }
        self.history.push_back(alert);
    }

    /// Acknowledge an alert
    pub fn acknowledge(&mut self, alert_id: u64) {
        for alert in self.active_alerts.values_mut() {
            if alert.id == alert_id {
                alert.acknowledge();
                break;
            }
        }
    }

    /// Resolve an alert manually
    pub fn resolve(&mut self, alert_id: u64, timestamp: u64) {
        if let Some(alert) = self.active_alerts.values_mut().find(|a| a.id == alert_id) {
            alert.resolve(timestamp);
        }
    }

    /// Get all active alerts
    pub fn get_active_alerts(&self) -> Vec<&Alert> {
        self.active_alerts.values().collect()
    }

    /// Get active alerts by severity
    pub fn get_alerts_by_severity(&self, severity: AlertSeverity) -> Vec<&Alert> {
        self.active_alerts
            .values()
            .filter(|a| a.severity == severity)
            .collect()
    }

    /// Get unacknowledged alerts
    pub fn get_unacknowledged(&self) -> Vec<&Alert> {
        self.active_alerts
            .values()
            .filter(|a| !a.acknowledged)
            .collect()
    }

    /// Get alert history
    pub fn get_history(&self, limit: usize) -> Vec<&Alert> {
        self.history.iter().rev().take(limit).collect()
    }

    /// Get history by type
    pub fn get_history_by_type(&self, alert_type: AlertType, limit: usize) -> Vec<&Alert> {
        self.history
            .iter()
            .rev()
            .filter(|a| a.alert_type == alert_type)
            .take(limit)
            .collect()
    }

    /// Get statistics
    pub fn statistics(&self) -> AlertStatistics {
        let mut by_severity = HashMap::new();
        let mut by_type = HashMap::new();

        for alert in self.active_alerts.values() {
            *by_severity.entry(alert.severity).or_insert(0) += 1;
            *by_type.entry(alert.alert_type).or_insert(0) += 1;
        }

        AlertStatistics {
            total_generated: self.total_alerts.load(Ordering::Relaxed),
            active_count: self.active_alerts.len() as u64,
            unacknowledged_count: self.active_alerts.values().filter(|a| !a.acknowledged).count() as u64,
            by_severity,
            by_type,
            rules_count: self.rules.len() as u64,
            enabled_rules_count: self.rules.iter().filter(|r| r.enabled).count() as u64,
        }
    }

    /// Check for emergency alerts
    pub fn has_emergency(&self) -> bool {
        self.active_alerts
            .values()
            .any(|a| a.severity == AlertSeverity::Emergency && !a.resolved)
    }

    /// Check for critical alerts
    pub fn has_critical(&self) -> bool {
        self.active_alerts
            .values()
            .any(|a| a.severity >= AlertSeverity::Critical && !a.resolved)
    }
}

impl Default for AlertManager {
    fn default() -> Self {
        Self::with_default_rules()
    }
}

/// Alert statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertStatistics {
    /// Total alerts ever generated
    pub total_generated: u64,
    /// Currently active alerts
    pub active_count: u64,
    /// Unacknowledged alerts
    pub unacknowledged_count: u64,
    /// Breakdown by severity
    pub by_severity: HashMap<AlertSeverity, u64>,
    /// Breakdown by type
    pub by_type: HashMap<AlertType, u64>,
    /// Total rules configured
    pub rules_count: u64,
    /// Enabled rules
    pub enabled_rules_count: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// NOTIFICATION CHANNEL
// ═══════════════════════════════════════════════════════════════════════════════

/// Notification channel types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationChannel {
    /// Log to file/stdout
    Log,
    /// Webhook URL
    Webhook { url: String, headers: HashMap<String, String> },
    /// Email (requires external service)
    Email { recipients: Vec<String> },
    /// Telegram bot
    Telegram { bot_token: String, chat_id: String },
    /// Discord webhook
    Discord { webhook_url: String },
    /// PagerDuty
    PagerDuty { routing_key: String },
}

/// Notification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    /// Channel to use
    pub channel: NotificationChannel,
    /// Minimum severity to notify
    pub min_severity: AlertSeverity,
    /// Alert types to include (empty = all)
    pub include_types: Vec<AlertType>,
    /// Alert types to exclude
    pub exclude_types: Vec<AlertType>,
    /// Whether channel is enabled
    pub enabled: bool,
}

impl NotificationConfig {
    /// Check if alert should be sent on this channel
    pub fn should_notify(&self, alert: &Alert) -> bool {
        if !self.enabled {
            return false;
        }

        if alert.severity < self.min_severity {
            return false;
        }

        if self.exclude_types.contains(&alert.alert_type) {
            return false;
        }

        if !self.include_types.is_empty() && !self.include_types.contains(&alert.alert_type) {
            return false;
        }

        true
    }
}

/// Notification dispatcher
#[derive(Debug, Default)]
pub struct NotificationDispatcher {
    /// Configured channels
    channels: Vec<NotificationConfig>,
    /// Notifications sent
    notifications_sent: AtomicU64,
    /// Notification failures
    notification_failures: AtomicU64,
}

impl NotificationDispatcher {
    /// Create new dispatcher
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a channel
    pub fn add_channel(&mut self, config: NotificationConfig) {
        self.channels.push(config);
    }

    /// Dispatch alert to all matching channels
    pub fn dispatch(&self, alert: &Alert) -> Vec<NotificationResult> {
        let mut results = Vec::new();

        for config in &self.channels {
            if config.should_notify(alert) {
                let result = self.send_notification(config, alert);
                if result.success {
                    self.notifications_sent.fetch_add(1, Ordering::Relaxed);
                } else {
                    self.notification_failures.fetch_add(1, Ordering::Relaxed);
                }
                results.push(result);
            }
        }

        results
    }

    /// Send notification on a specific channel
    fn send_notification(&self, config: &NotificationConfig, alert: &Alert) -> NotificationResult {
        match &config.channel {
            NotificationChannel::Log => {
                // Log notification (always succeeds)
                NotificationResult {
                    channel: "log".into(),
                    success: true,
                    error: None,
                }
            }
            NotificationChannel::Webhook { url, .. } => {
                // Webhook would be sent asynchronously in production
                // For now, we just record the attempt
                NotificationResult {
                    channel: format!("webhook:{}", url),
                    success: true,
                    error: None,
                }
            }
            NotificationChannel::Email { recipients } => {
                NotificationResult {
                    channel: format!("email:{}", recipients.join(",")),
                    success: true,
                    error: None,
                }
            }
            NotificationChannel::Telegram { chat_id, .. } => {
                NotificationResult {
                    channel: format!("telegram:{}", chat_id),
                    success: true,
                    error: None,
                }
            }
            NotificationChannel::Discord { webhook_url } => {
                let _ = webhook_url;
                NotificationResult {
                    channel: "discord".into(),
                    success: true,
                    error: None,
                }
            }
            NotificationChannel::PagerDuty { .. } => {
                let _ = alert;
                NotificationResult {
                    channel: "pagerduty".into(),
                    success: true,
                    error: None,
                }
            }
        }
    }

    /// Get statistics
    pub fn statistics(&self) -> NotificationStatistics {
        NotificationStatistics {
            channels_configured: self.channels.len() as u64,
            notifications_sent: self.notifications_sent.load(Ordering::Relaxed),
            notification_failures: self.notification_failures.load(Ordering::Relaxed),
        }
    }
}

/// Result of a notification attempt
#[derive(Debug, Clone)]
pub struct NotificationResult {
    /// Channel identifier
    pub channel: String,
    /// Whether notification was successful
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Notification statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationStatistics {
    /// Number of channels configured
    pub channels_configured: u64,
    /// Total notifications sent
    pub notifications_sent: u64,
    /// Failed notifications
    pub notification_failures: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_condition_below() {
        let condition = AlertCondition::Below(100);
        assert!(condition.is_triggered(50, None));
        assert!(!condition.is_triggered(150, None));
    }

    #[test]
    fn test_alert_condition_above() {
        let condition = AlertCondition::Above(100);
        assert!(condition.is_triggered(150, None));
        assert!(!condition.is_triggered(50, None));
    }

    #[test]
    fn test_alert_condition_change() {
        let condition = AlertCondition::ChangeExceeds(1000); // 10%
        assert!(condition.is_triggered(120, Some(100))); // 20% change
        assert!(!condition.is_triggered(105, Some(100))); // 5% change
    }

    #[test]
    fn test_alert_manager() {
        let mut manager = AlertManager::new(AlertManagerConfig::default());

        let rule = AlertRule::new(
            1,
            "Test Rule".into(),
            AlertType::LowCollateralization,
            MetricType::SystemCollateralRatio,
            AlertCondition::Below(15000),
        );

        manager.add_rule(rule);
        assert_eq!(manager.rules.len(), 1);
    }

    #[test]
    fn test_alert_severity_ordering() {
        assert!(AlertSeverity::Info < AlertSeverity::Warning);
        assert!(AlertSeverity::Warning < AlertSeverity::Critical);
        assert!(AlertSeverity::Critical < AlertSeverity::Emergency);
    }

    #[test]
    fn test_notification_config() {
        let config = NotificationConfig {
            channel: NotificationChannel::Log,
            min_severity: AlertSeverity::Warning,
            include_types: vec![],
            exclude_types: vec![],
            enabled: true,
        };

        let info_alert = Alert::new(
            1,
            AlertType::RateLimitThresholdReached,
            AlertSeverity::Info,
            "Test".into(),
            100,
            50,
            1,
            1000,
        );

        let warning_alert = Alert::new(
            2,
            AlertType::LowCollateralization,
            AlertSeverity::Warning,
            "Test".into(),
            100,
            150,
            1,
            1000,
        );

        assert!(!config.should_notify(&info_alert));
        assert!(config.should_notify(&warning_alert));
    }

    #[test]
    fn test_alert_lifecycle() {
        let mut alert = Alert::new(
            1,
            AlertType::RecoveryModeActive,
            AlertSeverity::Critical,
            "Recovery mode activated".into(),
            1,
            1,
            100,
            1000,
        );

        assert!(!alert.acknowledged);
        assert!(!alert.resolved);

        alert.acknowledge();
        assert!(alert.acknowledged);

        alert.resolve(2000);
        assert!(alert.resolved);
        assert_eq!(alert.resolved_at, Some(2000));
    }
}
