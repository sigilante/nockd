# RECIPE — building & deploying `hello-basic` end to end

The **canonical transcript** for the smallest possible NockApp: boot the trivial `basic`
kernel and emit a heartbeat metric on a loop. No HTTP, no chain, no TLS. It exists to prove
the build → deploy → observe loop with the least code, and to make the **one-shot → long-lived
fix** (the central lesson for any NockApp service) impossible to miss.

This is [`chain-watch`](../chain-watch/) with the chain removed. Where chain-watch polls an
RPC for the block height, hello-basic increments a tick counter — everything else (trivial
kernel booted from `out.jam`, long-lived Rust loop, one greppable `metric:` line, clean
SIGTERM, project-mode deploy) is identical. Read chain-watch's RECIPE.md for the chain/TLS
rough edges that do **not** apply here.

Verified on macOS (darwin, aarch64-apple-darwin) on 2026-06-24.

---

## ⭐ THE KEY LESSON — one-shot poke-and-exit → long-lived service

The `basic` template's stock `main.rs` boots the kernel, pokes it **once**, and **returns from
`main` — the process EXITS.** That is a one-shot, not a service.

Under `nockd` a deploy carries a restart policy. With `restart = always` (what you want for a
heartbeat), a process that exits is immediately **restarted** — so a one-shot app becomes a
**crash loop**: boot, exit, boot, exit, forever, churning PIDs. The TICKS column would never
climb because every "tick=1" comes from a fresh process.

