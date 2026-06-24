//! wordle: a playable Wordle (word-guessing) game served over HTTP, with ALL game logic
//! (target word selection, per-letter green/yellow/grey feedback with correct letter
//! multiplicity, win/loss detection) living in the Hoon kernel. This binary just boots the
//! kernel from `out.jam` and attaches the library's HTTP driver.
//!
//! Game state lives entirely in the Hoon kernel; nockd checkpoints kernel state, so an
//! in-progress game survives `nockd restart` for free.
//!
//! ## Port
//!
//! We pin the nockchain crates at a rev that includes PR #134's `HTTP_PORT` support, so the
//! stock `http_driver()` binds 127.0.0.1:$HTTP_PORT directly in local mode -- NO TCP proxy.
//! We set HTTP_PORT=8088 before the driver starts.
//!
//! ## Cache
//!
//! The driver caches GET responses keyed by URI. Guesses are POSTs (never cached, and their
//! response carries the freshly re-rendered grid), so GET `/` caching is harmless here.
//!
//! The spec asks for `EXPIRE_CACHE=0`, but at this rev (07577127) `EXPIRE_CACHE=0` PANICS the
//! driver: it builds `tokio::time::interval(Duration::from_secs(0))` ("`period` must be
//! non-zero"). So we set `EXPIRE_CACHE=1` (a 1-second TTL) -- the smallest value that doesn't
//! crash. GET `/` then re-pokes the kernel at least once a second (keeping it fresh + logging
//! the `metric: guesses=<N>` line); every guess/new is a POST whose response is never cached.

use std::error::Error;
use std::fs;

use nockapp::kernel::boot;
use nockapp::{http_driver, NockApp};
use tokio::signal::unix::{signal, SignalKind};

/// Public port this app serves on (also the HTTP_PORT the library driver binds in local mode).
const DEFAULT_PORT: &str = "8088";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1-second GET-cache TTL (set before the driver starts). NOT 0: EXPIRE_CACHE=0 panics this
    // rev's driver (Duration::ZERO -> "period must be non-zero"). 1 is the safe minimum; it keeps
    // GET / fresh by re-poking the kernel at least once a second. POSTs are never cached anyway.
    if std::env::var("EXPIRE_CACHE").is_err() {
        std::env::set_var("EXPIRE_CACHE", "1");
    }
    // Force local mode (bind 127.0.0.1, no ACME/HTTPS).
    std::env::set_var("HTTPS_DOMAIN", "localhost");
    // PR #134: the stock http_driver() reads HTTP_PORT for its local-mode bind. No proxy.
    // nockd may export NOCKD_APP_PORT (the port declared in nockd.toml); honor it if present.
    if let Ok(p) = std::env::var("NOCKD_APP_PORT") {
        std::env::set_var("HTTP_PORT", p);
    } else if std::env::var("HTTP_PORT").is_err() {
        std::env::set_var("HTTP_PORT", DEFAULT_PORT);
    }

    // boot::default_boot_cli builds a Cli struct directly; it does NOT parse argv, so nockd's
    // injected args do not collide with the boot CLI.
    let cli = boot::default_boot_cli(false);
    boot::init_default_tracing(&cli);

    let port = std::env::var("HTTP_PORT").unwrap_or_else(|_| DEFAULT_PORT.to_string());
    tracing::info!("wordle starting; HTTP port {port}");

    // The kernel jam is read cwd-relative. Under nockd the cwd is the app's state dir, where
    // nockd places out.jam; running by hand, run from the dir containing out.jam.
    let kernel = fs::read("out.jam").map_err(|e| format!("Failed to read out.jam: {}", e))?;

    // At this rev boot::setup takes `cli: Cli` (not Option<Cli>) and NockApp is generic
    // NockApp<J: Jammer> (inferred here).
    let mut nockapp: NockApp = boot::setup(&kernel, cli, &[], "wordle", None)
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
            tracing::info!("wordle: received SIGTERM; shutting down cleanly");
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("wordle: received SIGINT; shutting down cleanly");
        }
    }

    Ok(())
}
