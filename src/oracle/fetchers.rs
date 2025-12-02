//! Real price fetchers for exchange APIs.
//!
//! This module implements HTTP-based price fetching from major cryptocurrency
//! exchanges. Each fetcher implements the async fetch pattern and returns
//! properly formatted price data.
//!
//! Supported exchanges:
//! - Binance (spot and futures)
//! - Coinbase (via CoinGecko-compatible endpoint)
//! - Kraken
//! - Bitstamp
//!
//! All prices are returned in USD cents for consistency.

#[cfg(feature = "async-oracle")]
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};
use crate::oracle::sources::{Exchange, PriceSource, SourceCollection};

// ═══════════════════════════════════════════════════════════════════════════════
// EXCHANGE API RESPONSE TYPES
// ═══════════════════════════════════════════════════════════════════════════════

/// Binance ticker response
#[derive(Debug, Deserialize)]
pub struct BinanceTickerResponse {
    /// Symbol (e.g., "BTCUSDT")
    pub symbol: String,
    /// Last price as string
    pub price: String,
}

/// Binance 24hr ticker (includes volume)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binance24hrResponse {
    /// Symbol
    pub symbol: String,
    /// Last price
    pub last_price: String,
    /// 24h volume in quote asset (USDT)
    pub quote_volume: String,
    /// Price change percent
    pub price_change_percent: String,
}

/// Kraken ticker response
#[derive(Debug, Deserialize)]
pub struct KrakenResponse {
    /// Error messages
    pub error: Vec<String>,
    /// Result data
    pub result: Option<KrakenResult>,
}

#[derive(Debug, Deserialize)]
pub struct KrakenResult {
    /// XXBTZUSD ticker data
    #[serde(rename = "XXBTZUSD")]
    pub btc_usd: Option<KrakenPair>,
}

#[derive(Debug, Deserialize)]
pub struct KrakenPair {
    /// Current ask price [price, whole lot volume, lot volume]
    pub a: Vec<String>,
    /// Current bid price [price, whole lot volume, lot volume]
    pub b: Vec<String>,
    /// Last trade closed [price, lot volume]
    pub c: Vec<String>,
    /// Volume [today, last 24 hours]
    pub v: Vec<String>,
}

/// Bitstamp ticker response
#[derive(Debug, Deserialize)]
pub struct BitstampResponse {
    /// Last price
    pub last: String,
    /// 24h volume
    pub volume: String,
    /// Timestamp
    pub timestamp: String,
}

/// Coinbase response (v2 API)
#[derive(Debug, Deserialize)]
pub struct CoinbaseResponse {
    /// Data wrapper
    pub data: CoinbaseData,
}

#[derive(Debug, Deserialize)]
pub struct CoinbaseData {
    /// Currency (BTC)
    pub base: String,
    /// Quote currency (USD)
    pub currency: String,
    /// Amount (price)
    pub amount: String,
}

/// OKX ticker response
#[derive(Debug, Deserialize)]
pub struct OKXResponse {
    /// Response code
    pub code: String,
    /// Data array
    pub data: Vec<OKXTicker>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXTicker {
    /// Instrument ID
    pub inst_id: String,
    /// Last traded price
    pub last: String,
    /// 24h volume in quote currency
    pub vol_ccy24h: String,
}

/// Bybit ticker response
#[derive(Debug, Deserialize)]
pub struct BybitResponse {
    /// Return code
    pub ret_code: i32,
    /// Result data
    pub result: BybitResult,
}

#[derive(Debug, Deserialize)]
pub struct BybitResult {
    /// List of tickers
    pub list: Vec<BybitTicker>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitTicker {
    /// Symbol
    pub symbol: String,
    /// Last price
    pub last_price: String,
    /// 24h turnover in quote currency
    pub turnover24h: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// HTTP PRICE FETCHER
// ═══════════════════════════════════════════════════════════════════════════════

/// Configuration for HTTP price fetcher
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpFetcherConfig {
    /// Timeout in milliseconds
    pub timeout_ms: u64,
    /// User agent string
    pub user_agent: String,
    /// Retry count on failure
    pub max_retries: u8,
    /// Delay between retries in milliseconds
    pub retry_delay_ms: u64,
}

impl Default for HttpFetcherConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 10_000, // 10 seconds
            user_agent: "zkUSD-Oracle/1.0".to_string(),
            max_retries: 3,
            retry_delay_ms: 1_000,
        }
    }
}

