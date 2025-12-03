# zkUSD Development Plan

## Project Philosophy

**"Todas las implementaciones deben ser para producción"** - All implementations must be production-ready, NO mocks.

## Completed Features

### 1. Core Protocol (Phase 1) ✅

#### CDP Management (`src/core/cdp.rs`)
- CDP creation with unique ID generation
- Collateral deposits and withdrawals
- Debt minting and repayment
- Collateral ratio calculations
- CDP state tracking (Active, Closed, Liquidated)

#### Token System (`src/core/token.rs`)
- zkUSD token with ERC20-like interface
- Balance tracking
- Transfer and approval mechanisms
- Supply management

#### Vault (`src/core/vault.rs`)
- Collateral tracking
- Global collateral statistics

### 2. Liquidation System (Phase 1) ✅

#### Stability Pool (`src/liquidation/stability_pool.rs`)
- zkUSD deposits
- Proportional debt absorption
- BTC gain distribution (epoch-based)
- Snapshot system for fair rewards

#### Liquidation Engine (`src/liquidation/engine.rs`)
- Undercollateralized CDP detection
- Stability pool absorption
- Redistribution fallback
- Liquidation bonus calculation (10%)

### 3. Protocol State Machine (Phase 1) ✅

#### State Machine (`src/protocol/state_machine.rs`)
- Atomic operation processing
- State root computation
- Block height tracking
- Recovery mode detection

### 4. Zero-Knowledge Proofs (Phase 2) ✅

#### Circuit Definitions (`src/zkp/circuits.rs`)
- Deposit circuit
- Withdraw circuit (with ratio validation)
- Mint circuit (with MCR check)
- Repay circuit
- Liquidation circuit
- Price attestation circuit

#### Native Prover (`src/zkp/prover.rs`)
- Circuit execution
- Proof caching
- Backend abstraction

#### SP1 Integration (`src/zkp/sp1_prover.rs`)
- SP1 SDK integration
- ELF registry for guest programs
- Network and local proving modes
- Proof compression

#### Guest Programs (`guest/src/`)
- RISC-V programs for SP1 zkVM
- All major circuits implemented

### 5. Bitcoin Integration (Phase 2) ✅

#### UTXO Management (`src/btc/utxo.rs`)
- UTXO tracking
- Selection strategies (LargestFirst, BestMatch, etc.)
- CDP association
- Locking mechanism

#### Script Building (`src/btc/scripts.rs`)
- P2WPKH scripts
- Multisig scripts
- Timelock scripts
- OP_RETURN for protocol data

#### Transaction Builder (`src/btc/tx_builder.rs`)
- Transaction templates
- Fee estimation
- Protocol operations (deposit, withdraw, liquidation)

### 6. Storage Layer (Phase 2) ✅

#### Backend Abstraction (`src/storage/backend.rs`)
- StorageBackend trait
- InMemoryStore (testing)
- FileStore (JSON)
- BinaryStore (compact)

#### RocksDB Backend (`src/storage/rocks.rs`)
- Column families for data organization
- Batch writes
- Compression and bloom filters
- Configuration profiles (SSD, low-memory)

### 7. Oracle System (Phase 2) ✅

#### Price Fetchers (`src/oracle/fetchers.rs`)
- Binance, Coinbase, Kraken, Bitstamp, OKX, Bybit
- Concurrent fetching
- Retry logic
- Volume tracking

#### Async Service (`src/oracle/service.rs`)
- Background price updates
- Price validation (deviation, staleness)
- Broadcast channel for subscribers
- Statistics and monitoring

### 8. RPC Server (Phase 2) ✅

#### HTTP API (`src/bin/server.rs`)
- Health and status endpoints
- CDP CRUD operations
- Price endpoint
- Stability pool operations
- CORS and compression middleware

### 9. Charms Integration (Phase 1) ✅

#### Adapter (`src/charms/adapter.rs`)
- Protocol adapter for BitcoinOS
- Spell processing

#### Spells (`src/charms/spells.rs`)
- All operation types defined
- Signature verification

---

## Pending Features

### 10. Recovery Mode (Phase 3) ✅

When Total Collateral Ratio (TCR) < 150%, the protocol enters recovery mode with special rules:

**Implementation:**
- RecoveryModeManager with TCR calculations and validations
- RecoveryModeStatus for detailed system status reporting
- Operation validation (mint, withdrawal, CDP opening)
- SortedCDPs for efficient CDP ordering by ratio
- At-risk metrics calculation (CDPs and debt at risk)
- Recovery mode event history tracking
- Integration with ProtocolStateMachine

**Files:**
- `src/liquidation/recovery.rs`
- `src/protocol/state_machine.rs` (integrated)

