# nockd

**Status 2026.6: active development — the full deploy lifecycle and browser dashboard are
shipped; secrets, auth, and state-preserving upgrades are next.**

A self-hostable deployment platform for [NockApps](https://github.com/nockchain/nockchain).

![](./img/hero.jpg)

`nockd` runs the clean, content-addressed artifact that the Nockup toolchain produces — a
Rust wrapper binary plus a Nock kernel (`out.jam`) — as a supervised, stateful, long-lived
service that can attach to a Nockchain node over gRPC. It ships as a **single static binary**
that is both daemon and client, and serves a **browser dashboard** from that same binary.

It is useful on one box with zero control plane, and self-host-first by design: nothing
depends on a company staying solvent.

The authoritative architecture reference is **[DESIGN.md](./DESIGN.md)**.

---

## Install

```sh
cargo build --release          # produces ./target/release/nockd
export PATH="$PATH:$PWD/target/release"
```

`nockd` is one binary. `nockd serve` is the daemon; every other subcommand is a client of it.

To build NockApps from source (project-mode deploys, below) you also need the **nockup**
toolchain (it lives in [nockchain/nockchain](https://github.com/nockchain/nockchain)). The
daemon itself never compiles — builds always happen client-side (DESIGN principle 7) — so if
you only deploy **prebuilt** artifacts you don't need nockup at all.

---

## Quickstart — deploy your first NockApp

A NockApp you can deploy is a project directory containing a build manifest (`nockapp.toml`)
and a deploy manifest (`nockd.toml`). Ready-made examples live in
**[sigilante/nockapps-pack](https://github.com/sigilante/nockapps-pack)** (blog, minesweeper,
a price API, a chain-height watcher, …) — grab one to start.

```sh
# 1. Start the daemon (dashboard + API on http://127.0.0.1:4490).
nockd serve &

# 2. Deploy an app from its directory. project-mode builds it with nockup, then ships +
#    runs the artifact. nockd.toml carries all the config (see below).
cd nockapps-pack/minesweeper
nockd deploy -f nockd.toml

# 3. Watch it.
nockd ps                       # fleet: state · health · verified · CPU · RSS · status
nockd logs minesweeper         # recent log tail (--lines N; live streaming is in the dashboard)
```

Then open **http://127.0.0.1:4490** for the dashboard. For an HTTP app like minesweeper,
its tile shows an **"Open app ↗"** link straight to the running page.

That's the whole loop: write/grab an app → `nockd deploy -f nockd.toml` → it's a supervised
service with crash-restart, health, logs, metrics, and one-click rollback.

---

## The deploy manifest (`nockd.toml`)

Declarative, version-controllable deploy config — everything `nockd deploy -f` needs.

```toml
[deploy]
app         = "myapp"            # name (shows in ps/logs/dashboard; names the state dir)

# --- one build/ship mode ---
project     = "."               # build this dir with nockup (the intended UX), OR…
# bin       = "target/release/myapp"   #   …ship a prebuilt binary…
# jam       = "out.jam"                #   …and kernel (omit jam for binary-only apps)
# bin_target = "listen"          # multi-bin project? name the [[bin]] to ship
                                 #   (→ target/release/listen + listen.jam, not out.jam)

restart     = "always"          # always | on-failure | never
args        = []                # passed through to the app process

port        = 8085              # HTTP app? declare its port ONCE. nockd exports it as
                                #   NOCKD_APP_PORT and substitutes {port} in args; the
                                #   dashboard links to localhost:<port>. (Read it in your
                                #   app to bind — don't hardcode a port on both sides.)

icon        = "icon.svg"        # dashboard icon: a path (CLI base64-encodes it) or a
                                #   data: URI. Shown in the table/tiles/detail.

endpoint    = "mainnet-rpc"     # attach to a named Nockchain RPC endpoint (see Endpoints).
                                #   {endpoint} in args is replaced with the URL at launch,
                                #   and NOCKD_ENDPOINT_URL is set.

health_addr = "127.0.0.1:5555"  # app's private/admin gRPC addr for the health probe.

[deploy.status]                 # a custom one-line status metric for ps/dashboard
label = "BLOCK"
cmd   = "grep -oE 'block_height=[0-9]+' | tail -1 | grep -oE '[0-9]+'"
```

Note the keys are underscored (`health_addr`, `bin_target`), matching the struct exactly.

**The status command** is fully general: nockd runs it every 5s with the app's
**ANSI- and NUL-stripped recent log piped to stdin**, `cwd` = the app's state dir, and
`NOCKD_LOG`/`NOCKD_STATE_DIR`/`NOCKD_ENDPOINT_URL`/`NOCKD_ADMIN_ADDR` set. Its first stdout
line becomes the app's status. So a recipe is usually just a `grep` — log a clean metric line
like `metric: requests=42` and scrape it.

Every field also has a CLI flag (`nockd deploy <name> --bin … --web-port … --icon … …`); the
manifest is just the version-controllable form.

---

## Deploy modes

```sh
# Project mode (intended): build from source with nockup, then ship + run.
nockd deploy -f nockd.toml
nockd deploy myapp --project ./myapp --restart always

# Prebuilt: ship a template app's binary + kernel (no nockup needed).
nockd deploy myapp --bin ./target/release/myapp --jam ./out.jam --restart always

# Binary-only: an app that embeds its kernel (no out.jam) — e.g. a nockchain OBSERVER.
nockd deploy nockchain --bin /path/to/nockchain --restart always \
  --health-addr 127.0.0.1:5555 \
  --status-label BLOCK \
  --status-cmd 'grep -oE "block_height=[0-9]+" | tail -1 | grep -oE "[0-9]+"' \
  -- --bind-private-grpc-addr 127.0.0.1:5555
#   (observer = no --mine; dials default peers to sync; its cwd-relative ./.data.nockchain
#    state lands inside nockd's per-app state dir, isolated automatically.)
```

`nockchain-wallet` is **not** a fit — it's a one-shot command tool (pokes once, exits), not a
long-lived service to supervise.

---

## Lifecycle & observability

```sh
nockd ps                       # fleet + state + health + verified + CPU/RSS + status
nockd logs myapp               # recent log tail (--lines N); follow live in the dashboard / TUI
nockd reload myapp             # re-read nockd.toml, re-apply config, restart — NO rebuild
nockd rollback myapp           # revert to the previous artifact (bytes are retained) + restart
nockd restart|stop|start myapp # lifecycle
nockd down                     # stop all apps (keeps them deployed)
nockd up                       # start all stopped apps
nockd dash                     # interim live TUI (↑/↓ select · r/s/x · q quit)
```

- **Reload** is for config edits (port, args, status, endpoint, icon): the daemon re-reads
  the manifest it deployed from and re-applies it to the *current* artifact. Changed the
  *code*? Run `nockd deploy -f` again to rebuild and ship a new artifact.
- **Rollback** flips back to the previous artifact you ran (config untouched). The
  content-addressed store keeps old artifacts, so this is instant. The dashboard's Artifact
  panel shows the deploy history.

---

## The dashboard

Served from the same binary at **http://127.0.0.1:4490** — vanilla, no build step.

- **Fleet** — every app as a table or tiles: status glyph, icon, artifact, endpoint, uptime,
  health, verified, your custom status metric, and an **"Open app ↗"** relay link for HTTP
  apps. Click through for live logs (SSE), an event timeline, the artifact + deploy history,
  and per-app actions (Open · Reload · Rollback · Restart · Start · Stop).
- **Metrics** — a fleet-wide resource overview sorted by memory (the OOM-watch lens): total
  RSS/CPU, the heaviest app, and a per-app CPU% + RSS bar. Sampled every 5s.
- **Endpoints** — named Nockchain RPC targets with live reachability, latency, and chain
  height (add/remove from here too).

---

## Endpoints

Named Nockchain RPC targets, with live reachability + lag + chain height. Apps reference an
endpoint **by name**, so the URL can change without redeploying.

```sh
nockd endpoint add mainnet-rpc http://<host>:5555
nockd endpoint ls            # NAME · REACH · URL · LAG · HEIGHT · BEHIND · APPS
nockd endpoint rm mainnet-rpc
```

Reachability is a real gRPC handshake + health check (true round-trip latency), and for
Nockchain endpoints nockd reads the **chain-tip block height** from the public metrics
service — so `endpoint ls` and the dashboard show each endpoint's height and how many blocks
it is **behind** the most-current one. An app attaches via `endpoint = "mainnet-rpc"`; nockd
injects the URL at launch (`{endpoint}` in args → the URL, plus `NOCKD_ENDPOINT_URL`), so
re-pointing an endpoint (`nockd endpoint add <name> <new-url>` + restart) needs no redeploy.

---

## Verified deploys (attestations)

A deploy carries a **build attestation** — a signed statement (ed25519) binding the
artifact's hashes to a builder identity. The daemon verifies it (signature + hashes + trusted
builder) and shows each app as **verified / unverified / drift** in `ps`, the API, and the
dashboard.

```sh
nockd key gen                 # create your builder identity (once)
nockd deploy myapp --bin …    # auto-signs a self-attestation → verified
nockd deploy myapp --bin … --no-attest           # → unverified
nockd deploy myapp --bin … --attestation a.json  # attach someone else's attestation
nockd trust add <pubkey>      # trust another builder (e.g. an org/release key)
nockd trust ls
```

The daemon trusts its own builder key by default, so your self-built apps show **verified**.
For a binary like nockchain this is a *supply-chain* attestation ("built by a trusted
builder"); the deepest level — reproducible rebuild + compare (mirroring typhoon's
`generate --check`) — is a toolchain-side follow-up.

---

## How it works

- **Build/run split (principle 7):** project-mode shells out to `nockup` client-side; the
  daemon only runs artifacts and needs no toolchain.
- **Content-addressed store:** artifacts are `blake3`-hashed (binary + kernel). Identical
  bytes dedup; old artifacts are retained, which is what makes rollback instant.
- **Supervision:** apps run in their own process group (a Ctrl-C of the daemon doesn't
  signal them), with crash-restart + backoff, graceful SIGTERM→SIGKILL stop, and startup
  orphan cleanup. State lives in a per-app dir (`cwd` = that dir) under a single-daemon lock.
- **Health (DESIGN §5.3):** with `health_addr`, nockd probes the app's private gRPC health
  and surfaces `serving`/`unreachable`. The *swap-gate* form (block/rollback on unhealthy
  upgrades) lands with molt (Phase 2).

---

## Status / roadmap

**Done:** supervisor + content store + SQLite registry + control API; deploy / reload /
rollback / restart / stop / start; endpoint registry with live height; verified deploys
(attestations + trust); browser dashboard (Fleet / Metrics / Endpoints) with live logs,
relay links, icons, and resource metrics.

**Next:** encrypted secrets store, then **molt** — state-preserving kernel upgrade (the
Phase-2 headline). Also pending: dashboard auth / exposure hardening (the control API is
currently a localhost TCP listener with no auth — keep it local-only for now), backup/restore,
and reproducible-build verification. See [DESIGN.md §12](./DESIGN.md) for the full roadmap.

## License

MIT
