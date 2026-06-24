//! Process supervision + reconciliation (DESIGN §5.1, §8.2, principle 8).
//!
//! A single reconcile loop drives observed state toward desired state: it starts missing
//! instances, restarts crashed ones with exponential backoff, and stops removed ones.
//! The supervisor owns the only handles to running children, which (with the per-app map
//! key) gives us the single-writer guarantee of principle 8 for the common path.
//!
//! Phase 0 health = process liveness. The gRPC readiness gate (DESIGN §5.3/OQ3) is a
//! Phase 1 addition.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::process::{Child, Command};
use tracing::{info, warn};

use crate::config::{now_secs, Paths};
use crate::health::HealthState;
use crate::registry::{AppRow, Registry};
use crate::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Running,
    Stopping,
    Stopped,
    Crashed,
    Backoff,
}

struct Managed {
    /// This daemon owns every supervised process for its lifetime — apps do not survive a
    /// daemon restart (clean model: graceful stop on shutdown, fresh spawn on start).
    child: Option<Child>,
    pid: Option<u32>,
    started_at: i64,
    restarts: u32,
    backoff_until: i64,
    state: RunState,
    health: HealthState,
    /// Latest custom status line from the app's configured status command (e.g. block height).
    status_line: Option<String>,
    /// When set, we have sent SIGTERM and are awaiting graceful exit; past the deadline we
    /// escalate to SIGKILL. Distinguishes intentional termination from a crash in the reap.
    term_deadline: Option<i64>,
    /// Set when an operator asked for a restart, so the reap doesn't treat the resulting
    /// exit as a crash (no backoff, no restart-count bump).
    restart_requested: bool,
}

