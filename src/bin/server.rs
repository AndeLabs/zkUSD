//! zkUSD Protocol RPC Server
//!
//! Production-grade HTTP/JSON-RPC server for the zkUSD protocol.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::{info, warn};

use zkusd::core::cdp::{CDP, CDPId, CDPManager, CDPStatus};
use zkusd::core::config::ProtocolConfig;
use zkusd::core::token::{TokenAmount, ZkUSD};
use zkusd::core::vault::{CollateralAmount, Vault};
use zkusd::liquidation::stability_pool::StabilityPool;
use zkusd::oracle::price_feed::PriceFeed;
use zkusd::storage::backend::InMemoryStore;
use zkusd::utils::crypto::{Hash, PublicKey};

// ═══════════════════════════════════════════════════════════════════════════════
// SERVER STATE
// ═══════════════════════════════════════════════════════════════════════════════

/// Shared application state
pub struct AppState {
    pub config: ProtocolConfig,
    pub cdp_manager: RwLock<CDPManager>,
    pub token: RwLock<ZkUSD>,
    pub vault: RwLock<Vault>,
    pub stability_pool: RwLock<StabilityPool>,
    pub price_feed: RwLock<PriceFeed>,
    pub block_height: RwLock<u64>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            config: ProtocolConfig::default(),
            cdp_manager: RwLock::new(CDPManager::new()),
            token: RwLock::new(ZkUSD::new()),
            vault: RwLock::new(Vault::new()),
            stability_pool: RwLock::new(StabilityPool::new()),
            price_feed: RwLock::new(PriceFeed::new()),
            block_height: RwLock::new(0),
        }
    }

    pub async fn get_btc_price(&self) -> u64 {
        self.price_feed.read().await.price_cents()
    }

    pub async fn current_block(&self) -> u64 {
        *self.block_height.read().await
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// API TYPES
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self { success: true, data: Some(data), error: None }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self { success: false, data: None, error: Some(msg.into()) }
    }
}

#[derive(Debug, Serialize)]
pub struct ProtocolStatus {
    pub version: String,
    pub block_height: u64,
    pub btc_price_cents: u64,
    pub total_supply_cents: u64,
    pub total_collateral_sats: u64,
    pub active_cdps: u64,
    pub stability_pool_deposits_cents: u64,
    pub min_collateral_ratio: u64,
    pub recovery_mode: bool,
}

#[derive(Debug, Serialize)]
pub struct CDPInfo {
    pub id: String,
    pub owner: String,
    pub collateral_sats: u64,
    pub debt_cents: u64,
    pub ratio: u64,
    pub status: String,
    pub created_at: u64,
    pub last_updated: u64,
}

impl From<&CDP> for CDPInfo {
    fn from(cdp: &CDP) -> Self {
        Self {
            id: cdp.id.to_hex(),
            owner: hex::encode(cdp.owner.as_bytes()),
            collateral_sats: cdp.collateral_sats,
            debt_cents: cdp.debt_cents,
            ratio: 0, // Will be calculated with price
            status: format!("{:?}", cdp.status),
            created_at: cdp.created_at,
            last_updated: cdp.last_updated,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct OpenCDPRequest {
    pub owner: String,
    pub collateral_sats: u64,
    pub debt_cents: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct DepositCollateralRequest {
    pub amount_sats: u64,
}

#[derive(Debug, Deserialize)]
pub struct WithdrawCollateralRequest {
    pub amount_sats: u64,
}

#[derive(Debug, Deserialize)]
pub struct MintDebtRequest {
    pub amount_cents: u64,
}

#[derive(Debug, Deserialize)]
pub struct RepayDebtRequest {
    pub amount_cents: u64,
}

#[derive(Debug, Deserialize)]
pub struct TransferRequest {
    pub from: String,
    pub to: String,
    pub amount_cents: u64,
}

#[derive(Debug, Deserialize)]
pub struct StabilityDepositRequest {
    pub depositor: String,
    pub amount_cents: u64,
}

#[derive(Debug, Serialize)]
pub struct PriceInfo {
    pub price_cents: u64,
    pub formatted: String,
    pub timestamp: u64,
    pub source_count: u8,
    pub confidence: u8,
}

// ═══════════════════════════════════════════════════════════════════════════════
// HANDLERS
// ═══════════════════════════════════════════════════════════════════════════════

/// GET /health - Health check
async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "zkusd-server",
        "version": zkusd::VERSION
    }))
}

