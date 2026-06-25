//! Command-line surface. One binary, two modes (DESIGN §5.2): `nockd serve` runs the
//! daemon; the other subcommands are clients of it.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "nockd", version, about = "Self-hostable NockApp deployment platform")]
pub struct Cli {
    /// Daemon host to talk to (client commands).
    #[arg(long, global = true, default_value = "127.0.0.1", env = "NOCKD_HOST")]
    pub host: String,

    /// Daemon port.
    #[arg(long, global = true, default_value_t = 4490, env = "NOCKD_PORT")]
    pub port: u16,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run the daemon (supervisor + control API + dashboard).
    Serve {
        /// Data root (default ~/.nockd).
        #[arg(long, env = "NOCKD_ROOT")]
        root: Option<PathBuf>,
        /// Bind address for the API + dashboard.
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,
    },

    /// Build (via `nockup`) and/or ship a NockApp artifact and run it.
    ///
    /// Two modes: `--project <dir>` builds with the client-side toolchain (principle 7),
    /// or `--bin`/`--jam` ship a prebuilt artifact. The daemon never compiles.
    Deploy {
        /// App name (required for prebuilt mode; inferred from the manifest with --project).
        name: Option<String>,
        /// Read all deploy config from a nockd.toml manifest (flags are ignored).
        #[arg(long, short = 'f')]
        manifest: Option<PathBuf>,
        /// Project directory to build with `nockup`.
        #[arg(long)]
        project: Option<PathBuf>,
        /// For a multi-bin project, which `[[bin]]` target to ship (e.g. `listen` → builds
        /// `target/release/listen` + `listen.jam`). Omit for single-bin (`out.jam`).
        #[arg(long)]
        bin_target: Option<String>,
        /// Path to a prebuilt Rust wrapper binary.
        #[arg(long)]
        bin: Option<PathBuf>,
        /// Path to a prebuilt kernel (`out.jam`).
        #[arg(long)]
        jam: Option<PathBuf>,
        /// Nockchain public-gRPC endpoint URL (http://host:port).
        #[arg(long)]
        endpoint: Option<String>,
        /// Port an HTTP NockApp serves on. nockd exports it as NOCKD_APP_PORT (and substitutes
        /// `{port}` in args) and the dashboard links to localhost:<port>. (Named --web-port to
        /// avoid the global --port, which is the daemon's control-API port.)
        #[arg(long)]
        web_port: Option<u16>,
        /// App icon for the dashboard: a path to an image (png/jpg/gif/webp/svg/ico) or an
        /// inline `data:` URI. A path is encoded into a data URI at deploy time.
        #[arg(long)]
        icon: Option<String>,
        /// App's private/admin gRPC address for the health gate (host:port).
        #[arg(long)]
        health_addr: Option<String>,
        /// Shell command whose first stdout line becomes the app's custom status (e.g.
        /// block height). Runs every 5s with cwd=state dir and NOCKD_LOG/NOCKD_ENDPOINT set.
        #[arg(long)]
        status_cmd: Option<String>,
        /// Short label for the custom status (e.g. "BLOCK").
        #[arg(long)]
        status_label: Option<String>,
        /// Restart policy: always | on-failure | never.
        #[arg(long, default_value = "on-failure")]
        restart: String,
        /// Target triple recorded in the artifact identity.
        #[arg(long, default_value = env!("NOCKD_DEFAULT_TARGET"))]
        target: String,
        /// Attach an external build attestation (JSON) instead of self-signing.
        #[arg(long)]
        attestation: Option<PathBuf>,
        /// Don't auto-sign a self-attestation even if a builder key exists.
        #[arg(long)]
        no_attest: bool,
        /// Arguments passed through to the app process.
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// List deployed apps and their status.
    Ps,

    /// Manage trusted builder keys (whose attestations count as verified).
    Trust {
        #[command(subcommand)]
        action: TrustAction,
    },

    /// Stop all apps (keeps them deployed; daemon stays up).
    Down,

    /// Start all stopped apps.
    Up,

    /// Live TUI dashboard of the fleet.
    #[command(alias = "top")]
    Dash,

    /// Manage named Nockchain RPC endpoints.
    Endpoint {
        #[command(subcommand)]
        action: EndpointAction,
    },

    /// Manage the builder signing key (for build attestations).
    Key {
        #[command(subcommand)]
        action: KeyAction,
    },

    /// Sign a build attestation for an artifact (client/CI side).
    Attest {
        /// Artifact (bundle) hash, blake3 hex.
        #[arg(long)]
        artifact: String,
        /// Kernel (out.jam) hash, blake3 hex (empty for binary-only).
        #[arg(long, default_value = "")]
        kernel: String,
        /// Target triple.
        #[arg(long, default_value = env!("NOCKD_DEFAULT_TARGET"))]
        target: String,
        /// Write the attestation JSON here (default: stdout).
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Verify a build attestation's signature.
    VerifyAtt {
        /// Path to the attestation JSON.
        file: PathBuf,
    },

    /// Show an app's recent logs.
    Logs {
        name: String,
        #[arg(long, default_value_t = 200)]
        lines: usize,
    },

    /// Restart an app.
    Restart { name: String },

    /// Re-read an app's nockd.toml and re-apply its config (port, args, status, endpoint,
    /// restart), then restart it. Does NOT rebuild — for that, `nockd deploy -f` again.
    Reload { name: String },

    /// Roll an app back to the previous artifact it ran (reverts code, keeps config), then
    /// restart. Fails if only one artifact has been deployed.
    Rollback { name: String },

    /// Stop an app (keeps it deployed).
    Stop { name: String },

    /// Start a stopped app.
    Start { name: String },
}

#[derive(Subcommand)]
pub enum TrustAction {
    /// Trust a builder public key.
    Add { pubkey: String },
    /// List trusted builder keys.
    #[command(alias = "ls")]
    List,
    /// Stop trusting a builder key.
    #[command(alias = "rm")]
    Remove { pubkey: String },
}

#[derive(Subcommand)]
pub enum KeyAction {
    /// Generate a builder signing key (fails if one exists).
    Gen,
    /// Print the builder public key (the attestation identity).
    Show,
}

#[derive(Subcommand)]
pub enum EndpointAction {
    /// Register (or update) a named endpoint.
    Add {
        name: String,
        /// Public-gRPC URL, e.g. http://host:5555.
        url: String,
        /// remote | local-socket.
        #[arg(long, default_value = "remote")]
        kind: String,
    },
    /// List endpoints with reachability.
    #[command(alias = "ls")]
    List,
    /// Remove an endpoint.
    #[command(alias = "rm")]
    Remove { name: String },
}
