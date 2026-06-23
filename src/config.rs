//! Paths, time helpers, and the deploy manifest (`nockd.toml`).
//!
//! NOTE (bedrock deviation, tracked): DESIGN.md §5/§10 make the Control API's primary
//! listener a Unix socket. Phase 0 serves the API + dashboard over a localhost TCP
//! listener for simplicity; the Unix-socket-default hardening is a Phase 1 task.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::Deserialize;

/// On-disk layout, rooted at `~/.nockd` (override with `--root` / `NOCKD_ROOT`).
#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
    pub db: PathBuf,
    pub artifacts: PathBuf,
    pub state: PathBuf,
    pub logs: PathBuf,
}

impl Paths {
    pub fn resolve(custom_root: Option<PathBuf>) -> Result<Self> {
        let root = match custom_root {
            Some(r) => r,
            None => dirs::home_dir()
                .context("could not determine home directory")?
                .join(".nockd"),
        };
        let paths = Paths {
            db: root.join("nockd.sqlite"),
            artifacts: root.join("artifacts"),
            state: root.join("state"),
            logs: root.join("logs"),
            root,
        };
        for dir in [&paths.root, &paths.artifacts, &paths.state, &paths.logs] {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("creating {}", dir.display()))?;
        }
        Ok(paths)
    }

    pub fn state_dir(&self, app: &str) -> PathBuf {
        self.state.join(app)
    }

    pub fn log_file(&self, app: &str) -> PathBuf {
        self.logs.join(format!("{app}.log"))
    }
}

/// Remove ANSI/VT100 CSI escape sequences (e.g. `\x1b[32m`). Apps like nockchain emit color
/// even when piped; we strip for the status-command stdin and the TUI log pane.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&n) = chars.peek() {
                    chars.next();
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Seconds since the Unix epoch.
pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The deploy manifest (`nockd.toml`) — runtime config owned by nockd (DESIGN §7.2).
/// Defined ahead of use: `nockd deploy` currently takes flags; reading `nockd.toml` is the
/// next wire-up.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct DeployManifest {
    pub deploy: DeploySection,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct DeploySection {
    pub app: String,
    #[serde(default = "default_restart")]
    pub restart: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub nockchain: Option<NockchainSection>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct NockchainSection {
    /// A public-gRPC endpoint URL `http://host:port` (DESIGN §5.3).
    pub endpoint: Option<String>,
}

fn default_restart() -> String {
    "on-failure".to_string()
}

#[allow(dead_code)]
impl DeployManifest {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading manifest {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing manifest {}", path.display()))
    }
}