/// GET /status - Protocol status
async fn get_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let cdp_manager = state.cdp_manager.read().await;
    let token = state.token.read().await;
    let vault = state.vault.read().await;
    let stability_pool = state.stability_pool.read().await;
    let btc_price = state.get_btc_price().await;
    let block_height = state.current_block().await;

    let status = ProtocolStatus {
        version: zkusd::VERSION.to_string(),
        block_height,
        btc_price_cents: btc_price,
        total_supply_cents: token.total_supply().cents(),
        total_collateral_sats: vault.total_collateral().sats(),
        active_cdps: cdp_manager.active_count() as u64,
        stability_pool_deposits_cents: stability_pool.total_deposits().cents(),
        min_collateral_ratio: state.config.params.min_collateral_ratio,
        recovery_mode: state.config.recovery_mode,
    };

    Json(ApiResponse::ok(status))
}

/// GET /price - Current BTC price
async fn get_price(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let price_feed = state.price_feed.read().await;
    let price = price_feed.current_price();

    let info = PriceInfo {
        price_cents: price.price_cents,
        formatted: price.format_price(),
        timestamp: price.timestamp,
        source_count: price.source_count,
        confidence: price.confidence,
    };

    Json(ApiResponse::ok(info))
}

/// POST /price - Update price (for oracle nodes)
async fn update_price(
    State(state): State<Arc<AppState>>,
    Json(price_cents): Json<u64>,
) -> impl IntoResponse {
    let mut price_feed = state.price_feed.write().await;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let price_data = zkusd::oracle::price_feed::PriceData::new(price_cents, timestamp, 1);
    price_feed.update(price_data);

    info!("Price updated to {} cents", price_cents);
    Json(ApiResponse::ok("Price updated"))
}

/// GET /cdp/:id - Get CDP info
async fn get_cdp(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let cdp_id = match CDPId::from_hex(&id) {
        Ok(id) => id,
        Err(_) => return Json(ApiResponse::<CDPInfo>::err("Invalid CDP ID")),
    };

    let cdp_manager = state.cdp_manager.read().await;
    let btc_price = state.get_btc_price().await;

    match cdp_manager.get(&cdp_id) {
        Some(cdp) => {
            let mut info = CDPInfo::from(cdp);
            info.ratio = cdp.calculate_ratio(btc_price);
            Json(ApiResponse::ok(info))
        }
        None => Json(ApiResponse::err("CDP not found")),
    }
}

/// GET /cdps - List all CDPs
async fn list_cdps(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let cdp_manager = state.cdp_manager.read().await;
    let btc_price = state.get_btc_price().await;

    let cdps: Vec<CDPInfo> = cdp_manager
        .all_cdps()
        .into_iter()
        .map(|cdp| {
            let mut info = CDPInfo::from(cdp);
            info.ratio = cdp.calculate_ratio(btc_price);
            info
        })
        .collect();

    Json(ApiResponse::ok(cdps))
}

