use tracing::debug;

use crate::config::Config;

/// Position sizing engine.  
/// Scales target trades proportionally to our wallet vs target wallet value.
///
/// Formula:  
///   our_size = target_trade_size × (our_balance / target_value)  
///
/// With safety rails:
///   - Floor at min_bet_size (Polymarket global minimum = $1)
///   - Cap at max fraction of our balance per single trade (risk mgmt)
///   - Round to tick size for valid order
pub struct Sizer {
    min_bet: f64,
    max_single_trade_frac: f64,  // max % of our balance on one trade
}

impl Sizer {
    pub fn new(config: &Config) -> Self {
        Self {
            min_bet: config.min_bet_size.max(1.0), // Polymarket enforces $1 min
            max_single_trade_frac: 0.25, // never more than 25% of balance on one copy
        }
    }

    /// Compute the proportional size we should trade.
    ///
    /// - `target_trade_size`: The size the target traded (in shares/contracts)
    /// - `target_value`: Estimated total portfolio value of target
    /// - `our_balance`: Our available USDC balance
    /// - `price`: Current price of the asset (to convert shares → USDC cost)
    /// - `tick_size`: Market tick size for rounding
    ///
    /// Returns `None` if the scaled size is below minimum or we can't afford it.
    pub fn compute_size(
        &self,
        target_trade_size: f64,
        target_value: f64,
        our_balance: f64,
        price: f64,
        tick_size: f64,
    ) -> Option<f64> {
        if target_value <= 0.0 || our_balance <= 0.0 || price <= 0.0 {
            return None;
        }

        // Ratio: how much smaller our portfolio is vs target
        let ratio = our_balance / target_value;
        debug!("Sizing ratio: {:.6} (our={:.2} / target={:.2})", ratio, our_balance, target_value);

        // Scale the trade size proportionally
        let mut scaled_size = target_trade_size * ratio;

        // Cost check: size × price must not exceed max fraction of balance
        let max_cost = our_balance * self.max_single_trade_frac;
        let cost = scaled_size * price;
        if cost > max_cost {
            scaled_size = max_cost / price;
            debug!("Capped size from cost: {:.4} (max_cost={:.2})", scaled_size, max_cost);
        }

        // Round to tick size (e.g., 0.01)
        let tick = if tick_size > 0.0 { tick_size } else { 0.01 };
        scaled_size = (scaled_size / tick).floor() * tick;

        // Check minimum bet in USDC terms
        let final_cost = scaled_size * price;
        if final_cost < self.min_bet {
            // Try to bump up to minimum if affordable
            let min_size = (self.min_bet / price / tick).ceil() * tick;
            let min_cost = min_size * price;
            if min_cost <= our_balance * self.max_single_trade_frac {
                scaled_size = min_size;
                debug!("Bumped to min bet: size={:.4} cost={:.2}", scaled_size, min_cost);
            } else {
                debug!("Below min bet and can't afford bump, skipping");
                return None;
            }
        }

        if scaled_size < tick {
            return None;
        }

        debug!(
            "Final size: {:.4} (target_size={:.4}, ratio={:.6}, cost={:.2})",
            scaled_size,
            target_trade_size,
            ratio,
            scaled_size * price
        );
        Some(scaled_size)
    }

    /// For closing positions: compute exact proportional close size.
    /// When target closes X% of their position, we close X% of ours.
    pub fn compute_close_size(
        &self,
        target_closed_size: f64,
        target_total_before: f64,
        our_position_size: f64,
        tick_size: f64,
    ) -> Option<f64> {
        if target_total_before <= 0.0 || our_position_size <= 0.0 {
            return None;
        }

        // What fraction did the target close?
        let close_frac = (target_closed_size / target_total_before).min(1.0);
        let mut our_close_size = our_position_size * close_frac;

        let tick = if tick_size > 0.0 { tick_size } else { 0.01 };
        our_close_size = (our_close_size / tick).floor() * tick;

        if our_close_size < tick {
            return None;
        }

        debug!(
            "Close size: {:.4} (frac={:.4}, our_pos={:.4})",
            our_close_size, close_frac, our_position_size
        );
        Some(our_close_size)
    }
}
