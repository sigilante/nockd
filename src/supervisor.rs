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
    Stopped,
    Crashed,
    Backoff,
}

struct Managed {
    child: Option<Child>,
    pid: Option<u32>,
    started_at: i64,
    restarts: u32,
    backoff_until: i64,
    state: RunState,
    health: HealthState,
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
}

pub struct Supervisor {
    paths: Paths,
    procs: Mutex<HashMap<String, Managed>>,
}

const MAX_BACKOFF_SECS: i64 = 60;

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
        })
    }

    /// Record the latest health probe result (called by the daemon's probe loop).
    pub fn set_health(&self, name: &str, health: HealthState) {
        let mut procs = self.procs.lock().unwrap();
        if let Some(m) = procs.get_mut(name) {
            m.health = health;
        }
    }

    /// Force an immediate restart: kill the child and clear backoff.
    pub fn request_restart(&self, name: &str) {
        let mut procs = self.procs.lock().unwrap();
        if let Some(m) = procs.get_mut(name) {
            if let Some(child) = m.child.as_mut() {
                m.restart_requested = true;
                let _ = child.start_kill();
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

        // Reap exited children and update state for everything we track.
        for (name, m) in procs.iter_mut() {
            if let Some(child) = m.child.as_mut() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        m.child = None;
                        m.pid = None;
                        let code = status.code().unwrap_or(-1);
                        if m.restart_requested {
                            // Operator-initiated restart: no penalty, restart next tick.
                            m.restart_requested = false;
                            m.backoff_until = 0;
                            m.state = RunState::Backoff;
                        } else {
                            m.restarts += 1;
                            m.backoff_until = now_secs() + backoff_secs(m.restarts);
                            m.state = RunState::Backoff;
                            warn!(app = %name, code, "instance exited");
                            let _ = registry.add_event(name, "crash", &format!("exit code {code}"));
                        }
                    }
                    Ok(None) => {}
                    Err(e) => warn!(app = %name, error = %e, "try_wait failed"),
                }
            }
        }

        // Drop tracking for apps removed from the registry (kill if still alive).
        let tracked: Vec<String> = procs.keys().cloned().collect();
        for name in tracked {
            if !desired_names.contains(name.as_str()) {
                if let Some(mut m) = procs.remove(&name) {
                    if let Some(child) = m.child.as_mut() {
                        let _ = child.start_kill();
                    }
                }
            }
        }

        // Drive each desired app toward its target.
        for app in &apps {
            let entry = procs.entry(app.name.clone()).or_default();

            if app.desired_status == "stopped" {
                if let Some(child) = entry.child.as_mut() {
                    let _ = child.start_kill();
                    let _ = registry.add_event(&app.name, "stop", "stopped by request");
                }
                entry.child = None;
                entry.pid = None;
                entry.state = RunState::Stopped;
                entry.health = HealthState::Unknown;
                continue;
            }

            // desired running
            let alive = entry.child.is_some();
            if alive {
                entry.state = RunState::Running;
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
                    entry.pid = child.id();
                    entry.child = Some(child);
                    entry.started_at = now_secs();
                    entry.state = RunState::Running;
                    entry.health = HealthState::Unknown;
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
            .spawn()
            .with_context(|| format!("spawning {}", bin.display()))?;
        Ok(child)
    }
}

fn backoff_secs(restarts: u32) -> i64 {
    let shift = restarts.min(6); // cap the exponent
    (1_i64 << shift).min(MAX_BACKOFF_SECS)
}
