//! Versioned `/api/v1` surface for the browser dashboard (design/API-INTEGRATION.md).
//!
//! Enriches the App shape with what nockd can truly compute today (status mapping, uptime,
//! restarts, health, hashes) and serves live logs + events over SSE. Fields the backend
//! doesn't have yet — chain-attach lag, resources, verification, prev artifact, secrets —
//! are returned as null/empty rather than faked; their screens land with their features.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use crate::config::now_secs;
use crate::daemon::Daemon;
use crate::registry::{AppRow, EventRow};
use crate::supervisor::{RunState, RuntimeStatus};

/// The dashboard's App shape (design/API-INTEGRATION.md §2). Unknown fields are `null`.
#[derive(Debug, Serialize)]
pub struct AppV1 {
    pub name: String,
    pub status: String,         // running | degraded | stopped | crashing
    pub desired_status: String, // running | stopped
    pub artifact_hash: String,
    pub kernel_hash: Option<String>,
    pub prev_artifact: Option<String>,
    pub endpoint_name: Option<String>,
    pub restart_policy: String,
    pub restart_count: u32,
    pub uptime_s: Option<i64>,
    pub state_size_bytes: Option<u64>, // not yet sampled
    pub template: Option<String>,      // not yet recorded
    pub health: String,                // serving | notserving | unreachable | unknown
    pub chain_attach: Option<String>,  // not yet probed
    pub verified: String,              // unverified (no attestation yet)
    pub status_label: Option<String>,  // e.g. "BLOCK"
    pub status_line: Option<String>,   // e.g. "height 184302" — from the status command
    pub link: Option<String>,          // the app's own web page (relay link), if it serves one
    pub pid: Option<u32>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Map (desired, run-state, health) onto the dashboard's four-state status grammar.
fn derive_status(row: &AppRow, rt: Option<&RuntimeStatus>) -> String {
    use crate::health::HealthState;
    match rt.map(|r| r.state) {
        Some(RunState::Crashed) | Some(RunState::Backoff) => "crashing",
        Some(RunState::Stopped) | Some(RunState::Stopping) | None
            if row.desired_status == "stopped" =>
        {
            "stopped"
        }
        Some(RunState::Running) => match rt.map(|r| r.health) {
            Some(HealthState::NotServing) | Some(HealthState::Unreachable) => "degraded",
            _ => "running",
        },
        _ if row.desired_status == "stopped" => "stopped",
        _ => "running",
    }
    .to_string()
}

fn to_app_v1(row: AppRow, rt: Option<RuntimeStatus>) -> AppV1 {
    let status = derive_status(&row, rt.as_ref());
    let uptime_s = rt.as_ref().and_then(|r| {
        if r.state == RunState::Running && r.started_at > 0 {
            Some(now_secs() - r.started_at)
        } else {
            None
        }
    });
    let health = rt
        .as_ref()
        .map(|r| format!("{:?}", r.health).to_lowercase())
        .unwrap_or_else(|| "unknown".into());
    AppV1 {
        name: row.name,
        status,
        desired_status: row.desired_status,
        artifact_hash: row.artifact_hash,
        kernel_hash: (!row.kernel_hash.is_empty()).then_some(row.kernel_hash),
        prev_artifact: None,
        endpoint_name: row.endpoint,
        restart_policy: row.restart_policy,
        restart_count: rt.as_ref().map(|r| r.restarts).unwrap_or(0),
        uptime_s,
        state_size_bytes: None,
        template: None,
        health,
        chain_attach: None,
        verified: row.verified_status.unwrap_or_else(|| "unverified".into()),
        status_label: row.status_label,
        status_line: rt.as_ref().and_then(|r| r.status_line.clone()),
        link: row.link,
        pid: rt.as_ref().and_then(|r| r.pid),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

pub async fn list_apps(State(d): State<Arc<Daemon>>) -> impl IntoResponse {
    let rows = match d.registry.list_apps() {
        Ok(r) => r,
        Err(e) => return crate::api::ApiError::from(e).into_response(),
    };
    let apps: Vec<AppV1> = rows
        .into_iter()
        .map(|row| {
            let rt = d.supervisor.status(&row.name);
            to_app_v1(row, rt)
        })
        .collect();
    Json(apps).into_response()
}

pub async fn get_app(State(d): State<Arc<Daemon>>, Path(name): Path<String>) -> impl IntoResponse {
    match d.registry.get_app(&name) {
        Ok(Some(row)) => {
            let rt = d.supervisor.status(&row.name);
            Json(to_app_v1(row, rt)).into_response()
        }
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, format!("no such app: {name}"))
            .into_response(),
        Err(e) => crate::api::ApiError::from(e).into_response(),
    }
}

pub async fn app_events(
    State(d): State<Arc<Daemon>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match d.registry.list_app_events(&name, 100) {
        Ok(events) => Json(events).into_response(),
        Err(e) => crate::api::ApiError::from(e).into_response(),
    }
}

/// SSE: live log tail for one app — emits the recent tail, then follows appends.
pub async fn logs_sse(State(d): State<Arc<Daemon>>, Path(name): Path<String>) -> impl IntoResponse {
    let path = d.paths.log_file(&name);
    let stream = async_stream::stream! {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};

        // Seed with the last ~200 lines from a bounded tail (NOT the whole file — nockchain's
        // log can be GBs), then follow appends from the current end of file.
        let mut offset: u64 = crate::config::file_len(&path).await;
        let seed = crate::config::read_tail(&path, 128 * 1024).await;
        let lines: Vec<&str> = seed.lines().collect();
        let start = lines.len().saturating_sub(200);
        for line in &lines[start..] {
            // Strip NUL bytes (kernel-boot logs contain them); keep ANSI so the panel colors.
            yield Ok::<Event, Infallible>(Event::default().data(line.replace('\0', "")));
        }

        let mut tick = tokio::time::interval(Duration::from_millis(700));
        loop {
            tick.tick().await;
            let Ok(mut f) = tokio::fs::File::open(&path).await else { continue };
            let len = f.metadata().await.map(|m| m.len()).unwrap_or(offset);
            if len < offset {
                offset = 0; // rotated / truncated
            }
            if len > offset {
                if f.seek(std::io::SeekFrom::Start(offset)).await.is_ok() {
                    let mut buf = String::new();
                    if f.read_to_string(&mut buf).await.is_ok() {
                        for line in buf.lines() {
                            yield Ok::<Event, Infallible>(Event::default().data(line.replace('\0', "")));
                        }
                        offset = len;
                    }
                }
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// SSE: fleet-wide event stream — emits each new audit/event row as it lands.
pub async fn events_sse(State(d): State<Arc<Daemon>>) -> impl IntoResponse {
    let stream = async_stream::stream! {
        // Start from the current max id so we only stream genuinely new events.
        let mut last_id = d
            .registry
            .list_events(1)
            .ok()
            .and_then(|v| v.first().map(|e| e.id))
            .unwrap_or(0);

        let mut tick = tokio::time::interval(Duration::from_millis(800));
        loop {
            tick.tick().await;
            if let Ok(events) = d.registry.events_since(last_id, 100) {
                for ev in events {
                    last_id = ev.id;
                    if let Ok(json) = serde_json::to_string(&EventJson::from(ev)) {
                        yield Ok::<Event, Infallible>(Event::default().data(json));
                    }
                }
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ---- Endpoints (named Nockchain RPC targets) ----

#[derive(Serialize)]
pub struct EndpointV1 {
    pub name: String,
    pub url: String,
    pub kind: String,
    pub reachable: bool,
    pub lag_ms: Option<u64>,
    /// Chain-tip block height (Nockchain v2 endpoints).
    pub height: Option<u64>,
    /// Blocks behind the most-current reachable endpoint (0 = leading).
    pub behind: Option<u64>,
    pub attached_apps: Vec<String>,
}

#[derive(serde::Deserialize)]
pub struct NewEndpoint {
    pub name: String,
    pub url: String,
    #[serde(default = "default_kind")]
    pub kind: String,
}

fn default_kind() -> String {
    "remote".to_string()
}

pub async fn list_endpoints(State(d): State<Arc<Daemon>>) -> impl IntoResponse {
    let endpoints = match d.registry.list_endpoints() {
        Ok(e) => e,
        Err(e) => return crate::api::ApiError::from(e).into_response(),
    };
    let apps = d.registry.list_apps().unwrap_or_default();
    let mut out = Vec::with_capacity(endpoints.len());
    for ep in endpoints {
        let probe = if ep.kind == "remote" {
            crate::health::probe_endpoint(&ep.url).await
        } else {
            crate::health::EndpointProbe {
                reachable: std::path::Path::new(&ep.url).exists(),
                lag_ms: None,
                height: None,
            }
        };
        let attached_apps = apps
            .iter()
            .filter(|a| a.endpoint.as_deref() == Some(ep.name.as_str()))
            .map(|a| a.name.clone())
            .collect();
        out.push(EndpointV1 {
            name: ep.name,
            url: ep.url,
            kind: ep.kind,
            reachable: probe.reachable,
            lag_ms: probe.lag_ms,
            height: probe.height,
            behind: None, // filled below once we know the max height
            attached_apps,
        });
    }
    // "Behind" is relative to the most-current reachable endpoint we can see.
    if let Some(tip) = out.iter().filter_map(|e| e.height).max() {
        for e in &mut out {
            if let Some(h) = e.height {
                e.behind = Some(tip.saturating_sub(h));
            }
        }
    }
    Json(out).into_response()
}

pub async fn add_endpoint(
    State(d): State<Arc<Daemon>>,
    Json(req): Json<NewEndpoint>,
) -> impl IntoResponse {
    match d.registry.add_endpoint(&req.name, &req.url, &req.kind) {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => crate::api::ApiError::from(e).into_response(),
    }
}

pub async fn remove_endpoint(
    State(d): State<Arc<Daemon>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match d.registry.remove_endpoint(&name) {
        Ok(_) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => crate::api::ApiError::from(e).into_response(),
    }
}

#[derive(Serialize)]
struct EventJson {
    id: i64,
    ts: i64,
    app_name: String,
    kind: String,
    detail: String,
}

impl From<EventRow> for EventJson {
    fn from(e: EventRow) -> Self {
        EventJson {
            id: e.id,
            ts: e.ts,
            app_name: e.app_name,
            kind: e.kind,
            detail: e.detail,
        }
    }
}