/// HTTP-based price fetcher for real exchange APIs
#[cfg(feature = "async-oracle")]
pub struct HttpPriceFetcher {
    /// HTTP client
    client: Client,
    /// Configuration
    config: HttpFetcherConfig,
}

#[cfg(feature = "async-oracle")]
impl HttpPriceFetcher {
    /// Create a new HTTP price fetcher
    pub fn new(config: HttpFetcherConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_millis(config.timeout_ms))
            .user_agent(&config.user_agent)
            .build()
            .map_err(|e| Error::Internal(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self { client, config })
    }

    /// Create with default configuration
    pub fn with_defaults() -> Result<Self> {
        Self::new(HttpFetcherConfig::default())
    }

    /// Get current timestamp
    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Parse price string to cents (multiply by 100)
    fn parse_price_to_cents(price_str: &str) -> Result<u64> {
        let price: f64 = price_str.parse().map_err(|e| Error::InvalidParameter {
            name: "price".into(),
            reason: format!("Invalid price format: {}", e),
        })?;

        // Convert to cents (multiply by 100)
        Ok((price * 100.0).round() as u64)
    }

    /// Parse volume string to integer
    fn parse_volume(volume_str: &str) -> u64 {
        volume_str.parse::<f64>().ok().map(|v| v as u64).unwrap_or(0)
    }

    /// Fetch price from Binance
    pub async fn fetch_binance(&self) -> Result<PriceSource> {
        let url = "https://api.binance.com/api/v3/ticker/24hr?symbol=BTCUSDT";

        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| Error::Internal(format!("Binance request failed: {}", e)))?;

        let data: Binance24hrResponse = response
            .json()
            .await
            .map_err(|e| Error::Internal(format!("Failed to parse Binance response: {}", e)))?;

        let price_cents = Self::parse_price_to_cents(&data.last_price)?;
        let volume = Self::parse_volume(&data.quote_volume);

