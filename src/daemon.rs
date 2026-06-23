//! The daemon: owns the registry, artifact store, and supervisor, and runs the reconcile
//! loop plus the HTTP control API + dashboard (DESIGN §5).

use std::net::IpAddr;
use std::os::unix::io::AsRawFd;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::net::TcpListener;
use tracing::info;

use crate::config::Paths;
use crate::registry::Registry;
use crate::store::Store;
use crate::supervisor::Supervisor;

pub struct Daemon {
    pub paths: Paths,
    pub registry: Registry,
    pub store: Store,
    pub supervisor: Supervisor,
    /// Held for the daemon's lifetime; its flock guarantees a single daemon per root (OQ7).
    _lock: std::fs::File,
}

impl Daemon {
    pub fn new(paths: Paths) -> Result<Arc<Self>> {
        let lock = acquire_single_daemon_lock(&paths.root.join("nockd.lock"))?;
        let registry = Registry::open(&paths.db)?;
        let store = Store::new(paths.artifacts.clone());
        let supervisor = Supervisor::new(paths.clone());
        Ok(Arc::new(Daemon {
            paths,
            registry,
            store,
            supervisor,
            _lock: lock,
        }))
    }

    /// Reconcile once (used after API mutations for snappy response).
    pub fn reconcile(&self) {
        if let Err(e) = self.supervisor.reconcile(&self.registry, &self.store) {
            tracing::warn!(error = %e, "reconcile failed");
        }
    }
}

/// Take an exclusive, non-blocking flock so only one daemon runs per data root. A second
/// daemon would fight the first over the same registry/state and could SIGTERM its apps.
fn acquire_single_daemon_lock(path: &std::path::Path) -> Result<std::fs::File> {
    let f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("opening lock {}", path.display()))?;
    // Safety: flock on a valid fd. LOCK_NB so we fail fast instead of blocking.
    let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        bail!(
            "another nockd is already running for this data root (lock held: {}). \
             Stop it first, or use a different --root.",
            path.display()
        );
    }
    Ok(f)
}

/// Read up to the last `max_bytes` of a file as lossy UTF-8 (cheap tail for large logs).
async fn read_tail(path: &std::path::Path, max_bytes: u64) -> String {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let Ok(mut f) = tokio::fs::File::open(path).await else {
        return String::new();
    };
    let len = f.metadata().await.map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    if start > 0 && f.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf).await;
    String::from_utf8_lossy(&buf).into_owned()
}

/// Run an app's status command (`sh -c`) with cwd=state dir and the **ANSI-stripped recent
/// log piped to stdin**, returning its first stdout line (trimmed, capped). The piped stdin
/// means a recipe is just a grep — no perl/`$NOCKD_LOG`/ANSI handling needed. Times out so a
/// hung command can't stall the loop.
async fn run_status_cmd(
    daemon: &Daemon,
    app: &crate::registry::AppRow,
    cmd: &str,
) -> Option<String> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;

    let state_dir = daemon.paths.state_dir(&app.name);
    let log = daemon.paths.log_file(&app.name);
    let plain = crate::config::strip_ansi(&read_tail(&log, 256 * 1024).await);

    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(cmd)
        .current_dir(&state_dir)
        .env("NOCKD_APP", &app.name)
        .env("NOCKD_STATE_DIR", &state_dir)
        .env("NOCKD_LOG", &log)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(ep) = &app.endpoint {
        command.env("NOCKD_ENDPOINT", ep);
    }
    if let Some(addr) = &app.admin_addr {
        command.env("NOCKD_ADMIN_ADDR", addr);
    }

    let fut = async {
        let mut child = command.spawn().ok()?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(plain.as_bytes()).await;
            // Dropping stdin closes the pipe so the command sees EOF.
        }
        child.wait_with_output().await.ok()
    };
    let Some(output) = tokio::time::timeout(Duration::from_secs(5), fut).await.ok().flatten()
    else {
        tracing::debug!(app = %app.name, "status cmd timed out or failed to spawn");
        return None;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    tracing::debug!(
        app = %app.name,
        exit = ?output.status.code(),
        log_bytes = plain.len(),
        out_bytes = output.stdout.len(),
        "status cmd ran"
    );
    if !output.status.success() {
        return None;
    }
    let line = text.lines().find(|l| !l.trim().is_empty())?.trim();
    let capped: String = line.chars().take(80).collect();
    (!capped.is_empty()).then_some(capped)
}

pub async fn serve(daemon: Arc<Daemon>, host: IpAddr, port: u16) -> Result<()> {
    // Re-adopt any apps that survived a previous daemon (process-group isolated), so we
    // don't spawn conflicting duplicates (OQ6).
    daemon.supervisor.reattach(&daemon.registry);

    // Background reconcile loop (DESIGN §5.1).
    let bg = daemon.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            bg.reconcile();
        }
    });

    // Background health-probe loop (DESIGN §5.3/§8.2). Probes the private gRPC of any app
    // with an admin address; apps without one keep process-liveness only.
    let hp = daemon.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        loop {
            tick.tick().await;
            let apps = match hp.registry.list_apps() {
                Ok(a) => a,
                Err(_) => continue,
            };
            for app in apps {
                if let Some(addr) = app.admin_addr.clone() {
                    let state = crate::health::probe(&addr).await;
                    hp.supervisor.set_health(&app.name, state);
                }
            }
        }
    });

    // Background custom-status loop (e.g. block height): run each running app's configured
    // status command with cwd=state dir and surface the first stdout line.
    let sp = daemon.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        // Last value we logged per app, so we log the FIRST probe result (even None) and
        // then only on change — distinguishing "configured but not matching" from "unset".
        let mut last: std::collections::HashMap<String, Option<String>> =
            std::collections::HashMap::new();
        loop {
            tick.tick().await;
            let apps = match sp.registry.list_apps() {
                Ok(a) => a,
                Err(_) => continue,
            };
            for app in apps {
                let Some(cmd) = app.status_cmd.clone() else { continue };
                if !sp.supervisor.is_running(&app.name) {
                    tracing::debug!(app = %app.name, "status probe skipped (not running)");
                    continue;
                }
                let line = run_status_cmd(&sp, &app, &cmd).await;
                if last.get(&app.name) != Some(&line) {
                    match &line {
                        Some(v) => tracing::info!(app = %app.name, metric = %v, "status metric updated"),
                        None => tracing::info!(app = %app.name, "status command produced NO value — check the recipe matches the log (`nockd logs {} | grep ...`)", app.name),
                    }
                    last.insert(app.name.clone(), line.clone());
                }
                sp.supervisor.set_status_line(&app.name, line);
            }
        }
    });

    let app = crate::api::router(daemon.clone());
    let listener = TcpListener::bind((host, port))
        .await
        .with_context(|| format!("binding {host}:{port}"))?;
    info!("nockd listening on http://{host}:{port}  (dashboard at /)");
    axum::serve(listener, app).await.context("http server")?;
    Ok(())
}
