//! Circuit Breaker Pattern Implementation.
//!
//! Provides fault tolerance for external dependencies like oracles, RPC endpoints, etc.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::Instant;

// ═══════════════════════════════════════════════════════════════════════════════
// CIRCUIT STATE
// ═══════════════════════════════════════════════════════════════════════════════

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitState {
    /// Circuit is closed (normal operation)
    Closed,
    /// Circuit is open (failing, rejecting requests)
    Open,
    /// Circuit is half-open (testing if service recovered)
    HalfOpen,
}

impl CircuitState {
    /// Check if requests are allowed
    pub fn allows_request(&self) -> bool {
        match self {
            CircuitState::Closed => true,
            CircuitState::Open => false,
            CircuitState::HalfOpen => true, // Allow probe requests
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CIRCUIT BREAKER CONFIG
// ═══════════════════════════════════════════════════════════════════════════════

/// Configuration for circuit breaker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit
    pub failure_threshold: u32,
    /// Number of successes in half-open to close
    pub success_threshold: u32,
    /// Time in milliseconds before trying half-open
    pub reset_timeout_ms: u64,
    /// Window size for counting failures (milliseconds)
    pub window_size_ms: u64,
    /// Minimum requests before evaluation
    pub min_requests: u32,
    /// Failure rate threshold (percentage, 0-100)
    pub failure_rate_threshold: u8,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            reset_timeout_ms: 30_000, // 30 seconds
            window_size_ms: 60_000,   // 1 minute
            min_requests: 10,
            failure_rate_threshold: 50, // 50%
        }
    }
}

impl CircuitBreakerConfig {
    /// Create strict config (opens quickly)
    pub fn strict() -> Self {
        Self {
            failure_threshold: 3,
            success_threshold: 5,
            reset_timeout_ms: 60_000, // 1 minute
            window_size_ms: 30_000,
            min_requests: 5,
            failure_rate_threshold: 30,
        }
    }