        Ok(PriceSource::new(Exchange::Binance, price_cents, Self::current_timestamp())
            .with_volume(volume))
    }

    /// Fetch price from Coinbase
    pub async fn fetch_coinbase(&self) -> Result<PriceSource> {
        let url = "https://api.coinbase.com/v2/prices/BTC-USD/spot";

        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| Error::Internal(format!("Coinbase request failed: {}", e)))?;

        let data: CoinbaseResponse = response
            .json()
            .await
            .map_err(|e| Error::Internal(format!("Failed to parse Coinbase response: {}", e)))?;

        let price_cents = Self::parse_price_to_cents(&data.data.amount)?;

        Ok(PriceSource::new(
            Exchange::Coinbase,
            price_cents,
            Self::current_timestamp(),
        ))
    }

    /// Fetch price from Kraken
    pub async fn fetch_kraken(&self) -> Result<PriceSource> {
        let url = "https://api.kraken.com/0/public/Ticker?pair=XBTUSD";

        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| Error::Internal(format!("Kraken request failed: {}", e)))?;

        let data: KrakenResponse = response
            .json()
            .await
            .map_err(|e| Error::Internal(format!("Failed to parse Kraken response: {}", e)))?;

        if !data.error.is_empty() {
            return Err(Error::Internal(format!(
                "Kraken API error: {:?}",
                data.error
            )));
        }

        let result = data.result.ok_or_else(|| Error::Internal("No result from Kraken".into()))?;
        let btc_usd = result.btc_usd.ok_or_else(|| Error::Internal("No BTC/USD data from Kraken".into()))?;

        // Get last trade price (c[0])
        let price_str = btc_usd.c.first().ok_or_else(|| Error::Internal("No price in Kraken response".into()))?;
        let price_cents = Self::parse_price_to_cents(price_str)?;

        // Get 24h volume
        let volume = btc_usd.v.get(1).map(|v| Self::parse_volume(v)).unwrap_or(0);

        Ok(PriceSource::new(Exchange::Kraken, price_cents, Self::current_timestamp())
            .with_volume(volume))
    }

    /// Fetch price from Bitstamp
    pub async fn fetch_bitstamp(&self) -> Result<PriceSource> {
        let url = "https://www.bitstamp.net/api/v2/ticker/btcusd/";

        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| Error::Internal(format!("Bitstamp request failed: {}", e)))?;

        let data: BitstampResponse = response
            .json()
            .await
            .map_err(|e| Error::Internal(format!("Failed to parse Bitstamp response: {}", e)))?;

        let price_cents = Self::parse_price_to_cents(&data.last)?;
        let volume = Self::parse_volume(&data.volume);

        Ok(PriceSource::new(Exchange::Bitstamp, price_cents, Self::current_timestamp())
            .with_volume(volume))
    }

    /// Fetch price from OKX
    pub async fn fetch_okx(&self) -> Result<PriceSource> {
        let url = "https://www.okx.com/api/v5/market/ticker?instId=BTC-USDT";

        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| Error::Internal(format!("OKX request failed: {}", e)))?;

        let data: OKXResponse = response
            .json()
            .await
            .map_err(|e| Error::Internal(format!("Failed to parse OKX response: {}", e)))?;

        if data.code != "0" {
            return Err(Error::Internal(format!("OKX API error: code {}", data.code)));
        }

        let ticker = data.data.first().ok_or_else(|| Error::Internal("No ticker data from OKX".into()))?;
        let price_cents = Self::parse_price_to_cents(&ticker.last)?;
        let volume = Self::parse_volume(&ticker.vol_ccy24h);

        Ok(PriceSource::new(Exchange::OKX, price_cents, Self::current_timestamp())
            .with_volume(volume))
    }

    /// Fetch price from Bybit
    pub async fn fetch_bybit(&self) -> Result<PriceSource> {
        let url = "https://api.bybit.com/v5/market/tickers?category=spot&symbol=BTCUSDT";

        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| Error::Internal(format!("Bybit request failed: {}", e)))?;

        let data: BybitResponse = response
            .json()
            .await
            .map_err(|e| Error::Internal(format!("Failed to parse Bybit response: {}", e)))?;

        if data.ret_code != 0 {
            return Err(Error::Internal(format!(
                "Bybit API error: code {}",
                data.ret_code
            )));
        }

        let ticker = data.result.list.first().ok_or_else(|| Error::Internal("No ticker data from Bybit".into()))?;
        let price_cents = Self::parse_price_to_cents(&ticker.last_price)?;
        let volume = Self::parse_volume(&ticker.turnover24h);

        Ok(PriceSource::new(Exchange::Bybit, price_cents, Self::current_timestamp())
            .with_volume(volume))
    }

    /// Fetch price from a specific exchange
    pub async fn fetch_price(&self, exchange: Exchange) -> Result<PriceSource> {
        match exchange {
            Exchange::Binance => self.fetch_binance().await,
            Exchange::Coinbase => self.fetch_coinbase().await,
            Exchange::Kraken => self.fetch_kraken().await,
            Exchange::Bitstamp => self.fetch_bitstamp().await,
            Exchange::OKX => self.fetch_okx().await,
            Exchange::Bybit => self.fetch_bybit().await,
            Exchange::Custom(_) => Err(Error::InvalidParameter {
                name: "exchange".into(),
                reason: "Custom exchanges not supported for HTTP fetching".into(),
            }),
        }
    }

    /// Fetch prices from all supported exchanges concurrently
    pub async fn fetch_all(&self) -> SourceCollection {
        let timestamp = Self::current_timestamp();
        let mut collection = SourceCollection::new(timestamp);

        // Fetch all exchanges concurrently using tokio::join!
        let (binance, coinbase, kraken, bitstamp, okx, bybit) = tokio::join!(
            self.fetch_binance(),
            self.fetch_coinbase(),
            self.fetch_kraken(),
            self.fetch_bitstamp(),
            self.fetch_okx(),
            self.fetch_bybit(),
        );

        // Add successful fetches to collection
        if let Ok(source) = binance {
            collection.add(source);
        }
        if let Ok(source) = coinbase {
            collection.add(source);
        }
        if let Ok(source) = kraken {
            collection.add(source);
        }
        if let Ok(source) = bitstamp {
            collection.add(source);
        }
        if let Ok(source) = okx {
            collection.add(source);
        }
        if let Ok(source) = bybit {
            collection.add(source);
        }

        collection
    }

    /// Fetch prices from major exchanges only (for faster response)
    pub async fn fetch_major(&self) -> SourceCollection {
        let timestamp = Self::current_timestamp();
        let mut collection = SourceCollection::new(timestamp);

        // Fetch major exchanges concurrently
        let (binance, coinbase, kraken) = tokio::join!(
            self.fetch_binance(),
            self.fetch_coinbase(),
            self.fetch_kraken(),
        );

        if let Ok(source) = binance {
            collection.add(source);
        }
        if let Ok(source) = coinbase {
            collection.add(source);
        }
        if let Ok(source) = kraken {
            collection.add(source);
        }

        collection
    }

    /// Fetch with retry logic
    pub async fn fetch_with_retry(&self, exchange: Exchange) -> Result<PriceSource> {
        let mut last_error = None;

        for attempt in 0..self.config.max_retries {
            match self.fetch_price(exchange).await {
                Ok(source) => return Ok(source),
                Err(e) => {
                    last_error = Some(e);
                    if attempt < self.config.max_retries - 1 {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            self.config.retry_delay_ms * (attempt as u64 + 1),
                        ))
                        .await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            Error::Internal("Unknown error during fetch retry".into())
        }))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SYNCHRONOUS PRICE FETCHER (for non-async contexts)
