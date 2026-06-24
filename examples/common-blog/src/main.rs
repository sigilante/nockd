//! common-blog: a minimal self-hosted blog served straight from the Hoon kernel.
//!
//! Publish posts and read them back over HTTP. ALL blog logic + storage lives in the Hoon
//! kernel (`hoon/app/app.hoon`): the post map, the slugifier, the URI/form parsing, and the
//! HTML rendering. This binary just boots the kernel from `out.jam` and attaches an HTTP
//! driver.
//!
//! This is a NockApp *reimplementation* of the Urbit "Common Blog" app's core idea -- the
//! data model (slug -> {title, body} map) and the publish/read UX -- rebuilt in the NockApp
//! request/response (%req/%res noun) shape. None of the original Gall/Urbit machinery (sss
//! syndication, Clay export, the React editor SPA, ship-auth) is carried over.
//!
//! ## Persistence (the headline)
//!
//! Published posts live entirely in the Hoon kernel state. nockd checkpoints kernel state
//! (PMA + event log), so posts survive `nockd restart` / process restart "for free".
//!
//! ## Port binding
//!
//! The proven, batteries-included way to drive HTTP from a NockApp is the library's
//! `nockapp::http_driver()`, which speaks the %req/%res noun protocol our kernel expects. As
//! of PR #134 the local-mode driver reads the `HTTP_PORT` env var and binds
//! `127.0.0.1:<HTTP_PORT>` directly. nockd is the single source of truth for the port: it
//! exports `NOCKD_PORT` (declared once in `nockd.toml`), and we bridge that to the driver's
//! `HTTP_PORT`. Run by hand without nockd and it falls back to 8085. No in-process proxy, and
//! no shared :8080 backend, so this app coexists with the other example apps.
//!
//! We also set `EXPIRE_CACHE=0` so the library driver does NOT serve cached GET responses --
//! otherwise a `GET /` after publishing a new post would return the stale cached index, and
//! the kernel (which emits the `metric: posts=<N>` slog line) would never be re-poked. With
//! caching disabled, every request re-pokes the kernel, so `nockd ps` always has a current
//! POSTS count to grep.

use std::error::Error;
use std::fs;

use nockapp::kernel::boot;
use nockapp::{http_driver, NockApp};
use tokio::signal::unix::{signal, SignalKind};

/// Fallback port when run standalone (outside nockd). Under nockd, NOCKD_PORT wins, so the
/// port is declared only in nockd.toml.
const DEFAULT_PORT: &str = "8085";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // nockd owns the port: it exports NOCKD_PORT. Bridge it to the library driver's HTTP_PORT
    // (PR #134's override). Falls back to DEFAULT_PORT when run by hand. Set before the driver.
    let http_port = std::env::var("NOCKD_PORT").unwrap_or_else(|_| DEFAULT_PORT.to_string());
    std::env::set_var("HTTP_PORT", &http_port);
    // Defeat the library http driver's GET response cache so every request re-pokes the
    // kernel (fresh index/post after publishing + a `metric: posts=<N>` line per request).
    // Set before the driver starts.
    std::env::set_var("EXPIRE_CACHE", "0");
    // Force local mode (bind 127.0.0.1, no ACME/HTTPS).
    std::env::set_var("HTTPS_DOMAIN", "localhost");

    // boot::default_boot_cli builds a Cli struct directly; it does NOT parse argv, so nockd's
    // injected args do not collide with the boot CLI.
    let cli = boot::default_boot_cli(false);
    boot::init_default_tracing(&cli);

    tracing::info!("common-blog starting; serving http://127.0.0.1:{http_port}");

    // The kernel jam is read cwd-relative. Under nockd the cwd is the app's state dir, where
    // nockd places out.jam; running by hand, run from the dir containing out.jam.
    let kernel = fs::read("out.jam").map_err(|e| format!("Failed to read out.jam: {}", e))?;

    // At this rev boot::setup takes `cli: Cli` (not Option<Cli>) and NockApp is generic
    // NockApp<J: Jammer> (inferred here).
    let mut nockapp: NockApp = boot::setup(&kernel, cli, &[], "common-blog", None)
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
            tracing::info!("common-blog: received SIGTERM; shutting down cleanly");
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("common-blog: received SIGINT; shutting down cleanly");
        }
    }

    Ok(())
}