    /// Create relaxed config (more tolerant)
    pub fn relaxed() -> Self {
        Self {
            failure_threshold: 10,
            success_threshold: 2,
            reset_timeout_ms: 15_000, // 15 seconds
            window_size_ms: 120_000,  // 2 minutes
            min_requests: 20,
            failure_rate_threshold: 70,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CIRCUIT BREAKER
// ═══════════════════════════════════════════════════════════════════════════════

/// Circuit breaker for a single service
#[derive(Debug)]
pub struct CircuitBreaker {
    /// Name/identifier
    name: String,
    /// Configuration
    config: CircuitBreakerConfig,
    /// Current state
    state: RwLock<CircuitState>,
    /// Failure count in current window
    failures: AtomicU64,
    /// Success count (for half-open)
    successes: AtomicU64,
    /// Total requests in current window
    requests: AtomicU64,
    /// Last failure time
    last_failure: RwLock<Option<Instant>>,
    /// Time circuit was opened
    opened_at: RwLock<Option<Instant>>,
    /// Statistics
    stats: CircuitBreakerStats,
}

impl CircuitBreaker {
    /// Create new circuit breaker
    pub fn new(name: impl Into<String>, config: CircuitBreakerConfig) -> Self {
        Self {
            name: name.into(),
            config,
            state: RwLock::new(CircuitState::Closed),
            failures: AtomicU64::new(0),
            successes: AtomicU64::new(0),
            requests: AtomicU64::new(0),
            last_failure: RwLock::new(None),
            opened_at: RwLock::new(None),
            stats: CircuitBreakerStats::default(),
        }
    }

    /// Create with default config
    pub fn with_defaults(name: impl Into<String>) -> Self {
        Self::new(name, CircuitBreakerConfig::default())
    }

    /// Get name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get current state
    pub fn state(&self) -> CircuitState {
        *self.state.read().unwrap()
    }

    /// Check if request is allowed
    pub fn allow_request(&self) -> bool {
        self.check_state_transition();
        self.state().allows_request()
    }

    /// Try to execute with circuit breaker protection
    pub fn call<T, E, F>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: FnOnce() -> Result<T, E>,
    {
        if !self.allow_request() {
            self.stats.rejected.fetch_add(1, Ordering::Relaxed);
            return Err(CircuitBreakerError::CircuitOpen(self.name.clone()));
        }

        self.requests.fetch_add(1, Ordering::Relaxed);
        self.stats.total_requests.fetch_add(1, Ordering::Relaxed);

        match f() {
            Ok(result) => {
                self.record_success();
                Ok(result)
            }
            Err(e) => {
                self.record_failure();
                Err(CircuitBreakerError::ServiceError(e))
            }
        }
    }

    /// Record a success
    pub fn record_success(&self) {
        self.stats.successes.fetch_add(1, Ordering::Relaxed);
        self.successes.fetch_add(1, Ordering::Relaxed);

        let state = *self.state.read().unwrap();

        if state == CircuitState::HalfOpen {
            let successes = self.successes.load(Ordering::Relaxed);
            if successes >= self.config.success_threshold as u64 {
                self.close();
            }
        }
    }

    /// Record a failure
    pub fn record_failure(&self) {
        self.stats.failures.fetch_add(1, Ordering::Relaxed);
        self.failures.fetch_add(1, Ordering::Relaxed);

        *self.last_failure.write().unwrap() = Some(Instant::now());

        let state = *self.state.read().unwrap();

        match state {
            CircuitState::Closed => {
                let failures = self.failures.load(Ordering::Relaxed);
                let requests = self.requests.load(Ordering::Relaxed);

                // Check failure count threshold
                if failures >= self.config.failure_threshold as u64 {
                    self.open();
                    return;
                }

                // Check failure rate threshold
                if requests >= self.config.min_requests as u64 {
                    let rate = (failures * 100) / requests;
                    if rate >= self.config.failure_rate_threshold as u64 {
                        self.open();
                    }
                }
            }
            CircuitState::HalfOpen => {
                // Single failure in half-open reopens circuit
                self.open();
            }
            CircuitState::Open => {
                // Already open, nothing to do
            }
        }
    }

    /// Open the circuit
    fn open(&self) {
        *self.state.write().unwrap() = CircuitState::Open;
        *self.opened_at.write().unwrap() = Some(Instant::now());
        self.stats.times_opened.fetch_add(1, Ordering::Relaxed);
        self.successes.store(0, Ordering::Relaxed);
    }

    /// Close the circuit
    fn close(&self) {
        *self.state.write().unwrap() = CircuitState::Closed;
        *self.opened_at.write().unwrap() = None;
        self.failures.store(0, Ordering::Relaxed);
        self.successes.store(0, Ordering::Relaxed);
        self.requests.store(0, Ordering::Relaxed);
    }

    /// Transition to half-open
    fn half_open(&self) {
        *self.state.write().unwrap() = CircuitState::HalfOpen;
        self.successes.store(0, Ordering::Relaxed);
    }

    /// Check for state transitions based on time
    fn check_state_transition(&self) {
        let state = *self.state.read().unwrap();

        if state == CircuitState::Open {
            if let Some(opened_at) = *self.opened_at.read().unwrap() {
                let elapsed = opened_at.elapsed().as_millis() as u64;
                if elapsed >= self.config.reset_timeout_ms {
                    self.half_open();
                }
            }
        }
    }

    /// Force reset the circuit
    pub fn reset(&self) {
        self.close();
    }

    /// Force open the circuit
    pub fn force_open(&self) {
        self.open();
    }

    /// Get statistics
    pub fn statistics(&self) -> CircuitBreakerStatistics {
        CircuitBreakerStatistics {
            name: self.name.clone(),
            state: self.state(),
            total_requests: self.stats.total_requests.load(Ordering::Relaxed),
            successes: self.stats.successes.load(Ordering::Relaxed),
            failures: self.stats.failures.load(Ordering::Relaxed),
            rejected: self.stats.rejected.load(Ordering::Relaxed),
            times_opened: self.stats.times_opened.load(Ordering::Relaxed),
            current_failures: self.failures.load(Ordering::Relaxed),
            current_requests: self.requests.load(Ordering::Relaxed),
        }
    }
}

/// Internal statistics counters
#[derive(Debug, Default)]
struct CircuitBreakerStats {
    total_requests: AtomicU64,
    successes: AtomicU64,
    failures: AtomicU64,
    rejected: AtomicU64,
    times_opened: AtomicU64,
}

/// Public statistics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerStatistics {
    /// Circuit name
    pub name: String,
    /// Current state
    pub state: CircuitState,
    /// Total requests ever
    pub total_requests: u64,
    /// Total successes
    pub successes: u64,
    /// Total failures
    pub failures: u64,
    /// Requests rejected due to open circuit
    pub rejected: u64,
    /// Times circuit was opened
    pub times_opened: u64,
    /// Current window failures
    pub current_failures: u64,
    /// Current window requests
    pub current_requests: u64,
}

impl CircuitBreakerStatistics {
    /// Calculate failure rate
    pub fn failure_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            (self.failures as f64 / self.total_requests as f64) * 100.0
        }
    }

