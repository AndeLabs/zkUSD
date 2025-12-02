# zkUSD - Decentralized Stablecoin on BitcoinOS

A production-ready decentralized stablecoin protocol backed by Bitcoin, built on BitcoinOS with zkBTC collateral and Charms token standard.

## Overview

zkUSD is a CDP-based (Collateralized Debt Position) stablecoin where users can:
- Lock Bitcoin (via zkBTC) as collateral
- Mint zkUSD stablecoins against their collateral
- Participate in the stability pool for liquidation rewards
- Redeem zkUSD for underlying collateral

All operations are verified using zero-knowledge proofs for trustless execution on Bitcoin.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        zkUSD Protocol                            │
├─────────────────────────────────────────────────────────────────┤
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────────┐ │
│  │   CDP    │  │ Stability│  │  Oracle  │  │   Liquidation    │ │
│  │ Manager  │  │   Pool   │  │  Service │  │     Engine       │ │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────────┬─────────┘ │
│       │             │             │                  │           │
├───────┴─────────────┴─────────────┴──────────────────┴───────────┤
│                      Protocol State Machine                       │
├──────────────────────────────────────────────────────────────────┤
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────────┐ │
│  │   ZKP    │  │  Charms  │  │  Bitcoin │  │     Storage      │ │
│  │  Prover  │  │  Adapter │  │TX Builder│  │    (RocksDB)     │ │
│  └──────────┘  └──────────┘  └──────────┘  └──────────────────┘ │
└──────────────────────────────────────────────────────────────────┘
```

## Features

### Core Protocol
- **CDP Management**: Open, manage, and close collateralized debt positions
- **Stability Pool**: Deposit zkUSD to earn liquidation rewards
- **Liquidations**: Automatic liquidation of undercollateralized positions
- **Redemptions**: Redeem zkUSD for BTC at face value

### Production Infrastructure
- **SP1 zkVM Integration**: Zero-knowledge proofs using Succinct's SP1
- **RocksDB Storage**: High-performance persistent storage
- **Async Oracle Service**: Real-time price feeds from 6 exchanges
- **Bitcoin TX Builder**: Native Bitcoin transaction construction
- **HTTP/JSON API**: Full-featured RPC server

## Installation

```bash
# Clone the repository
git clone https://github.com/AndeLabs/zkUSD
cd zkUSD

# Build with all features
cargo build --release --features full

# Build with specific features
cargo build --release --features "async-oracle,rpc-server"
```

## Features Flags

| Feature | Description |
|---------|-------------|
| `std` | Standard library (default) |
| `async-oracle` | Async price fetching from exchanges |
| `rpc-server` | HTTP/JSON API server |
| `sp1-prover` | SP1 zkVM for production proofs |
| `rocksdb-storage` | RocksDB persistent storage |
| `full` | All features enabled |

## Quick Start

### Running the RPC Server

```bash
# Start the server
cargo run --release --features rpc-server --bin zkusd-server

# The server will be available at http://127.0.0.1:3000
```

### API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/status` | GET | Protocol status |
| `/price` | GET | Current BTC price |
| `/cdp` | POST | Open new CDP |
| `/cdp/{id}` | GET | Get CDP details |
| `/cdp/{id}/deposit` | POST | Deposit collateral |
| `/cdp/{id}/withdraw` | POST | Withdraw collateral |
| `/cdp/{id}/mint` | POST | Mint zkUSD |
| `/cdp/{id}/repay` | POST | Repay debt |
| `/pool/status` | GET | Stability pool status |
| `/pool/deposit` | POST | Deposit to stability pool |

## Project Structure

