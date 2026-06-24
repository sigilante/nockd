//! chain-watch — a long-lived NockApp service that polls the Nockchain public gRPC
//! endpoint for the heaviest block height and logs one greppable metric line per poll.
//!
//! Architecture: this is a real, supervised NockApp. We boot the (trivial) Hoon kernel
//! from `out.jam` via `nockapp::kernel::boot::setup` so the process is a valid NockApp the
//! way nockd expects, then run the chain-polling loop in Rust. The kernel here is just the
//! `basic` template kernel — all the chain logic lives in this file.
//!
//! Endpoint resolution (nockd "endpoint-by-name", DESIGN §5.3): nockd resolves a named
//! endpoint to a URL and (a) substitutes `{endpoint}` in our args and (b) sets the
//! `NOCKD_ENDPOINT_URL` env var. We accept either, preferring an explicit `--endpoint <url>`.
//!
//! Status metric (DESIGN status-cmd): we print `metric: height=<N>` on its own line so the
//! deploy manifest's status command — `grep -aoE 'height=[0-9]+' | tail -1 | grep -aoE '[0-9]+'`
//! — surfaces the live chain tip in `nockd ps`. (The `-a` is load-bearing: the kernel boot
//! log contains NUL bytes, which make BSD grep treat the stream as binary; see RECIPE.md.)

use std::error::Error;
use std::fs;
use std::time::Duration;

use nockapp::kernel::boot;
use nockapp::noun::slab::NounSlab;
use nockapp::wire::{SystemWire, Wire};
use nockapp::NockApp;
use nockapp_grpc::services::public_nockchain::v2::client::PublicNockchainGrpcClient;
use nockvm::noun::{D, T};
use nockvm_macros::tas;
use tracing::{error, info, warn};

const POLL_INTERVAL: Duration = Duration::from_secs(10);

/// Resolve the chain endpoint URL: prefer an explicit `--endpoint <url>` CLI arg (nockd
/// substitutes `{endpoint}` here), then fall back to the `NOCKD_ENDPOINT_URL` env var that
/// nockd also sets, then a sane public default.
fn resolve_endpoint() -> String {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--endpoint" => {
                if let Some(url) = args.next() {
                    return url;
                }
            }
            other => {
                if let Some(url) = other.strip_prefix("--endpoint=") {
                    return url.to_string();
                }
            }
        }
    }
    if let Ok(url) = std::env::var("NOCKD_ENDPOINT_URL") {
        if !url.is_empty() {
            return url;
        }
    }
    "https://rpc.nockchain.net".to_string()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = boot::default_boot_cli(false);
    boot::init_default_tracing(&cli);

    // The nockchain dependency graph pulls in BOTH rustls crypto providers (`ring` and
    // `aws-lc-rs`), so rustls cannot auto-pick one and panics on the first TLS handshake
    // ("Could not automatically determine the process-level CryptoProvider"). Install one
    // explicitly before any gRPC/TLS use. Required for the `https://` public RPC endpoint.
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
    let mut nockapp: NockApp = boot::setup(&kernel, cli, &[], "chain-watch", None).await?;

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

    let endpoint = resolve_endpoint();
    info!("chain-watch starting; polling endpoint={endpoint} every {}s", POLL_INTERVAL.as_secs());
    // The greppable boot marker so logs always show the resolved endpoint.
    println!("metric: endpoint={endpoint}");

    // Graceful shutdown: nockd SIGTERMs on stop/restart. Exit the loop cleanly on SIGTERM
    // (and Ctrl-C when run by hand).
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    // Connect lazily, reconnecting on failure so a transient endpoint blip doesn't kill us.
    let mut client: Option<PublicNockchainGrpcClient> = None;
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
                if client.is_none() {
                    match PublicNockchainGrpcClient::connect(endpoint.clone()).await {
                        Ok(c) => {
                            info!("connected to {endpoint}");
                            client = Some(c);
                        }
                        Err(e) => {
                            warn!("connect to {endpoint} failed, will retry: {e}");
                            continue;
                        }
                    }
                }
                if let Some(c) = client.as_mut() {
                    match c.explorer_heaviest_height().await {
                        Ok(height) => {
                            let height: u64 = height;
                            // The one clean, greppable metric line nockd's status-cmd scrapes.
                            println!("metric: height={height}");
                        }
                        Err(e) => {
                            error!("explorer_heaviest_height failed: {e}; reconnecting next tick");
                            // Drop the client so we reconnect on the next tick.
                            client = None;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