impl Default for Managed {
    fn default() -> Self {
        Managed {
            child: None,
            pid: None,
            started_at: 0,
            restarts: 0,
            backoff_until: 0,
            state: RunState::Stopped,
            health: HealthState::Unknown,
            status_line: None,
            term_deadline: None,
            restart_requested: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub state: RunState,
    pub pid: Option<u32>,
    pub started_at: i64,
    pub restarts: u32,
    pub health: HealthState,
    pub status_line: Option<String>,
}

pub struct Supervisor {
    paths: Paths,
    procs: Mutex<HashMap<String, Managed>>,
    shutting_down: std::sync::atomic::AtomicBool,
}

const MAX_BACKOFF_SECS: i64 = 60;
/// Grace period between SIGTERM and SIGKILL on stop/restart (DESIGN §5.1: let the app reach
/// a clean snapshot before kill). Stateful nodes recover via replay even on SIGKILL, but a
/// clean shutdown is preferable.
const GRACE_SECS: i64 = 10;

impl Supervisor {
    pub fn new(paths: Paths) -> Self {
        Supervisor {
            paths,
            procs: Mutex::new(HashMap::new()),
            shutting_down: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn status(&self, name: &str) -> Option<RuntimeStatus> {
        let procs = self.procs.lock().unwrap();
        procs.get(name).map(|m| RuntimeStatus {
            state: m.state,
            pid: m.pid,
            started_at: m.started_at,
            restarts: m.restarts,
            health: m.health,
            status_line: m.status_line.clone(),
        })
    }

    /// Record the latest health probe result (called by the daemon's probe loop).
    pub fn set_health(&self, name: &str, health: HealthState) {
        let mut procs = self.procs.lock().unwrap();
        if let Some(m) = procs.get_mut(name) {
            m.health = health;
        }
    }

    /// Record the latest custom status line (called by the daemon's status-probe loop).
    pub fn set_status_line(&self, name: &str, line: Option<String>) {
        let mut procs = self.procs.lock().unwrap();
        if let Some(m) = procs.get_mut(name) {
            m.status_line = line;
        }
    }

    /// Whether an app is currently running (used to gate the status probe).
    pub fn is_running(&self, name: &str) -> bool {
        let procs = self.procs.lock().unwrap();
        procs.get(name).map(|m| m.state == RunState::Running).unwrap_or(false)
    }

    /// Request a graceful restart: SIGTERM now, escalate to SIGKILL after the grace period,
    /// then start fresh (no crash penalty).
    pub fn request_restart(&self, name: &str) {
        let mut procs = self.procs.lock().unwrap();
        if let Some(m) = procs.get_mut(name) {
            if let Some(pid) = m.pid {
                m.restart_requested = true;
                m.term_deadline = Some(now_secs() + GRACE_SECS);
                info!(app = %name, pid, "restart requested → sending SIGTERM");
                send_term(pid);
            }
            m.backoff_until = 0;
        }
    }

    /// Reconcile observed state to desired state. Called on a timer and after mutations.
    pub fn reconcile(&self, registry: &Registry, store: &Store) -> Result<()> {
        // During shutdown we are tearing apps down; don't fight it by respawning.
        if self.shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }

        let apps = registry.list_apps()?;
        let desired_names: std::collections::HashSet<&str> =
            apps.iter().map(|a| a.name.as_str()).collect();

        let mut procs = self.procs.lock().unwrap();

        // Reap exited children, escalate stalled terminations, and update state.
        for (name, m) in procs.iter_mut() {
            let exited: Option<i32> = match m.child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(status)) => Some(status.code().unwrap_or(-1)),
                    Ok(None) => None,
                    Err(e) => {
                        warn!(app = %name, error = %e, "try_wait failed");
                        None
                    }
                },
                None => None,
            };

            if let Some(code) = exited {
                m.child = None;
                m.pid = None;
                let _ = std::fs::remove_file(self.paths.pid_file(name));
                if m.restart_requested {
                    // Operator-initiated restart: no penalty, restart next tick.
                    m.restart_requested = false;
                    m.term_deadline = None;
                    m.backoff_until = 0;
                    m.state = RunState::Backoff;
                } else if m.term_deadline.is_some() {
                    // Intentional stop (SIGTERM): clean, no crash/backoff.
                    m.term_deadline = None;
                    m.state = RunState::Stopped;
                    m.health = HealthState::Unknown;
                    m.status_line = None;
                    let _ = registry.add_event(name, "stop", "stopped");
                } else {
                    m.restarts += 1;
                    m.backoff_until = now_secs() + backoff_secs(m.restarts);
                    m.state = RunState::Backoff;
                    warn!(app = %name, code, "instance exited");
                    let _ = registry.add_event(name, "crash", &format!("exit code {code}"));
                }
                continue;
            }

            // Still alive: if a graceful termination has timed out, escalate to SIGKILL.
            if m.term_deadline.is_some_and(|d| now_secs() >= d) {
                if let Some(pid) = m.pid {
                    warn!(app = %name, "graceful stop timed out; sending SIGKILL");
                    send_kill(pid);
                }
            }
        }

        // Drop tracking for apps removed from the registry (kill if still alive).
        let tracked: Vec<String> = procs.keys().cloned().collect();
        for name in tracked {
            if !desired_names.contains(name.as_str()) {
                if let Some(m) = procs.remove(&name) {
                    if let Some(pid) = m.pid {
                        info!(app = %name, pid, "app removed from registry → SIGKILL");
                        send_kill(pid);
                    }
                    let _ = std::fs::remove_file(self.paths.pid_file(&name));
                }
            }
        }

        // Drive each desired app toward its target.
        for app in &apps {
            let entry = procs.entry(app.name.clone()).or_default();

            let running = entry.child.is_some();

            if app.desired_status == "stopped" {
                if running {
                    // Begin a graceful stop once; reap/escalation drives it to completion.
                    if entry.term_deadline.is_none() {
                        if let Some(pid) = entry.pid {
                            info!(app = %app.name, pid, "desired=stopped → sending SIGTERM");
                            send_term(pid);
                        }
                        entry.term_deadline = Some(now_secs() + GRACE_SECS);
                        entry.state = RunState::Stopping;
                        let _ = registry.add_event(&app.name, "stop", "SIGTERM sent");
                    }
                } else {
                    entry.state = RunState::Stopped;
                    entry.term_deadline = None;
                    entry.health = HealthState::Unknown;
                    entry.status_line = None;
                }
                continue;
            }

            // desired running
            if running {
                // A pending termination (e.g. a graceful restart) shows as Stopping.
                entry.state = if entry.term_deadline.is_some() {
                    RunState::Stopping
                } else {
                    RunState::Running
                };
                continue;
            }
            if app.restart_policy == "never" && entry.restarts > 0 {
                entry.state = RunState::Crashed;
                continue;
            }
            if now_secs() < entry.backoff_until {
                entry.state = RunState::Backoff;
                continue;
            }

            // Resolve the app's named endpoint to its URL (registry), so apps reference an
            // endpoint by name and the URL can change without redeploying.
            let endpoint_url = app
                .endpoint
                .as_deref()
                .and_then(|name| registry.get_endpoint(name).ok().flatten())
                .map(|e| e.url);

            match self.spawn(app, endpoint_url.as_deref(), store) {
                Ok(child) => {
                    let pid = child.id();
                    entry.pid = pid;
                    entry.child = Some(child);
                    entry.started_at = now_secs();
                    entry.state = RunState::Running;
                    entry.health = HealthState::Unknown;
                    entry.term_deadline = None;
                    entry.restart_requested = false;
                    // Record the pid for re-adoption if this daemon restarts.
                    if let Some(pid) = pid {
                        let _ = std::fs::write(
                            self.paths.pid_file(&app.name),
                            format!("{pid} {}", entry.started_at),
                        );
                    }
                    info!(app = %app.name, pid = ?entry.pid, "instance started");
                    let _ = registry.add_event(&app.name, "start", "instance started");
                }
                Err(e) => {
                    entry.restarts += 1;
                    entry.backoff_until = now_secs() + backoff_secs(entry.restarts);
                    entry.state = RunState::Backoff;
                    warn!(app = %app.name, error = %e, "spawn failed");
                    let _ = registry.add_event(&app.name, "error", &format!("spawn failed: {e}"));
                }
            }
        }

        Ok(())
    }

