//! Error types for the zkUSD protocol.
//!
//! This module defines all error types used throughout the protocol,
//! providing clear and actionable error messages.

use thiserror::Error;

/// Result type alias for zkUSD operations
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for the zkUSD protocol
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum Error {
    // ═══════════════════════════════════════════════════════════════════
    // CDP Errors
    // ═══════════════════════════════════════════════════════════════════

    /// CDP not found in the system
    #[error("CDP not found: {0}")]
    CDPNotFound(String),

    /// CDP already exists
    #[error("CDP already exists: {0}")]
    CDPAlreadyExists(String),

    /// CDP is not active
    #[error("CDP is not active: {0}")]
    CDPNotActive(String),

    /// Insufficient collateral for the requested operation
    #[error("Insufficient collateral: required {required}, available {available}")]
    InsufficientCollateral {
        /// Required collateral amount
        required: u64,
        /// Available collateral amount
        available: u64,
    },

    /// Collateralization ratio below minimum
    #[error("Collateralization ratio {current}% below minimum {minimum}%")]
    CollateralizationRatioTooLow {
        /// Current ratio percentage
        current: u64,
        /// Minimum required ratio percentage
        minimum: u64,
    },

    /// Debt amount below protocol minimum
    #[error("Debt amount {amount} below minimum {minimum}")]
    DebtBelowMinimum {
        /// Requested debt amount
        amount: u64,
        /// Protocol minimum debt
        minimum: u64,
    },

    /// Debt amount exceeds protocol maximum
    #[error("Debt amount {amount} exceeds maximum {maximum}")]
    DebtExceedsMaximum {
        /// Requested debt amount
        amount: u64,
        /// Protocol maximum debt
        maximum: u64,
    },

    /// Cannot withdraw collateral - would undercollateralize CDP
    #[error("Withdrawal would undercollateralize CDP")]
    WithdrawalWouldUndercollateralize,

    // ═══════════════════════════════════════════════════════════════════
    // Liquidation Errors
    // ═══════════════════════════════════════════════════════════════════

    /// CDP is healthy and cannot be liquidated
    #[error("CDP {0} is healthy and cannot be liquidated")]
    CDPHealthy(String),

    /// Insufficient funds in stability pool
    #[error("Insufficient stability pool balance: required {required}, available {available}")]
    InsufficientStabilityPool {
        /// Required amount
        required: u64,
        /// Available amount
        available: u64,
    },

    /// Liquidation already in progress
    #[error("Liquidation already in progress for CDP {0}")]
    LiquidationInProgress(String),

    // ═══════════════════════════════════════════════════════════════════
    // Oracle Errors
    // ═══════════════════════════════════════════════════════════════════

    /// Price is stale (not updated recently)
    #[error("Price is stale: last update {last_update}s ago, max allowed {max_age}s")]
    StalePrice {
        /// Seconds since last update
        last_update: u64,
        /// Maximum allowed age in seconds
        max_age: u64,
    },

    /// Price deviation too high between sources
    #[error("Price deviation {deviation}% exceeds maximum {max_deviation}%")]
    PriceDeviationTooHigh {
        /// Actual deviation percentage
        deviation: u64,
        /// Maximum allowed deviation percentage
        max_deviation: u64,
    },

    /// Insufficient oracle sources
    #[error("Insufficient oracle sources: got {got}, need {need}")]
    InsufficientOracleSources {
        /// Number of sources provided
        got: usize,
        /// Number of sources required
        need: usize,
    },

    /// Invalid price proof
    #[error("Invalid price proof")]
    InvalidPriceProof,

    /// Price out of bounds
    #[error("Price {price} out of bounds [{min}, {max}]")]
    PriceOutOfBounds {
        /// Actual price
        price: u64,
        /// Minimum allowed price
        min: u64,
        /// Maximum allowed price
        max: u64,
    },

    // ═══════════════════════════════════════════════════════════════════
    // Authorization Errors
    // ═══════════════════════════════════════════════════════════════════

    /// Not authorized to perform this action
    #[error("Not authorized: {0}")]
    Unauthorized(String),

    /// Invalid signature
    #[error("Invalid signature")]
    InvalidSignature,

    /// Cryptographic operation failed
    #[error("Crypto error in {operation}: {details}")]
    CryptoError {
        /// Operation that failed
        operation: String,
        /// Error details
        details: String,
    },

    /// Signer mismatch
    #[error("Signer mismatch: expected {expected}, got {got}")]
    SignerMismatch {
        /// Expected signer
        expected: String,
        /// Actual signer
        got: String,
    },

    // ═══════════════════════════════════════════════════════════════════
    // Validation Errors
    // ═══════════════════════════════════════════════════════════════════

    /// Invalid input parameter
    #[error("Invalid parameter {name}: {reason}")]
    InvalidParameter {
        /// Parameter name
        name: String,
        /// Reason for invalidity
        reason: String,
    },

    /// Amount is zero
    #[error("Amount cannot be zero")]
    ZeroAmount,

    /// Overflow in calculation
    #[error("Arithmetic overflow in {operation}")]
    Overflow {
        /// Operation that overflowed
        operation: String,
    },

    /// Underflow in calculation
    #[error("Arithmetic underflow in {operation}")]
    Underflow {
        /// Operation that underflowed
        operation: String,
    },

    // ═══════════════════════════════════════════════════════════════════
    // Protocol Errors
    // ═══════════════════════════════════════════════════════════════════

    /// Protocol is paused
    #[error("Protocol is paused")]
    ProtocolPaused,

    /// Protocol is in recovery mode
    #[error("Protocol is in recovery mode")]
    RecoveryMode,

    /// System debt ceiling reached
    #[error("System debt ceiling reached: current {current}, max {max}")]
    DebtCeilingReached {
        /// Current system debt
        current: u64,
        /// Maximum allowed debt
        max: u64,
    },

    /// Invariant violation detected
    #[error("Invariant violation: {0}")]
    InvariantViolation(String),

    // ═══════════════════════════════════════════════════════════════════
    // Serialization Errors
    // ═══════════════════════════════════════════════════════════════════

    /// Serialization failed
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Deserialization failed
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    // ═══════════════════════════════════════════════════════════════════
    // Internal Errors
    // ═══════════════════════════════════════════════════════════════════

    /// Internal error (should not happen in production)
    #[error("Internal error: {0}")]
    Internal(String),

    /// Lock acquisition failed
    #[error("Failed to acquire lock")]
    Lock,

    /// Storage error
    #[error("Storage error: {0}")]
    Storage(String),
}

