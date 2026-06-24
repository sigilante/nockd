//! HTTP control API + dashboard routes (DESIGN §5.1, §9). CLI and dashboard are both
//! clients of this one API.

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::daemon::Daemon;
use crate::registry::EventRow;
use crate::supervisor::RuntimeStatus;

/// Deploy request body (client → daemon). Binary + kernel are base64 (Phase 0; streaming
/// upload is a later refinement).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployRequest {
    pub name: String,
    pub target_triple: String,
    pub bin_b64: String,
    /// Base64 kernel. Absent for binary-only artifacts that embed their kernel.
    #[serde(default)]
    pub jam_b64: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default = "default_restart")]
    pub restart: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// App's private/admin gRPC address for health probing (DESIGN §5.3).
    #[serde(default)]
    pub admin_addr: Option<String>,
    /// Optional custom status command + label (e.g. block height).
    #[serde(default)]
    pub status_cmd: Option<String>,
    #[serde(default)]
    pub status_label: Option<String>,
    /// The port an HTTP NockApp serves on. Exported as `NOCKD_PORT` (and `{port}` in args);
    /// the dashboard derives a `localhost:<port>` relay link from it.
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub provenance: Option<String>,
    /// Optional build attestation (JSON) — a signed statement binding these hashes.
    #[serde(default)]
    pub attestation: Option<String>,
}

fn default_restart() -> String {
    "on-failure".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployResponse {
    pub name: String,
    pub artifact_hash: String,
    pub kernel_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStatus {
    pub name: String,
    pub desired_status: String,
    pub artifact_hash: String,
    pub kernel_hash: String,
    pub endpoint: Option<String>,
    pub restart_policy: String,
    pub status_label: Option<String>,
    pub verified: String,
    pub runtime: Option<RuntimeStatus>,
}

pub fn router(daemon: Arc<Daemon>) -> Router {
    use crate::apiv1;
    Router::new()
        .route("/api/health", get(|| async { "ok" }))
        // Legacy surface used by the CLI + TUI.
        .route("/api/apps", get(list_apps).post(deploy))
        .route("/api/apps/:name/restart", post(restart))
        .route("/api/apps/:name/stop", post(stop))
        .route("/api/apps/:name/start", post(start))
        .route("/api/apps/:name/logs", get(logs))
        .route("/api/events", get(events))
        // Versioned surface for the browser dashboard (design/API-INTEGRATION.md).
        .route("/api/v1/apps", get(apiv1::list_apps))
        .route("/api/v1/apps/:name", get(apiv1::get_app))
        .route("/api/v1/apps/:name/events", get(apiv1::app_events))
        .route("/api/v1/apps/:name/logs", get(apiv1::logs_sse)) // SSE (follow)
        .route("/api/v1/apps/:name/restart", post(restart))
        .route("/api/v1/apps/:name/stop", post(stop))
        .route("/api/v1/apps/:name/start", post(start))
        .route("/api/v1/events", get(apiv1::events_sse)) // SSE (follow)
        .route("/api/v1/down", post(down))
        .route("/api/v1/up", post(up))
        .route("/api/v1/trust", get(trust_list).post(trust_add))
        .route("/api/v1/trust/:pubkey", axum::routing::delete(trust_remove))
        .route(
            "/api/v1/endpoints",
            get(apiv1::list_endpoints).post(apiv1::add_endpoint),
        )
        .route("/api/v1/endpoints/:name", axum::routing::delete(apiv1::remove_endpoint))
        // Embedded dashboard assets + SPA fallback (DESIGN §9.2).
        .fallback(crate::dashboard::static_handler)
        // Artifact uploads (binary + kernel, base64) blow past axum's 2 MB default. Allow
        // large bodies for now; streaming/multipart upload is a later refinement (DESIGN
        // §9 API). 1 GiB is generous for a NockApp binary (e.g. nockchain).
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024))
        .with_state(daemon)
}

async fn list_apps(State(d): State<Arc<Daemon>>) -> Result<Json<Vec<AppStatus>>, ApiError> {
    let rows = d.registry.list_apps()?;
    let statuses = rows
        .into_iter()
        .map(|a| {
            let runtime = d.supervisor.status(&a.name);
            AppStatus {
                runtime,
                name: a.name,
                desired_status: a.desired_status,
                artifact_hash: a.artifact_hash,
                kernel_hash: a.kernel_hash,
                endpoint: a.endpoint,
                restart_policy: a.restart_policy,
                status_label: a.status_label,
                verified: a.verified_status.clone().unwrap_or_else(|| "unverified".into()),
            }
        })
        .collect();
    Ok(Json(statuses))
}

