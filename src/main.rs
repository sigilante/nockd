//! nockd — a self-hostable deployment platform for NockApps.
//! See DESIGN.md for the authoritative architecture.

mod api;
mod cli;
mod client;
mod config;
mod daemon;
mod dashboard;
mod registry;
mod store;
mod supervisor;

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
            bin,
            jam,
            endpoint,
            restart,
            target,
            args,
        } => {
            let bin_bytes = std::fs::read(&bin)
                .with_context(|| format!("reading binary {}", bin.display()))?;
            let jam_bytes = std::fs::read(&jam)
                .with_context(|| format!("reading kernel {}", jam.display()))?;
            let engine = base64::engine::general_purpose::STANDARD;
            let req = api::DeployRequest {
                name: name.clone(),
                target_triple: target,
                bin_b64: engine.encode(&bin_bytes),
                jam_b64: engine.encode(&jam_bytes),
                endpoint,
                restart,
                args,
                provenance: None,
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
            println!("{:<16} {:<10} {:<14} {:<8} {}", "NAME", "STATE", "KERNEL", "PID", "ENDPOINT");
            for a in apps {
                let (state, pid) = match &a.runtime {
                    Some(rt) => (
                        format!("{:?}", rt.state).to_lowercase(),
                        rt.pid.map(|p| p.to_string()).unwrap_or_else(|| "—".into()),
                    ),
                    None => (a.desired_status.clone(), "—".into()),
                };
                let kernel = a.kernel_hash.chars().take(12).collect::<String>();
                println!(
                    "{:<16} {:<10} {:<14} {:<8} {}",
                    a.name,
                    state,
                    kernel,
                    pid,
                    a.endpoint.unwrap_or_else(|| "—".into())
                );
            }
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
