//! Rate Limiting for RPC Server.
//!
//! Implements production-grade rate limiting using token bucket algorithm
//! with support for IP-based and API key-based limits.
//!
//! # Features
//!
//! - Token bucket algorithm for smooth rate limiting
//! - Per-IP and per-API-key limits
//! - Burst allowance for traffic spikes
//! - Automatic cleanup of stale entries
//! - Configurable limits for different endpoints
//! - DDoS protection with connection tracking

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// CONFIGURATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Rate limiter configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimiterConfig {
    /// Requests per second for unauthenticated users
    pub default_rps: u32,
    /// Burst allowance (max tokens)
    pub default_burst: u32,
    /// Requests per second for authenticated users (API key)
    pub authenticated_rps: u32,
    /// Burst for authenticated users
    pub authenticated_burst: u32,
    /// Cleanup interval in seconds
    pub cleanup_interval_secs: u64,
    /// Entry TTL (time-to-live) in seconds
    pub entry_ttl_secs: u64,
    /// Maximum concurrent connections per IP
    pub max_connections_per_ip: u32,
    /// Global maximum connections
    pub max_global_connections: u32,
    /// Enable IP whitelisting
    pub enable_whitelist: bool,
    /// Enable IP blacklisting
    pub enable_blacklist: bool,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            default_rps: 10,
            default_burst: 20,
            authenticated_rps: 100,
            authenticated_burst: 200,
            cleanup_interval_secs: 60,
            entry_ttl_secs: 300,
            max_connections_per_ip: 100,
            max_global_connections: 10000,
            enable_whitelist: true,
            enable_blacklist: true,
        }
    }
}

impl RateLimiterConfig {
    /// Create config optimized for high traffic
    pub fn high_traffic() -> Self {
        Self {
            default_rps: 50,
            default_burst: 100,
            authenticated_rps: 500,
            authenticated_burst: 1000,
            cleanup_interval_secs: 30,
            entry_ttl_secs: 180,
            max_connections_per_ip: 500,
            max_global_connections: 50000,
            ..Default::default()
        }
    }

