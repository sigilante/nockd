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

    /// Build (TODO) + ship a NockApp artifact and run it. Phase 0 takes a prebuilt
    /// binary + kernel; client-side `nockup` build is a later step (principle 7).
    Deploy {
        /// App name.
        name: String,
        /// Path to the built Rust wrapper binary.
        #[arg(long)]
        bin: PathBuf,
        /// Path to the compiled kernel (`out.jam`).
        #[arg(long)]
        jam: PathBuf,
        /// Nockchain public-gRPC endpoint URL (http://host:port).
        #[arg(long)]
        endpoint: Option<String>,
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
