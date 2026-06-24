//! conway: Conway's Game of Life served over HTTP, with ALL game logic (the live-cell
//! set, neighbor counting, and the next-generation step) living in the Hoon kernel.
//! This binary just boots the kernel from `out.jam` and attaches the library's HTTP driver.
//!
//! Game state lives entirely in the Hoon kernel; nockd checkpoints kernel state, so the
//! current board + generation count survive `nockd restart` for free.
//!
//! ## Port
//!
//! We pin the nockchain crates at a rev that includes PR #134's `HTTP_PORT` support, so the
//! stock `http_driver()` binds 127.0.0.1:$HTTP_PORT directly in local mode -- NO TCP proxy.
//! We set HTTP_PORT=8089 before the driver starts.
//!
//! ## Cache
//!
//! The driver caches GET responses keyed by URI. Every page here is GET `/`, so a cached
//! board would go stale after a step. We want GET `/` to re-poke the kernel for a fresh
//! board + a `metric: gen=<N>` slog line.
//!
//! IMPORTANT: `EXPIRE_CACHE=0` PANICS at this rev. The driver builds
//! `tokio::time::interval(Duration::from_secs(0))`, and `interval(ZERO)` panics with
//! "`period` must be non-zero" -- which kills a tokio worker and makes nockd restart the
//! process (losing in-memory board state). So we use `EXPIRE_CACHE=1` (a 1-second cache
//! TTL): GET `/` re-pokes the kernel at least once a second, and mutations
//! (toggle/step/clear/random) are POSTs whose responses are never cached and already carry
//! the freshly re-rendered board, so the page is always effectively fresh.

use std::error::Error;
use std::fs;

use nockapp::kernel::boot;
use nockapp::{http_driver, NockApp};
use tokio::signal::unix::{signal, SignalKind};

/// Public port this app serves on (also the HTTP_PORT the library driver binds in local mode).
const DEFAULT_PORT: &str = "8089";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Keep GET responses fresh with a 1-second cache TTL so GET / re-pokes the kernel at
    // least once a second (fresh board + a `metric: gen=<N>` slog line). Set before the
    // driver starts. Must NOT be 0: Duration::ZERO panics the driver's invalidation timer
    // (see the module docs), which would make nockd restart the process and drop board state.
    std::env::set_var("EXPIRE_CACHE", "1");
    // Force local mode (bind 127.0.0.1, no ACME/HTTPS).
    std::env::set_var("HTTPS_DOMAIN", "localhost");
    // PR #134: the stock http_driver() reads HTTP_PORT for its local-mode bind. No proxy.
    std::env::set_var("HTTP_PORT", DEFAULT_PORT);

    // boot::default_boot_cli builds a Cli struct directly; it does NOT parse argv, so nockd's
    // injected args do not collide with the boot CLI.
    let cli = boot::default_boot_cli(false);
    boot::init_default_tracing(&cli);

    let port = std::env::var("HTTP_PORT").unwrap_or_else(|_| DEFAULT_PORT.to_string());
    tracing::info!("conway starting; HTTP port {port}");

    // The kernel jam is read cwd-relative. Under nockd the cwd is the app's state dir, where
    // nockd places out.jam; running by hand, run from the dir containing out.jam.
    let kernel = fs::read("out.jam").map_err(|e| format!("Failed to read out.jam: {}", e))?;

    // At this rev boot::setup takes `cli: Cli` (not Option<Cli>) and NockApp is generic
    // NockApp<J: Jammer> (inferred here).
    let mut nockapp: NockApp = boot::setup(&kernel, cli, &[], "conway", None)
        .await
        .map_err(|e| format!("Kernel setup failed: {}", e))?;

    nockapp.add_io_driver(http_driver()).await;

    // Run the app, racing against SIGTERM/SIGINT so nockd stop/restart shuts us down cleanly.
    let mut sigterm = signal(SignalKind::terminate())?;
    tokio::select! {
        res = nockapp.run() => {
            res.map_err(|e| format!("NockApp run failed: {}", e))?;
        }
        _ = sigterm.recv() => {
            tracing::info!("conway: received SIGTERM; shutting down cleanly");
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("conway: received SIGINT; shutting down cleanly");
        }
    }

    Ok(())
}
