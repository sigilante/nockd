# hello-basic — the minimal supervised NockApp

The smallest thing that proves the NockApp **build → deploy → observe** loop with the least
possible code. No HTTP, no chain, no TLS. It boots the trivial `basic` Hoon kernel (so it is
a real, supervised NockApp the way `nockd` expects) and then runs a tiny Rust loop that emits
a heartbeat metric forever:

```
metric: ticks=1
metric: ticks=2
metric: ticks=3
...
```

`nockd`'s status command scrapes that line and surfaces the climbing counter as the **TICKS**
column in `nockd ps`. That is the whole demo: deploy it, watch TICKS climb.

This is [`chain-watch`](../chain-watch/) **minus the chain** — same structure (trivial kernel
booted from `out.jam` + a long-lived Rust loop that logs one greppable metric line), but
instead of polling an RPC for the block height it just increments a tick counter. If you want
the chain-reading version, read that example next.

## What it is made of

- `hoon/` — the stock `basic` template kernel (does nothing but boot; all behavior is in Rust).
- `src/main.rs` — boots the kernel, then a tokio loop that ticks every 5s and prints
  `metric: ticks=<N>`. Handles SIGTERM/Ctrl-C cleanly (exits 0).
- `nockd.toml` — project-mode deploy manifest with the TICKS status metric.
- `Cargo.toml` / `rust-toolchain.toml` — pinned to nockchain rev `6d29078…` and nightly
  `2026-04-03` (see RECIPE.md for why both pins are load-bearing).

## The one lesson worth remembering

The `basic` template's stock `main.rs` pokes the kernel **once and then exits**. That is not
a service. Under `nockd` with `restart = always`, a process that exits is a process that gets
**restarted forever — a crash loop**. A supervised NockApp must **stay alive**. So this app,
after booting, enters a loop that only leaves on SIGTERM (nockd's stop/restart signal) and
exits cleanly. See RECIPE.md for the full story.

## Build

`nockup project build` resolves the project as a *subdirectory by name*, so build from the
parent dir (or pass an absolute path):

```sh
export PATH="$PATH:/Users/neal/.nockup/bin"
cd /Users/neal/zorp/nockapps/examples
nockup project build hello-basic
# → out.jam + target/release/hello-basic
```

## Deploy (project mode) + watch TICKS climb

`nockd` is the daemon (`export PATH="$PATH:/Users/neal/zorp/nockd/target/release"`). Deploy
registers/builds the artifact; restart swaps it in:

```sh
cd /Users/neal/zorp/nockapps/examples/hello-basic
nockd deploy -f nockd.toml      # project-mode build via nockup, registers the artifact
nockd restart hello-basic       # swap the new artifact in and start it
```

Now watch the heartbeat climb:

```sh
nockd ps | grep hello-basic
# hello-basic  running  unknown  verified  <pid>  —  TICKS 3
sleep 12
nockd ps | grep hello-basic
# hello-basic  running  unknown  verified  <pid>  —  TICKS 5   ← climbing, same PID
```

And the raw heartbeat lines:

```sh
nockd logs hello-basic | tail
# metric: ticks=134
# metric: ticks=135
# metric: ticks=136
```

Stop / restart cleanly (never `pkill` — nockd runs the app from its artifact path, not name):

```sh
nockd restart hello-basic
nockd stop hello-basic
```

## Definition of done (verified)

- `nockup project build hello-basic` → clean build, `out.jam` produced.
- `nockd deploy -f nockd.toml` → project-mode build succeeds; artifact registered.
- `nockd ps` → **running**, **verified**, **TICKS** incrementing over time (observed 3 → 5 → 8).
- `nockd logs hello-basic` → the `metric: ticks=<N>` heartbeat lines.
- Clean SIGTERM (exit 0) confirmed by direct smoke test.
