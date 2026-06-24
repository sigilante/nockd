//! balance-api — an HTTP "explorer backend" NockApp service.
//!
//! Given a Nockchain pubkey, it returns that pubkey's balance read LIVE from the chain over
//! gRPC. Chain access is READ-ONLY (the public `wallet_get_balance` RPC).
//!
//!   GET /balance/<pubkey>   -> {"pubkey":"...","balance":<nicks>,"notes":<n>,"height":<h>}
//!   GET /?pubkey=<pubkey>   -> same
//!   GET /                   -> 400 (missing pubkey) with usage help
//!
//! Architecture (see RECIPE.md): this is the chain-watch shape — a Rust chain client plus a
//! trivial Hoon kernel — but with an HTTP listener instead of a poll loop. We boot the
//! `basic` template kernel from `out.jam` so the process is a valid supervised NockApp the
//! way nockd expects; ALL logic lives in this Rust file. We deliberately do NOT route the
//! chain read through the kernel (the bundled http_driver is hardcoded to :8080 with
//! crate-private helpers — see GOTCHAS.md), and instead serve HTTP directly with axum and
//! call the chain client per request.
//!
//! Endpoint resolution (nockd "endpoint-by-name", DESIGN §5.3): nockd resolves a named
//! endpoint to a URL and (a) substitutes `{endpoint}` in our args and (b) sets the
//! `NOCKD_ENDPOINT_URL` env var. We accept either, preferring an explicit `--endpoint <url>`.
//!
//! Status metric (DESIGN status-cmd): we print `metric: requests=<N>` on its own line on
//! every lookup so the deploy manifest's status command surfaces the cumulative request
//! count as the REQ column in `nockd ps`.

use std::error::Error;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use nockapp::kernel::boot;
use nockapp::noun::slab::NounSlab;
use nockapp::wire::{SystemWire, Wire};
use nockapp::NockApp;
use nockapp_grpc::services::public_nockchain::v2::client::{
    BalanceRequest, PublicNockchainGrpcClient,
};
use nockapp_grpc::pb::common::v2::note::NoteVersion;
use nockvm::noun::{D, T};
use nockvm_macros::tas;
use serde::Deserialize;
use serde_json::json;
use tracing::{error, info, warn};

const DEFAULT_HTTP_PORT: u16 = 8082;

/// Shared HTTP-handler state: the resolved chain endpoint URL and a cumulative request
/// counter (for the `metric: requests=<N>` status line).
struct AppState {
    endpoint: String,
    requests: AtomicU64,
}

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

/// Resolve the HTTP listen port: `--port <n>` / `--port=<n>` CLI arg, else `BALANCE_API_PORT`
/// env var, else the default 8082.
fn resolve_port() -> u16 {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                if let Some(p) = args.next().and_then(|s| s.parse().ok()) {
                    return p;
                }
            }
            other => {
                if let Some(p) = other.strip_prefix("--port=").and_then(|s| s.parse().ok()) {
                    return p;
                }
            }
        }
    }
    if let Ok(p) = std::env::var("BALANCE_API_PORT").and_then(|s| s.parse().map_err(|_| std::env::VarError::NotPresent)) {
        return p;
    }
    DEFAULT_HTTP_PORT
}

/// A Nockchain v0/schnorr pubkey is a base58 string (132 chars for these "cheetah point"
/// keys). We do a cheap syntactic check here to reject obvious garbage with a 400 before
/// spending a gRPC round-trip; the chain is the real authority and rejects malformed input
/// with its own error, which we also surface.
fn looks_like_pubkey(s: &str) -> bool {
    // Base58 alphabet (Bitcoin/Flickr): no 0, O, I, l.
    const B58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    !s.is_empty()
        && s.len() >= 40
        && s.len() <= 256
        && s.bytes().all(|b| B58.contains(&b))
}

/// Sum the per-note `assets` (nicks) across every UTXO in a `Balance`, handling both the
/// legacy v1 note shape and the v2 NoteV1 shape.
fn total_nicks(balance: &nockapp_grpc::pb::common::v2::Balance) -> u128 {
    balance
        .notes
        .iter()
        .filter_map(|entry| entry.note.as_ref())
        .filter_map(|note| match note.note_version.as_ref() {
            Some(NoteVersion::Legacy(n)) => n.assets.as_ref().map(|a| a.value as u128),
            Some(NoteVersion::V1(n)) => n.assets.as_ref().map(|a| a.value as u128),
            None => None,
        })
        .sum()
}

/// The core chain read, shared by both routes. Connects to the endpoint, fetches the
/// (auto-paged) balance for the pubkey, and returns (total nicks, note count, chain height).
async fn lookup_balance(endpoint: &str, pubkey: &str) -> Result<(u128, usize, u64), String> {
    let mut client = PublicNockchainGrpcClient::connect(endpoint.to_string())
        .await
        .map_err(|e| format!("connect to {endpoint} failed: {e}"))?;
    let balance = client
        .wallet_get_balance(&BalanceRequest::Address(pubkey.to_string()))
        .await
        .map_err(|e| format!("wallet_get_balance failed: {e}"))?;
    let total = total_nicks(&balance);
    let notes = balance.notes.len();
    let height = balance.height.as_ref().map(|h| h.value).unwrap_or(0);
    Ok((total, notes, height))
}

