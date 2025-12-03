//! zkUSD Command Line Interface.
//!
//! Provides operator tools for protocol management.

pub mod commands;
pub mod config;
pub mod output;

pub use commands::*;
pub use config::*;
pub use output::*;

use std::path::PathBuf;

// ═══════════════════════════════════════════════════════════════════════════════
// CLI APPLICATION
// ═══════════════════════════════════════════════════════════════════════════════

/// CLI Application state
#[derive(Debug)]
pub struct CliApp {
    /// Configuration
    config: CliConfig,
    /// Output formatter
    output: OutputFormatter,
    /// Verbose mode
    verbose: bool,
}

impl CliApp {
    /// Create new CLI application
    pub fn new(config: CliConfig) -> Self {
        Self {
            config,
            output: OutputFormatter::default(),
            verbose: false,
        }
    }

    /// Enable verbose output
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Set output format
    pub fn with_format(mut self, format: OutputFormat) -> Self {
        self.output = OutputFormatter::new(format);
        self
    }

    /// Get configuration
    pub fn config(&self) -> &CliConfig {
        &self.config
    }

    /// Get output formatter
    pub fn output(&self) -> &OutputFormatter {
        &self.output
    }

    /// Check if verbose
    pub fn is_verbose(&self) -> bool {
        self.verbose
    }

    /// Execute a command
    pub fn execute(&self, command: Command) -> CliResult<CommandOutput> {
        if self.verbose {
            self.output.info(&format!("Executing: {:?}", command));
        }

        match command {
            Command::Status(cmd) => cmd.execute(self),
            Command::Cdp(cmd) => cmd.execute(self),
            Command::Oracle(cmd) => cmd.execute(self),
            Command::Pool(cmd) => cmd.execute(self),
            Command::Governance(cmd) => cmd.execute(self),
            Command::Config(cmd) => cmd.execute(self),
            Command::Backup(cmd) => cmd.execute(self),
            Command::Monitor(cmd) => cmd.execute(self),
        }
    }
}

impl Default for CliApp {
    fn default() -> Self {
        Self::new(CliConfig::default())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CLI RESULT
// ═══════════════════════════════════════════════════════════════════════════════

/// CLI Error types
#[derive(Debug, Clone)]
pub enum CliError {
    /// Configuration error
    Config(String),
    /// Connection error
    Connection(String),
    /// Command execution error
    Execution(String),
    /// Invalid argument
    InvalidArgument(String),
    /// IO error
    Io(String),
    /// Not found
    NotFound(String),
    /// Permission denied
    PermissionDenied(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Config(msg) => write!(f, "Configuration error: {}", msg),
            CliError::Connection(msg) => write!(f, "Connection error: {}", msg),
            CliError::Execution(msg) => write!(f, "Execution error: {}", msg),
            CliError::InvalidArgument(msg) => write!(f, "Invalid argument: {}", msg),
            CliError::Io(msg) => write!(f, "IO error: {}", msg),
            CliError::NotFound(msg) => write!(f, "Not found: {}", msg),
            CliError::PermissionDenied(msg) => write!(f, "Permission denied: {}", msg),
        }
    }
}

impl std::error::Error for CliError {}

/// CLI Result type
pub type CliResult<T> = std::result::Result<T, CliError>;

// ═══════════════════════════════════════════════════════════════════════════════
// COMMAND OUTPUT
// ═══════════════════════════════════════════════════════════════════════════════

/// Command execution output
#[derive(Debug, Clone)]
pub struct CommandOutput {
    /// Success status
    pub success: bool,
    /// Output message
    pub message: String,
    /// Structured data (JSON serializable)
    pub data: Option<serde_json::Value>,
    /// Warnings
    pub warnings: Vec<String>,
}

impl CommandOutput {
    /// Create success output
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: None,
            warnings: Vec::new(),
        }
    }

    /// Create success with data
    pub fn success_with_data(message: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: Some(data),
            warnings: Vec::new(),
        }
    }

    /// Create error output
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            data: None,
            warnings: Vec::new(),
        }
    }

    /// Add warning
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMMAND TRAIT
// ═══════════════════════════════════════════════════════════════════════════════

/// Trait for executable commands
pub trait Executable {
    /// Execute the command
    fn execute(&self, app: &CliApp) -> CliResult<CommandOutput>;
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_app_creation() {
        let app = CliApp::default();
        assert!(!app.is_verbose());
    }

    #[test]
    fn test_cli_app_verbose() {
        let app = CliApp::default().with_verbose(true);
        assert!(app.is_verbose());
    }

    #[test]
    fn test_command_output_success() {
        let output = CommandOutput::success("Test message");
        assert!(output.success);
        assert_eq!(output.message, "Test message");
    }

    #[test]
    fn test_command_output_with_warning() {
        let output = CommandOutput::success("OK")
            .with_warning("Warning 1")
            .with_warning("Warning 2");
        assert_eq!(output.warnings.len(), 2);
    }

    #[test]
    fn test_cli_error_display() {
        let err = CliError::Config("bad config".into());
        assert!(err.to_string().contains("Configuration error"));
    }
}
