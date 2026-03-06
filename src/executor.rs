use std::collections::HashMap;
use anyhow::Result;
use tracing::{info, warn, error};

use crate::client::PolymarketClient;
use crate::config::Config;
use crate::monitor::PositionDelta;
use crate::sizer::Sizer;
use crate::types::Side;

/// Execution engine: translates position deltas into actual orders.
/// Handles slippage protection, order book analysis, and execution.
pub struct Executor {
    client: PolymarketClient,
    sizer: Sizer,
    config: Config,
    /// Track our positions: token_id -> (size, side)
    our_positions: HashMap<String, (f64, Side)>,
}

impl Executor {
    pub fn new(client: PolymarketClient, config: Config) -> Self {
        let sizer = Sizer::new(&config);
        Self {
            client,
            sizer,
            config,
            our_positions: HashMap::new(),
        }
    }

    /// Refresh our known positions from the API
    pub async fn sync_positions(&mut self) -> Result<()> {
        let positions = self.client.get_my_positions().await?;
        self.our_positions.clear();
        for pos in &positions {
            if let Some(token_id) = &pos.asset {
                let size = pos.size.unwrap_or(0.0);
                if size.abs() < 1e-9 {
                    continue;
                }
                let side = match pos.side.as_deref() {
                    Some(s) if s.eq_ignore_ascii_case("SELL") => Side::Sell,
                    _ => Side::Buy,
                };
                self.our_positions.insert(token_id.clone(), (size, side));
            }
        }
        Ok(())
    }

    /// Execute a list of deltas from the monitor
    pub async fn execute_deltas(&mut self, deltas: &[PositionDelta]) -> Result<()> {
        if deltas.is_empty() {
            return Ok(());
        }

        // Get fresh balances and target value for sizing
        let our_balance = self.client.get_balance().await.unwrap_or(0.0);
        let target_value = self.client
            .estimate_wallet_value(&self.config.target_wallet)
            .await
            .unwrap_or(1.0);

        info!(
            "Executing {} deltas | our_balance={:.2} | target_value={:.2} | ratio={:.6}",
            deltas.len(),
            our_balance,
            target_value,
            our_balance / target_value
        );

        for delta in deltas {
            if let Err(e) = self.execute_single(delta, our_balance, target_value).await {
                error!("Failed to execute delta {:?}: {}", delta, e);
            }
        }

        // Re-sync our positions after executing
        if let Err(e) = self.sync_positions().await {
            warn!("Failed to sync positions after execution: {}", e);
        }

        Ok(())
    }

    async fn execute_single(
        &mut self,
        delta: &PositionDelta,
        our_balance: f64,
        target_value: f64,
    ) -> Result<()> {
        match delta {
            PositionDelta::Opened {
                token_id,
                condition_id: _,
                size,
                side,
                price,
            } => {
                self.open_position(token_id, *side, *size, *price, our_balance, target_value)
                    .await
            }
            PositionDelta::Increased {
                token_id,
                condition_id: _,
                added_size,
                new_total: _,
                side,
                price,
            } => {
                self.open_position(token_id, *side, *added_size, *price, our_balance, target_value)
                    .await
            }
            PositionDelta::Decreased {
                token_id,
                condition_id: _,
                removed_size,
                new_total,
                side: _,
                price,
            } => {
                let target_total_before = new_total + removed_size;
                self.close_position(token_id, *removed_size, target_total_before, *price)
                    .await
            }
            PositionDelta::Closed {
                token_id,
                condition_id: _,
                old_size: _,
                side: _,
            } => self.full_close_position(token_id).await,
        }
    }

    /// Open or increase a position (BUY or SELL)
    async fn open_position(
        &mut self,
        token_id: &str,
        side: Side,
        target_size: f64,
        price: f64,
        our_balance: f64,
        target_value: f64,
    ) -> Result<()> {
        // Get the tick size for this market (default 0.01)
        let tick_size = 0.01;

        let our_size = match self.sizer.compute_size(
            target_size,
            target_value,
            our_balance,
            price,
            tick_size,
        ) {
            Some(s) => s,
            None => {
                warn!(
                    "Skipping {} {}: computed size below minimum",
                    side, token_id
                );
                return Ok(());
            }
        };

        // Get best price with slippage protection
        let exec_price = self.get_execution_price(token_id, side, price).await?;

        if self.config.dry_run {
            info!(
                "[DRY RUN] {} {:.4} of {} @ {:.4} (target traded {:.4} @ {:.4})",
                side, our_size, token_id, exec_price, target_size, price
            );
            return Ok(());
        }

        info!(
            "EXECUTING {} {:.4} of {} @ {:.4}",
            side, our_size, token_id, exec_price
        );

        let result = self.client
            .place_market_order(token_id, side, our_size, exec_price)
            .await?;

        match result.success {
            Some(true) => {
                info!(
                    "ORDER FILLED: {} {:.4} @ {:.4} | order_id={:?}",
                    side,
                    our_size,
                    exec_price,
                    result.order_id
                );
                // Update local tracking
                let entry = self.our_positions.entry(token_id.to_string()).or_insert((0.0, side));
                entry.0 += our_size;
            }
            _ => {
                warn!(
                    "ORDER FAILED: {} | status={:?} | error={:?}",
                    token_id, result.status, result.error_msg
                );
            }
        }

        Ok(())
    }

