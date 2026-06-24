//! hello-basic — the minimal supervised NockApp.
//!
//! This is the smallest thing that proves the NockApp build -> deploy -> observe loop: it
//! boots the trivial `basic` Hoon kernel (so it is a real, supervised NockApp the way nockd
//! expects) and then runs a tokio loop that emits a heartbeat metric forever. There is NO
//! chain, NO HTTP, NO TLS — it is `chain-watch` with the chain removed: same structure, but
//! instead of polling an RPC for the height, it just increments a tick counter.
//!
//! THE KEY LESSON (read RECIPE.md): the `basic` template's stock `main.rs` pokes the kernel
//! ONCE and then EXITS. That is not a long-lived service. Under nockd with `restart = always`
//! a process that exits is a process that gets restarted — i.e. a crash loop. A supervised
//! NockApp must STAY ALIVE. So after booting we enter a loop that ticks every few seconds and
//! only leaves on SIGTERM (nockd's stop/restart signal) or Ctrl-C, exiting cleanly (0).
//!
//! Status metric (DESIGN status-cmd): we print `metric: ticks=<N>` on its own line each loop.
//! The deploy manifest's status command — `grep -oE 'ticks=[0-9]+' | tail -1 | grep -oE
//! '[0-9]+'` — surfaces the climbing counter as the TICKS column in `nockd ps`. nockd strips
//! NUL bytes from the boot log now, so plain `grep` works (no `-a` needed).

use std::error::Error;
use std::fs;
use std::time::Duration;

use nockapp::kernel::boot;
use nockapp::noun::slab::NounSlab;
use nockapp::wire::{SystemWire, Wire};
use nockapp::NockApp;
use nockvm::noun::{D, T};
use nockvm_macros::tas;
use tracing::{info, warn};

const TICK_INTERVAL: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = boot::default_boot_cli(false);
    boot::init_default_tracing(&cli);

    // Boot the kernel so this is a real, supervised NockApp (consumes out.jam, sets up the
    // PMA / event-log state dir, etc.). nockd stages out.jam into the app's state dir, which
    // is also our cwd, so the relative read works both under `nockup run` and under nockd.
    let kernel = fs::read("out.jam").map_err(|e| format!("Failed to read out.jam: {}", e))?;
    let mut nockapp: NockApp = boot::setup(&kernel, cli, &[], "hello-basic", None).await?;

    // Fire the kernel's demo poke once so the kernel is genuinely exercised at boot.
    let mut poke_slab = NounSlab::new();
    let command_noun = T(&mut poke_slab, &[D(tas!(b"cause")), D(0x0)]);
    poke_slab.set_root(command_noun);
    if let Err(e) = nockapp.poke(SystemWire.to_wire(), poke_slab).await {
        warn!("kernel boot poke failed (non-fatal): {e:?}");
    }
    // Keep the kernel handle alive for the lifetime of the process (a supervised NockApp owns
    // its state dir). We intentionally do not call `nockapp.run()`: this app has no Hoon-side
    // IO drivers; its work is the Rust heartbeat loop below.
    let _nockapp = nockapp;

    info!("hello from NockApp: hello-basic booted; heartbeat every {}s", TICK_INTERVAL.as_secs());

    // Graceful shutdown: nockd SIGTERMs on stop/restart (then SIGKILL after a grace period).
    // Leave the loop cleanly on SIGTERM (and Ctrl-C when run by hand) and exit 0.
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    let mut ticks: u64 = 0;
    let mut ticker = tokio::time::interval(TICK_INTERVAL);

    loop {
        tokio::select! {
            _ = sigterm.recv() => {
                info!("received SIGTERM; shutting down cleanly");
                break;
            }
            _ = tokio::signal::ctrl_c() => {
                info!("received Ctrl-C; shutting down cleanly");
                break;
            }
            _ = ticker.tick() => {
                ticks += 1;
                // The one clean, greppable metric line nockd's status-cmd scrapes.
                println!("metric: ticks={ticks}");
            }
        }
    }

    Ok(())
}
