//! nockd — a self-hostable deployment platform for NockApps.
//! See DESIGN.md for the authoritative architecture.

mod api;
mod apiv1;
mod attest;
mod buildkit;
mod cli;
mod client;
mod config;
mod daemon;
mod dashboard;
mod health;
mod nockchain;
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
            bin_target,
            bin,
            jam,
            endpoint,
            web_port,
            icon,
            health_addr,
            status_cmd,
            status_label,
            restart,
            target,
            attestation,
            no_attest,
            args,
        } => {
            // Record the manifest's absolute path so the daemon can re-read it for "Reload"
            // (the daemon runs elsewhere/with a different cwd, so a relative path is useless).
            let manifest_path: Option<String> = manifest.as_ref().map(|p| {
                std::fs::canonicalize(p)
                    .unwrap_or_else(|_| p.clone())
                    .to_string_lossy()
                    .into_owned()
            });

            // A manifest supplies all config; otherwise use the flags.
            let (name, project, bin_target, bin, jam, endpoint, port, icon, health_addr, status_cmd, status_label, restart, target, args) =
                if let Some(mpath) = manifest {
                    let d = config::DeployManifest::load(&mpath)?.deploy;
                    (
                        Some(d.app),
                        d.project,
                        d.bin_target,
                        d.bin,
                        d.jam,
                        d.endpoint,
                        d.port,
                        d.icon,
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
                        name, project, bin_target, bin, jam, endpoint, web_port, icon, health_addr, status_cmd,
                        status_label, restart, target, args,
                    )
                };

            // Resolve the icon (a path → data URI, an inline data: URI → as-is) relative to the
            // manifest's dir, or the cwd for flag deploys. Done client-side so the daemon stores
            // a ready-to-serve data URI.
            let icon = match icon {
                Some(spec) => {
                    let base = manifest_path
                        .as_deref()
                        .and_then(|p| std::path::Path::new(p).parent())
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    Some(config::resolve_icon(&base, &spec)?)
                }
                None => None,
            };

            let (name, bin_path, jam_path, provenance) = if let Some(proj) = project {
                let built = buildkit::build_project(&proj, bin_target.as_deref())?;
                // Multi-bin: default the app name to the shipped bin target, not the package.
                let name = name.or(bin_target).unwrap_or(built.name);
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
            let jam_bytes: Option<Vec<u8>> = match &jam_path {
                Some(p) => Some(
                    std::fs::read(p).with_context(|| format!("reading kernel {}", p.display()))?,
                ),
                None => None,
            };
            let jam_b64 = jam_bytes.as_ref().map(|b| engine.encode(b));
            // Attestation: an external one if provided, else self-sign with the builder key
            // (unless --no-attest). The client computes the same hashes the daemon will.
            let attestation_json = if let Some(path) = attestation {
                Some(std::fs::read_to_string(&path).with_context(|| {
                    format!("reading attestation {}", path.display())
                })?)
            } else if !no_attest {
                let key_path = config::Paths::resolve(None)?.builder_key();
                match attest::load_key(&key_path) {
                    Ok(key) => {
                        let rec = store::Store::compute_hashes(
                            jam_bytes.as_deref(),
                            &bin_bytes,
                            &target,
                        );
                        let prov = provenance
                            .as_deref()
                            .and_then(|p| serde_json::from_str(p).ok())
                            .unwrap_or(serde_json::json!({}));
                        let att = attest::create(
                            &key,
                            &rec.artifact_hash,
                            &rec.kernel_hash,
                            &target,
                            prov,
                        )?;
                        Some(serde_json::to_string(&att)?)
                    }
                    Err(_) => None, // no builder key → no self-attestation
                }
            } else {
                None
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
                port,
                manifest_path,
                icon,
                provenance,
                attestation: attestation_json,
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
                "{:<16} {:<10} {:<10} {:<10} {:<8} {:<18} {}",
                "NAME", "STATE", "HEALTH", "VERIFIED", "PID", "ENDPOINT", "STATUS"
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
                    "{:<16} {:<10} {:<10} {:<10} {:<8} {:<18} {}",
                    a.name,
                    state,
                    health,
                    a.verified,
                    pid,
                    a.endpoint.unwrap_or_else(|| "—".into()),
                    status_line,
                );
            }
        }

        Commands::Down => {
            let (changed, total) = Client::new(&cli.host, cli.port).fleet("down").await?;
            println!("stopped {changed} of {total} apps");
        }

        Commands::Up => {
            let (changed, total) = Client::new(&cli.host, cli.port).fleet("up").await?;
            println!("started {changed} of {total} apps");
        }

        Commands::Trust { action } => {
            let client = Client::new(&cli.host, cli.port);
            match action {
                cli::TrustAction::Add { pubkey } => {
                    client.trust_add(&pubkey).await?;
                    println!("now trusting builder {pubkey}");
                }
                cli::TrustAction::Remove { pubkey } => {
                    client.trust_remove(&pubkey).await?;
                    println!("stopped trusting {pubkey}");
                }
                cli::TrustAction::List => {
                    let keys = client.trust_list().await?;
                    if keys.is_empty() {
                        println!("no trusted builders");
                    } else {
                        for k in keys {
                            println!("{k}");
                        }
                    }
                }
            }
        }

        Commands::Key { action } => {
            let paths = config::Paths::resolve(None)?;
            let key_path = paths.builder_key();
            match action {
                cli::KeyAction::Gen => {
                    let key = attest::generate_key();
                    attest::save_key(&key, &key_path)?;
                    println!("builder key created: {}", key_path.display());
                    println!("public key (builder identity): {}", attest::pubkey_hex(&key));
                }
                cli::KeyAction::Show => {
                    let key = attest::load_key(&key_path)
                        .context("no builder key — run `nockd key gen`")?;
                    println!("{}", attest::pubkey_hex(&key));
                }
            }
        }

        Commands::Attest {
            artifact,
            kernel,
            target,
            out,
        } => {
            let paths = config::Paths::resolve(None)?;
            let key = attest::load_key(&paths.builder_key())
                .context("no builder key — run `nockd key gen`")?;
            let provenance = serde_json::json!({ "note": "provenance is captured at build time" });
            let att = attest::create(&key, &artifact, &kernel, &target, provenance)?;
            let json = serde_json::to_string_pretty(&att)?;
            match out {
                Some(p) => {
                    std::fs::write(&p, json)?;
                    println!("attestation written to {}", p.display());
                }
                None => println!("{json}"),
            }
        }

        Commands::VerifyAtt { file } => {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;
            let att: attest::Attestation = serde_json::from_str(&text).context("parsing attestation")?;
            match attest::verify_signature(&att) {
                Ok(builder) => {
                    println!("✓ signature valid");
                    println!("  builder:  {builder}");
                    println!("  artifact: {}", att.payload.artifact_hash);
                    println!("  kernel:   {}", att.payload.kernel_hash);
                }
                Err(e) => {
                    println!("✗ INVALID: {e}");
                    std::process::exit(1);
                }
            }
        }

        Commands::Dash => {
            tui::run(&cli.host, cli.port).await?;
        }

        Commands::Endpoint { action } => {
            let client = Client::new(&cli.host, cli.port);
            match action {
                cli::EndpointAction::Add { name, url, kind } => {
                    client.add_endpoint(&name, &url, &kind).await?;
                    println!("added endpoint {name} → {url}");
                }
                cli::EndpointAction::Remove { name } => {
                    client.remove_endpoint(&name).await?;
                    println!("removed endpoint {name}");
                }
                cli::EndpointAction::List => {
                    let eps = client.endpoints().await?;
                    let arr = eps.as_array().cloned().unwrap_or_default();
                    if arr.is_empty() {
                        println!("no endpoints registered");
                    } else {
                        println!(
                            "{:<16} {:<8} {:<30} {:<8} {:<12} {:<8} {}",
                            "NAME", "REACH", "URL", "LAG", "HEIGHT", "BEHIND", "APPS"
                        );
                        for e in arr {
                            let reach = if e["reachable"].as_bool().unwrap_or(false) { "ok" } else { "down" };
                            let lag = e["lag_ms"].as_u64().map(|l| format!("{l}ms")).unwrap_or_else(|| "—".into());
                            let height = e["height"].as_u64().map(|h| h.to_string()).unwrap_or_else(|| "—".into());
                            let behind = e["behind"].as_u64().map(|b| if b == 0 { "tip".into() } else { format!("-{b}") }).unwrap_or_else(|| "—".into());
                            let apps = e["attached_apps"].as_array().map(|a| a.len()).unwrap_or(0);
                            println!(
                                "{:<16} {:<8} {:<30} {:<8} {:<12} {:<8} {}",
                                e["name"].as_str().unwrap_or(""),
                                reach,
                                e["url"].as_str().unwrap_or(""),
                                lag,
                                height,
                                behind,
                                apps,
                            );
                        }
                    }
                }
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

        Commands::Reload { name } => {
            Client::new(&cli.host, cli.port).action(&name, "reload").await?;
            println!("reloaded {name} (manifest re-read; config re-applied)");
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
