use serde::{Deserialize, Serialize};

// ─── Market / Token ───────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct Market {
    pub condition_id: String,
    pub question: Option<String>,
    pub tokens: Option<Vec<Token>>,
    pub active: Option<bool>,
    pub closed: Option<bool>,
    pub minimum_order_size: Option<f64>,
    pub minimum_tick_size: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Token {
    pub token_id: String,
    pub outcome: Option<String>,
    pub price: Option<f64>,
}

// ─── Order book ───────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct OrderBookResponse {
    pub market: Option<String>,
    pub asset_id: Option<String>,
    pub bids: Option<Vec<OrderBookLevel>>,
    pub asks: Option<Vec<OrderBookLevel>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderBookLevel {
    pub price: String,
    pub size: String,
}

// ─── Orders ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct OrderRequest {
    pub token_id: String,
    pub price: f64,
    pub size: f64,
    pub side: Side,
    #[serde(rename = "type")]
    pub order_type: OrderType,
    pub funder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "UPPERCASE")]
pub enum Side {
    Buy,
    Sell,
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Side::Buy => write!(f, "BUY"),
            Side::Sell => write!(f, "SELL"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum OrderType {
    Gtc,  // Good til cancelled
    Fok,  // Fill or kill
    Ioc,  // Immediate or cancel
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderResponse {
    pub success: Option<bool>,
    #[serde(rename = "orderID")]
    pub order_id: Option<String>,
    pub status: Option<String>,
    pub error_msg: Option<String>,
}

// ─── Positions (used for monitoring target) ───────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Position {
    pub asset: Option<String>,          // token_id
    pub condition_id: Option<String>,
    pub size: Option<f64>,
    #[serde(rename = "avgPrice")]
    pub avg_price: Option<f64>,
    pub side: Option<String>,
    pub market: Option<String>,
    pub outcome: Option<String>,
    #[serde(rename = "curPrice")]
    pub cur_price: Option<f64>,
    #[serde(rename = "cashPnl")]
    pub cash_pnl: Option<f64>,
    #[serde(rename = "percentPnl")]
    pub percent_pnl: Option<f64>,
    #[serde(rename = "proxyWallet")]
    pub proxy_wallet: Option<String>,
}

// ─── Trade activity from Gamma API ────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GammaActivity {
    pub id: Option<String>,
    #[serde(rename = "conditionId")]
    pub condition_id: Option<String>,
    pub asset: Option<String>,          // token_id
    pub side: Option<String>,           // "BUY" | "SELL"
    pub size: Option<f64>,
    pub price: Option<f64>,
    #[serde(rename = "type")]
    pub activity_type: Option<String>,  // "TRADE"
    pub timestamp: Option<String>,
    #[serde(rename = "transactionHash")]
    pub transaction_hash: Option<String>,
    pub outcome: Option<String>,
    pub market: Option<String>,
    #[serde(rename = "proxyWallet")]
    pub proxy_wallet: Option<String>,
}

// ─── Snapshot of a wallet's positions for diffing ─────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct PositionSnapshot {
    pub token_id: String,
    pub size: f64,
    pub side: Side,
    pub price: f64,
    pub condition_id: String,
}

// ─── Balance ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct BalanceAllowance {
    pub balance: Option<String>,
    pub allowance: Option<String>,
}

// ─── Price / Book ─────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct BookPrice {
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub mid: Option<f64>,
    pub spread: Option<f64>,
}

// ─── Open orders ──────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct OpenOrder {
    pub id: Option<String>,
    pub asset_id: Option<String>,
    pub side: Option<String>,
    pub price: Option<String>,
    pub original_size: Option<String>,
    pub size_matched: Option<String>,
    pub status: Option<String>,
}
