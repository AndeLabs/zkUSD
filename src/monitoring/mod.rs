//! Production Monitoring and Alerting System for zkUSD.
//!
//! This module provides comprehensive monitoring capabilities:
//! - Protocol health metrics
//! - Performance tracking
//! - Alert management
//! - Anomaly detection
//!
//! # Components
//!
//! - **Metrics**: Collection of protocol metrics
//! - **Alerts**: Rule-based alerting system
//! - **Health**: Protocol health scoring

pub mod metrics;
pub mod alerts;
pub mod health;

pub use metrics::*;
pub use alerts::*;
pub use health::*;
