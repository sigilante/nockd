//! http-static: an HTTP server that serves STATIC content straight from the Hoon kernel.
//!
//! This is the simplest "serve a page" NockApp -- the counterpart to http-counter, but with
//! NO mutable state: every `GET /` returns the same HTML, every `GET /about` returns the same
//! about page. The page lives in the Hoon kernel (`hoon/app/app.hoon`); this binary just boots
//! the kernel from `out.jam` and attaches an HTTP driver.
//!
//! ## Port binding
//!
//! The proven, batteries-included way to drive HTTP from a NockApp is the library's
//! `nockapp::http_driver()`, which speaks the %req/%res noun protocol our kernel expects. As
//! of PR #134 the local-mode driver reads the `HTTP_PORT` env var and binds
//! `127.0.0.1:<HTTP_PORT>` directly, so we just set `HTTP_PORT=8083` before the driver starts
//! -- no in-process proxy, and no shared :8080 backend, so this app coexists with http-counter
//! (which binds 8081 the same way).
//!
//! We also set `EXPIRE_CACHE=0` so the library driver does NOT serve cached GET responses --
//! even though the content is static, the cache would otherwise stop re-poking the kernel,
//! and the kernel is what emits the `metric: requests=<N>` slog line. With caching disabled,
//! every request re-pokes the kernel, so `nockd ps` always has a current REQ count to grep.

use std::error::Error;
use std::fs;

use nockapp::kernel::boot;
use nockapp::{http_driver, NockApp};
use tokio::signal::unix::{signal, SignalKind};

/// Fallback port when run standalone (outside nockd). Under nockd, NOCKD_APP_PORT wins, so the
/// port is declared only in nockd.toml.
const DEFAULT_PORT: &str = "8083";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // nockd owns the port: it exports NOCKD_APP_PORT (declared once in nockd.toml). Bridge it
    // to the library driver's HTTP_PORT (PR #134's override); fall back to DEFAULT_PORT when
    // run standalone. Set before the driver starts.
    let http_port = std::env::var("NOCKD_APP_PORT").unwrap_or_else(|_| DEFAULT_PORT.to_string());
    std::env::set_var("HTTP_PORT", &http_port);
    // Defeat the library http driver's GET response cache so every request re-pokes the
    // kernel (so the kernel emits a `metric: requests=<N>` line per request). Set before the
    // driver starts.
    std::env::set_var("EXPIRE_CACHE", "0");
    // Force local mode (bind 127.0.0.1, no ACME/HTTPS).
    std::env::set_var("HTTPS_DOMAIN", "localhost");

    // boot::default_boot_cli builds a Cli struct directly; it does NOT parse argv, so nockd's
    // injected args do not collide with the boot CLI.
    let cli = boot::default_boot_cli(false);
    boot::init_default_tracing(&cli);

    tracing::info!("http-static starting; serving http://127.0.0.1:{http_port}");

    // The kernel jam is read cwd-relative. Under nockd the cwd is the app's state dir, where
    // nockd places out.jam; running by hand, run from the dir containing out.jam.
    let kernel = fs::read("out.jam").map_err(|e| format!("Failed to read out.jam: {}", e))?;

    // At this rev boot::setup takes `cli: Cli` (not Option<Cli>) and NockApp is generic
    // NockApp<J: Jammer> (inferred here).
    let mut nockapp: NockApp = boot::setup(&kernel, cli, &[], "http-static", None)
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
            tracing::info!("http-static: received SIGTERM; shutting down cleanly");
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("http-static: received SIGINT; shutting down cleanly");
        }
    }

    Ok(())
}