    /// Calculate success rate
    pub fn success_rate(&self) -> f64 {
        100.0 - self.failure_rate()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CIRCUIT BREAKER ERROR
// ═══════════════════════════════════════════════════════════════════════════════

/// Circuit breaker error wrapper
#[derive(Debug)]
pub enum CircuitBreakerError<E> {
    /// Circuit is open, request rejected
    CircuitOpen(String),
    /// Service returned an error
    ServiceError(E),
}

impl<E: std::fmt::Display> std::fmt::Display for CircuitBreakerError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitBreakerError::CircuitOpen(name) => {
                write!(f, "Circuit breaker '{}' is open", name)
            }
            CircuitBreakerError::ServiceError(e) => {
                write!(f, "Service error: {}", e)
            }
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for CircuitBreakerError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CircuitBreakerError::ServiceError(e) => Some(e),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CIRCUIT BREAKER REGISTRY
// ═══════════════════════════════════════════════════════════════════════════════

/// Registry for managing multiple circuit breakers
#[derive(Debug, Default)]
pub struct CircuitBreakerRegistry {
    /// Circuit breakers by name
    breakers: RwLock<HashMap<String, CircuitBreaker>>,
}

impl CircuitBreakerRegistry {
    /// Create new registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a circuit breaker
    pub fn register(&self, breaker: CircuitBreaker) {
        let name = breaker.name.clone();
        self.breakers.write().unwrap().insert(name, breaker);
    }

    /// Create and register a circuit breaker
    pub fn create(&self, name: impl Into<String>, config: CircuitBreakerConfig) {
        let breaker = CircuitBreaker::new(name, config);
        self.register(breaker);
    }

    /// Get circuit breaker by name
    pub fn get(&self, name: &str) -> Option<CircuitState> {
        self.breakers.read().unwrap().get(name).map(|b| b.state())
    }

    /// Check if request is allowed
    pub fn allow_request(&self, name: &str) -> bool {
        self.breakers
            .read()
            .unwrap()
            .get(name)
            .map(|b| b.allow_request())
            .unwrap_or(true) // Allow if not registered
    }

    /// Record success
    pub fn record_success(&self, name: &str) {
        if let Some(breaker) = self.breakers.read().unwrap().get(name) {
            breaker.record_success();
        }
    }

    /// Record failure
    pub fn record_failure(&self, name: &str) {
        if let Some(breaker) = self.breakers.read().unwrap().get(name) {
            breaker.record_failure();
        }
    }

    /// Reset a circuit
    pub fn reset(&self, name: &str) {
        if let Some(breaker) = self.breakers.read().unwrap().get(name) {
            breaker.reset();
        }
    }

    /// Reset all circuits
    pub fn reset_all(&self) {
        for breaker in self.breakers.read().unwrap().values() {
            breaker.reset();
        }
    }

    /// Get all statistics
    pub fn all_statistics(&self) -> Vec<CircuitBreakerStatistics> {
        self.breakers
            .read()
            .unwrap()
            .values()
            .map(|b| b.statistics())
            .collect()
    }

    /// Get summary
    pub fn summary(&self) -> RegistrySummary {
        let breakers = self.breakers.read().unwrap();
        let total = breakers.len();
        let mut closed = 0;
        let mut open = 0;
        let mut half_open = 0;

        for b in breakers.values() {
            match b.state() {
                CircuitState::Closed => closed += 1,
                CircuitState::Open => open += 1,
                CircuitState::HalfOpen => half_open += 1,
            }
        }

        RegistrySummary {
            total,
            closed,
            open,
            half_open,
        }
    }
}

/// Registry summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySummary {
    /// Total circuits
    pub total: usize,
    /// Closed circuits
    pub closed: usize,
    /// Open circuits
    pub open: usize,
    /// Half-open circuits
    pub half_open: usize,
}

impl RegistrySummary {
    /// Check if all circuits are healthy
    pub fn all_healthy(&self) -> bool {
        self.open == 0 && self.half_open == 0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_initial_state() {
        let cb = CircuitBreaker::with_defaults("test");
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn test_circuit_opens_on_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        // Record 3 failures
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_request());
    }