impl Error {
    /// Returns true if this error is recoverable
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Error::InsufficientCollateral { .. }
                | Error::CollateralizationRatioTooLow { .. }
                | Error::DebtBelowMinimum { .. }
                | Error::StalePrice { .. }
                | Error::InsufficientStabilityPool { .. }
        )
    }

    /// Returns true if this is a critical error requiring immediate attention
    pub fn is_critical(&self) -> bool {
        matches!(
            self,
            Error::InvariantViolation(_)
                | Error::Internal(_)
                | Error::Overflow { .. }
                | Error::Underflow { .. }
        )
    }

    /// Returns the error code for external systems
    pub fn code(&self) -> u32 {
        match self {
            // CDP errors: 1xxx
            Error::CDPNotFound(_) => 1001,
            Error::CDPAlreadyExists(_) => 1002,
            Error::CDPNotActive(_) => 1003,
            Error::InsufficientCollateral { .. } => 1004,
            Error::CollateralizationRatioTooLow { .. } => 1005,
            Error::DebtBelowMinimum { .. } => 1006,
            Error::DebtExceedsMaximum { .. } => 1007,
            Error::WithdrawalWouldUndercollateralize => 1008,

            // Liquidation errors: 2xxx
            Error::CDPHealthy(_) => 2001,
            Error::InsufficientStabilityPool { .. } => 2002,
            Error::LiquidationInProgress(_) => 2003,

            // Oracle errors: 3xxx
            Error::StalePrice { .. } => 3001,
            Error::PriceDeviationTooHigh { .. } => 3002,
            Error::InsufficientOracleSources { .. } => 3003,
            Error::InvalidPriceProof => 3004,
            Error::PriceOutOfBounds { .. } => 3005,

            // Authorization errors: 4xxx
            Error::Unauthorized(_) => 4001,
            Error::InvalidSignature => 4002,
            Error::CryptoError { .. } => 4003,
            Error::SignerMismatch { .. } => 4004,

            // Validation errors: 5xxx
            Error::InvalidParameter { .. } => 5001,
            Error::ZeroAmount => 5002,
            Error::Overflow { .. } => 5003,
            Error::Underflow { .. } => 5004,

            // Protocol errors: 6xxx
            Error::ProtocolPaused => 6001,
            Error::RecoveryMode => 6002,
            Error::DebtCeilingReached { .. } => 6003,
            Error::InvariantViolation(_) => 6004,

            // Serialization errors: 7xxx
            Error::Serialization(_) => 7001,
            Error::Deserialization(_) => 7002,

            // Internal errors: 9xxx
            Error::Internal(_) => 9001,
            Error::Lock => 9002,
            Error::Storage(_) => 9003,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes_unique() {
        // Ensure all error codes are unique
        let codes = vec![
            Error::CDPNotFound("".into()).code(),
            Error::CDPAlreadyExists("".into()).code(),
            Error::InsufficientCollateral { required: 0, available: 0 }.code(),
            Error::StalePrice { last_update: 0, max_age: 0 }.code(),
            Error::Unauthorized("".into()).code(),
            Error::ZeroAmount.code(),
            Error::ProtocolPaused.code(),
            Error::Internal("".into()).code(),
        ];

        let mut unique_codes = codes.clone();
        unique_codes.sort();
        unique_codes.dedup();

        assert_eq!(codes.len(), unique_codes.len(), "Error codes must be unique");
    }

    #[test]
    fn test_error_display() {
        let err = Error::InsufficientCollateral {
            required: 1000,
            available: 500,
        };
        assert!(err.to_string().contains("1000"));
        assert!(err.to_string().contains("500"));
    }

    #[test]
    fn test_is_recoverable() {
        assert!(Error::InsufficientCollateral { required: 0, available: 0 }.is_recoverable());
        assert!(!Error::Internal("test".into()).is_recoverable());
    }

    #[test]
    fn test_is_critical() {
        assert!(Error::InvariantViolation("test".into()).is_critical());
        assert!(Error::Overflow { operation: "test".into() }.is_critical());
        assert!(!Error::CDPNotFound("test".into()).is_critical());
    }
}
