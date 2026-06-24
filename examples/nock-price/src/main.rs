//! nock-price — a long-lived NockApp service that polls the live $NOCK token price from
//! multiple venues and logs greppable per-source and aggregate USD metric lines per tick.
//!
//! Architecture: this is a real, supervised NockApp. We boot the (trivial) Hoon kernel
//! from `out.jam` via `nockapp::kernel::boot::setup` so the process is a valid NockApp the
//! way nockd expects, then run the price-polling loop in Rust. The kernel here is just the
//! `basic` template kernel — all the price logic lives in this file. (Same shape as the
//! `chain-watch` example; see its RECIPE.md for the build/deploy gotchas this inherits.)
//!
//! Sources (all probed live 2026-06-24), each fetched INDEPENDENTLY and resiliently — any
//! source that errors, times out, or returns 0/missing is simply skipped for that tick and
//! never crashes the loop:
//!   - BASE   : DexScreener, highest-liquidity Base pair (Aerodrome NOCK/USDC). priceUsd.
//!   - KRAKEN : public Ticker NOCKUSD, result.NOCKUSD.c[0]. Returns 0 until its listing
//!              trades; self-activates once c[0] > 0.
//!   - SAFETRADE : via the CoinGecko mirror (safe.trade is Cloudflare-blocked to servers).
//!                 coins/nockchain/tickers, the ticker whose market.name == "SafeTrade".
//!
//! Aggregate = median of the live (non-skipped) sources.
//!
//! Status metric (DESIGN status-cmd): we print `metric: nock_usd=<price>` on its OWN line
//! (distinct from the per-source `nock_usd_base=` etc.) so the deploy manifest's status
//! command — `grep -aoE 'nock_usd=[0-9.]+' | tail -1 | grep -aoE '[0-9.]+'` — surfaces the
//! live aggregate price in `nockd ps`. (The `-a` is load-bearing: the kernel boot log
//! contains NUL bytes, which make BSD grep treat the stream as binary; see RECIPE.md. The
//! `[0-9.]+` allows the decimal point, unlike chain-watch's integer height.)

use std::error::Error;
use std::fs;
use std::time::Duration;

use nockapp::kernel::boot;
use nockapp::noun::slab::NounSlab;
use nockapp::wire::{SystemWire, Wire};
use nockapp::NockApp;
use nockvm::noun::{D, T};
use nockvm_macros::tas;
use serde_json::Value;
use tracing::{info, warn};

/// Poll every 60s — respects the public APIs' rate limits (CoinGecko in particular).
const POLL_INTERVAL: Duration = Duration::from_secs(60);
/// Per-request timeout so a slow/hung source can't stall the whole tick.
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

const DEXSCREENER_URL: &str =
    "https://api.dexscreener.com/latest/dex/tokens/0x9B5E262cF9bb04869ab40b19AF91D2dc85761722";
const KRAKEN_URL: &str = "https://api.kraken.com/0/public/Ticker?pair=NOCKUSD";
const COINGECKO_URL: &str = "https://api.coingecko.com/api/v3/coins/nockchain/tickers";

/// BASE anchor: DexScreener. Pick the highest-liquidity Base pair and read its USD price.
/// Returns `None` (skip this tick) on any error, non-positive, or missing field.
async fn fetch_base(client: &reqwest::Client) -> Option<f64> {
    let resp = client.get(DEXSCREENER_URL).send().await.ok()?;
    let json: Value = resp.json().await.ok()?;
    let pairs = json.get("pairs")?.as_array()?;
    let mut best: Option<(f64, f64)> = None; // (liquidity_usd, price_usd)
    for p in pairs {
        if p.get("chainId").and_then(Value::as_str) != Some("base") {
            continue;
        }
        let liq = p
            .get("liquidity")
            .and_then(|l| l.get("usd"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        // priceUsd is a string in the DexScreener payload.
        let price = p
            .get("priceUsd")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<f64>().ok());
        if let Some(price) = price {
            if price > 0.0 && best.map_or(true, |(bl, _)| liq > bl) {
                best = Some((liq, price));
            }
        }
    }
    best.map(|(_, price)| price).filter(|p| *p > 0.0)
}

/// KRAKEN: result.NOCKUSD.c[0] (last-trade price, a string). Returns 0 until the listing
/// trades; we treat 0/error/missing as skip. It self-activates once c[0] > 0.
async fn fetch_kraken(client: &reqwest::Client) -> Option<f64> {
    let resp = client.get(KRAKEN_URL).send().await.ok()?;
    let json: Value = resp.json().await.ok()?;
    // Read the first key under result (the canonical pair name may be "NOCKUSD" or similar).
    let result = json.get("result")?.as_object()?;
    let pair = result.values().next()?;
    let price = pair
        .get("c")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<f64>().ok())?;
    if price > 0.0 {
        Some(price)
    } else {
        None
    }
}