    #[test]
    fn test_circuit_breaker_reset() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        cb.reset();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn test_circuit_breaker_call() {
        let cb = CircuitBreaker::with_defaults("test");

        let result: Result<i32, CircuitBreakerError<String>> = cb.call(|| Ok(42));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);

        let stats = cb.statistics();
        assert_eq!(stats.successes, 1);
    }

    #[test]
    fn test_circuit_breaker_call_failure() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        let _: Result<i32, CircuitBreakerError<String>> =
            cb.call(|| Err::<i32, String>("error".into()));
        let _: Result<i32, CircuitBreakerError<String>> =
            cb.call(|| Err::<i32, String>("error".into()));

        assert_eq!(cb.state(), CircuitState::Open);

        let result: Result<i32, CircuitBreakerError<String>> = cb.call(|| Ok(42));
        assert!(matches!(result, Err(CircuitBreakerError::CircuitOpen(_))));
    }

    #[test]
    fn test_registry() {
        let registry = CircuitBreakerRegistry::new();
        registry.create("oracle", CircuitBreakerConfig::default());
        registry.create("rpc", CircuitBreakerConfig::default());

        assert!(registry.allow_request("oracle"));
        assert!(registry.allow_request("rpc"));
        assert!(registry.allow_request("unknown")); // Unknown allowed

        let summary = registry.summary();
        assert_eq!(summary.total, 2);
        assert_eq!(summary.closed, 2);
        assert!(summary.all_healthy());
    }

    #[test]
    fn test_statistics() {
        let cb = CircuitBreaker::with_defaults("test");

        // Use call() which properly tracks all stats including total_requests
        let _: Result<i32, CircuitBreakerError<&str>> = cb.call(|| Ok(1));
        let _: Result<i32, CircuitBreakerError<&str>> = cb.call(|| Ok(2));
        let _: Result<i32, CircuitBreakerError<&str>> = cb.call(|| Err("fail"));

        let stats = cb.statistics();
        assert_eq!(stats.successes, 2);
        assert_eq!(stats.failures, 1);
        assert_eq!(stats.total_requests, 3);
        // failure_rate = 1/3 * 100 = 33.33%
        assert!(stats.failure_rate() > 30.0 && stats.failure_rate() < 35.0);
    }

    #[test]
    fn test_config_presets() {
        let strict = CircuitBreakerConfig::strict();
        let relaxed = CircuitBreakerConfig::relaxed();

        assert!(strict.failure_threshold < relaxed.failure_threshold);
        assert!(strict.reset_timeout_ms > relaxed.reset_timeout_ms);
    }
}
