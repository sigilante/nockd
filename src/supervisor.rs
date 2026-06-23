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
    /// Set when this daemon spawned the process and owns its handle. `None` + `adopted` means
    /// a process re-adopted from a previous daemon, monitored by pid.
    child: Option<Child>,
    /// True for a re-adopted process (no owned `Child`; liveness checked via the pid).
    adopted: bool,
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
            adopted: false,
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
                send_term(pid);
            }
            m.backoff_until = 0;
        }
    }

    /// Reconcile observed state to desired state. Called on a timer and after mutations.
    pub fn reconcile(&self, registry: &Registry, store: &Store) -> Result<()> {
        let apps = registry.list_apps()?;
        let desired_names: std::collections::HashSet<&str> =
            apps.iter().map(|a| a.name.as_str()).collect();

        let mut procs = self.procs.lock().unwrap();

        // Reap exited processes — owned children via try_wait, re-adopted ones via pid
        // liveness — escalate stalled terminations, and update state.
        for (name, m) in procs.iter_mut() {
            let exited: Option<i32> = if let Some(child) = m.child.as_mut() {
                match child.try_wait() {
                    Ok(Some(status)) => Some(status.code().unwrap_or(-1)),
                    Ok(None) => None,
                    Err(e) => {
                        warn!(app = %name, error = %e, "try_wait failed");
                        None
                    }
                }
            } else if m.adopted {
                match m.pid {
                    Some(pid) if !pid_alive(pid) => Some(-1), // adopted: exit code unknown
                    _ => None,
                }
            } else {
                None
            };

            if let Some(code) = exited {
                m.child = None;
                m.pid = None;
                m.adopted = false;
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
                        send_kill(pid);
                    }
                    let _ = std::fs::remove_file(self.paths.pid_file(&name));
                }
            }
        }

        // Drive each desired app toward its target.
        for app in &apps {
            let entry = procs.entry(app.name.clone()).or_default();

            let running = entry.child.is_some() || entry.adopted;

            if app.desired_status == "stopped" {
                if running {
                    // Begin a graceful stop once; reap/escalation drives it to completion.
                    if entry.term_deadline.is_none() {
                        if let Some(pid) = entry.pid {
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

            match self.spawn(app, store) {
                Ok(child) => {
                    let pid = child.id();
                    entry.pid = pid;
                    entry.child = Some(child);
                    entry.adopted = false;
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
    /// stdout/stderr captured to the app's log file.
    fn spawn(&self, app: &AppRow, store: &Store) -> Result<Child> {
        let state_dir = self.paths.state_dir(&app.name);
        store.stage_jam(&app.artifact_hash, &state_dir)?;

        let log_path = self.paths.log_file(&app.name);
        let out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("opening log {}", log_path.display()))?;
        let err = out.try_clone().context("cloning log handle")?;

        let bin = store.bin_path(&app.artifact_hash);
        let child = Command::new(&bin)
            .current_dir(&state_dir)
            .args(&app.args)
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            // Own process group: a supervised app must NOT receive the daemon's controlling-
            // terminal signals. Otherwise Ctrl-C'ing `nockd serve` SIGINTs every app too
            // (nockchain was exiting 130/143 for exactly this reason). nockd still stops apps
            // deliberately via kill(pid, …).
            .process_group(0)
            .spawn()
            .with_context(|| format!("spawning {}", bin.display()))?;
        Ok(child)
    }

    /// Re-adopt an already-running process (a survivor of a previous daemon) by pid, so a
    /// daemon restart doesn't orphan it and spawn a conflicting duplicate (OQ6).
    pub fn adopt(&self, name: &str, pid: u32, started_at: i64) {
        let mut procs = self.procs.lock().unwrap();
        let m = procs.entry(name.to_string()).or_default();
        m.child = None;
        m.adopted = true;
        m.pid = Some(pid);
        m.started_at = started_at;
        m.state = RunState::Running;
        m.health = HealthState::Unknown;
        info!(app = %name, pid, "re-adopted running instance");
    }

    /// On daemon startup, re-adopt any desired-running app whose recorded pid is still alive
    /// (it survived a daemon restart thanks to its own process group). Stale pidfiles are
    /// cleaned. Note: relies on pid liveness; a recycled pid is a small, accepted risk.
    pub fn reattach(&self, registry: &Registry) {
        let apps = match registry.list_apps() {
            Ok(a) => a,
            Err(_) => return,
        };
        for app in apps {
            if app.desired_status != "running" {
                continue;
            }
            let pidfile = self.paths.pid_file(&app.name);
            let Ok(contents) = std::fs::read_to_string(&pidfile) else {
                continue;
            };
            let mut parts = contents.split_whitespace();
            let Some(pid) = parts.next().and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            let started_at = parts
                .next()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or_else(now_secs);
            if pid_alive(pid) {
                self.adopt(&app.name, pid, started_at);
            } else {
                let _ = std::fs::remove_file(&pidfile);
            }
        }
    }
}

/// Whether a pid is alive (kill with signal 0 probes existence without delivering a signal).
fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
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