    /// Partially close a position
    async fn close_position(
        &mut self,
        token_id: &str,
        target_removed: f64,
        target_total_before: f64,
        price: f64,
    ) -> Result<()> {
        let (our_size, our_side) = match self.our_positions.get(token_id) {
            Some(&(s, side)) => (s, side),
            None => {
                warn!("No position to close for {}", token_id);
                return Ok(());
            }
        };

        let tick_size = 0.01;
        let close_size = match self.sizer.compute_close_size(
            target_removed,
            target_total_before,
            our_size,
            tick_size,
        ) {
            Some(s) => s,
            None => {
                warn!("Computed close size too small for {}", token_id);
                return Ok(());
            }
        };

        // To close: sell if we bought, buy if we sold
        let close_side = match our_side {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        };

        let exec_price = self.get_execution_price(token_id, close_side, price).await?;

        if self.config.dry_run {
            info!(
                "[DRY RUN] CLOSE {} {:.4} of {} @ {:.4}",
                close_side, close_size, token_id, exec_price
            );
            return Ok(());
        }

        info!(
            "CLOSING {} {:.4} of {} @ {:.4}",
            close_side, close_size, token_id, exec_price
        );

        let result = self.client
            .place_market_order(token_id, close_side, close_size, exec_price)
            .await?;

        match result.success {
            Some(true) => {
                info!(
                    "CLOSE FILLED: {:.4} @ {:.4} | order_id={:?}",
                    close_size, exec_price, result.order_id
                );
                if let Some(entry) = self.our_positions.get_mut(token_id) {
                    entry.0 -= close_size;
                    if entry.0 < 0.01 {
                        self.our_positions.remove(token_id);
                    }
                }
            }
            _ => {
                warn!(
                    "CLOSE FAILED: {} | status={:?} | error={:?}",
                    token_id, result.status, result.error_msg
                );
            }
        }

        Ok(())
    }

    /// Fully close a position (target exited entirely)
    async fn full_close_position(&mut self, token_id: &str) -> Result<()> {
        let (our_size, our_side) = match self.our_positions.get(token_id) {
            Some(&(s, side)) => (s, side),
            None => {
                warn!("No position to fully close for {}", token_id);
                return Ok(());
            }
        };

        let close_side = match our_side {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        };

        // Get current price
        let mid_price = self.client.get_midpoint(token_id).await.unwrap_or(0.5);
        let exec_price = self.get_execution_price(token_id, close_side, mid_price).await?;

        if self.config.dry_run {
            info!(
                "[DRY RUN] FULL CLOSE {} {:.4} of {} @ {:.4}",
                close_side, our_size, token_id, exec_price
            );
            return Ok(());
        }

        info!(
            "FULL CLOSE {} {:.4} of {} @ {:.4}",
            close_side, our_size, token_id, exec_price
        );

        let result = self.client
            .place_market_order(token_id, close_side, our_size, exec_price)
            .await?;

        match result.success {
            Some(true) => {
                info!(
                    "FULL CLOSE FILLED: {:.4} @ {:.4} | order_id={:?}",
                    our_size, exec_price, result.order_id
                );
                self.our_positions.remove(token_id);
            }
            _ => {
                warn!(
                    "FULL CLOSE FAILED: {} | status={:?} | error={:?}",
                    token_id, result.status, result.error_msg
                );
            }
        }

        Ok(())
    }

    /// Determine execution price with slippage protection.
    /// For BUY: use best ask + slippage buffer (worst case price we'll accept)
    /// For SELL: use best bid - slippage buffer
    async fn get_execution_price(
        &self,
        token_id: &str,
        side: Side,
        fallback_price: f64,
    ) -> Result<f64> {
        let book = self.client.get_order_book(token_id).await;
        let slippage_mult = self.config.max_slippage_bps as f64 / 10_000.0;

        let price = match book {
            Ok(ob) => {
                match side {
                    Side::Buy => {
                        // Best ask is our entry for buying
                        let best_ask = ob
                            .asks
                            .as_ref()
                            .and_then(|a| a.first())
                            .and_then(|l| l.price.parse::<f64>().ok())
                            .unwrap_or(fallback_price);
                        // Worst price = ask + slippage
                        let worst = best_ask * (1.0 + slippage_mult);
                        worst.min(0.99) // Polymarket prices are 0-1
                    }
                    Side::Sell => {
                        // Best bid is our entry for selling
                        let best_bid = ob
                            .bids
                            .as_ref()
                            .and_then(|b| b.first())
                            .and_then(|l| l.price.parse::<f64>().ok())
                            .unwrap_or(fallback_price);
                        // Worst price = bid - slippage
                        let worst = best_bid * (1.0 - slippage_mult);
                        worst.max(0.01) // Floor at 1 cent
                    }
                }
            }
            Err(_) => {
                warn!("Could not fetch order book for {}, using fallback price", token_id);
                match side {
                    Side::Buy => (fallback_price * (1.0 + slippage_mult)).min(0.99),
                    Side::Sell => (fallback_price * (1.0 - slippage_mult)).max(0.01),
                }
            }
        };

        // Round to 2 decimal places (Polymarket tick)
        let rounded = (price * 100.0).round() / 100.0;
        Ok(rounded)
    }
}