### 11. Event Indexing System (Phase 3) ✅

System for tracking and indexing protocol events for blockchain explorers and analytics.

**Implementation:**
- All event types defined (CDPOpened, CollateralDeposited, etc.)
- In-memory event store with indexes (by block, type, CDP, account)
- Persistent RocksDB storage (optional, with feature flag)
- Event querying API with filters and pagination
- Real-time subscription system (with async-oracle feature)
- Event statistics and monitoring

**Files:**
- `src/protocol/events.rs` (event types)
- `src/events/mod.rs` (module root)
- `src/events/storage.rs` (storage backends)
- `src/events/indexer.rs` (main indexer API)

---

### 12. BitcoinOS Integration (Phase 4) ✅

Full integration with BitcoinOS Charms token standard.

**Implementation:**
- BitcoinOSExecutor for processing Charm spells
- ExecutionContext and ExecutionResult types
- UtxoTracker for UTXO management
- Full spell parameter types for all operations
- CDP operations (OpenCDP, CloseCDP, Deposit, Withdraw, Mint, Repay)
- Liquidation and redemption support
- Stability pool operations (Deposit, Withdraw, ClaimGains)

**Files:**
- `src/charms/executor.rs` (main executor)
- `src/charms/spells.rs` (spell parameters and builders)
- `src/charms/adapter.rs` (protocol adapter)

---

### 13. On-Chain Governance (Phase 5) ✅

Complete on-chain governance system with proposals, voting, and execution.

**Implementation:**
- GovernanceSystem coordinator with config and all subsystems
- Proposal management (ProposalId, ProposalStatus, ProposalManager)
- Token-weighted voting system with delegation support
- VoteTally for tracking votes (for/against/abstain)
- Timelock queue for delayed execution
- GovernanceExecutor for approved operations
- ProtocolParameter enum for all governable parameters
- GovernanceOperation enum for all possible operations

**Features:**
- Proposal creation with voting threshold
- Delegate voting power to other addresses
- Quorum requirement (4% of total supply default)
- Timelock delay (2 days minimum, 30 days maximum)
- Guardian role for emergency actions
- Comprehensive test coverage

**Files:**
- `src/governance/mod.rs` (main coordinator)
- `src/governance/proposal.rs` (proposal management)
- `src/governance/voting.rs` (voting system)
- `src/governance/timelock.rs` (execution queue)
- `src/governance/executor.rs` (operation execution)
- `src/governance/parameters.rs` (protocol parameters)

---

### 14. Rate Limiting & DDoS Protection (Phase 5) ✅

Production-grade rate limiting for RPC API protection.

**Implementation:**
- Token bucket algorithm with configurable rates
- Per-IP rate limiting
- Per-API-key rate limiting with configurable quotas
- IP whitelist for trusted sources
- IP blacklist for known attackers
- Global connection limiting
- Burst allowance for temporary spikes
- Statistics and monitoring

**Configuration:**
- Default: 100 req/sec per IP, 1000 req/sec per API key
- Whitelist bypass for operators
- Cleanup interval for stale entries
- Configurable burst allowance

**Files:**
- `src/rpc/mod.rs` (module root)
- `src/rpc/rate_limiter.rs` (rate limiting logic)
- `src/rpc/middleware.rs` (Axum middleware, feature-gated)

---

### 15. Dynamic Fee System (Phase 5) ✅

Dynamic fees that adjust based on protocol utilization and market conditions.

**Implementation:**
- Base rate with time decay (decays 1% per 12 hours of inactivity)
- Utilization-based fee premium (up to 5x at high utilization)
- Separate borrowing and redemption fee calculations
- Fee floor (0.5%) and ceiling (5%) for borrowing
- Redemption fee decay based on time since last redemption
- Fee statistics tracking (total collected, fee counts, averages)
- Configurable parameters for all fee calculations

**Features:**
- `calculate_borrowing_fee()` - Based on utilization and base rate
- `calculate_redemption_fee()` - Based on redemption volume and time
- `decay_base_rate()` - Automatic rate decay over time
- `record_borrowing()` / `record_redemption()` - Activity tracking

**Files:**
- `src/core/fees.rs` (fee calculator)
- `src/core/mod.rs` (module integration)

---

### 16. Monitoring & Alerting System (Phase 5) ✅

Comprehensive protocol monitoring with alerting and health scoring.

**Metrics Collection:**
- MetricType enum (25+ protocol metrics)
- MetricTimeSeries for historical tracking
- MetricsCollector for aggregation
- Counter and Gauge atomic metric types
- Rate of change calculations

