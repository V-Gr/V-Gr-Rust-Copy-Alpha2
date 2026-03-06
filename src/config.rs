use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Config {
    // Polymarket CLOB API credentials
    pub api_key: String,
    pub api_secret: String,
    pub api_passphrase: String,
    pub private_key: String,
    pub funder_address: String,

    // Target wallet to copy
    pub target_wallet: String,

    // Tuning
    pub poll_interval_ms: u64,
    pub min_bet_size: f64,
    pub max_slippage_bps: u64,
    pub dry_run: bool,
    pub initial_balance: Option<f64>,

    // Endpoints
    pub clob_api_url: String,
    pub gamma_api_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenv::dotenv().ok();

        Ok(Self {
            api_key: std::env::var("POLYMARKET_API_KEY")
                .context("POLYMARKET_API_KEY not set")?,
            api_secret: std::env::var("POLYMARKET_API_SECRET")
                .context("POLYMARKET_API_SECRET not set")?,
            api_passphrase: std::env::var("POLYMARKET_API_PASSPHRASE")
                .context("POLYMARKET_API_PASSPHRASE not set")?,
            private_key: std::env::var("POLYMARKET_PRIVATE_KEY")
                .unwrap_or_default(),
            funder_address: std::env::var("POLYMARKET_FUNDER_ADDRESS")
                .context("POLYMARKET_FUNDER_ADDRESS not set")?,
            target_wallet: std::env::var("TARGET_WALLET_ADDRESS")
                .context("TARGET_WALLET_ADDRESS not set")?,
            poll_interval_ms: std::env::var("POLL_INTERVAL_MS")
                .unwrap_or_else(|_| "500".to_string())
                .parse()
                .unwrap_or(500),
            min_bet_size: std::env::var("MIN_BET_SIZE")
                .unwrap_or_else(|_| "1.0".to_string())
                .parse()
                .unwrap_or(1.0),
            max_slippage_bps: std::env::var("MAX_SLIPPAGE_BPS")
                .unwrap_or_else(|_| "50".to_string())
                .parse()
                .unwrap_or(50),
            dry_run: std::env::var("DRY_RUN")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .unwrap_or(false),
            initial_balance: std::env::var("INITIAL_BALANCE")
                .ok()
                .and_then(|s| s.parse().ok()),
            clob_api_url: "https://clob.polymarket.com".to_string(),
            gamma_api_url: "https://data-api.polymarket.com".to_string(),
        })
    }
}