    /// Spawn one instance: stage the kernel into the state dir, run the binary there with
    /// stdout/stderr captured to the app's log file. The `{endpoint}`/`{port}` placeholders in
    /// args are substituted with the resolved endpoint URL / declared port, which are also
    /// exported as `NOCKD_ENDPOINT_URL` / `NOCKD_PORT` for apps that read config from the
    /// environment (so the port lives only in the deploy config, not hardcoded in the app).
    fn spawn(&self, app: &AppRow, endpoint_url: Option<&str>, store: &Store) -> Result<Child> {
        let state_dir = self.paths.state_dir(&app.name);
        store.stage_jam(&app.artifact_hash, &state_dir)?;

        let log_path = self.paths.log_file(&app.name);
        let out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("opening log {}", log_path.display()))?;
        let err = out.try_clone().context("cloning log handle")?;

        let port_str = app.port.map(|p| p.to_string());
        let args: Vec<String> = app
            .args
            .iter()
            .map(|a| {
                let a = match endpoint_url {
                    Some(url) => a.replace("{endpoint}", url),
                    None => a.clone(),
                };
                match &port_str {
                    Some(p) => a.replace("{port}", p),
                    None => a,
                }
            })
            .collect();

        let bin = store.bin_path(&app.artifact_hash);
        let mut command = Command::new(&bin);
        command
            .current_dir(&state_dir)
            .args(&args)
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            // Own process group: a supervised app must NOT receive the daemon's controlling-
            // terminal signals. Otherwise Ctrl-C'ing `nockd serve` SIGINTs every app too
            // (nockchain was exiting 130/143 for exactly this reason). nockd still stops apps
            // deliberately via kill(pid, …).
            .process_group(0);
        if let Some(url) = endpoint_url {
            command.env("NOCKD_ENDPOINT_URL", url);
        }
        if let Some(p) = &port_str {
            command.env("NOCKD_PORT", p);
        }
        let child = command
            .spawn()
            .with_context(|| format!("spawning {}", bin.display()))?;
        Ok(child)
    }

    /// On startup, clean up any process left by a previous (unclean) daemon death. We do NOT
    /// re-adopt — this daemon spawns fresh instances. SIGTERM each leftover for a clean PMA
    /// flush, escalate to SIGKILL, and clear the pidfile, so reconcile starts from a blank
    /// slate with no duplicates or stale state.
    pub async fn cleanup_orphans(&self, registry: &Registry) {
        let apps = match registry.list_apps() {
            Ok(a) => a,
            Err(_) => return,
        };
        let mut killed = Vec::new();
        for app in &apps {
            let pidfile = self.paths.pid_file(&app.name);
            if let Ok(contents) = std::fs::read_to_string(&pidfile) {
                if let Some(pid) = contents
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse::<u32>().ok())
                {
                    if pid_alive(pid) {
                        warn!(app = %app.name, pid, "orphan from a previous daemon → SIGTERM");
                        send_term(pid);
                        killed.push(pid);
                    }
                }
            }
            let _ = std::fs::remove_file(&pidfile);
        }
        if !killed.is_empty() {
            wait_for_exit(&killed).await;
            for pid in killed {
                if pid_alive(pid) {
                    send_kill(pid);
                }
            }
        }
    }

    /// Gracefully stop every managed app (called on daemon shutdown): SIGTERM, wait the grace
    /// period for a clean PMA flush, SIGKILL stragglers, and clear pidfiles. Sets a flag so
    /// the reconcile loop won't respawn while we tear down.
    pub async fn stop_all(&self) {
        self.shutting_down
            .store(true, std::sync::atomic::Ordering::SeqCst);
        // Take the child handles so we can SIGTERM and then await (reap) them — the reconcile
        // loop is paused, so nothing else will reap, and an unreaped child lingers as a zombie.
        let children: Vec<(u32, Child)> = {
            let mut procs = self.procs.lock().unwrap();
            procs
                .values_mut()
                .filter_map(|m| Some((m.pid?, m.child.take()?)))
                .collect()
        };
        if !children.is_empty() {
            info!(count = children.len(), "stopping all apps (SIGTERM)");
            for (pid, _) in &children {
                send_term(*pid);
            }
            // SIGTERM was sent to all up front, so they exit in parallel; await each with a
            // grace cap, escalating to SIGKILL on timeout.
            for (_, mut child) in children {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(GRACE_SECS as u64),
                    child.wait(),
                )
                .await
                {
                    Ok(_) => {}
                    Err(_) => {
                        let _ = child.start_kill();
                        let _ = child.wait().await;
                    }
                }
            }
        }
        let procs = self.procs.lock().unwrap();
        for name in procs.keys() {
            let _ = std::fs::remove_file(self.paths.pid_file(name));
        }
    }
}

/// Whether a pid is alive (kill with signal 0 probes existence without delivering a signal).
fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// Wait (polling) until all pids exit, capped at the grace period — so a clean shutdown is
/// fast for well-behaved apps but bounded for stuck ones.
async fn wait_for_exit(pids: &[u32]) {
    for _ in 0..(GRACE_SECS * 5) {
        if pids.iter().all(|p| !pid_alive(*p)) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// Force-kill a process by pid (SIGKILL).
fn send_kill(pid: u32) {
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
}

/// Send SIGTERM to a process (graceful shutdown request).
fn send_term(pid: u32) {
    // Safety: kill(2) with a valid pid + signal; ignores the result (process may already
    // be gone, which is fine — the reap handles the exit).
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
}

fn backoff_secs(restarts: u32) -> i64 {
    let shift = restarts.min(6); // cap the exponent
    (1_i64 << shift).min(MAX_BACKOFF_SECS)
}
