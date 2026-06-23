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
        /// Project directory to build with `nockup`.
        #[arg(long)]
        project: Option<PathBuf>,
        /// Path to a prebuilt Rust wrapper binary.
        #[arg(long)]
        bin: Option<PathBuf>,
        /// Path to a prebuilt kernel (`out.jam`).
        #[arg(long)]
        jam: Option<PathBuf>,
        /// Nockchain public-gRPC endpoint URL (http://host:port).
        #[arg(long)]
        endpoint: Option<String>,
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
        /// Arguments passed through to the app process.
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// List deployed apps and their status.
    Ps,

    /// Live TUI dashboard of the fleet.
    #[command(alias = "top")]
    Dash,

    /// Show an app's recent logs.
    Logs {
        name: String,
        #[arg(long, default_value_t = 200)]
        lines: usize,
    },

    /// Restart an app.
    Restart { name: String },

    /// Stop an app (keeps it deployed).
    Stop { name: String },

    /// Start a stopped app.
    Start { name: String },
}