/// SAFETRADE via the CoinGecko mirror: the ticker whose market.name == "SafeTrade".
/// converted_last.usd (USDT ≈ USD). Skip if absent.
async fn fetch_safetrade(client: &reqwest::Client) -> Option<f64> {
    let resp = client.get(COINGECKO_URL).send().await.ok()?;
    let json: Value = resp.json().await.ok()?;
    let tickers = json.get("tickers")?.as_array()?;
    for t in tickers {
        if t.get("market").and_then(|m| m.get("name")).and_then(Value::as_str) == Some("SafeTrade")
        {
            let price = t
                .get("converted_last")
                .and_then(|c| c.get("usd"))
                .and_then(Value::as_f64);
            if let Some(price) = price {
                if price > 0.0 {
                    return Some(price);
                }
            }
        }
    }
    None
}

/// Median of the live sources (robust to one venue being an outlier).
fn median(mut vals: Vec<f64>) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = vals.len();
    Some(if n % 2 == 1 {
        vals[n / 2]
    } else {
        (vals[n / 2 - 1] + vals[n / 2]) / 2.0
    })
}

/// Fetch all sources for one tick, log per-source lines, and log/return the aggregate.
async fn poll_once(client: &reqwest::Client) {
    let (base, kraken, safetrade) =
        tokio::join!(fetch_base(client), fetch_kraken(client), fetch_safetrade(client));

    let mut live: Vec<f64> = Vec::new();

    match base {
        Some(p) => {
            println!("metric: nock_usd_base={p:.5}");
            live.push(p);
        }
        None => {
            println!("metric: nock_usd_base=skip");
            warn!("base (dexscreener) source unavailable this tick");
        }
    }
    match kraken {
        Some(p) => {
            println!("metric: nock_usd_kraken={p:.5}");
            live.push(p);
        }
        None => {
            // Expected until the Kraken listing trades; self-activates once c[0] > 0.
            println!("metric: nock_usd_kraken=skip");
        }
    }
    match safetrade {
        Some(p) => {
            println!("metric: nock_usd_safetrade={p:.5}");
            live.push(p);
        }
        None => {
            println!("metric: nock_usd_safetrade=skip");
            warn!("safetrade (coingecko mirror) source unavailable this tick");
        }
    }

    match median(live) {
        Some(agg) => {
            // The aggregate on its OWN line. Note `nock_usd=` will NOT match the
            // `nock_usd_base=` etc. lines under an `=`-anchored grep, so the status-cmd
            // cleanly isolates this value.
            println!("metric: nock_usd={agg:.5}");
        }
        None => {
            warn!("all price sources unavailable this tick; no aggregate");
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = boot::default_boot_cli(false);
    boot::init_default_tracing(&cli);

    // The nockchain dependency graph pulls in BOTH rustls crypto providers (`ring` and
    // `aws-lc-rs`), so rustls cannot auto-pick one and panics on the first TLS handshake
    // ("Could not automatically determine the process-level CryptoProvider"). Install one
    // explicitly before any HTTPS use. Every price API below is https.
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        // Already installed (e.g. by a dependency) — fine.
        warn!("a rustls CryptoProvider was already installed");
    }

    // Boot the kernel so this is a real, supervised NockApp (consumes out.jam, sets up the
    // PMA/event-log state dir, etc.). nockd stages out.jam into the app's state dir, which
    // is also our cwd, so the relative read works both under `nockup run` and under nockd.
    let kernel = fs::read("out.jam").map_err(|e| format!("Failed to read out.jam: {}", e))?;
    let mut nockapp: NockApp = boot::setup(&kernel, cli, &[], "nock-price", None).await?;

    // Fire the kernel's demo poke once so the kernel is genuinely exercised at boot.
    let mut poke_slab = NounSlab::new();
    let command_noun = T(&mut poke_slab, &[D(tas!(b"cause")), D(0x0)]);
    poke_slab.set_root(command_noun);
    if let Err(e) = nockapp.poke(SystemWire.to_wire(), poke_slab).await {
        warn!("kernel boot poke failed (non-fatal): {e:?}");
    }
    // Keep the kernel handle alive for the lifetime of the process (a supervised NockApp
    // owns its state dir). We intentionally do not call `nockapp.run()`: this app has no
    // Hoon-side IO drivers; its work is the Rust poll loop below.
    let _nockapp = nockapp;

    info!(
        "nock-price starting; polling {} sources every {}s",
        3,
        POLL_INTERVAL.as_secs()
    );

    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent("nock-price/0.1 (NockApp price watcher)")
        .build()?;

    // Graceful shutdown: nockd SIGTERMs on stop/restart. Exit the loop cleanly on SIGTERM
    // (and Ctrl-C when run by hand).
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut ticker = tokio::time::interval(POLL_INTERVAL);

    loop {
        tokio::select! {
            _ = sigterm.recv() => {
                info!("received SIGTERM; shutting down cleanly");
                break;
            }
            _ = tokio::signal::ctrl_c() => {
                info!("received Ctrl-C; shutting down cleanly");
                break;
            }
            _ = ticker.tick() => {
                poll_once(&client).await;
            }
        }
    }

    Ok(())
}
