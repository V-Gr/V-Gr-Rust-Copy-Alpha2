#![allow(dead_code)]

mod auth;
mod client;
mod config;
mod executor;
mod monitor;
mod sizer;
mod types;

use anyhow::Result;
use tracing::{info, warn, error};

use crate::client::PolymarketClient;
use crate::config::Config;
use crate::executor::Executor;
use crate::monitor::WalletMonitor;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .with_thread_ids(false)
        .compact()
        .init();

    info!("╔══════════════════════════════════════════╗");
    info!("║       CopyMet - Polymarket Copy Bot      ║");
    info!("╚══════════════════════════════════════════╝");

    // Load config
    let config = Config::from_env()?;
    info!("Target wallet: {}", config.target_wallet);
    info!("Funder address: {}", config.funder_address);
    info!("Poll interval: {}ms", config.poll_interval_ms);
    info!("Min bet size: ${:.2}", config.min_bet_size);
    info!("Max slippage: {}bps", config.max_slippage_bps);
    info!("Dry run: {}", config.dry_run);

    // Initialize client
    let client = PolymarketClient::new(config.clone())?;

    // Health check: verify API connectivity
    match client.get_server_time().await {
        Ok(t) => info!("Connected to Polymarket CLOB (server time: {})", t),
        Err(e) => {
            error!("Failed to connect to Polymarket CLOB: {}", e);
            return Err(e);
        }
    }

    // Check our balance
    let _our_balance = match client.get_balance().await {
        Ok(b) => {
            if b > 0.0 {
                info!("Our USDC balance: ${:.2}", b);
            } else {
                warn!("Our USDC balance: $0.00 — set INITIAL_BALANCE in .env to override");
            }
            b
        }
        Err(e) => {
            warn!("Could not fetch balance: {} — set INITIAL_BALANCE in .env", e);
            config.initial_balance.unwrap_or(0.0)
        }
    };

    // Check target wallet positions (already filtered: settled/redeemable excluded)
    let target_positions = client.get_wallet_positions(&config.target_wallet).await?;
    info!(
        "Target wallet has {} active positions (settled markets excluded)",
        target_positions.len()
    );
    for pos in &target_positions {
        info!(
            "  - {} | size={:.4} | side={} | price={:.4} | {}",
            pos.asset.as_deref().unwrap_or("?"),
            pos.size.unwrap_or(0.0),
            pos.outcome.as_deref().unwrap_or("?"),
            pos.cur_price.unwrap_or(pos.avg_price.unwrap_or(0.0)),
            pos.title.as_deref().unwrap_or(pos.market.as_deref().unwrap_or("?")),
        );
    }

    // Initialize monitor and executor
    let mut monitor = WalletMonitor::new(config.target_wallet.clone());
    let mut executor = Executor::new(client.clone(), config.clone());

    // Sync our existing positions
    if let Err(e) = executor.sync_positions().await {
        warn!("Could not sync our positions: {}", e);
    }

    // First poll: capture baseline (don't act on existing positions)
    info!("Capturing baseline snapshot of target positions...");
    let initial_deltas = monitor.poll(&client).await;
    info!(
        "Baseline captured: {} positions tracked (ignoring {} initial deltas)",
        monitor.tracked_count(),
        initial_deltas.len()
    );

    // Main loop: poll → diff → execute
    let poll_interval = tokio::time::Duration::from_millis(config.poll_interval_ms);
    info!("Starting copy trading loop...");

    // Graceful shutdown on Ctrl+C
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("Shutdown signal received, stopping...");
        r.store(false, std::sync::atomic::Ordering::SeqCst);
    });

    let mut tick = 0u64;
    while running.load(std::sync::atomic::Ordering::SeqCst) {
        tick += 1;

        // Poll target for changes
        let deltas = monitor.poll(&client).await;

        if !deltas.is_empty() {
            info!("── Tick {} ── {} deltas detected ──", tick, deltas.len());
            for d in &deltas {
                match d {
                    monitor::PositionDelta::Opened { token_id, size, side, price, .. } => {
                        info!("  → OPEN  {} {:.4} @ {:.4} [{}]", side, size, price, &token_id[..8.min(token_id.len())]);
                    }
                    monitor::PositionDelta::Increased { token_id, added_size, side, price, .. } => {
                        info!("  → ADD   {} {:.4} @ {:.4} [{}]", side, added_size, price, &token_id[..8.min(token_id.len())]);
                    }
                    monitor::PositionDelta::Decreased { token_id, removed_size, side, price, .. } => {
                        info!("  → TRIM  {} {:.4} @ {:.4} [{}]", side, removed_size, price, &token_id[..8.min(token_id.len())]);
                    }
                    monitor::PositionDelta::Closed { token_id, old_size, side, .. } => {
                        info!("  → CLOSE {} {:.4} [{}]", side, old_size, &token_id[..8.min(token_id.len())]);
                    }
                }
            }

            // Execute the copy trades
            if let Err(e) = executor.execute_deltas(&deltas).await {
                error!("Execution error: {}", e);
            }
        }

        // Periodic status log every 120 ticks (~60s at 500ms interval)
        if tick % 120 == 0 {
            info!(
                "── Heartbeat ── tick={} | tracking {} target positions",
                tick,
                monitor.tracked_count()
            );
        }

        tokio::time::sleep(poll_interval).await;
    }

    info!("CopyMet stopped gracefully.");
    Ok(())
}
