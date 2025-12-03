//! CLI Configuration.
//!
//! Configuration management for the CLI tool.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ═══════════════════════════════════════════════════════════════════════════════
// CLI CONFIGURATION
// ═══════════════════════════════════════════════════════════════════════════════

/// CLI Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    /// RPC endpoint URL
    pub rpc_url: String,
    /// Data directory
    pub data_dir: PathBuf,
    /// API key for authenticated requests
    pub api_key: Option<String>,
    /// Network (mainnet, testnet, regtest)
    pub network: Network,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Maximum retries for failed requests
    pub max_retries: u32,
    /// Enable TLS verification
    pub tls_verify: bool,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://127.0.0.1:3000".into(),
            data_dir: default_data_dir(),
            api_key: None,
            network: Network::Testnet,
            timeout_secs: 30,
            max_retries: 3,
            tls_verify: true,
        }
    }
}

impl CliConfig {
    /// Create new configuration
    pub fn new(rpc_url: String, network: Network) -> Self {
        Self {
            rpc_url,
            network,
            ..Default::default()
        }
    }

    /// Load from file
    pub fn load(path: &PathBuf) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::Io(e.to_string()))?;

        serde_json::from_str(&content)
            .map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Save to file
    pub fn save(&self, path: &PathBuf) -> Result<(), ConfigError> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| ConfigError::Serialize(e.to_string()))?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ConfigError::Io(e.to_string()))?;
        }

        std::fs::write(path, content)
            .map_err(|e| ConfigError::Io(e.to_string()))
    }

    /// Load from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(url) = std::env::var("ZKUSD_RPC_URL") {
            config.rpc_url = url;
        }

        if let Ok(dir) = std::env::var("ZKUSD_DATA_DIR") {
            config.data_dir = PathBuf::from(dir);
        }

        if let Ok(key) = std::env::var("ZKUSD_API_KEY") {
            config.api_key = Some(key);
        }

        if let Ok(network) = std::env::var("ZKUSD_NETWORK") {
            config.network = network.parse().unwrap_or(Network::Testnet);
        }

        if let Ok(timeout) = std::env::var("ZKUSD_TIMEOUT") {
            if let Ok(secs) = timeout.parse() {
                config.timeout_secs = secs;
            }
        }

        config
    }

    /// Get default config file path
    pub fn default_path() -> PathBuf {
        default_data_dir().join("config.json")
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.rpc_url.is_empty() {
            return Err(ConfigError::Validation("RPC URL cannot be empty".into()));
        }

        if self.timeout_secs == 0 {
            return Err(ConfigError::Validation("Timeout must be greater than 0".into()));
        }

        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// NETWORK
// ═══════════════════════════════════════════════════════════════════════════════

/// Network type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    /// Bitcoin mainnet
    Mainnet,
    /// Bitcoin testnet
    Testnet,
    /// Bitcoin regtest
    Regtest,
    /// Local development
    Local,
}

impl Network {
    /// Get network name
    pub fn name(&self) -> &'static str {
        match self {
            Network::Mainnet => "mainnet",
            Network::Testnet => "testnet",
            Network::Regtest => "regtest",
            Network::Local => "local",
        }
    }

    /// Check if production network
    pub fn is_production(&self) -> bool {
        matches!(self, Network::Mainnet)
    }
}

impl std::str::FromStr for Network {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mainnet" | "main" => Ok(Network::Mainnet),
            "testnet" | "test" => Ok(Network::Testnet),
            "regtest" | "reg" => Ok(Network::Regtest),
            "local" | "dev" => Ok(Network::Local),
            _ => Err(ConfigError::Validation(format!("Unknown network: {}", s))),
        }
    }
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CONFIG ERROR
// ═══════════════════════════════════════════════════════════════════════════════

/// Configuration error
#[derive(Debug, Clone)]
pub enum ConfigError {
    /// IO error
    Io(String),
    /// Parse error
    Parse(String),
    /// Serialization error
    Serialize(String),
    /// Validation error
    Validation(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(msg) => write!(f, "IO error: {}", msg),
            ConfigError::Parse(msg) => write!(f, "Parse error: {}", msg),
            ConfigError::Serialize(msg) => write!(f, "Serialization error: {}", msg),
            ConfigError::Validation(msg) => write!(f, "Validation error: {}", msg),
        }
    }
}

impl std::error::Error for ConfigError {}

// ═══════════════════════════════════════════════════════════════════════════════
// HELPER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Get default data directory
fn default_data_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".zkusd");
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join("Library/Application Support/zkUSD");
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("zkUSD");
        }
    }

    PathBuf::from(".zkusd")
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = CliConfig::default();
        assert_eq!(config.network, Network::Testnet);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_network_parsing() {
        assert_eq!("mainnet".parse::<Network>().unwrap(), Network::Mainnet);
        assert_eq!("testnet".parse::<Network>().unwrap(), Network::Testnet);
        assert_eq!("regtest".parse::<Network>().unwrap(), Network::Regtest);
        assert_eq!("local".parse::<Network>().unwrap(), Network::Local);
    }

    #[test]
    fn test_network_display() {
        assert_eq!(Network::Mainnet.to_string(), "mainnet");
        assert_eq!(Network::Testnet.to_string(), "testnet");
    }

    #[test]
    fn test_config_validation() {
        let mut config = CliConfig::default();
        assert!(config.validate().is_ok());

        config.rpc_url = String::new();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_network_is_production() {
        assert!(Network::Mainnet.is_production());
        assert!(!Network::Testnet.is_production());
        assert!(!Network::Regtest.is_production());
    }
}
