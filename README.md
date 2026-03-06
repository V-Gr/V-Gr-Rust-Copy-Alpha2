# CopyMet — Polymarket Copy Trading Bot

Ultra-fast Rust copy trading bot for Polymarket. Monitors a target wallet and replicates positions with proportional sizing.

## Architecture

```
┌─────────────┐     ┌──────────────┐     ┌──────────────┐     ┌───────────┐
│   Monitor   │────▸│  Diff Engine  │────▸│    Sizer     │────▸│  Executor │
│ (poll target│     │ (detect Δ)   │     │ (scale size) │     │ (place    │
│  positions) │     │              │     │              │     │  orders)  │
└─────────────┘     └──────────────┘     └──────────────┘     └───────────┘
       │                                        │                    │
       ▼                                        ▼                    ▼
  Gamma API                              Balance Ratio          CLOB API
  (public)                            our_bal / target_val    (HMAC auth)
```

## How It Works

1. **Baseline Capture**: On startup, snapshots the target wallet's positions without acting on them.
2. **Polling Loop**: Every `POLL_INTERVAL_MS` (default 500ms), fetches target positions from the Gamma API.
3. **Delta Detection**: Diffs current snapshot vs previous — detects OPEN, INCREASE, DECREASE, CLOSE events.
4. **Proportional Sizing**: Scales target trade sizes by `(our_balance / target_portfolio_value)`.
   - Target has $5000 portfolio, you have $100 → ratio = 0.02 → you trade 2% of their size.
   - Floor at $1 minimum bet (Polymarket enforced).
   - Cap at 25% of your balance per single trade (risk management).
5. **Execution**: Places FOK (Fill-or-Kill) orders with slippage protection against the order book.
6. **Close Mirroring**: When target reduces/closes positions, closes proportionally.

## Setup

### 1. Prerequisites
- Rust toolchain (`rustup install stable`)
- Polymarket CLOB API credentials (get from [Polymarket](https://polymarket.com))

### 2. Configure
Copy `.env.example` to `.env` and fill in your credentials:

```bash
cp .env.example .env
```

```env
POLYMARKET_API_KEY=your_api_key
POLYMARKET_API_SECRET=your_api_secret_base64
POLYMARKET_API_PASSPHRASE=your_passphrase
POLYMARKET_FUNDER_ADDRESS=0xYourWalletAddress
TARGET_WALLET_ADDRESS=0xTargetWalletToFollow

# Optional tuning
POLL_INTERVAL_MS=500        # How fast to poll (ms)
MIN_BET_SIZE=1.0            # Minimum order size in USDC
MAX_SLIPPAGE_BPS=50         # Max slippage in basis points (50 = 0.5%)
DRY_RUN=true                # Set to true to log without executing
RUST_LOG=info               # Logging level (debug for verbose)
```

### 3. Build & Run

```bash
# Build optimized release binary
cargo build --release

# Run (dry run mode first!)
DRY_RUN=true cargo run --release

# Run for real
cargo run --release
```

## Configuration Reference

| Variable | Required | Default | Description |
|---|---|---|---|
| `POLYMARKET_API_KEY` | Yes | — | CLOB API key |
| `POLYMARKET_API_SECRET` | Yes | — | CLOB API secret (base64) |
| `POLYMARKET_API_PASSPHRASE` | Yes | — | CLOB API passphrase |
| `POLYMARKET_FUNDER_ADDRESS` | Yes | — | Your wallet/proxy address |
| `TARGET_WALLET_ADDRESS` | Yes | — | Wallet to copy trade |
| `POLL_INTERVAL_MS` | No | 500 | Polling interval in ms |
| `MIN_BET_SIZE` | No | 1.0 | Minimum bet in USDC |
| `MAX_SLIPPAGE_BPS` | No | 50 | Max slippage (basis points) |
| `DRY_RUN` | No | false | Log trades without executing |
| `RUST_LOG` | No | info | Log level |

## Position Sizing Formula

```
ratio = our_usdc_balance / target_estimated_portfolio_value

scaled_size = target_trade_size × ratio

# Safety caps:
if scaled_size × price > 25% of our balance:
    scaled_size = (25% of balance) / price

if scaled_size × price < $1:
    bump to minimum or skip

# For closes:
close_fraction = target_closed / target_total_before_close
our_close = our_position_size × close_fraction
```

## Risk Management

- **Max single trade**: 25% of balance (configurable in code)
- **Slippage protection**: Orders use FOK at best book price + slippage buffer
- **Min bet enforcement**: Polymarket's $1 minimum is always respected
- **Dry run mode**: Test everything before going live
- **Graceful shutdown**: Ctrl+C stops cleanly without orphaned orders

## Modules

| File | Purpose |
|---|---|
| `main.rs` | Orchestration, startup, main loop |
| `config.rs` | Environment variable loading |
| `auth.rs` | HMAC-SHA256 signing for CLOB API |
| `client.rs` | HTTP client for CLOB + Gamma APIs |
| `monitor.rs` | Target wallet position tracking & diffing |
| `sizer.rs` | Proportional position sizing engine |
| `executor.rs` | Order execution with slippage protection |
| `types.rs` | Data structures for API responses |