async fn deploy(
    State(d): State<Arc<Daemon>>,
    Json(req): Json<DeployRequest>,
) -> Result<Json<DeployResponse>, ApiError> {
    let engine = base64::engine::general_purpose::STANDARD;
    let bin = engine.decode(&req.bin_b64).map_err(|e| anyhow::anyhow!("bad bin_b64: {e}"))?;
    let jam = match &req.jam_b64 {
        Some(j) => Some(engine.decode(j).map_err(|e| anyhow::anyhow!("bad jam_b64: {e}"))?),
        None => None,
    };

    let rec = d.store.put(jam.as_deref(), &bin, &req.target_triple)?;
    d.registry.put_artifact(&rec, req.provenance.as_deref())?;

    // Verify the attestation (if any) and record the artifact's verification status.
    let (verified, builder) = match &req.attestation {
        Some(json) => match serde_json::from_str::<crate::attest::Attestation>(json) {
            Ok(att) => crate::attest::assess(&att, &rec.artifact_hash, &rec.kernel_hash, |b| {
                d.registry.is_trusted_builder(b)
            }),
            Err(_) => ("drift".to_string(), None), // malformed attestation
        },
        None => ("unverified".to_string(), None),
    };
    tracing::info!(
        has_attestation = req.attestation.is_some(),
        artifact = %&rec.artifact_hash[..16.min(rec.artifact_hash.len())],
        verified = %verified,
        builder = builder.as_deref().unwrap_or("-"),
        "deploy verification"
    );
    d.registry
        .set_artifact_verification(&rec.artifact_hash, &verified, builder.as_deref())?;

    let state_path = d.paths.state_dir(&req.name);
    d.registry.upsert_app(
        &req.name,
        &rec.artifact_hash,
        req.endpoint.as_deref(),
        &req.restart,
        &req.args,
        &state_path.to_string_lossy(),
        req.admin_addr.as_deref(),
        req.status_cmd.as_deref(),
        req.status_label.as_deref(),
        req.port,
    )?;
    d.registry.add_event(
        &req.name,
        "deploy",
        &format!("artifact {}", &rec.artifact_hash[..16.min(rec.artifact_hash.len())]),
    )?;

    // Start it now rather than waiting for the next tick.
    d.reconcile();

    Ok(Json(DeployResponse {
        name: req.name,
        artifact_hash: rec.artifact_hash,
        kernel_hash: rec.kernel_hash,
    }))
}

async fn restart(
    State(d): State<Arc<Daemon>>,
    Path(name): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    if d.registry.get_app(&name)?.is_none() {
        return Err(ApiError::not_found(&name));
    }
    d.registry.set_desired(&name, "running")?;
    d.supervisor.request_restart(&name);
    d.registry.add_event(&name, "restart", "restart requested")?;
    d.reconcile();
    Ok(Json(OkResponse { ok: true }))
}

async fn stop(State(d): State<Arc<Daemon>>, Path(name): Path<String>) -> Result<Json<OkResponse>, ApiError> {
    if d.registry.get_app(&name)?.is_none() {
        return Err(ApiError::not_found(&name));
    }
    d.registry.set_desired(&name, "stopped")?;
    d.reconcile();
    Ok(Json(OkResponse { ok: true }))
}

async fn start(State(d): State<Arc<Daemon>>, Path(name): Path<String>) -> Result<Json<OkResponse>, ApiError> {
    if d.registry.get_app(&name)?.is_none() {
        return Err(ApiError::not_found(&name));
    }
    d.registry.set_desired(&name, "running")?;
    d.reconcile();
    Ok(Json(OkResponse { ok: true }))
}

#[derive(Debug, Deserialize)]
struct LogsQuery {
    #[serde(default = "default_lines")]
    lines: usize,
}

fn default_lines() -> usize {
    200
}

async fn logs(
    State(d): State<Arc<Daemon>>,
    Path(name): Path<String>,
    Query(q): Query<LogsQuery>,
) -> Result<String, ApiError> {
    // Bounded tail read so a multi-GB nockchain log doesn't get slurped into memory.
    // Size the window to the requested line count (long nockchain lines → ~1 KB each), capped.
    let window = (q.lines as u64 * 1024).clamp(256 * 1024, 8 * 1024 * 1024);
    let text = crate::config::read_tail(&d.paths.log_file(&name), window).await;
    let tail: Vec<&str> = text.lines().rev().take(q.lines).collect();
    Ok(tail.into_iter().rev().collect::<Vec<_>>().join("\n"))
}

/// Set every app's desired status (used by `nockd down` / `nockd up`), then reconcile.
async fn fleet_set(d: &Arc<Daemon>, status: &str) -> Result<Json<serde_json::Value>, ApiError> {
    let apps = d.registry.list_apps()?;
    let mut changed = 0u32;
    for a in &apps {
        if a.desired_status != status {
            d.registry.set_desired(&a.name, status)?;
            changed += 1;
        }
    }
    d.reconcile();
    Ok(Json(serde_json::json!({ "changed": changed, "total": apps.len() })))
}

async fn down(State(d): State<Arc<Daemon>>) -> Result<Json<serde_json::Value>, ApiError> {
    fleet_set(&d, "stopped").await
}

async fn up(State(d): State<Arc<Daemon>>) -> Result<Json<serde_json::Value>, ApiError> {
    fleet_set(&d, "running").await
}

#[derive(Deserialize)]
struct TrustReq {
    pubkey: String,
}

async fn trust_list(State(d): State<Arc<Daemon>>) -> Result<Json<Vec<String>>, ApiError> {
    Ok(Json(d.registry.list_trusted_builders()?))
}

async fn trust_add(
    State(d): State<Arc<Daemon>>,
    Json(req): Json<TrustReq>,
) -> Result<Json<OkResponse>, ApiError> {
    d.registry.add_trusted_builder(&req.pubkey)?;
    Ok(Json(OkResponse { ok: true }))
}

async fn trust_remove(
    State(d): State<Arc<Daemon>>,
    Path(pubkey): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    d.registry.remove_trusted_builder(&pubkey)?;
    Ok(Json(OkResponse { ok: true }))
}

async fn events(State(d): State<Arc<Daemon>>) -> Result<Json<Vec<EventRow>>, ApiError> {
    Ok(Json(d.registry.list_events(200)?))
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct OkResponse {
    pub ok: bool,
}

/// Maps anyhow errors to HTTP responses.
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn not_found(name: &str) -> Self {
        ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("no such app: {name}"),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, self.message).into_response()
    }
}