```
zkusd/
├── src/
│   ├── bin/
│   │   ├── zkusd.rs          # CLI binary
│   │   └── server.rs         # RPC server
│   ├── btc/                  # Bitcoin integration
│   │   ├── mod.rs
│   │   ├── utxo.rs           # UTXO management
│   │   ├── scripts.rs        # Script builders
│   │   └── tx_builder.rs     # Transaction construction
│   ├── charms/               # Charms token integration
│   │   ├── mod.rs
│   │   ├── adapter.rs        # Protocol adapter
│   │   ├── spells.rs         # Spell definitions
│   │   └── token.rs          # Token management
│   ├── core/                 # Core protocol logic
│   │   ├── mod.rs
│   │   ├── cdp.rs            # CDP manager
│   │   ├── token.rs          # zkUSD token
│   │   └── vault.rs          # Collateral vault
│   ├── liquidation/          # Liquidation system
│   │   ├── mod.rs
│   │   ├── engine.rs         # Liquidation engine
│   │   └── stability_pool.rs # Stability pool
│   ├── oracle/               # Price feeds
│   │   ├── mod.rs
│   │   ├── aggregator.rs     # Price aggregation
│   │   ├── fetchers.rs       # Exchange fetchers
│   │   ├── price_feed.rs     # Price feed manager
│   │   ├── service.rs        # Async oracle service
│   │   └── sources.rs        # Price sources
│   ├── protocol/             # Protocol state machine
│   │   ├── mod.rs
│   │   └── state_machine.rs  # Main state machine
│   ├── spells/               # Protocol operations
│   │   ├── mod.rs
│   │   ├── cdp_spells.rs     # CDP operations
│   │   └── redemption.rs     # Redemption logic
│   ├── storage/              # Persistence
│   │   ├── mod.rs
│   │   ├── backend.rs        # Storage backends
│   │   ├── rocks.rs          # RocksDB backend
│   │   └── state.rs          # State persistence
│   ├── utils/                # Utilities
│   │   ├── mod.rs
│   │   ├── constants.rs      # Protocol constants
│   │   ├── crypto.rs         # Cryptographic primitives
│   │   ├── math.rs           # Safe math operations
│   │   └── validation.rs     # Input validation
│   ├── zkp/                  # Zero-knowledge proofs
│   │   ├── mod.rs
│   │   ├── circuits.rs       # Circuit definitions
│   │   ├── inputs.rs         # Proof inputs
│   │   ├── prover.rs         # Prover implementations
│   │   ├── sp1_prover.rs     # SP1 zkVM integration
│   │   └── verifier.rs       # Proof verification
│   ├── error.rs              # Error types
│   └── lib.rs                # Library root
├── guest/                    # SP1 guest programs
│   ├── Cargo.toml
│   └── src/
│       ├── common.rs         # Shared types
│       ├── deposit.rs        # Deposit circuit
│       ├── withdraw.rs       # Withdraw circuit
│       ├── mint.rs           # Mint circuit
│       ├── repay.rs          # Repay circuit
│       ├── liquidation.rs    # Liquidation circuit
│       └── price_attestation.rs # Price circuit
├── tests/
│   └── integration_tests.rs  # Integration tests
├── Cargo.toml
└── README.md
```

## Protocol Constants

| Constant | Value | Description |
|----------|-------|-------------|
| Minimum Collateral Ratio | 150% | MCR for minting |
| Critical Collateral Ratio | 150% | Triggers recovery mode |
| Liquidation Bonus | 10% | Bonus for liquidators |
| Minting Fee | 0.5% | One-time fee on debt |
| Redemption Fee | 0.5-5% | Dynamic fee based on base rate |
| Minimum Debt | $200 | Minimum debt per CDP |

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run with all features
cargo test --features full

# Run specific test
cargo test test_cdp_lifecycle
```

### Building Guest Programs (SP1)

```bash
cd guest
cargo build --release --target riscv32im-succinct-zkvm-elf
```

## Supported Exchanges (Oracle)

| Exchange | Endpoint |
|----------|----------|
| Binance | `api.binance.com` |
| Coinbase | `api.coinbase.com` |
| Kraken | `api.kraken.com` |
| Bitstamp | `bitstamp.net` |
| OKX | `okx.com` |
| Bybit | `api.bybit.com` |

## Security Considerations

- All CDPs must maintain minimum 150% collateral ratio
- Liquidations are incentivized with 10% bonus
- Price feeds require minimum 3 sources with <5% deviation
- Zero-knowledge proofs verify all state transitions
- Recovery mode activates when system TCR < 150%

## License

MIT License

## Contributing

Contributions are welcome! Please read our contributing guidelines before submitting PRs.

## Roadmap

- [x] Core CDP Protocol
- [x] Stability Pool
- [x] Liquidation Engine
- [x] SP1 ZK Integration
- [x] RocksDB Storage
- [x] Async Oracle Service
- [x] RPC Server
- [x] Bitcoin TX Builder
- [ ] Recovery Mode
- [ ] Event Indexing System
- [ ] BitcoinOS Integration
- [ ] Mainnet Deployment