**The fix is the whole point of this example:** after booting, do not return. Enter a loop
that does the app's work (here: increment a counter, print `metric: ticks=<N>` every 5s) and
only leaves the loop on **SIGTERM** (nockd's stop/restart signal) or Ctrl-C, then exits 0.

```rust
let mut sigterm = tokio::signal::unix::signal(SignalKind::terminate())?;
let mut ticks: u64 = 0;
let mut ticker = tokio::time::interval(Duration::from_secs(5));
loop {
    tokio::select! {
        _ = sigterm.recv()            => { info!("received SIGTERM; shutting down cleanly"); break; }
        _ = tokio::signal::ctrl_c()   => { info!("received Ctrl-C; shutting down cleanly"); break; }
        _ = ticker.tick()             => { ticks += 1; println!("metric: ticks={ticks}"); }
    }
}
Ok(())  // clean exit 0
```

A correctly long-lived app shows ONE stable PID in `nockd ps` and a climbing metric. If you
see the PID changing every few seconds, you shipped the one-shot.

---

## 0. Environment

```sh
export PATH="$PATH:/Users/neal/.nockup/bin"                 # nockup + hoonc (client-side build)
export PATH="$PATH:/Users/neal/zorp/nockd/target/release"   # nockd (NOT on PATH by default)
```

A `nockd serve` daemon was already running on `127.0.0.1:4490` with a builder key and the
**project-mode fix** (see §3). No endpoint and no port are needed — hello-basic talks to
nothing.

---

## 1. Scaffold

hello-basic reuses the trivial `basic` kernel verbatim. Rather than re-running
`nockup project init` (which nests the project in a `<name>/` subdir and leaves `rev = ""` in
Cargo.toml — see chain-watch RECIPE.md, ROUGH EDGES 1 & 2), the hoon tree (`hoon/app/app.hoon`,
`hoon/lib/lib.hoon`, `hoon/common/wrapper.hoon`), `build.rs`, `rust-toolchain.toml`, and
`nockapp.lock` were copied from the sibling `chain-watch` example, and the manifests written
by hand. The kernel is the stock template kernel — it just needs to boot to make a valid
NockApp; the heartbeat lives entirely in Rust.

---

## 2. Cargo.toml — pin rev + nightly, drop the chain deps

- All three runtime crates pinned to `rev = "6d29078e69b64febabe3d8d20a64c06b969a16ed"` (the
  proven chain-watch rev; the `basic` template's own pin can be stale). Bumping to this rev
  brings API drift you must follow in `main.rs` (see §4).
- **No `nockapp-grpc`** and **no `rustls`**: hello-basic opens no TLS connection, so it needs
  neither the gRPC client nor chain-watch's rustls crypto-provider pin. (Those two are
  chain-watch's #2 and #6 rough edges; they simply do not arise here — a nice property of the
  minimal app.)
- `rust-toolchain.toml` pins `nightly-2026-04-03`. **Load-bearing:** without it `nockup`
  builds with the ambient toolchain and `nockvm` at this rev fails with a cryptic
  `error[E0658]: use of unstable library feature 'cold_path'`. (chain-watch RECIPE ROUGH
  EDGE 4.)

---

## 3. Build

`nockup project build` resolves the project as a **subdirectory by name** — so `build` (no
arg) or `build .` from inside the project both fail. Build from the PARENT dir with the name
(or pass an absolute path):

```sh
cd /Users/neal/zorp/nockapps/examples
nockup project build hello-basic
# ✓ Cargo build completed successfully!
# hoonc: output written successfully to '.../hello-basic/out.jam'
# ✓ Hoon compilation completed successfully!
```

Result: `out.jam` (~569K) + `target/release/hello-basic` (~8M Mach-O arm64). One iteration —
no build errors (the API-drift fixes were applied up front per the GOTCHAS, and there are no
chain/TLS deps to trip over).

> Harmless build noise (same as chain-watch): `error: invalid character '{' in package name:
> '{{project_name}}'` (cargo scanning the template dir in the nockchain checkout, not your
> project) and `warning: unused import: 'fs'` from the template's `build.rs`. Build succeeds.

---

## 4. main.rs — the API-drift fixes (applied up front)

Bumping to rev `6d29078…` changes two signatures from what the `basic` template's `main.rs`
assumes; apply both or the build fails:

- `boot::setup` takes `cli` directly, **not** `Some(cli)`:
  ```rust
  let mut nockapp: NockApp = boot::setup(&kernel, cli, &[], "hello-basic", None).await?;
  ```
- `NockApp<J>` is generic over a `Jammer`; `NockApp` with no explicit `J` infers fine here.

The kernel is booted from `out.jam` (cwd-relative; nockd stages `out.jam` into the app's state
dir, which is also the app's cwd), poked once at boot to exercise it, then the handle is kept
alive while the heartbeat loop runs. We do **not** call `nockapp.run()` — this app has no
Hoon-side IO drivers; its work is the Rust loop.

---

## 5. Smoke test the binary directly (before nockd)

```sh
cd examples/hello-basic
./target/release/hello-basic &   # run ~13s, then: kill -TERM <pid>
```

Output:

```
... hello_basic: hello from NockApp: hello-basic booted; heartbeat every 5s
metric: ticks=1
metric: ticks=2
metric: ticks=3
... hello_basic: received SIGTERM; shutting down cleanly
```

Exit code **0** on SIGTERM. Ticks increment. No chain, no TLS, no panics.

---

## 6. Deploy (project mode — now works)

chain-watch's RECIPE recorded project-mode deploy as BROKEN (nockd ran `nockup project build`
with no arg from inside the dir, which nockup rejected). **That is fixed in the running
daemon**: nockd now passes the absolute project path to `nockup project build`, so project
mode round-trips. (Confirmed: the deploy below built and registered with no `--bin`/`--jam`
fallback.)

```sh
cd examples/hello-basic
nockd deploy -f nockd.toml
# ✓ Cargo build completed successfully!
# ✓ Hoon compilation completed successfully!
# deployed hello-basic
#   artifact ade6889803d60ab82f0d751b8da6b701caaa682015c7a02e309ac41191bcaec7
#   kernel   c0c7804ff125f323f69256eefbe0296cb11848247af72a20bd4197c41d09233d

nockd restart hello-basic        # deploy registers the artifact; restart swaps it in & starts
# restarted hello-basic
```

---

## 7. Verify it's working — TICKS climbs

```sh
nockd ps | grep hello-basic
# hello-basic  running  unknown  verified  99820  —  TICKS 3
sleep 12
nockd ps | grep hello-basic
# hello-basic  running  unknown  verified  99820  —  TICKS 5    ← same PID, climbing
sleep 12
nockd ps | grep hello-basic
# hello-basic  running  unknown  verified  99820  —  TICKS 8
```

- **running + verified** ✅ (self-signed attestation, trusted builder key).
- **TICKS climbing 3 → 5 → 8** ✅ with a **stable PID** — the long-lived loop works; no crash loop.

The heartbeat lines in the log:

```sh
nockd logs hello-basic | tail
# metric: ticks=134
# metric: ticks=135
# metric: ticks=136
```

Status recipe (no `-a` needed — nockd strips NUL bytes from the boot log now, fixing the BSD
grep issue chain-watch hit; see chain-watch RECIPE ROUGH EDGE 8):

```toml
[deploy.status]
label = "TICKS"
cmd   = "grep -oE 'ticks=[0-9]+' | tail -1 | grep -oE '[0-9]+'"
```

---

## 8. NEW rough edges found building hello-basic

1. **`nockd logs <app>` runtime stdout does not survive a pipe / redirect.** `nockd logs
   hello-basic` printed the live `metric: ticks=...` lines fine to an interactive terminal
   (and to `... | tail`), but `nockd logs hello-basic | grep metric`, `... > file`, and even a
   `script(1)` pty capture returned **nothing** for the runtime stdout lines — the pty capture
   only contained the captured **kernel-boot tracing** stream, not the app's `println!`
   output. The app's stdout heartbeat and the captured tracing log appear to be two streams
   that `nockd logs` muxes differently depending on whether stdout is a tty. This did not
   affect the demo (the **TICKS** status column is populated from the same recent-log buffer
   the status-cmd greps, and it climbs correctly — proving the lines ARE captured), but it
   means **scripting against `nockd logs` output is unreliable**; assert on `nockd ps`'s status
   column instead. Worth a nockd fix (line-buffer + tee both streams regardless of tty), or at
   least a doc note.

2. **Confirmation that project-mode deploy now round-trips** (was chain-watch's central
   pathfinder finding as BROKEN). Not a new bug — a resolved one — but worth recording: the
   `--bin`/`--jam` fallback is no longer required for single-bin projects.

Everything else (toolchain pin, build-from-parent-dir, the API drift, the harmless template
`{{project_name}}` noise) is already documented in chain-watch's RECIPE and reproduced here.

---

## 9. Cleanup conventions

- Stop/restart via nockd: `nockd restart hello-basic` / `nockd stop hello-basic`.
- **Never** `pkill -f hello-basic` — nockd runs the app from its **artifact path**, not its
  name, so pkill-by-name misses it and fights the supervisor.