/// POST /cdp - Open new CDP
async fn open_cdp(
    State(state): State<Arc<AppState>>,
    Json(req): Json<OpenCDPRequest>,
) -> impl IntoResponse {
    let owner_bytes = match hex::decode(&req.owner) {
        Ok(b) if b.len() == 33 => b,
        _ => return Json(ApiResponse::<CDPInfo>::err("Invalid owner public key")),
    };

    let mut owner_arr = [0u8; 33];
    owner_arr.copy_from_slice(&owner_bytes);
    let owner = PublicKey::new(owner_arr);

    let block_height = state.current_block().await;
    let btc_price = state.get_btc_price().await;
    let min_ratio = state.config.params.min_collateral_ratio;

    let cdp = match CDP::with_collateral(owner, req.collateral_sats, 1, block_height) {
        Ok(mut cdp) => {
            if let Some(debt) = req.debt_cents {
                if debt > 0 {
                    if let Err(e) = cdp.mint_debt(debt, btc_price, min_ratio, block_height) {
                        return Json(ApiResponse::err(format!("Failed to mint debt: {}", e)));
                    }
                }
            }
            cdp
        }
        Err(e) => return Json(ApiResponse::err(format!("Failed to create CDP: {}", e))),
    };

    let cdp_id = cdp.id;
    let mut cdp_manager = state.cdp_manager.write().await;
    cdp_manager.register(cdp);

    // Update vault
    let mut vault = state.vault.write().await;
    let _ = vault.deposit(cdp_id, CollateralAmount::from_sats(req.collateral_sats), block_height, Hash::zero());

    // Mint tokens if debt requested
    if let Some(debt) = req.debt_cents {
        if debt > 0 {
            let mut token = state.token.write().await;
            let _ = token.mint(owner, TokenAmount::from_cents(debt), block_height, Hash::zero());
        }
    }

    let cdp = cdp_manager.get(&cdp_id).unwrap();
    let mut info = CDPInfo::from(cdp);
    info.ratio = cdp.calculate_ratio(btc_price);

    info!("CDP opened: {}", cdp_id.to_hex());
    Json(ApiResponse::ok(info))
}

/// POST /cdp/:id/deposit - Deposit collateral
async fn deposit_collateral(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<DepositCollateralRequest>,
) -> impl IntoResponse {
    let cdp_id = match CDPId::from_hex(&id) {
        Ok(id) => id,
        Err(_) => return Json(ApiResponse::<CDPInfo>::err("Invalid CDP ID")),
    };

    let block_height = state.current_block().await;
    let btc_price = state.get_btc_price().await;

    let mut cdp_manager = state.cdp_manager.write().await;

    match cdp_manager.get_mut(&cdp_id) {
        Some(cdp) => {
            if let Err(e) = cdp.deposit_collateral(req.amount_sats, block_height) {
                return Json(ApiResponse::err(format!("Deposit failed: {}", e)));
            }

            // Update vault
            let mut vault = state.vault.write().await;
            let _ = vault.deposit(cdp_id, CollateralAmount::from_sats(req.amount_sats), block_height, Hash::zero());

            let mut info = CDPInfo::from(&*cdp);
            info.ratio = cdp.calculate_ratio(btc_price);
            Json(ApiResponse::ok(info))
        }
        None => Json(ApiResponse::err("CDP not found")),
    }
}

/// POST /cdp/:id/withdraw - Withdraw collateral
async fn withdraw_collateral(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<WithdrawCollateralRequest>,
) -> impl IntoResponse {
    let cdp_id = match CDPId::from_hex(&id) {
        Ok(id) => id,
        Err(_) => return Json(ApiResponse::<CDPInfo>::err("Invalid CDP ID")),
    };

    let block_height = state.current_block().await;
    let btc_price = state.get_btc_price().await;
    let min_ratio = state.config.params.min_collateral_ratio;

    let mut cdp_manager = state.cdp_manager.write().await;

    match cdp_manager.get_mut(&cdp_id) {
        Some(cdp) => {
            if let Err(e) = cdp.withdraw_collateral(req.amount_sats, btc_price, min_ratio, block_height) {
                return Json(ApiResponse::err(format!("Withdrawal failed: {}", e)));
            }

            // Update vault
            let mut vault = state.vault.write().await;
            let _ = vault.withdraw(cdp_id, CollateralAmount::from_sats(req.amount_sats), block_height, Hash::zero());

            let mut info = CDPInfo::from(&*cdp);
            info.ratio = cdp.calculate_ratio(btc_price);
            Json(ApiResponse::ok(info))
        }
        None => Json(ApiResponse::err("CDP not found")),
    }
}

