//! nockd — a self-hostable deployment platform for NockApps.
//! See DESIGN.md for the authoritative architecture.

mod api;
mod apiv1;
mod buildkit;
mod cli;
mod client;
mod config;
mod daemon;
mod dashboard;
mod health;
mod registry;
mod store;
mod supervisor;
mod tui;

use std::net::IpAddr;

use anyhow::{Context, Result};
use base64::Engine;
use clap::Parser;

use cli::{Cli, Commands};
use client::Client;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nockd=info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { root, bind } => {
            let paths = config::Paths::resolve(root)?;
            let daemon = daemon::Daemon::new(paths)?;
            let bind_ip: IpAddr = bind.parse().context("invalid --bind address")?;
            daemon::serve(daemon, bind_ip, cli.port).await?;
        }

        Commands::Deploy {
            name,
            manifest,
            project,
            bin,
            jam,
            endpoint,
            health_addr,
            status_cmd,
            status_label,
            restart,
            target,
            args,
        } => {
            // A manifest supplies all config; otherwise use the flags.
            let (name, project, bin, jam, endpoint, health_addr, status_cmd, status_label, restart, target, args) =
                if let Some(mpath) = manifest {
                    let d = config::DeployManifest::load(&mpath)?.deploy;
                    (
                        Some(d.app),
                        d.project,
                        d.bin,
                        d.jam,
                        d.endpoint,
                        d.health_addr,
                        d.status.cmd,
                        d.status.label,
                        d.restart,
                        d.target
                            .unwrap_or_else(|| env!("NOCKD_DEFAULT_TARGET").to_string()),
                        d.args,
                    )
                } else {
                    (
                        name, project, bin, jam, endpoint, health_addr, status_cmd,
                        status_label, restart, target, args,
                    )
                };

            let (name, bin_path, jam_path, provenance) = if let Some(proj) = project {
                let built = buildkit::build_project(&proj)?;
                let name = name.unwrap_or(built.name);
                (name, built.bin, Some(built.jam), Some(built.provenance))
            } else {
                let name = name.context("app name required (or pass --project <dir>)")?;
                let bin = bin.context("--bin required when not using --project")?;
                // --jam is optional: binary-only artifacts embed their kernel (e.g. nockchain).
                (name, bin, jam, None)
            };

            let bin_bytes = std::fs::read(&bin_path)
                .with_context(|| format!("reading binary {}", bin_path.display()))?;
            let engine = base64::engine::general_purpose::STANDARD;
            let jam_b64 = match &jam_path {
                Some(p) => {
                    let bytes =
                        std::fs::read(p).with_context(|| format!("reading kernel {}", p.display()))?;
                    Some(engine.encode(&bytes))
                }
                None => None,
            };
            let req = api::DeployRequest {
                name: name.clone(),
                target_triple: target,
                bin_b64: engine.encode(&bin_bytes),
                jam_b64,
                endpoint,
                restart,
                args,
                admin_addr: health_addr,
                status_cmd,
                status_label,
                provenance,
            };
            let client = Client::new(&cli.host, cli.port);
            let resp = client.deploy(&req).await?;
            println!(
                "deployed {}\n  artifact {}\n  kernel   {}",
                resp.name, resp.artifact_hash, resp.kernel_hash
            );
        }

        Commands::Ps => {
            let client = Client::new(&cli.host, cli.port);
            let apps = client.list().await?;
            if apps.is_empty() {
                println!("no apps deployed");
                return Ok(());
            }
            println!(
                "{:<16} {:<10} {:<12} {:<14} {:<8} {:<20} {}",
                "NAME", "STATE", "HEALTH", "KERNEL", "PID", "ENDPOINT", "STATUS"
            );
            for a in apps {
                let (state, health, pid) = match &a.runtime {
                    Some(rt) => (
                        format!("{:?}", rt.state).to_lowercase(),
                        format!("{:?}", rt.health).to_lowercase(),
                        rt.pid.map(|p| p.to_string()).unwrap_or_else(|| "—".into()),
                    ),
                    None => (a.desired_status.clone(), "unknown".into(), "—".into()),
                };
                let kernel = a.kernel_hash.chars().take(12).collect::<String>();
                let status_line = a
                    .runtime
                    .as_ref()
                    .and_then(|rt| rt.status_line.clone())
                    .map(|line| {
                        let label = a.status_label.as_deref().unwrap_or("").trim();
                        if label.is_empty() { line } else { format!("{label} {line}") }
                    })
                    .unwrap_or_default();
                println!(
                    "{:<16} {:<10} {:<12} {:<14} {:<8} {:<20} {}",
                    a.name,
                    state,
                    health,
                    kernel,
                    pid,
                    a.endpoint.unwrap_or_else(|| "—".into()),
                    status_line,
                );
            }
        }

        Commands::Dash => {
            tui::run(&cli.host, cli.port).await?;
        }

        Commands::Logs { name, lines } => {
            let client = Client::new(&cli.host, cli.port);
            print!("{}", client.logs(&name, lines).await?);
        }

        Commands::Restart { name } => {
            Client::new(&cli.host, cli.port).action(&name, "restart").await?;
            println!("restarted {name}");
        }

        Commands::Stop { name } => {
            Client::new(&cli.host, cli.port).action(&name, "stop").await?;
            println!("stopped {name}");
        }

        Commands::Start { name } => {
            Client::new(&cli.host, cli.port).action(&name, "start").await?;
            println!("started {name}");
        }
    }

    Ok(())
}
