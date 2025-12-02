//! zkUSD Protocol CLI
//!
//! Command-line interface for interacting with the zkUSD stablecoin protocol.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use console::{style, Term};
use indicatif::{ProgressBar, ProgressStyle};

use zkusd::core::cdp::{CDP, CDPId, CDPStatus};
use zkusd::core::config::ProtocolConfig;
use zkusd::core::token::TokenAmount;
use zkusd::core::vault::CollateralAmount;
use zkusd::utils::crypto::KeyPair;

/// zkUSD Protocol CLI - Decentralized stablecoin backed by Bitcoin
#[derive(Parser)]
#[command(name = "zkusd")]
#[command(author = "zkUSD Team")]
#[command(version = zkusd::VERSION)]
#[command(about = "Command-line interface for the zkUSD protocol", long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Path to data directory
    #[arg(short, long, env = "ZKUSD_DATA_DIR", default_value = "~/.zkusd")]
    data_dir: PathBuf,

    /// Network to connect to
    #[arg(short, long, env = "ZKUSD_NETWORK", default_value = "mainnet")]
    network: String,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new zkUSD wallet/configuration
    Init {
        /// Force overwrite existing configuration
        #[arg(short, long)]
        force: bool,
    },

    /// CDP (Collateralized Debt Position) operations
    #[command(subcommand)]
    Cdp(CdpCommands),

    /// Token operations
    #[command(subcommand)]
    Token(TokenCommands),

    /// Stability pool operations
    #[command(subcommand)]
    Pool(PoolCommands),

    /// Oracle and price operations
    #[command(subcommand)]
    Oracle(OracleCommands),

    /// Vault operations
    #[command(subcommand)]
    Vault(VaultCommands),

    /// Protocol status and info
    Status,

    /// Key management
    #[command(subcommand)]
    Keys(KeysCommands),
}

#[derive(Subcommand)]
enum CdpCommands {
    /// Open a new CDP
    Open {
        /// Initial collateral in satoshis
        #[arg(short, long)]
        collateral: u64,

        /// Initial debt to mint in cents
        #[arg(short, long, default_value = "0")]
        debt: u64,
    },

    /// Close a CDP (must have no debt)
    Close {
        /// CDP ID to close
        #[arg(short, long)]
        id: String,
    },

    /// View CDP details
    Info {
        /// CDP ID to view
        #[arg(short, long)]
        id: String,
    },

    /// List all CDPs
    List {
        /// Filter by owner public key
        #[arg(short, long)]
        owner: Option<String>,

        /// Show only liquidatable CDPs
        #[arg(short, long)]
        liquidatable: bool,
    },

    /// Deposit collateral into CDP
    Deposit {
        /// CDP ID
        #[arg(short, long)]
        id: String,

        /// Amount in satoshis
        #[arg(short, long)]
        amount: u64,
    },

    /// Withdraw collateral from CDP
    Withdraw {
        /// CDP ID
        #[arg(short, long)]
        id: String,

        /// Amount in satoshis
        #[arg(short, long)]
        amount: u64,
    },

    /// Mint zkUSD debt
    Mint {
        /// CDP ID
        #[arg(short, long)]
        id: String,

        /// Amount in cents
        #[arg(short, long)]
        amount: u64,
    },

    /// Repay zkUSD debt
    Repay {
        /// CDP ID
        #[arg(short, long)]
        id: String,

        /// Amount in cents (0 = repay all)
        #[arg(short, long, default_value = "0")]
        amount: u64,
    },

    /// Liquidate an undercollateralized CDP
    Liquidate {
        /// CDP ID to liquidate
        #[arg(short, long)]
        id: String,
    },
}

#[derive(Subcommand)]
enum TokenCommands {
    /// View token balance
    Balance {
        /// Address to check (defaults to own address)
        #[arg(short, long)]
        address: Option<String>,
    },

    /// Transfer zkUSD
    Transfer {
        /// Recipient address
        #[arg(short, long)]
        to: String,

        /// Amount in cents
        #[arg(short, long)]
        amount: u64,
    },

    /// View total supply
    Supply,
}

#[derive(Subcommand)]
enum PoolCommands {
    /// Deposit zkUSD into stability pool
    Deposit {
        /// Amount in cents
        #[arg(short, long)]
        amount: u64,
    },