/// POST /cdp/:id/mint - Mint debt
async fn mint_debt(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<MintDebtRequest>,
) -> impl IntoResponse {
    let cdp_id = match CDPId::from_hex(&id) {
        Ok(id) => id,
        Err(_) => return Json(ApiResponse::<CDPInfo>::err("Invalid CDP ID")),
    };

    let block_height = state.current_block().await;
    let btc_price = state.get_btc_price().await;
    let min_ratio = state.config.params.min_collateral_ratio;

    let mut cdp_manager = state.cdp_manager.write().await;

    match cdp_manager.get_mut(&cdp_id) {
        Some(cdp) => {
            let owner = cdp.owner;
            if let Err(e) = cdp.mint_debt(req.amount_cents, btc_price, min_ratio, block_height) {
                return Json(ApiResponse::err(format!("Mint failed: {}", e)));
            }

            // Mint tokens
            let mut token = state.token.write().await;
            let _ = token.mint(owner, TokenAmount::from_cents(req.amount_cents), block_height, Hash::zero());

            let mut info = CDPInfo::from(&*cdp);
            info.ratio = cdp.calculate_ratio(btc_price);
            Json(ApiResponse::ok(info))
        }
        None => Json(ApiResponse::err("CDP not found")),
    }
}

/// POST /cdp/:id/repay - Repay debt
async fn repay_debt(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<RepayDebtRequest>,
) -> impl IntoResponse {
    let cdp_id = match CDPId::from_hex(&id) {
        Ok(id) => id,
        Err(_) => return Json(ApiResponse::<CDPInfo>::err("Invalid CDP ID")),
    };

    let block_height = state.current_block().await;
    let btc_price = state.get_btc_price().await;

    let mut cdp_manager = state.cdp_manager.write().await;

    match cdp_manager.get_mut(&cdp_id) {
        Some(cdp) => {
            let owner = cdp.owner;
            let amount = if req.amount_cents == 0 { cdp.debt_cents } else { req.amount_cents };

            if let Err(e) = cdp.repay_debt(amount, block_height) {
                return Json(ApiResponse::err(format!("Repay failed: {}", e)));
            }

            // Burn tokens
            let mut token = state.token.write().await;
            let _ = token.burn(owner, TokenAmount::from_cents(amount), block_height, Hash::zero());

            let mut info = CDPInfo::from(&*cdp);
            info.ratio = cdp.calculate_ratio(btc_price);
            Json(ApiResponse::ok(info))
        }
        None => Json(ApiResponse::err("CDP not found")),
    }
}

/// POST /cdp/:id/close - Close CDP
async fn close_cdp(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let cdp_id = match CDPId::from_hex(&id) {
        Ok(id) => id,
        Err(_) => return Json(ApiResponse::<String>::err("Invalid CDP ID")),
    };

    let block_height = state.current_block().await;

    let mut cdp_manager = state.cdp_manager.write().await;

    match cdp_manager.get_mut(&cdp_id) {
        Some(cdp) => {
            if let Err(e) = cdp.close(block_height) {
                return Json(ApiResponse::err(format!("Close failed: {}", e)));
            }

            // Withdraw remaining collateral from vault
            let mut vault = state.vault.write().await;
            let collateral = vault.collateral_of(&cdp_id);
            if collateral.sats() > 0 {
                let _ = vault.withdraw(cdp_id, collateral, block_height, Hash::zero());
            }

            info!("CDP closed: {}", cdp_id.to_hex());
            Json(ApiResponse::ok("CDP closed successfully".to_string()))
        }
        None => Json(ApiResponse::err("CDP not found")),
    }
}

/// GET /token/balance/:address - Get token balance
async fn get_balance(
    State(state): State<Arc<AppState>>,
    Path(address): Path<String>,
) -> impl IntoResponse {
    let owner_bytes = match hex::decode(&address) {
        Ok(b) if b.len() == 33 => b,
        _ => return Json(ApiResponse::<u64>::err("Invalid address")),
    };

    let mut owner_arr = [0u8; 33];
    owner_arr.copy_from_slice(&owner_bytes);
    let owner = PublicKey::new(owner_arr);

    let token = state.token.read().await;
    let balance = token.balance_of(&owner);

    Json(ApiResponse::ok(balance.cents()))
}

/// GET /token/supply - Get total supply
async fn get_supply(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let token = state.token.read().await;
    Json(ApiResponse::ok(token.total_supply().cents()))
}

