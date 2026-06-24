//! echo-grpc / listen — the SERVER NockApp for the private-gRPC echo demo.
//!
//! This is the binary you deploy under nockd. It boots the `listen` Hoon kernel from
//! `out.jam` (so it is a real, supervised NockApp the way nockd expects) and attaches the
//! private NockApp gRPC server driver. That driver exposes the kernel's +poke / +peek over
//! the private gRPC service on a local plaintext address.
//!
//! The echo roundtrip: a gRPC poke carries a cause `[%echo val]` which +poke stores in
//! kernel state and acks; a gRPC peek on `/echo` returns the stored value. See README.md.
//!
//! Metric: each poke makes the kernel emit `[%echoed val]`; a tiny effect-counting driver
//! here prints `metric: pokes=<N>` on its own line so nockd's status-cmd can scrape it
//! (grep -aoE 'pokes=[0-9]+' | tail -1 | grep -aoE '[0-9]+'). The `-a` is load-bearing on
//! macOS: the kernel boot log contains NUL bytes that make BSD grep treat stdin as binary.

use std::fs;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use nockapp::driver::{make_driver, NockAppHandle};
use nockapp::kernel::boot;
use nockapp::{exit_driver, NockApp, NockAppError};
use nockapp_grpc::services::private_nockapp::grpc_server_driver;
use nockvm::noun::{NounAllocator, D};
use nockvm_macros::tas;
use tracing::{error, info, warn};

/// Default private gRPC bind address. Overridable with `--grpc-addr <ip:port>` (nockd passes
/// this in the deploy args). Documented address for this demo: 127.0.0.1:5561.
const DEFAULT_GRPC_ADDR: &str = "127.0.0.1:5561";

/// Resolve the private gRPC bind address: prefer `--grpc-addr <ip:port>`, then the
/// `ECHO_GRPC_ADDR` env var, then the documented default.
fn resolve_grpc_addr() -> String {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--grpc-addr" => {
                if let Some(addr) = args.next() {
                    return addr;
                }
            }
            other => {
                if let Some(addr) = other.strip_prefix("--grpc-addr=") {
                    return addr.to_string();
                }
            }
        }
    }
    if let Ok(addr) = std::env::var("ECHO_GRPC_ADDR") {
        if !addr.is_empty() {
            return addr;
        }
    }
    DEFAULT_GRPC_ADDR.to_string()
}

/// A tiny IO driver that watches the kernel's effect stream for `[%echoed val]` effects,
/// counts them, and prints the greppable `metric: pokes=<N>` line that nockd's status-cmd
/// scrapes. This is how the Rust supervisor observes individual gRPC pokes served by the
/// kernel.
fn echo_metric_driver(counter: Arc<AtomicU64>) -> nockapp::driver::IODriverFn {
    make_driver(move |handle: NockAppHandle| async move {
        loop {
            match handle.next_effect().await {
                Ok(effect) => {
                    let is_echoed = {
                        let root = unsafe { effect.root() };
                        let space = effect.noun_space();
                        match root.in_space(&space).as_cell() {
                            Ok(cell) => unsafe {
                                cell.head().noun().raw_equals(&D(tas!(b"echoed")))
                            },
                            Err(_) => false,
                        }
                    };
                    if is_echoed {
                        let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
                        // The one clean, greppable metric line nockd's status-cmd reads.
                        println!("metric: pokes={n}");
                        info!("served echo poke #{n}");
                    }
                }
                Err(NockAppError::ChannelClosedError) => break,
                Err(_) => continue,
            }
        }
        Ok(())
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = boot::default_boot_cli(false);
    boot::init_default_tracing(&cli);

    // The nockchain dep graph enables BOTH rustls crypto providers; install one explicitly so
    // any TLS use does not panic on auto-select. The private gRPC surface is local plaintext,
    // so this is defensive, but harmless and cheap.
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        warn!("a rustls CryptoProvider was already installed");
    }

    let grpc_addr_str = resolve_grpc_addr();
    let grpc_addr: SocketAddr = grpc_addr_str
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid --grpc-addr '{grpc_addr_str}': {e}"))?;

    // Boot the kernel from out.jam (nockd stages our jam into the state dir as out.jam, which
    // is also our cwd, so this relative read works under both `talk`-driven smoke tests and
    // nockd). At rev 6d29078 `boot::setup` takes `cli` (not `Some(cli)`) — API drift from the
    // grpc template.
    let kernel = fs::read("out.jam")
        .or_else(|_| fs::read("listen.jam"))
        .map_err(|e| anyhow::anyhow!("Failed to read kernel (out.jam / listen.jam): {e}"))?;
    let mut nockapp: NockApp = boot::setup(&kernel, cli, &[], "echo-grpc", None)
        .await
        .map_err(|e| anyhow::anyhow!("kernel setup failed: {e}"))?;

    info!("echo-grpc listen: private gRPC server binding {grpc_addr}");
    println!("metric: grpc-addr={grpc_addr}");

    let counter = Arc::new(AtomicU64::new(0));
    nockapp.add_io_driver(echo_metric_driver(counter.clone())).await;
    nockapp.add_io_driver(grpc_server_driver(grpc_addr)).await;
    nockapp.add_io_driver(exit_driver()).await;

    // nockapp.run() owns the kernel loop and exits cleanly when the kernel exits / on the
    // exit driver. The grpc_server_driver inherits a SIGTERM-aware shutdown via the runtime;
    // on SIGTERM the process terminates and nockd records a clean stop.
    info!("echo-grpc listen: starting kernel loop");
    match nockapp.run().await {
        Ok(_) => Ok(()),
        // The NockApp framework installs SIGTERM/SIGINT handlers and exits with code
        // 128+signum (143 = SIGTERM, 130 = SIGINT). nockd sends SIGTERM on stop/restart, so
        // treat those as a clean shutdown rather than an error.
        Err(e) => match e {
            NockAppError::Exit(143) => {
                info!("received SIGTERM; shut down cleanly");
                Ok(())
            }
            NockAppError::Exit(130) => {
                info!("received SIGINT; shut down cleanly");
                Ok(())
            }
            other => {
                error!("kernel loop exited with error: {other:?}");
                Err(anyhow::anyhow!("kernel loop error: {other:?}"))
            }
        },
    }
}