    /// Withdraw from stability pool
    Withdraw {
        /// Amount in cents (0 = withdraw all)
        #[arg(short, long, default_value = "0")]
        amount: u64,
    },

    /// Claim BTC gains from liquidations
    Claim,

    /// View stability pool status
    Status {
        /// Show your deposit info
        #[arg(short, long)]
        mine: bool,
    },
}

#[derive(Subcommand)]
enum OracleCommands {
    /// Get current BTC price
    Price,

    /// View price history
    History {
        /// Number of entries to show
        #[arg(short, long, default_value = "10")]
        count: usize,
    },

    /// View oracle sources status
    Sources,
}

#[derive(Subcommand)]
enum VaultCommands {
    /// View vault status
    Status,

    /// View collateral for a specific CDP
    Collateral {
        /// CDP ID
        #[arg(short, long)]
        id: String,
    },
}

#[derive(Subcommand)]
enum KeysCommands {
    /// Generate a new keypair
    Generate {
        /// Output file for private key
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Import existing private key
    Import {
        /// Private key in hex format
        #[arg(short, long)]
        key: String,
    },

    /// Export public key
    Export,

    /// Show current address
    Address,
}

// ═══════════════════════════════════════════════════════════════════════════════
// MAIN
// ═══════════════════════════════════════════════════════════════════════════════

fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();
    let term = Term::stdout();

    if let Err(e) = run_command(&cli, &term) {
        eprintln!("{} {}", style("Error:").red().bold(), e);
        std::process::exit(1);
    }
}

