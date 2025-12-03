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

## Future Phases

### Phase 5: Testnet Deployment
- Testnet deployment
- Cross-contract calls
- Integration testing

### Phase 6: Security & Audit
- Security audit
- Formal verification of critical paths
- Bug bounty program

### Phase 7: Mainnet
- Mainnet deployment
- Liquidity bootstrapping
- Governance setup

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
