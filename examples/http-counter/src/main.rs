//! http-counter: an HTTP server that serves and increments a counter whose value PERSISTS
//! across restarts.
//!
//! The counter lives entirely in the Hoon kernel state. nockd checkpoints kernel state (PMA
//! + event log), so the count survives `nockd restart` / process restart "for free" -- this
//! binary just boots the kernel from `out.jam` and attaches an HTTP driver.
//!
//! ## Port binding
//!
//! The proven, batteries-included way to drive HTTP from a NockApp is the library's
//! `nockapp::http_driver()`, which speaks the %req/%res noun protocol our kernel expects. As
//! of PR #134 the local-mode driver reads the `HTTP_PORT` env var and binds
//! `127.0.0.1:<HTTP_PORT>` directly, so we just set `HTTP_PORT=8081` before the driver starts
//! -- no in-process proxy, and no shared :8080 backend, so this app coexists with http-static
//! (which binds 8083 the same way).
//!
//! We also set `EXPIRE_CACHE=0` so the library driver does NOT serve cached GET responses --
//! every request re-pokes the kernel, which (a) keeps the displayed count fresh after every
//! increment and (b) makes the kernel emit its `metric: count=<N>` slog line on EVERY
//! request, so `nockd ps` always has a current COUNT to grep.

use std::error::Error;
use std::fs;

use nockapp::kernel::boot;
use nockapp::{http_driver, NockApp};
use tokio::signal::unix::{signal, SignalKind};

/// Port this app serves on (bound directly by the library driver via HTTP_PORT).
const HTTP_PORT: &str = "8081";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Bind the local HTTP driver to our port directly (PR #134's HTTP_PORT override). Set
    // before the driver starts.
    std::env::set_var("HTTP_PORT", HTTP_PORT);
    // Defeat the library http driver's GET response cache so every request re-pokes the
    // kernel (fresh count + a metric log line per request). Set before the driver starts.
    std::env::set_var("EXPIRE_CACHE", "0");
    // Force local mode (bind 127.0.0.1, no ACME/HTTPS).
    std::env::set_var("HTTPS_DOMAIN", "localhost");

    // boot::default_boot_cli builds a Cli struct directly; it does NOT parse argv, so nockd's
    // injected args do not collide with the boot CLI.
    let cli = boot::default_boot_cli(false);
    boot::init_default_tracing(&cli);

    tracing::info!("http-counter starting; serving http://127.0.0.1:{HTTP_PORT}");

    // The kernel jam is read cwd-relative. Under nockd the cwd is the app's state dir, where
    // nockd places out.jam; running by hand, run from the dir containing out.jam.
    let kernel = fs::read("out.jam").map_err(|e| format!("Failed to read out.jam: {}", e))?;

    // At this rev boot::setup takes `cli: Cli` (not Option<Cli>) and NockApp is generic
    // NockApp<J: Jammer> (inferred here).
    let mut nockapp: NockApp = boot::setup(&kernel, cli, &[], "http-counter", None)
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
            tracing::info!("http-counter: received SIGTERM; shutting down cleanly");
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("http-counter: received SIGINT; shutting down cleanly");
        }
    }

    Ok(())
}
