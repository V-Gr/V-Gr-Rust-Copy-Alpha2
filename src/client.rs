use anyhow::{Context, Result};
use reqwest::Client;
use tracing::{debug, warn};

use crate::auth::build_hmac_headers;
use crate::config::Config;
use crate::types::*;

/// High-performance HTTP client for Polymarket CLOB + Gamma APIs.
/// All methods are non-blocking async. Reuses a single connection pool.
#[derive(Clone)]
pub struct PolymarketClient {
    http: Client,
    config: Config,
}

impl PolymarketClient {
    pub fn new(config: Config) -> Result<Self> {
        let http = Client::builder()
            .pool_max_idle_per_host(20)
            .tcp_keepalive(Some(std::time::Duration::from_secs(30)))
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        Ok(Self { http, config })
    }

    // ─── Auth-ed request helper ───────────────────────────────────

    async fn clob_request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&str>,
    ) -> Result<reqwest::Response> {
        let url = format!("{}{}", self.config.clob_api_url, path);
        let body_str = body.unwrap_or("");

        let headers = build_hmac_headers(
            &self.config.api_key,
            &self.config.api_secret,
            &self.config.api_passphrase,
            method.as_str(),
            path,
            body_str,
        )?;

        let method_str = method.as_str().to_string();
        let mut req = self.http.request(method, &url);
        for (k, v) in &headers {
            req = req.header(k, v);
        }
        if !body_str.is_empty() {
            req = req.header("Content-Type", "application/json").body(body_str.to_string());
        }
        let resp = req.send().await.context("CLOB request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            debug!("CLOB {} {} -> {} : {}", method_str, path, status, body);
            anyhow::bail!("CLOB API error {}: {}", status, body);
        }
        Ok(resp)
    }

    // ─── Public endpoints ─────────────────────────────────────────

    /// Get current server timestamp (health check + clock sync)
    pub async fn get_server_time(&self) -> Result<u64> {
        let url = format!("{}/time", self.config.clob_api_url);
        let resp = self.http.get(&url).send().await?;
        let text = resp.text().await?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        Ok(v.as_u64().unwrap_or(0))
    }

    /// Get order book for a token
    pub async fn get_order_book(&self, token_id: &str) -> Result<OrderBookResponse> {
        let url = format!("{}/book?token_id={}", self.config.clob_api_url, token_id);
        let resp = self.http.get(&url).send().await?;
        let book: OrderBookResponse = resp.json().await?;
        Ok(book)
    }

    /// Get market by condition_id
    pub async fn get_market(&self, condition_id: &str) -> Result<Market> {
        let url = format!("{}/markets/{}", self.config.clob_api_url, condition_id);
        let resp = self.http.get(&url).send().await?;
        let market: Market = resp.json().await?;
        Ok(market)
    }

    /// Get midpoint price for a token
    pub async fn get_midpoint(&self, token_id: &str) -> Result<f64> {
        let url = format!("{}/midpoint?token_id={}", self.config.clob_api_url, token_id);
        let resp = self.http.get(&url).send().await?;
        let text = resp.text().await?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        let mid = v["mid"].as_str().unwrap_or("0").parse::<f64>().unwrap_or(0.0);
        Ok(mid)
    }

    /// Get best bid/ask spread for a token
    pub async fn get_spread(&self, token_id: &str) -> Result<BookPrice> {
        let url = format!("{}/spread?token_id={}", self.config.clob_api_url, token_id);
        let resp = self.http.get(&url).send().await?;
        let price: BookPrice = resp.json().await?;
        Ok(price)
    }

    // ─── Auth-ed: Account ─────────────────────────────────────────

    /// Get USDC balance.
    /// Tries: CLOB balance-allowance → Data API position sum → INITIAL_BALANCE env.
    pub async fn get_balance(&self) -> Result<f64> {
        // Try CLOB balance-allowance first
        match self.clob_request(
            reqwest::Method::GET,
            "/balance-allowance?asset_type=USDC",
            None,
        ).await {
            Ok(resp) => {
                let text = resp.text().await?;
                debug!("Balance-allowance response: {}", text);
                let ba: BalanceAllowance = serde_json::from_str(&text)?;
                let balance = ba.balance
                    .unwrap_or_else(|| "0".to_string())
                    .parse::<f64>()
                    .unwrap_or(0.0);
                if balance > 0.0 {
                    return Ok(balance);
                }
                // CLOB returned 0 — may mean we need a different signature_type
                warn!("CLOB balance-allowance returned 0, trying fallbacks");
            }
            Err(e) => {
                warn!("CLOB balance-allowance failed: {} — trying fallbacks", e);
            }
        }

        // Fallback 2: estimate from Data API positions (sum of currentValue)
        let positions = self.get_wallet_positions(&self.config.funder_address).await?;
        let mut total = 0.0_f64;
        for pos in &positions {
            let cv = pos.current_value.unwrap_or(0.0);
            total += cv;
        }
        if total > 0.01 {
            return Ok(total);
        }

        // Fallback 3: use configured INITIAL_BALANCE
        if let Some(ib) = self.config.initial_balance {
            if ib > 0.0 {
                return Ok(ib);
            }
        }

        Ok(0.0)
    }

    /// Get all open orders
    pub async fn get_open_orders(&self) -> Result<Vec<OpenOrder>> {
        let resp = self.clob_request(
            reqwest::Method::GET,
            "/orders?state=LIVE",
            None,
        ).await?;
        let text = resp.text().await?;
        let orders: Vec<OpenOrder> = serde_json::from_str(&text).unwrap_or_default();
        Ok(orders)
    }

    /// Cancel an order by ID
    pub async fn cancel_order(&self, order_id: &str) -> Result<()> {
        let body = serde_json::json!({ "orderID": order_id }).to_string();
        let resp = self.clob_request(
            reqwest::Method::DELETE,
            "/order",
            Some(&body),
        ).await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await?;
            warn!("Cancel order {} failed: {} - {}", order_id, status, text);
        }
        Ok(())
    }

    /// Cancel all open orders
    pub async fn cancel_all_orders(&self) -> Result<()> {
        self.clob_request(
            reqwest::Method::DELETE,
            "/orders",
            None,
        ).await?;
        Ok(())
    }

    // ─── Auth-ed: Trading ─────────────────────────────────────────

    /// Place a GTC limit order
    pub async fn place_order(&self, order: &OrderRequest) -> Result<OrderResponse> {
        let body = serde_json::to_string(order)?;
        debug!("Placing order: {}", body);

        let resp = self.clob_request(
            reqwest::Method::POST,
            "/order",
            Some(&body),
        ).await?;

        let text = resp.text().await?;
        debug!("Order response: {}", text);
        let order_resp: OrderResponse = serde_json::from_str(&text)?;
        Ok(order_resp)
    }

    /// Place a market-like order using FOK at best available price
    pub async fn place_market_order(
        &self,
        token_id: &str,
        side: Side,
        size: f64,
        worst_price: f64,
    ) -> Result<OrderResponse> {
        let order = OrderRequest {
            token_id: token_id.to_string(),
            price: worst_price,
            size,
            side,
            order_type: OrderType::Fok,
            funder: self.config.funder_address.clone(),
        };
        self.place_order(&order).await
    }

    /// Place a limit GTC order
    pub async fn place_limit_order(
        &self,
        token_id: &str,
        side: Side,
        size: f64,
        price: f64,
    ) -> Result<OrderResponse> {
        let order = OrderRequest {
            token_id: token_id.to_string(),
            price,
            size,
            side,
            order_type: OrderType::Gtc,
            funder: self.config.funder_address.clone(),
        };
        self.place_order(&order).await
    }

    // ─── Gamma API (public, no auth) ──────────────────────────────

    /// Fetch recent activity/trades for a wallet from the Data API
    pub async fn get_wallet_activity(
        &self,
        wallet: &str,
        limit: u32,
    ) -> Result<Vec<GammaActivity>> {
        let url = format!(
            "{}/activity?user={}&limit={}",
            self.config.gamma_api_url, wallet, limit
        );
        let resp = self.http.get(&url).send().await?;
        let text = resp.text().await?;
        let activities: Vec<GammaActivity> = serde_json::from_str(&text).unwrap_or_default();
        Ok(activities)
    }

    /// Fetch current positions for a wallet from the Data API.
    /// Automatically filters out settled/redeemable positions.
    pub async fn get_wallet_positions(&self, wallet: &str) -> Result<Vec<Position>> {
        let url = format!(
            "{}/positions?user={}&sizeThreshold=0&limit=500",
            self.config.gamma_api_url, wallet
        );
        let resp = self.http.get(&url).send().await?;
        let text = resp.text().await?;
        debug!("Data API positions response (first 500 chars): {}", &text[..text.len().min(500)]);
        // The Data API returns { "value": [...], "Count": N }
        let wrapper: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let raw: Vec<Position> = if let Some(arr) = wrapper.get("value").and_then(|v| v.as_array()) {
            arr.iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        } else {
            serde_json::from_str(&text).unwrap_or_default()
        };
        let total = raw.len();
        // Filter out settled/redeemable positions (market resolved, not tradeable)
        let positions: Vec<Position> = raw.into_iter()
            .filter(|p| !p.redeemable.unwrap_or(false))
            .collect();
        if total != positions.len() {
            debug!("Filtered {}/{} positions (removed {} settled/redeemable)",
                positions.len(), total, total - positions.len());
        }
        Ok(positions)
    }

    /// Estimate target wallet's total portfolio value (sum of positions * price + USDC idle)
    pub async fn estimate_wallet_value(&self, wallet: &str) -> Result<f64> {
        let positions = self.get_wallet_positions(wallet).await?;
        let mut total = 0.0_f64;
        for pos in &positions {
            let size = pos.size.unwrap_or(0.0);
            let price = pos.cur_price.unwrap_or(pos.avg_price.unwrap_or(0.5));
            total += size * price;
        }
        // We can't easily get idle USDC of target, so we estimate from positions only
        // This is conservative — the ratio will slightly over-size if target has idle cash
        if total < 1.0 {
            total = 1.0; // floor to avoid div by zero
        }
        Ok(total)
    }

    /// Get our own positions (via Data API using funder address)
    pub async fn get_my_positions(&self) -> Result<Vec<Position>> {
        self.get_wallet_positions(&self.config.funder_address).await
    }
}
