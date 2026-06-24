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
    pub run: PathBuf,
    pub keys: PathBuf,
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
            run: root.join("run"),
            keys: root.join("keys"),
            root,
        };
        for dir in [&paths.root, &paths.artifacts, &paths.state, &paths.logs, &paths.run] {
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

    /// Pidfile recording a supervised process's pid, for re-adoption across daemon restarts.
    pub fn pid_file(&self, app: &str) -> PathBuf {
        self.run.join(format!("{app}.pid"))
    }

    /// The builder signing key (ed25519 seed). Lives where you build (dev/CI), not the daemon.
    pub fn builder_key(&self) -> PathBuf {
        self.keys.join("builder.key")
    }
}

/// Remove ANSI/VT100 CSI escape sequences (e.g. `\x1b[32m`) and NUL bytes. Apps like
/// nockchain emit color even when piped, and the kernel-boot log contains NULs — which make
/// BSD grep (macOS) treat the stream as binary and silently suppress `-o`, so a status recipe
/// would produce blank output. Stripping here makes the status-command stdin and the TUI log
/// clean and grep-friendly on every platform.
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
        } else if c != '\0' {
            out.push(c);
        }
    }
    out
}

/// Read up to the last `max_bytes` of a file as lossy UTF-8 — a cheap tail so we never load
/// a multi-GB log (nockchain's grows fast) into memory.
pub async fn read_tail(path: &std::path::Path, max_bytes: u64) -> String {
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

/// Current size of a file in bytes (0 if missing) — used to set the SSE follow offset.
pub async fn file_len(path: &std::path::Path) -> u64 {
    tokio::fs::metadata(path).await.map(|m| m.len()).unwrap_or(0)
}

/// Seconds since the Unix epoch.
pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The deploy manifest (`nockd.toml`) — declarative, version-controllable deploy config
/// owned by nockd (DESIGN §7.2). `nockd deploy -f nockd.toml` reads everything from here.
#[derive(Debug, Clone, Deserialize)]
pub struct DeployManifest {
    pub deploy: DeploySection,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeploySection {
    /// App name (referenced by ps/logs/stop; names the state dir).
    pub app: String,
    /// Build mode: a project dir built via `nockup` (alternative to bin/jam).
    #[serde(default)]
    pub project: Option<PathBuf>,
    /// For a multi-bin project, which `[[bin]]` target to ship (→ `target/release/<t>` +
    /// `<t>.jam`). Omit for a single-bin project (`<package>` + `out.jam`).
    #[serde(default)]
    pub bin_target: Option<String>,
    /// Prebuilt mode: the wrapper binary, and an optional kernel (omit for binary-only apps).
    #[serde(default)]
    pub bin: Option<PathBuf>,
    #[serde(default)]
    pub jam: Option<PathBuf>,
    #[serde(default = "default_restart")]
    pub restart: String,
    /// Target triple recorded in artifact identity (defaults to the daemon's).
    #[serde(default)]
    pub target: Option<String>,
    /// Arguments passed through to the app process.
    #[serde(default)]
    pub args: Vec<String>,
    /// App's private/admin gRPC address for the health probe.
    #[serde(default)]
    pub health_addr: Option<String>,
    /// Named Nockchain endpoint this app attaches to (see the endpoint registry).
    #[serde(default)]
    pub endpoint: Option<String>,
    /// The port an HTTP NockApp serves on. nockd is the single source of truth: it exports
    /// `NOCKD_APP_PORT` and substitutes `{port}` in args so the app binds the port nockd declares
    /// (no hardcoded port on either side), and the dashboard derives an "open app" link to
    /// `localhost:<port>` from it.
    #[serde(default)]
    pub port: Option<u16>,
    /// Custom status command + label (e.g. block height).
    #[serde(default)]
    pub status: StatusSection,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StatusSection {
    pub label: Option<String>,
    pub cmd: Option<String>,
}

fn default_restart() -> String {
    "on-failure".to_string()
}

impl DeployManifest {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading manifest {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing manifest {}", path.display()))
    }
}