    /// Create config for development/testing
    pub fn development() -> Self {
        Self {
            default_rps: 1000,
            default_burst: 2000,
            authenticated_rps: 5000,
            authenticated_burst: 10000,
            cleanup_interval_secs: 300,
            entry_ttl_secs: 600,
            max_connections_per_ip: 1000,
            max_global_connections: 100000,
            enable_whitelist: false,
            enable_blacklist: false,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TOKEN BUCKET
// ═══════════════════════════════════════════════════════════════════════════════

/// Token bucket for rate limiting
#[derive(Debug)]
pub struct TokenBucket {
    /// Current tokens available
    tokens: f64,
    /// Maximum tokens (burst capacity)
    max_tokens: f64,
    /// Refill rate (tokens per second)
    refill_rate: f64,
    /// Last update time
    last_update: Instant,
}

impl TokenBucket {
    /// Create new token bucket
    pub fn new(tokens_per_second: f64, burst: f64) -> Self {
        Self {
            tokens: burst,
            max_tokens: burst,
            refill_rate: tokens_per_second,
            last_update: Instant::now(),
        }
    }

    /// Try to consume tokens, returns true if successful
    pub fn try_consume(&mut self, tokens: f64) -> bool {
        self.refill();

        if self.tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }

    /// Refill tokens based on elapsed time
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update).as_secs_f64();

        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_update = now;
    }

    /// Get current token count
    pub fn available_tokens(&mut self) -> f64 {
        self.refill();
        self.tokens
    }

    /// Get time until n tokens are available
    pub fn time_until_available(&mut self, tokens: f64) -> Duration {
        self.refill();

        if self.tokens >= tokens {
            Duration::ZERO
        } else {
            let needed = tokens - self.tokens;
            Duration::from_secs_f64(needed / self.refill_rate)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RATE LIMIT ENTRY
// ═══════════════════════════════════════════════════════════════════════════════

/// Entry for a rate-limited client
#[derive(Debug)]
struct RateLimitEntry {
    /// Token bucket for this client
    bucket: TokenBucket,
    /// Last activity time
    last_seen: Instant,
    /// Total requests made
    total_requests: u64,
    /// Rejected requests
    rejected_requests: u64,
    /// Active connections
    active_connections: u32,
}

impl RateLimitEntry {
    fn new(rps: f64, burst: f64) -> Self {
        Self {
            bucket: TokenBucket::new(rps, burst),
            last_seen: Instant::now(),
            total_requests: 0,
            rejected_requests: 0,
            active_connections: 0,
        }
    }

    fn try_request(&mut self) -> bool {
        self.last_seen = Instant::now();
        self.total_requests += 1;

        if self.bucket.try_consume(1.0) {
            true
        } else {
            self.rejected_requests += 1;
            false
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RATE LIMITER
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of rate limit check
#[derive(Debug, Clone)]
pub enum RateLimitResult {
    /// Request allowed
    Allowed,
    /// Request rate limited
    RateLimited {
        /// Time to wait before retry
        retry_after: Duration,
    },
    /// IP is blacklisted
    Blacklisted,
    /// Too many connections
    TooManyConnections,
}

impl RateLimitResult {
    /// Check if request is allowed
    pub fn is_allowed(&self) -> bool {
        matches!(self, RateLimitResult::Allowed)
    }
}

/// Main rate limiter
pub struct RateLimiter {
    /// Configuration
    config: RateLimiterConfig,
    /// Per-IP rate limits
    ip_limits: RwLock<HashMap<IpAddr, RateLimitEntry>>,
    /// Per-API-key rate limits
    key_limits: RwLock<HashMap<String, RateLimitEntry>>,
    /// Whitelisted IPs (exempt from rate limiting)
    whitelist: RwLock<Vec<IpAddr>>,
    /// Blacklisted IPs (blocked entirely)
    blacklist: RwLock<Vec<IpAddr>>,
    /// Global connection count
    global_connections: AtomicU64,
    /// Statistics
    stats: RateLimiterStats,
}

impl RateLimiter {
    /// Create new rate limiter
    pub fn new(config: RateLimiterConfig) -> Self {
        Self {
            config,
            ip_limits: RwLock::new(HashMap::new()),
            key_limits: RwLock::new(HashMap::new()),
            whitelist: RwLock::new(Vec::new()),
            blacklist: RwLock::new(Vec::new()),
            global_connections: AtomicU64::new(0),
            stats: RateLimiterStats::new(),
        }
    }

    /// Create with default config
    pub fn default_config() -> Self {
        Self::new(RateLimiterConfig::default())
    }

    /// Check if IP is allowed to make request
    pub fn check_ip(&self, ip: IpAddr) -> RateLimitResult {
        // Check whitelist
        if self.config.enable_whitelist && self.is_whitelisted(&ip) {
            self.stats.record_allowed();
            return RateLimitResult::Allowed;
        }

        // Check blacklist
        if self.config.enable_blacklist && self.is_blacklisted(&ip) {
            self.stats.record_blacklisted();
            return RateLimitResult::Blacklisted;
        }

        // Check global connection limit
        if self.global_connections.load(Ordering::Relaxed) as u32 >= self.config.max_global_connections {
            self.stats.record_connection_limit();
            return RateLimitResult::TooManyConnections;
        }

        // Get or create entry
        let mut limits = self.ip_limits.write().unwrap();
        let entry = limits.entry(ip).or_insert_with(|| {
            RateLimitEntry::new(
                self.config.default_rps as f64,
                self.config.default_burst as f64,
            )
        });

        // Check connection limit per IP
        if entry.active_connections >= self.config.max_connections_per_ip {
            self.stats.record_connection_limit();
            return RateLimitResult::TooManyConnections;
        }

        // Try to consume token
        if entry.try_request() {
            self.stats.record_allowed();
            RateLimitResult::Allowed
        } else {
            let retry_after = entry.bucket.time_until_available(1.0);
            self.stats.record_rate_limited();
            RateLimitResult::RateLimited { retry_after }
        }
    }

    /// Check if API key is allowed to make request
    pub fn check_api_key(&self, api_key: &str, ip: IpAddr) -> RateLimitResult {
        // Still check blacklist for the IP
        if self.config.enable_blacklist && self.is_blacklisted(&ip) {
            self.stats.record_blacklisted();
            return RateLimitResult::Blacklisted;
        }

        let mut limits = self.key_limits.write().unwrap();
        let entry = limits.entry(api_key.to_string()).or_insert_with(|| {
            RateLimitEntry::new(
                self.config.authenticated_rps as f64,
                self.config.authenticated_burst as f64,
            )
        });

        if entry.try_request() {
            self.stats.record_allowed();
            RateLimitResult::Allowed
        } else {
            let retry_after = entry.bucket.time_until_available(1.0);
            self.stats.record_rate_limited();
            RateLimitResult::RateLimited { retry_after }
        }
    }

    /// Register connection open
    pub fn connection_opened(&self, ip: IpAddr) {
        self.global_connections.fetch_add(1, Ordering::Relaxed);

        let mut limits = self.ip_limits.write().unwrap();
        if let Some(entry) = limits.get_mut(&ip) {
            entry.active_connections = entry.active_connections.saturating_add(1);
        }
    }

    /// Register connection closed
    pub fn connection_closed(&self, ip: IpAddr) {
        self.global_connections.fetch_sub(1, Ordering::Relaxed);

        let mut limits = self.ip_limits.write().unwrap();
        if let Some(entry) = limits.get_mut(&ip) {
            entry.active_connections = entry.active_connections.saturating_sub(1);
        }
    }

    /// Add IP to whitelist
    pub fn whitelist_ip(&self, ip: IpAddr) {
        let mut whitelist = self.whitelist.write().unwrap();
        if !whitelist.contains(&ip) {
            whitelist.push(ip);
        }
    }

    /// Remove IP from whitelist
    pub fn unwhitelist_ip(&self, ip: &IpAddr) {
        let mut whitelist = self.whitelist.write().unwrap();
        whitelist.retain(|i| i != ip);
    }

    /// Add IP to blacklist
    pub fn blacklist_ip(&self, ip: IpAddr) {
        let mut blacklist = self.blacklist.write().unwrap();
        if !blacklist.contains(&ip) {
            blacklist.push(ip);
        }
    }

    /// Remove IP from blacklist
    pub fn unblacklist_ip(&self, ip: &IpAddr) {
        let mut blacklist = self.blacklist.write().unwrap();
        blacklist.retain(|i| i != ip);
    }

    /// Check if IP is whitelisted
    pub fn is_whitelisted(&self, ip: &IpAddr) -> bool {
        self.whitelist.read().unwrap().contains(ip)
    }

    /// Check if IP is blacklisted
    pub fn is_blacklisted(&self, ip: &IpAddr) -> bool {
        self.blacklist.read().unwrap().contains(ip)
    }

    /// Cleanup stale entries
    pub fn cleanup(&self) {
        let ttl = Duration::from_secs(self.config.entry_ttl_secs);
        let now = Instant::now();

        // Cleanup IP limits
        {
            let mut limits = self.ip_limits.write().unwrap();
            limits.retain(|_, entry| {
                now.duration_since(entry.last_seen) < ttl
            });
        }

        // Cleanup API key limits
        {
            let mut limits = self.key_limits.write().unwrap();
            limits.retain(|_, entry| {
                now.duration_since(entry.last_seen) < ttl
            });
        }

        self.stats.record_cleanup();
    }

    /// Get statistics
    pub fn statistics(&self) -> RateLimiterStatistics {
        RateLimiterStatistics {
            total_requests: self.stats.total_requests.load(Ordering::Relaxed),
            allowed_requests: self.stats.allowed_requests.load(Ordering::Relaxed),
            rate_limited_requests: self.stats.rate_limited.load(Ordering::Relaxed),
            blacklisted_requests: self.stats.blacklisted.load(Ordering::Relaxed),
            connection_limited: self.stats.connection_limited.load(Ordering::Relaxed),
            active_ips: self.ip_limits.read().unwrap().len(),
            active_api_keys: self.key_limits.read().unwrap().len(),
            global_connections: self.global_connections.load(Ordering::Relaxed),
            whitelist_size: self.whitelist.read().unwrap().len(),
            blacklist_size: self.blacklist.read().unwrap().len(),
            cleanups: self.stats.cleanups.load(Ordering::Relaxed),
        }
    }

    /// Get entry for IP (for monitoring)
    pub fn get_ip_info(&self, ip: &IpAddr) -> Option<IpRateLimitInfo> {
        let limits = self.ip_limits.read().unwrap();
        limits.get(ip).map(|e| IpRateLimitInfo {
            total_requests: e.total_requests,
            rejected_requests: e.rejected_requests,
            active_connections: e.active_connections,
            last_seen_secs_ago: e.last_seen.elapsed().as_secs(),
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// STATISTICS
// ═══════════════════════════════════════════════════════════════════════════════

/// Internal stats tracking
struct RateLimiterStats {
    total_requests: AtomicU64,
    allowed_requests: AtomicU64,
    rate_limited: AtomicU64,
    blacklisted: AtomicU64,
    connection_limited: AtomicU64,
    cleanups: AtomicU64,
}

impl RateLimiterStats {
    fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            allowed_requests: AtomicU64::new(0),
            rate_limited: AtomicU64::new(0),
            blacklisted: AtomicU64::new(0),
            connection_limited: AtomicU64::new(0),
            cleanups: AtomicU64::new(0),
        }
    }

    fn record_allowed(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.allowed_requests.fetch_add(1, Ordering::Relaxed);
    }

    fn record_rate_limited(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.rate_limited.fetch_add(1, Ordering::Relaxed);
    }

    fn record_blacklisted(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.blacklisted.fetch_add(1, Ordering::Relaxed);
    }

    fn record_connection_limit(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.connection_limited.fetch_add(1, Ordering::Relaxed);
    }

    fn record_cleanup(&self) {
        self.cleanups.fetch_add(1, Ordering::Relaxed);
    }
}

/// Exported statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimiterStatistics {
    /// Total requests processed
    pub total_requests: u64,
    /// Requests allowed
    pub allowed_requests: u64,
    /// Requests rate limited
    pub rate_limited_requests: u64,
    /// Requests from blacklisted IPs
    pub blacklisted_requests: u64,
    /// Requests rejected due to connection limit
    pub connection_limited: u64,
    /// Number of active IP entries
    pub active_ips: usize,
    /// Number of active API key entries
    pub active_api_keys: usize,
    /// Current global connection count
    pub global_connections: u64,
    /// Whitelist size
    pub whitelist_size: usize,
    /// Blacklist size
    pub blacklist_size: usize,
    /// Number of cleanups performed
    pub cleanups: u64,
}

/// Information about a rate-limited IP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpRateLimitInfo {
    /// Total requests from this IP
    pub total_requests: u64,
    /// Rejected requests from this IP
    pub rejected_requests: u64,
    /// Active connections from this IP
    pub active_connections: u32,
    /// Seconds since last request
    pub last_seen_secs_ago: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ENDPOINT-SPECIFIC LIMITS
// ═══════════════════════════════════════════════════════════════════════════════

/// Rate limits for specific endpoints
#[derive(Debug, Clone)]
pub struct EndpointLimits {
    /// Limits per endpoint path
    limits: HashMap<String, (u32, u32)>, // (rps, burst)
}

impl Default for EndpointLimits {
    fn default() -> Self {
        let mut limits = HashMap::new();

        // Read-heavy endpoints (higher limits)
        limits.insert("/api/v1/price".into(), (100, 200));
        limits.insert("/api/v1/cdp/".into(), (50, 100));
        limits.insert("/api/v1/stats".into(), (50, 100));

        // Write endpoints (lower limits)
        limits.insert("/api/v1/cdp/open".into(), (5, 10));
        limits.insert("/api/v1/cdp/close".into(), (5, 10));
        limits.insert("/api/v1/cdp/deposit".into(), (10, 20));
        limits.insert("/api/v1/cdp/withdraw".into(), (10, 20));
        limits.insert("/api/v1/cdp/mint".into(), (10, 20));
        limits.insert("/api/v1/cdp/repay".into(), (10, 20));

        // Stability pool
        limits.insert("/api/v1/stability/deposit".into(), (5, 10));
        limits.insert("/api/v1/stability/withdraw".into(), (5, 10));

        // Governance (very restricted)
        limits.insert("/api/v1/governance/".into(), (2, 5));

        Self { limits }
    }
}

impl EndpointLimits {
    /// Get limits for an endpoint
    pub fn get(&self, path: &str) -> (u32, u32) {
        // Check for exact match first
        if let Some(&limits) = self.limits.get(path) {
            return limits;
        }

        // Check for prefix match
        for (prefix, &limits) in &self.limits {
            if path.starts_with(prefix) {
                return limits;
            }
        }

        // Default limits
        (10, 20)
    }

    /// Set limits for an endpoint
    pub fn set(&mut self, path: impl Into<String>, rps: u32, burst: u32) {
        self.limits.insert(path.into(), (rps, burst));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_token_bucket() {
        let mut bucket = TokenBucket::new(10.0, 20.0);

        // Should start with full bucket
        assert!(bucket.try_consume(15.0));

        // Should have 5 tokens left
        assert!(!bucket.try_consume(10.0));
        assert!(bucket.try_consume(5.0));
    }

    #[test]
    fn test_rate_limiter_allow() {
        let limiter = RateLimiter::new(RateLimiterConfig::development());
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        let result = limiter.check_ip(ip);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_rate_limiter_limit() {
        let config = RateLimiterConfig {
            default_rps: 1,
            default_burst: 2,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        // First two should pass (burst)
        assert!(limiter.check_ip(ip).is_allowed());
        assert!(limiter.check_ip(ip).is_allowed());

        // Third should be limited
        let result = limiter.check_ip(ip);
        assert!(matches!(result, RateLimitResult::RateLimited { .. }));
    }

    #[test]
    fn test_blacklist() {
        let limiter = RateLimiter::new(RateLimiterConfig::default());
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));

        limiter.blacklist_ip(ip);
        let result = limiter.check_ip(ip);
        assert!(matches!(result, RateLimitResult::Blacklisted));
    }

    #[test]
    fn test_whitelist() {
        let config = RateLimiterConfig {
            default_rps: 1,
            default_burst: 1,
            enable_whitelist: true,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1));

        limiter.whitelist_ip(ip);

        // Whitelisted IPs bypass rate limiting
        for _ in 0..100 {
            assert!(limiter.check_ip(ip).is_allowed());
        }
    }

    #[test]
    fn test_api_key_limits() {
        let limiter = RateLimiter::new(RateLimiterConfig::default());
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        let result = limiter.check_api_key("test-key-123", ip);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_statistics() {
        let limiter = RateLimiter::new(RateLimiterConfig::development());
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        limiter.check_ip(ip);
        limiter.check_ip(ip);

        let stats = limiter.statistics();
        assert_eq!(stats.total_requests, 2);
        assert_eq!(stats.allowed_requests, 2);
    }

    #[test]
    fn test_connection_tracking() {
        let limiter = RateLimiter::new(RateLimiterConfig::default());
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        limiter.connection_opened(ip);
        limiter.connection_opened(ip);

        let stats = limiter.statistics();
        assert_eq!(stats.global_connections, 2);

        limiter.connection_closed(ip);
        let stats = limiter.statistics();
        assert_eq!(stats.global_connections, 1);
    }
}