/// Bump the cumulative request counter and emit the greppable status metric line.
fn record_request(state: &AppState) {
    let n = state.requests.fetch_add(1, Ordering::Relaxed) + 1;
    // The one clean, greppable metric line nockd's status-cmd scrapes.
    println!("metric: requests={n}");
}

/// Build the JSON 200 / error responses for a resolved pubkey.
async fn serve_pubkey(state: &AppState, pubkey: &str) -> axum::response::Response {
    record_request(state);
    if !looks_like_pubkey(pubkey) {
        warn!("rejecting malformed pubkey: {pubkey:?}");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid pubkey",
                "detail": "expected a base58 Nockchain pubkey",
                "pubkey": pubkey,
            })),
        )
            .into_response();
    }

    match lookup_balance(&state.endpoint, pubkey).await {
        Ok((balance, notes, height)) => {
            info!("balance pubkey={pubkey} balance={balance} notes={notes} height={height}");
            (
                StatusCode::OK,
                Json(json!({
                    "pubkey": pubkey,
                    "balance": balance, // total nicks across all UTXOs
                    "unit": "nicks",
                    "notes": notes,
                    "height": height,
                })),
            )
                .into_response()
        }
        Err(e) => {
            error!("lookup failed for {pubkey}: {e}");
            // The chain rejects malformed addresses with its own message; surface it as 400.
            // Connection/transport problems are 502 (upstream chain unreachable).
            let status = if e.contains("connect to") {
                StatusCode::BAD_GATEWAY
            } else {
                StatusCode::BAD_REQUEST
            };
            (
                status,
                Json(json!({
                    "error": "lookup failed",
                    "detail": e,
                    "pubkey": pubkey,
                })),
            )
                .into_response()
        }
    }
}

/// `GET /balance/<pubkey>`
async fn balance_path(
    State(state): State<Arc<AppState>>,
    Path(pubkey): Path<String>,
) -> impl IntoResponse {
    serve_pubkey(&state, &pubkey).await
}

#[derive(Deserialize)]
struct RootQuery {
    pubkey: Option<String>,
}

/// `GET /?pubkey=<pubkey>` (and `GET /` with no pubkey -> 400 usage help)
async fn root(
    State(state): State<Arc<AppState>>,
    Query(q): Query<RootQuery>,
) -> axum::response::Response {
    match q.pubkey {
        Some(pubkey) => serve_pubkey(&state, &pubkey).await,
        None => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "missing pubkey",
                "usage": "GET /balance/<pubkey>  or  GET /?pubkey=<pubkey>",
            })),
        )
            .into_response(),
    }
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
        warn!("a rustls CryptoProvider was already installed");
    }

    // Boot the kernel so this is a real, supervised NockApp (consumes out.jam, sets up the
    // PMA/event-log state dir). nockd stages out.jam into the app's state dir, which is also
    // our cwd, so the relative read works both under `nockup run` and under nockd.
    let kernel = fs::read("out.jam").map_err(|e| format!("Failed to read out.jam: {}", e))?;
    let mut nockapp: NockApp = boot::setup(&kernel, cli, &[], "balance-api", None).await?;

    // Fire the kernel's demo poke once so the kernel is genuinely exercised at boot.
    let mut poke_slab = NounSlab::new();
    let command_noun = T(&mut poke_slab, &[D(tas!(b"cause")), D(0x0)]);
    poke_slab.set_root(command_noun);
    if let Err(e) = nockapp.poke(SystemWire.to_wire(), poke_slab).await {
        warn!("kernel boot poke failed (non-fatal): {e:?}");
    }
    // Keep the kernel handle alive for the lifetime of the process. We do not call
    // `nockapp.run()`: this app's work is the Rust HTTP server below, not Hoon-side drivers.
    let _nockapp = nockapp;

    let endpoint = resolve_endpoint();
    let port = resolve_port();
    let state = Arc::new(AppState {
        endpoint: endpoint.clone(),
        requests: AtomicU64::new(0),
    });

    info!("balance-api starting; chain endpoint={endpoint}; http port={port}");
    // Greppable boot markers so logs always show the resolved config.
    println!("metric: endpoint={endpoint}");
    println!("metric: requests=0");

    let app = Router::new()
        .route("/", get(root))
        .route("/balance/:pubkey", get(balance_path))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("failed to bind {addr}: {e}"))?;
    info!("listening on http://{addr}");

    // Graceful shutdown: nockd SIGTERMs on stop/restart. Shut the server down cleanly on
    // SIGTERM (and Ctrl-C when run by hand).
    let shutdown = async {
        let mut sigterm = match tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        ) {
            Ok(s) => s,
            Err(e) => {
                error!("failed to install SIGTERM handler: {e}");
                // Fall back to just ctrl_c.
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            _ = sigterm.recv() => info!("received SIGTERM; shutting down cleanly"),
            _ = tokio::signal::ctrl_c() => info!("received Ctrl-C; shutting down cleanly"),
        }
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|e| format!("http server error: {e}"))?;

    Ok(())
}