**Alert System:**
- AlertSeverity (Info, Warning, Critical, Emergency)
- AlertType for all protocol alerts
- AlertRule with configurable conditions
- AlertCondition (Above, Below, Equals, ChangeExceeds, RateExceeds)
- AlertManager with default production rules
- NotificationDispatcher for multi-channel alerts
- Notification channels: Log, Webhook, Email, Telegram, Discord, PagerDuty

**Health Scoring:**
- HealthStatus (Healthy, Degraded, Warning, Critical, Emergency)
- HealthComponent (Collateralization, Oracle, StabilityPool, Liquidation, Performance, Governance)
- ComponentScore with weighted factors
- HealthChecker for computing overall health
- HealthReport with recommendations
- Auto-resolve for transient alerts

**Files:**
- `src/monitoring/mod.rs` (module root)
- `src/monitoring/metrics.rs` (metrics collection)
- `src/monitoring/alerts.rs` (alerting system)
- `src/monitoring/health.rs` (health scoring)

---

### 17. CLI Tools (Phase 5) ✅

Command-line interface for protocol operators.

**Commands:**
- `status` - Protocol overview, health, metrics, alerts
- `cdp` - List, get, find risky/liquidatable CDPs, calculate ratios
- `oracle` - Price, sources, history, health
- `pool` - Stability pool status, depositors, gains
- `governance` - Proposals, voting power, parameters
- `config` - Show, set, validate configuration
- `backup` - Create, restore, list, verify backups
- `monitor` - Dashboard, watch metrics, export, alert rules

**Output Formats:**
- Text (human-readable with colors)
- JSON / JSON-Pretty
- Table
- Minimal (values only)

**Files:**
- `src/cli/mod.rs` (app entry)
- `src/cli/config.rs` (configuration)
- `src/cli/output.rs` (formatters)
- `src/cli/commands.rs` (all commands)

---

### 18. Circuit Breaker Pattern (Phase 5) ✅

Fault tolerance for external dependencies.

**Implementation:**
- CircuitBreaker with Closed/Open/HalfOpen states
- Configurable failure thresholds and success thresholds
- Automatic timeout and recovery
- CircuitBreakerRegistry for multiple services
- Statistics tracking (success rate, failure rate, rejections)

**Configuration Presets:**
- Default: 5 failures to open, 3 successes to close, 30s timeout
- Strict: 3 failures, 5 successes, 60s timeout
- Relaxed: 10 failures, 2 successes, 15s timeout

**Files:**
- `src/utils/circuit_breaker.rs`

---

### 19. Backup & Restore System (Phase 5) ✅

Disaster recovery capabilities for protocol data.

**Implementation:**
- BackupFile with metadata and records
- SHA256 checksum verification
- Multiple data types (CDPs, Vault, Balances, StabilityPool, Prices, Config, Events, Governance)
- BackupManager with automatic scheduling
- Configurable retention (max backups)
- Backup verification and integrity checking

**Features:**
- Binary backup format with magic bytes
- Backup statistics (count, size, oldest/newest)
- Auto-cleanup of old backups
- Block-height based scheduling

**Files:**
- `src/storage/backup.rs`

---

## Future Phases

### Phase 6: Testnet Deployment
- Testnet deployment on BitcoinOS
- Cross-contract calls
- Integration testing with real Bitcoin testnet
- Performance benchmarking

### Phase 7: Security & Audit
- Security audit by third party
- Formal verification of critical paths
- Bug bounty program
- Penetration testing

### Phase 8: Mainnet
- Mainnet deployment
- Liquidity bootstrapping
- Governance activation
- Monitoring dashboard

---

## Technical Debt

1. **Unused imports** - Clean up warnings
2. **Missing documentation** - Add docs to all public items
3. **Test coverage** - Add more unit tests for edge cases
4. **Error messages** - Improve error context

---

## Build Commands

```bash
# Development build
cargo build

# Release build with all features
cargo build --release --features full

# Run tests
cargo test

# Run RPC server
cargo run --features rpc-server --bin zkusd-server

# Build SP1 guest programs
cd guest && cargo build --release
```

---

## Configuration

### Environment Variables

```bash
# RPC Server
ZKUSD_HOST=127.0.0.1
ZKUSD_PORT=3000

# Oracle
ZKUSD_ORACLE_INTERVAL=30  # seconds
ZKUSD_ORACLE_MIN_SOURCES=3

# Storage
ZKUSD_DATA_DIR=/var/lib/zkusd

# ZK Prover
SP1_PROVER=network  # or "local"
SP1_PRIVATE_KEY=your_api_key
```

---

## Contact

- Repository: https://github.com/AndeLabs/zkUSD
- Branch: `claude/zkusd-stablecoin-01E3efADMYT3yecFF3be76ku`