/// GET /pool/status - Stability pool status
async fn get_pool_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let pool = state.stability_pool.read().await;

    #[derive(Serialize)]
    struct PoolStatus {
        total_deposits_cents: u64,
        total_btc_gains_sats: u64,
        depositor_count: u64,
    }

    let status = PoolStatus {
        total_deposits_cents: pool.total_deposits().cents(),
        total_btc_gains_sats: pool.total_btc_gains().sats(),
        depositor_count: pool.depositor_count() as u64,
    };

    Json(ApiResponse::ok(status))
}

/// POST /pool/deposit - Deposit to stability pool
async fn pool_deposit(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StabilityDepositRequest>,
) -> impl IntoResponse {
    let depositor_bytes = match hex::decode(&req.depositor) {
        Ok(b) if b.len() == 33 => b,
        _ => return Json(ApiResponse::<String>::err("Invalid depositor address")),
    };

    let mut depositor_arr = [0u8; 33];
    depositor_arr.copy_from_slice(&depositor_bytes);
    let depositor = PublicKey::new(depositor_arr);

    let block_height = state.current_block().await;
    let mut pool = state.stability_pool.write().await;

    match pool.deposit(depositor, TokenAmount::from_cents(req.amount_cents), block_height) {
        Ok(_) => {
            info!("Stability pool deposit: {} cents from {}", req.amount_cents, req.depositor);
            Json(ApiResponse::ok("Deposit successful".to_string()))
        }
        Err(e) => Json(ApiResponse::err(format!("Deposit failed: {}", e))),
    }
}

/// POST /block - Advance block height (for testing/simulation)
async fn advance_block(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut block_height = state.block_height.write().await;
    *block_height += 1;
    Json(ApiResponse::ok(*block_height))
}

// ═══════════════════════════════════════════════════════════════════════════════
// MAIN
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // Create shared state
    let state = Arc::new(AppState::new());

    // Initialize with default price
    {
        let mut price_feed = state.price_feed.write().await;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let price_data = zkusd::oracle::price_feed::PriceData::new(10_000_000, timestamp, 3);
        price_feed.update(price_data);
    }

    // Build router
    let app = Router::new()
        // Health & Status
        .route("/health", get(health_check))
        .route("/status", get(get_status))

        // Price
        .route("/price", get(get_price))
        .route("/price", post(update_price))

        // CDP operations
        .route("/cdp", post(open_cdp))
        .route("/cdp/:id", get(get_cdp))
        .route("/cdps", get(list_cdps))
        .route("/cdp/:id/deposit", post(deposit_collateral))
        .route("/cdp/:id/withdraw", post(withdraw_collateral))
        .route("/cdp/:id/mint", post(mint_debt))
        .route("/cdp/:id/repay", post(repay_debt))
        .route("/cdp/:id/close", post(close_cdp))

        // Token operations
        .route("/token/balance/:address", get(get_balance))
        .route("/token/supply", get(get_supply))

        // Stability pool
        .route("/pool/status", get(get_pool_status))
        .route("/pool/deposit", post(pool_deposit))

        // Admin/Testing
        .route("/block", post(advance_block))

        // Middleware
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    // Get bind address from env or default
    let addr: SocketAddr = std::env::var("ZKUSD_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()
        .expect("Invalid bind address");

    info!("Starting zkUSD server on {}", addr);
    info!("API endpoints:");
    info!("  GET  /health              - Health check");
    info!("  GET  /status              - Protocol status");
    info!("  GET  /price               - Current BTC price");
    info!("  POST /price               - Update price");
    info!("  POST /cdp                 - Open new CDP");
    info!("  GET  /cdp/:id             - Get CDP info");
    info!("  GET  /cdps                - List all CDPs");
    info!("  POST /cdp/:id/deposit     - Deposit collateral");
    info!("  POST /cdp/:id/withdraw    - Withdraw collateral");
    info!("  POST /cdp/:id/mint        - Mint debt");
    info!("  POST /cdp/:id/repay       - Repay debt");
    info!("  POST /cdp/:id/close       - Close CDP");
    info!("  GET  /token/balance/:addr - Get balance");
    info!("  GET  /token/supply        - Get total supply");
    info!("  GET  /pool/status         - Stability pool status");
    info!("  POST /pool/deposit        - Deposit to pool");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
