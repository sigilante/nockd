//! http-static: an HTTP server that serves STATIC content straight from the Hoon kernel.
//!
//! This is the simplest "serve a page" NockApp -- the counterpart to http-counter, but with
//! NO mutable state: every `GET /` returns the same HTML, every `GET /about` returns the same
//! about page. The page lives in the Hoon kernel (`hoon/app/app.hoon`); this binary just boots
//! the kernel from `out.jam` and attaches an HTTP driver.
//!
//! ## Why the proxy?
//!
//! The proven, batteries-included way to drive HTTP from a NockApp is the library's
//! `nockapp::http_driver()`, which speaks the %req/%res noun protocol our kernel expects.
//! At the pinned rev (6d29078) it binds local mode to `127.0.0.1:8080` with NO port override,
//! and the noun-space helpers needed to write an equivalent custom driver out-of-crate are
//! private. So we run the library driver on 8080 and expose the app on the required port 8083
//! with a tiny in-process TCP proxy (8083 -> 8080). The proxy is transparent: clients hit 8083.
//! (Upstream PR #134 adds an HTTP_PORT override that removes the need for this proxy; the suite
//! will bump to it later when all revs move together. Until then we mirror http-counter.)
//!
//! NOTE: because the library driver's local backend port (8080) is HARDCODED at this rev, two
//! library-driver NockApps cannot run in local mode at the same time -- they would both try to
//! bind 8080. Run http-static OR http-counter, not both. (Override the PUBLIC port with
//! HTTP_PORT; the backend stays 8080.)
//!
//! We also set `EXPIRE_CACHE=0` so the library driver does NOT serve cached GET responses --
//! even though the content is static, the cache would otherwise stop re-poking the kernel,
//! and the kernel is what emits the `metric: requests=<N>` slog line. With caching disabled,
//! every request re-pokes the kernel, so `nockd ps` always has a current REQ count to grep.

use std::error::Error;
use std::fs;

use nockapp::kernel::boot;
use nockapp::{http_driver, NockApp};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::signal::unix::{signal, SignalKind};

/// Public port this app serves on. Override with HTTP_PORT.
const DEFAULT_PORT: u16 = 8083;
/// The library http_driver()'s hardcoded local-mode bind port.
const BACKEND_PORT: u16 = 8080;

fn resolve_port() -> u16 {
    std::env::var("HTTP_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT)
}

/// Transparent TCP proxy: accept on `front_port`, forward each connection to
/// 127.0.0.1:`back_port`. Runs until the process exits.
async fn run_proxy(front_port: u16, back_port: u16) {
    let listener = match TcpListener::bind(("127.0.0.1", front_port)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("http-static proxy: failed to bind 127.0.0.1:{front_port}: {e}");
            return;
        }
    };
    tracing::info!("http-static listening on http://127.0.0.1:{front_port} (-> :{back_port})");
    loop {
        match listener.accept().await {
            Ok((mut inbound, _peer)) => {
                tokio::spawn(async move {
                    match TcpStream::connect(("127.0.0.1", back_port)).await {
                        Ok(mut outbound) => {
                            if let Err(e) = copy_bidirectional(&mut inbound, &mut outbound).await {
                                tracing::debug!("proxy connection ended: {e}");
                            }
                        }
                        Err(e) => {
                            tracing::warn!("proxy: backend :{back_port} not ready: {e}");
                        }
                    }
                });
            }
            Err(e) => tracing::warn!("proxy accept error: {e}"),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Defeat the library http driver's GET response cache so every request re-pokes the
    // kernel (so the kernel emits a `metric: requests=<N>` line per request). Set before the
    // driver starts.
    std::env::set_var("EXPIRE_CACHE", "0");
    // Force local mode (bind 127.0.0.1:8080, no ACME/HTTPS).
    std::env::set_var("HTTPS_DOMAIN", "localhost");

    // boot::default_boot_cli builds a Cli struct directly; it does NOT parse argv, so nockd's
    // injected args do not collide with the boot CLI.
    let cli = boot::default_boot_cli(false);
    boot::init_default_tracing(&cli);

    let port = resolve_port();
    tracing::info!("http-static starting; public port {port}");

    // The kernel jam is read cwd-relative. Under nockd the cwd is the app's state dir, where
    // nockd places out.jam; running by hand, run from the dir containing out.jam.
    let kernel = fs::read("out.jam").map_err(|e| format!("Failed to read out.jam: {}", e))?;

    // At rev 6d29078 boot::setup takes `cli: Cli` (not Option<Cli>) and NockApp is generic
    // NockApp<J: Jammer> (inferred here).
    let mut nockapp: NockApp = boot::setup(&kernel, cli, &[], "http-static", None)
        .await
        .map_err(|e| format!("Kernel setup failed: {}", e))?;

    nockapp.add_io_driver(http_driver()).await;

    // Expose the app on the required port via the transparent proxy.
    tokio::spawn(run_proxy(port, BACKEND_PORT));

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