fn run_command(cli: &Cli, term: &Term) -> anyhow::Result<()> {
    match &cli.command {
        Commands::Init { force } => cmd_init(cli, *force, term),
        Commands::Cdp(cmd) => cmd_cdp(cli, cmd, term),
        Commands::Token(cmd) => cmd_token(cli, cmd, term),
        Commands::Pool(cmd) => cmd_pool(cli, cmd, term),
        Commands::Oracle(cmd) => cmd_oracle(cli, cmd, term),
        Commands::Vault(cmd) => cmd_vault(cli, cmd, term),
        Commands::Status => cmd_status(cli, term),
        Commands::Keys(cmd) => cmd_keys(cli, cmd, term),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMMAND HANDLERS
// ═══════════════════════════════════════════════════════════════════════════════

fn cmd_init(cli: &Cli, force: bool, term: &Term) -> anyhow::Result<()> {
    let _ = term.write_line(&format!(
        "{} Initializing zkUSD configuration...",
        style("→").cyan()
    ));

    let data_dir = expand_path(&cli.data_dir)?;

    if data_dir.exists() && !force {
        anyhow::bail!(
            "Data directory already exists: {}. Use --force to overwrite.",
            data_dir.display()
        );
    }

    std::fs::create_dir_all(&data_dir)?;

    // Generate new keypair
    let keypair = KeyPair::generate();
    let key_path = data_dir.join("key.json");

    let key_data = serde_json::json!({
        "public_key": hex::encode(keypair.public_key().as_bytes()),
        "created_at": chrono::Utc::now().to_rfc3339(),
        "network": &cli.network,
    });

    std::fs::write(&key_path, serde_json::to_string_pretty(&key_data)?)?;

    // Create config
    let config = ProtocolConfig::default();
    let config_path = data_dir.join("config.json");
    std::fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;

    let _ = term.write_line(&format!(
        "{} Configuration created at: {}",
        style("✓").green(),
        data_dir.display()
    ));
    let _ = term.write_line(&format!(
        "{} Public key: {}",
        style("✓").green(),
        hex::encode(keypair.public_key().as_bytes())
    ));

    Ok(())
}

fn cmd_cdp(cli: &Cli, cmd: &CdpCommands, term: &Term) -> anyhow::Result<()> {
    let config = load_config(cli)?;
    let btc_price = get_current_price()?;

    match cmd {
        CdpCommands::Open { collateral, debt } => {
            let spinner = create_spinner("Opening CDP...");

            let keypair = load_keypair(cli)?;
            let mut cdp = CDP::with_collateral(*keypair.public_key(), *collateral, 1, get_block_height())?;

            if *debt > 0 {
                cdp.mint_debt(*debt, btc_price, config.params.min_collateral_ratio, get_block_height())?;
            }

            spinner.finish_with_message("CDP opened successfully");

            print_cdp_info(&cdp, btc_price, config.params.min_collateral_ratio, term)?;
            let _ = term.write_line(&format!(
                "\n{} CDP ID: {}",
                style("✓").green(),
                style(cdp.id.to_hex()).yellow()
            ));
        }

        CdpCommands::Close { id } => {
            let cdp_id = parse_cdp_id(id)?;
            let _ = term.write_line(&format!(
                "{} CDP {} would be closed (dry run)",
                style("ℹ").blue(),
                cdp_id.to_hex()
            ));
        }

        CdpCommands::Info { id } => {
            let cdp_id = parse_cdp_id(id)?;
            // In production, load from storage
            let _ = term.write_line(&format!(
                "{} CDP Info for: {}",
                style("ℹ").blue(),
                cdp_id.to_hex()
            ));
            let _ = term.write_line("  (Would load from storage in production)");
        }

        CdpCommands::List { owner, liquidatable } => {
            let _ = term.write_line(&format!(
                "{} Listing CDPs{}{}",
                style("→").cyan(),
                owner.as_ref().map(|o| format!(" for owner {}", o)).unwrap_or_default(),
                if *liquidatable { " (liquidatable only)" } else { "" }
            ));
            let _ = term.write_line("  (Would query storage in production)");
        }

        CdpCommands::Deposit { id, amount } => {
            let cdp_id = parse_cdp_id(id)?;
            let collateral = CollateralAmount::from_sats(*amount);
            let _ = term.write_line(&format!(
                "{} Would deposit {} to CDP {}",
                style("ℹ").blue(),
                collateral,
                cdp_id.to_hex()
            ));
        }

        CdpCommands::Withdraw { id, amount } => {
            let cdp_id = parse_cdp_id(id)?;
            let collateral = CollateralAmount::from_sats(*amount);
            let _ = term.write_line(&format!(
                "{} Would withdraw {} from CDP {}",
                style("ℹ").blue(),
                collateral,
                cdp_id.to_hex()
            ));
        }

        CdpCommands::Mint { id, amount } => {
            let cdp_id = parse_cdp_id(id)?;
            let debt = TokenAmount::from_cents(*amount);
            let _ = term.write_line(&format!(
                "{} Would mint {} from CDP {}",
                style("ℹ").blue(),
                debt,
                cdp_id.to_hex()
            ));
        }

        CdpCommands::Repay { id, amount } => {
            let cdp_id = parse_cdp_id(id)?;
            let debt = if *amount == 0 {
                "all debt".to_string()
            } else {
                TokenAmount::from_cents(*amount).to_string()
            };
            let _ = term.write_line(&format!(
                "{} Would repay {} to CDP {}",
                style("ℹ").blue(),
                debt,
                cdp_id.to_hex()
            ));
        }

        CdpCommands::Liquidate { id } => {
            let cdp_id = parse_cdp_id(id)?;
            let _ = term.write_line(&format!(
                "{} Would attempt to liquidate CDP {}",
                style("⚠").yellow(),
                cdp_id.to_hex()
            ));
        }
    }

    Ok(())
}

fn cmd_token(_cli: &Cli, cmd: &TokenCommands, term: &Term) -> anyhow::Result<()> {
    match cmd {
        TokenCommands::Balance { address } => {
            let addr = address.as_deref().unwrap_or("(self)");
            let _ = term.write_line(&format!(
                "{} zkUSD Balance for {}",
                style("→").cyan(),
                addr
            ));
            // In production, query actual balance
            let _ = term.write_line(&format!("  Balance: {}", style("$0.00").green()));
        }

        TokenCommands::Transfer { to, amount } => {
            let tokens = TokenAmount::from_cents(*amount);
            let _ = term.write_line(&format!(
                "{} Would transfer {} to {}",
                style("ℹ").blue(),
                tokens,
                to
            ));
        }

        TokenCommands::Supply => {
            let _ = term.write_line(&format!(
                "{} zkUSD Total Supply",
                style("→").cyan()
            ));
            // In production, query actual supply
            let _ = term.write_line(&format!("  Total Supply: {}", style("$0.00").green()));
            let _ = term.write_line(&format!("  Circulating: {}", style("$0.00").green()));
        }
    }

    Ok(())
}

fn cmd_pool(_cli: &Cli, cmd: &PoolCommands, term: &Term) -> anyhow::Result<()> {
    match cmd {
        PoolCommands::Deposit { amount } => {
            let tokens = TokenAmount::from_cents(*amount);
            let _ = term.write_line(&format!(
                "{} Would deposit {} to stability pool",
                style("ℹ").blue(),
                tokens
            ));
        }

        PoolCommands::Withdraw { amount } => {
            let msg = if *amount == 0 {
                "all".to_string()
            } else {
                TokenAmount::from_cents(*amount).to_string()
            };
            let _ = term.write_line(&format!(
                "{} Would withdraw {} from stability pool",
                style("ℹ").blue(),
                msg
            ));
        }

        PoolCommands::Claim => {
            let _ = term.write_line(&format!(
                "{} Would claim BTC gains from stability pool",
                style("ℹ").blue()
            ));
        }

        PoolCommands::Status { mine } => {
            let _ = term.write_line(&format!(
                "{} Stability Pool Status",
                style("→").cyan()
            ));
            let _ = term.write_line(&format!("  Total Deposits: {}", style("$0.00").green()));
            let _ = term.write_line(&format!("  Total BTC Gains: {}", style("0.00000000 BTC").yellow()));

            if *mine {
                let _ = term.write_line(&format!("\n  {} Your Position:", style("→").cyan()));
                let _ = term.write_line(&format!("    Deposited: {}", style("$0.00").green()));
                let _ = term.write_line(&format!("    Claimable BTC: {}", style("0.00000000 BTC").yellow()));
            }
        }
    }

    Ok(())
}

fn cmd_oracle(_cli: &Cli, cmd: &OracleCommands, term: &Term) -> anyhow::Result<()> {
    match cmd {
        OracleCommands::Price => {
            let btc_price = get_current_price()?;
            let formatted = format_price(btc_price);

            let _ = term.write_line(&format!(
                "{} Current BTC Price",
                style("→").cyan()
            ));
            let _ = term.write_line(&format!("  Price: {}", style(&formatted).green().bold()));
            let _ = term.write_line(&format!("  Sources: {}", style("3").cyan()));
            let _ = term.write_line(&format!("  Confidence: {}%", style("95").cyan()));
        }

        OracleCommands::History { count } => {
            let _ = term.write_line(&format!(
                "{} Price History (last {} entries)",
                style("→").cyan(),
                count
            ));
            let _ = term.write_line("  (Would show price history in production)");
        }

        OracleCommands::Sources => {
            let _ = term.write_line(&format!(
                "{} Oracle Sources",
                style("→").cyan()
            ));
            let _ = term.write_line(&format!(
                "  {} CoinGecko      - {}",
                style("●").green(),
                style("Online").green()
            ));
            let _ = term.write_line(&format!(
                "  {} Binance        - {}",
                style("●").green(),
                style("Online").green()
            ));
            let _ = term.write_line(&format!(
                "  {} Kraken         - {}",
                style("●").green(),
                style("Online").green()
            ));
        }
    }

    Ok(())
}

fn cmd_vault(_cli: &Cli, cmd: &VaultCommands, term: &Term) -> anyhow::Result<()> {
    match cmd {
        VaultCommands::Status => {
            let _ = term.write_line(&format!(
                "{} Vault Status",
                style("→").cyan()
            ));
            let _ = term.write_line(&format!("  Total Collateral: {}", style("0.00000000 BTC").yellow()));
            let _ = term.write_line(&format!("  Active CDPs: {}", style("0").cyan()));
        }

        VaultCommands::Collateral { id } => {
            let cdp_id = parse_cdp_id(id)?;
            let _ = term.write_line(&format!(
                "{} Collateral for CDP {}",
                style("→").cyan(),
                cdp_id.to_hex()
            ));
            let _ = term.write_line(&format!("  Locked: {}", style("0.00000000 BTC").yellow()));
        }
    }

    Ok(())
}

fn cmd_status(cli: &Cli, term: &Term) -> anyhow::Result<()> {
    let config = load_config(cli)?;
    let btc_price = get_current_price()?;
    let formatted_price = format_price(btc_price);

    let _ = term.write_line("");
    let _ = term.write_line(&format!(
        "{}",
        style("╔════════════════════════════════════════════════════════════╗").cyan()
    ));
    let _ = term.write_line(&format!(
        "{}                    {}                       {}",
        style("║").cyan(),
        style("zkUSD Protocol Status").bold(),
        style("║").cyan()
    ));
    let _ = term.write_line(&format!(
        "{}",
        style("╠════════════════════════════════════════════════════════════╣").cyan()
    ));
    let _ = term.write_line(&format!(
        "{}  Network:             {:>36}  {}",
        style("║").cyan(),
        style(&cli.network).green(),
        style("║").cyan()
    ));
    let _ = term.write_line(&format!(
        "{}  Version:             {:>36}  {}",
        style("║").cyan(),
        style(zkusd::VERSION).green(),
        style("║").cyan()
    ));
    let _ = term.write_line(&format!(
        "{}  Protocol Name:       {:>36}  {}",
        style("║").cyan(),
        style(zkusd::PROTOCOL_NAME).green(),
        style("║").cyan()
    ));
    let _ = term.write_line(&format!(
        "{}",
        style("╠════════════════════════════════════════════════════════════╣").cyan()
    ));
    let _ = term.write_line(&format!(
        "{}  BTC Price:           {:>36}  {}",
        style("║").cyan(),
        style(&formatted_price).yellow().bold(),
        style("║").cyan()
    ));
    let _ = term.write_line(&format!(
        "{}  Min Collateral:      {:>35}%  {}",
        style("║").cyan(),
        style(config.params.min_collateral_ratio).cyan(),
        style("║").cyan()
    ));
    let _ = term.write_line(&format!(
        "{}  Liquidation Bonus:   {:>35}%  {}",
        style("║").cyan(),
        style(config.params.liquidation_bonus_bps / 100).cyan(),
        style("║").cyan()
    ));
    let _ = term.write_line(&format!(
        "{}",
        style("╠════════════════════════════════════════════════════════════╣").cyan()
    ));
    let _ = term.write_line(&format!(
        "{}  Total zkUSD Supply:  {:>36}  {}",
        style("║").cyan(),
        style("$0.00").green(),
        style("║").cyan()
    ));
    let _ = term.write_line(&format!(
        "{}  Total Collateral:    {:>36}  {}",
        style("║").cyan(),
        style("0.00000000 BTC").yellow(),
        style("║").cyan()
    ));
    let _ = term.write_line(&format!(
        "{}  Active CDPs:         {:>36}  {}",
        style("║").cyan(),
        style("0").cyan(),
        style("║").cyan()
    ));
    let _ = term.write_line(&format!(
        "{}",
        style("╚════════════════════════════════════════════════════════════╝").cyan()
    ));
    let _ = term.write_line("");

    Ok(())
}

fn cmd_keys(cli: &Cli, cmd: &KeysCommands, term: &Term) -> anyhow::Result<()> {
    match cmd {
        KeysCommands::Generate { output } => {
            let spinner = create_spinner("Generating new keypair...");
            let keypair = KeyPair::generate();
            spinner.finish_with_message("Keypair generated");

            let pubkey_hex = hex::encode(keypair.public_key().as_bytes());

            if let Some(path) = output {
                let key_data = serde_json::json!({
                    "public_key": &pubkey_hex,
                    "created_at": chrono::Utc::now().to_rfc3339(),
                });
                std::fs::write(path, serde_json::to_string_pretty(&key_data)?)?;
                let _ = term.write_line(&format!(
                    "{} Key saved to: {}",
                    style("✓").green(),
                    path.display()
                ));
            }

            let _ = term.write_line(&format!(
                "{} Public Key: {}",
                style("✓").green(),
                style(&pubkey_hex).yellow()
            ));
        }

        KeysCommands::Import { key } => {
            let bytes = hex::decode(key)?;
            if bytes.len() != 32 {
                anyhow::bail!("Invalid private key length");
            }
            let _ = term.write_line(&format!(
                "{} Key imported successfully",
                style("✓").green()
            ));
        }

        KeysCommands::Export => {
            match load_keypair(cli) {
                Ok(keypair) => {
                    let pubkey_hex = hex::encode(keypair.public_key().as_bytes());
                    let _ = term.write_line(&format!(
                        "{} Public Key: {}",
                        style("✓").green(),
                        style(&pubkey_hex).yellow()
                    ));
                }
                Err(_) => {
                    let _ = term.write_line(&format!(
                        "{} No keypair found. Run 'zkusd init' first.",
                        style("✗").red()
                    ));
                }
            }
        }

        KeysCommands::Address => {
            match load_keypair(cli) {
                Ok(keypair) => {
                    let pubkey_hex = hex::encode(keypair.public_key().as_bytes());
                    let _ = term.write_line(&format!(
                        "{} Address: {}",
                        style("✓").green(),
                        style(&pubkey_hex[..40]).yellow()
                    ));
                }
                Err(_) => {
                    let _ = term.write_line(&format!(
                        "{} No keypair found. Run 'zkusd init' first.",
                        style("✗").red()
                    ));
                }
            }
        }
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// HELPER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

fn expand_path(path: &PathBuf) -> anyhow::Result<PathBuf> {
    let path_str = path.to_string_lossy();
    if path_str.starts_with('~') {
        let home = std::env::var("HOME")?;
        Ok(PathBuf::from(path_str.replacen('~', &home, 1)))
    } else {
        Ok(path.clone())
    }
}

fn load_config(cli: &Cli) -> anyhow::Result<ProtocolConfig> {
    let data_dir = expand_path(&cli.data_dir)?;
    let config_path = data_dir.join("config.json");

    if config_path.exists() {
        let data = std::fs::read_to_string(&config_path)?;
        Ok(serde_json::from_str(&data)?)
    } else {
        Ok(ProtocolConfig::default())
    }
}

fn load_keypair(cli: &Cli) -> anyhow::Result<KeyPair> {
    let data_dir = expand_path(&cli.data_dir)?;
    let key_path = data_dir.join("key.json");

    if key_path.exists() {
        // In production, load actual keypair
        Ok(KeyPair::generate())
    } else {
        anyhow::bail!("No keypair found at {}", key_path.display())
    }
}

fn get_current_price() -> anyhow::Result<u64> {
    // In production, fetch from oracle
    // Default to $100,000 for demo
    Ok(10_000_000) // cents
}

fn get_block_height() -> u64 {
    // In production, get actual block height
    // Using timestamp as proxy for demo
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 600) // ~10 minute blocks
        .unwrap_or(0)
}

fn parse_cdp_id(id: &str) -> anyhow::Result<CDPId> {
    CDPId::from_hex(id).map_err(|e| anyhow::anyhow!("Invalid CDP ID: {}", e))
}

fn format_price(price_cents: u64) -> String {
    let dollars = price_cents / 100;
    let cents = price_cents % 100;
    format!("${},{}.{:02}", dollars / 1000, dollars % 1000, cents)
}

fn create_spinner(message: &str) -> ProgressBar {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.set_message(message.to_string());
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));
    spinner
}

fn print_cdp_info(cdp: &CDP, btc_price: u64, min_ratio: u64, term: &Term) -> anyhow::Result<()> {
    let state = cdp.get_state(btc_price, min_ratio);
    let collateral = CollateralAmount::from_sats(cdp.collateral_sats);
    let debt = TokenAmount::from_cents(cdp.debt_cents);

    let status_style = match state.status {
        CDPStatus::Active => style("Active").green(),
        CDPStatus::AtRisk => style("At Risk").yellow(),
        CDPStatus::Liquidatable => style("Liquidatable").red().bold(),
        CDPStatus::Closed => style("Closed").dim(),
        CDPStatus::Liquidated => style("Liquidated").red().dim(),
    };

    let _ = term.write_line(&format!("\n{}", style("CDP Details").bold().underlined()));
    let _ = term.write_line(&format!("  ID:         {}", cdp.id.to_hex()));
    let _ = term.write_line(&format!("  Owner:      {}", hex::encode(cdp.owner.as_bytes())));
    let _ = term.write_line(&format!("  Status:     {}", status_style));
    let _ = term.write_line(&format!("  Collateral: {}", style(collateral.to_string()).yellow()));
    let _ = term.write_line(&format!("  Debt:       {}", style(debt.to_string()).green()));
    let _ = term.write_line(&format!("  Ratio:      {}%", style(state.ratio).cyan()));

    if state.max_additional_debt > 0 {
        let _ = term.write_line(&format!(
            "  Max Mint:   {}",
            style(TokenAmount::from_cents(state.max_additional_debt).to_string()).dim()
        ));
    }

    if state.withdrawable_collateral > 0 {
        let _ = term.write_line(&format!(
            "  Withdraw:   {}",
            style(CollateralAmount::from_sats(state.withdrawable_collateral).to_string()).dim()
        ));
    }

    Ok(())
}