// ═══════════════════════════════════════════════════════════════════════════════

/// Synchronous price fetcher using blocking HTTP calls
#[cfg(not(feature = "async-oracle"))]
pub struct SyncPriceFetcher {
    config: HttpFetcherConfig,
}

#[cfg(not(feature = "async-oracle"))]
impl SyncPriceFetcher {
    /// Create a new synchronous fetcher
    pub fn new(config: HttpFetcherConfig) -> Self {
        Self { config }
    }

    /// Create with defaults
    pub fn with_defaults() -> Self {
        Self::new(HttpFetcherConfig::default())
    }

    /// Get current timestamp
    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    // Note: In non-async mode, actual HTTP fetching would require
    // a blocking HTTP client like ureq. For now, we return mock data.

    /// Fetch price (mock implementation for sync mode)
    pub fn fetch_price(&self, exchange: Exchange) -> Result<PriceSource> {
        // This would use a blocking HTTP client in production
        // For now, return a placeholder indicating async feature is needed
        Err(Error::Internal(
            "Sync HTTP fetching not implemented. Enable 'async-oracle' feature for real price fetching.".into()
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRICE FETCHER RESULT
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of a price fetch operation with metadata
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// The collected prices
    pub collection: SourceCollection,
    /// Number of successful fetches
    pub successful: usize,
    /// Number of failed fetches
    pub failed: usize,
    /// Fetch duration in milliseconds
    pub duration_ms: u64,
    /// Errors from failed fetches
    pub errors: Vec<(Exchange, String)>,
}

impl FetchResult {
    /// Create a new fetch result
    pub fn new(collection: SourceCollection, errors: Vec<(Exchange, String)>, duration_ms: u64) -> Self {
        let successful = collection.len();
        let failed = errors.len();
        Self {
            collection,
            successful,
            failed,
            duration_ms,
            errors,
        }
    }

    /// Check if fetch was successful (at least one source)
    pub fn is_successful(&self) -> bool {
        self.successful > 0
    }

    /// Check if fetch has minimum required sources
    pub fn has_minimum_sources(&self, min: usize) -> bool {
        self.successful >= min
    }

    /// Get aggregated price if available
    pub fn aggregated_price(&self) -> Option<u64> {
        self.collection.median_price()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_price_to_cents() {
        #[cfg(feature = "async-oracle")]
        {
            let cents = HttpPriceFetcher::parse_price_to_cents("100000.50").unwrap();
            assert_eq!(cents, 10000050);

            let cents = HttpPriceFetcher::parse_price_to_cents("99999.99").unwrap();
            assert_eq!(cents, 9999999);
        }
    }

    #[test]
    fn test_http_fetcher_config_default() {
        let config = HttpFetcherConfig::default();
        assert_eq!(config.timeout_ms, 10_000);
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_fetch_result() {
        let collection = SourceCollection::new(1000);
        let errors = vec![(Exchange::Coinbase, "timeout".to_string())];
        let result = FetchResult::new(collection, errors, 100);

        assert!(!result.is_successful());
        assert!(!result.has_minimum_sources(1));
        assert_eq!(result.failed, 1);
    }
}
