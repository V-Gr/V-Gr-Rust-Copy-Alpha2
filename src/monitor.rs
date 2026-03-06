use std::collections::HashMap;
use tracing::{debug, info, warn};

use crate::client::PolymarketClient;
use crate::types::{Position, PositionSnapshot, Side};

/// Monitors target wallet positions and detects deltas (new, changed, closed).
pub struct WalletMonitor {
    /// Last known snapshot: token_id -> PositionSnapshot
    last_snapshot: HashMap<String, PositionSnapshot>,
    target_wallet: String,
}

#[derive(Debug, Clone)]
pub enum PositionDelta {
    /// New position opened by target
    Opened {
        token_id: String,
        condition_id: String,
        size: f64,
        side: Side,
        price: f64,
    },
    /// Existing position increased
    Increased {
        token_id: String,
        condition_id: String,
        added_size: f64,
        new_total: f64,
        side: Side,
        price: f64,
    },
    /// Existing position decreased (partial close)
    Decreased {
        token_id: String,
        condition_id: String,
        removed_size: f64,
        new_total: f64,
        side: Side,
        price: f64,
    },
    /// Position fully closed
    Closed {
        token_id: String,
        condition_id: String,
        old_size: f64,
        side: Side,
    },
}

impl WalletMonitor {
    pub fn new(target_wallet: String) -> Self {
        Self {
            last_snapshot: HashMap::new(),
            target_wallet,
        }
    }

    /// Poll for position changes. Returns a list of deltas since last poll.
    pub async fn poll(&mut self, client: &PolymarketClient) -> Vec<PositionDelta> {
        let positions = match client.get_wallet_positions(&self.target_wallet).await {
            Ok(p) => p,
            Err(e) => {
                warn!("Failed to fetch target positions: {}", e);
                return vec![];
            }
        };

        let new_snapshot = Self::build_snapshot(&positions);
        let deltas = self.diff(&new_snapshot);
        self.last_snapshot = new_snapshot;
        deltas
    }

    /// Build a snapshot map from raw positions
    fn build_snapshot(positions: &[Position]) -> HashMap<String, PositionSnapshot> {
        let mut map = HashMap::new();
        for pos in positions {
            let token_id = match &pos.asset {
                Some(id) => id.clone(),
                None => continue,
            };
            let size = pos.size.unwrap_or(0.0);
            if size.abs() < 1e-9 {
                continue; // skip zero-size
            }
            let side_str = pos.side.as_deref().unwrap_or("BUY");
            let side = if side_str.eq_ignore_ascii_case("SELL") {
                Side::Sell
            } else {
                Side::Buy
            };
            let price = pos.cur_price.unwrap_or(pos.avg_price.unwrap_or(0.5));
            let condition_id = pos.condition_id.clone().unwrap_or_default();

            map.insert(
                token_id.clone(),
                PositionSnapshot {
                    token_id,
                    size,
                    side,
                    price,
                    condition_id,
                },
            );
        }
        map
    }

    /// Diff current snapshot against previous to generate deltas
    fn diff(&self, current: &HashMap<String, PositionSnapshot>) -> Vec<PositionDelta> {
        let mut deltas = Vec::new();

        // Check for new or changed positions
        for (token_id, cur) in current {
            match self.last_snapshot.get(token_id) {
                None => {
                    // Brand new position
                    info!(
                        "TARGET OPENED: {} size={:.4} side={} price={:.4}",
                        token_id, cur.size, cur.side, cur.price
                    );
                    deltas.push(PositionDelta::Opened {
                        token_id: token_id.clone(),
                        condition_id: cur.condition_id.clone(),
                        size: cur.size,
                        side: cur.side,
                        price: cur.price,
                    });
                }
                Some(prev) => {
                    let size_diff = cur.size - prev.size;
                    // Use a reasonable threshold for change detection
                    if size_diff.abs() < 0.01 {
                        continue; // no meaningful change
                    }
                    if size_diff > 0.0 {
                        debug!(
                            "TARGET INCREASED: {} by {:.4} (now {:.4})",
                            token_id, size_diff, cur.size
                        );
                        deltas.push(PositionDelta::Increased {
                            token_id: token_id.clone(),
                            condition_id: cur.condition_id.clone(),
                            added_size: size_diff,
                            new_total: cur.size,
                            side: cur.side,
                            price: cur.price,
                        });
                    } else {
                        debug!(
                            "TARGET DECREASED: {} by {:.4} (now {:.4})",
                            token_id, size_diff.abs(), cur.size
                        );
                        deltas.push(PositionDelta::Decreased {
                            token_id: token_id.clone(),
                            condition_id: cur.condition_id.clone(),
                            removed_size: size_diff.abs(),
                            new_total: cur.size,
                            side: cur.side,
                            price: cur.price,
                        });
                    }
                }
            }
        }

        // Check for closed positions (in old snapshot but not in current)
        for (token_id, prev) in &self.last_snapshot {
            if !current.contains_key(token_id) {
                info!(
                    "TARGET CLOSED: {} size={:.4} side={}",
                    token_id, prev.size, prev.side
                );
                deltas.push(PositionDelta::Closed {
                    token_id: token_id.clone(),
                    condition_id: prev.condition_id.clone(),
                    old_size: prev.size,
                    side: prev.side,
                });
            }
        }

        deltas
    }

    /// Check if this is the first poll (no prior snapshot)
    pub fn is_first_poll(&self) -> bool {
        self.last_snapshot.is_empty()
    }

    /// Get count of tracked positions  
    pub fn tracked_count(&self) -> usize {
        self.last_snapshot.len()
    }
}
