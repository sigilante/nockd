# nockd

**Status ~2026.6.22:  In active development.**

A self-hostable deployment platform for [NockApps](https://github.com/nockchain/nockchain).

![](./img/hero.jpg)

`nockd` runs the clean, content-addressed artifact that the Nockup toolchain produces — a
Rust wrapper binary plus a Nock kernel (`out.jam`) — as a supervised, stateful, long-lived
service that attaches to a Nockchain node over gRPC. It ships as a single static binary
that is both daemon and client, and serves a browser dashboard from that same binary.

It is useful on one box with zero control plane, and self-host-first by design: nothing
depends on a company staying solvent.

```sh
nockd serve              # run the daemon (supervisor + API + dashboard)
nockd deploy myapp       # build, ship, and run a NockApp as a supervised service
nockd ps                 # list apps + status
nockd logs myapp -f      # follow live logs
nockd rollback myapp     # one-step rollback to the previous artifact
```

## Status

Early. The authoritative design reference is **[DESIGN.md](./DESIGN.md)** — read it first;
it is the bedrock truth this codebase follows.

What runs today: `nockd serve` supervises content-addressed artifacts with crash-restart
and a SQLite registry, probes app health over gRPC, exposes an HTTP control API, and is
driven by `nockd deploy/ps/logs/restart/stop` plus a live `nockd dash` TUI.

```sh
cargo build

nockd serve &                                  # daemon on http://127.0.0.1:4490

# Build with the client-side toolchain (the daemon never compiles):
nockd deploy --project ./myapp --restart always --health-addr 127.0.0.1:5599

# …or ship a prebuilt artifact (template app: binary + out.jam):
nockd deploy myapp --bin ./target/release/myapp --jam ./out.jam --restart always

# …or a binary-only app that embeds its kernel — e.g. a nockchain OBSERVER,
# with a custom status that surfaces block height at a glance:
nockd deploy nockchain --bin /path/to/nockchain --restart always \
  --health-addr 127.0.0.1:5555 \
  --status-label BLOCK \
  --status-cmd 'grep -oE "block_height=[0-9]+" | tail -1 | grep -oE "[0-9]+"' \
  -- --bind-private-grpc-addr 127.0.0.1:5555
#   (no --jam; observer = no --mine; dials default peers to sync; the node's
#    cwd-relative ./.data.nockchain state lands inside nockd's per-app state dir.
#    nockchain emits ANSI color even when piped; the dashboard log panel RENDERS it
#      (I/W levels) + highlights NockApp verbs.
#    --status-cmd runs every 5s with the ANSI-stripped recent log piped to STDIN,
#      cwd=state dir, and NOCKD_LOG/NOCKD_STATE_DIR/NOCKD_ENDPOINT/NOCKD_ADMIN_ADDR set;
#      its first stdout line shows in ps, the TUI, and the dashboard tile band. The
#      recipe is just a grep — nockd handles the log + ANSI for you.)

# The status command is fully general — any app, any shell pipeline. It can scrape the
# app's log (stdin), read state files (cwd = the app's state dir), or query the app's
# gRPC/HTTP via the NOCKD_ENDPOINT / NOCKD_ADMIN_ADDR env vars. Examples:
#   wallet balance : --status-label BAL  --status-cmd 'grep -oE "balance=[0-9]+" | tail -1 | grep -oE "[0-9]+"'
#   http server    : --status-label REQ  --status-cmd 'curl -s "$NOCKD_ENDPOINT/metrics" | jq -r .requests'
#   state file     : --status-label SEQ  --status-cmd 'cat ./.data.myapp/height 2>/dev/null'

nockd ps                                       # fleet + state + health
nockd dash                                     # live TUI (↑/↓ select · r/s/x · q quit)
nockd logs nockchain
```

### Declarative deploy (`nockd.toml`)

Put the whole deployment in a version-controllable manifest and deploy with `-f`:

```toml
# nockd.toml
[deploy]
app         = "nockchain"
bin         = "/path/to/nockchain"     # or: project = "./myapp" to build via nockup
restart     = "always"                 # always | on-failure | never
args        = ["--bind-private-grpc-addr", "127.0.0.1:5555"]
health_addr = "127.0.0.1:5555"
endpoint    = "mainnet-rpc"            # named endpoint (see the registry)

[deploy.status]
label = "BLOCK"
cmd   = "grep -oE 'block_height=[0-9]+' | tail -1 | grep -oE '[0-9]+'"
```

```sh
nockd deploy -f nockd.toml
```

### Endpoints

Named Nockchain RPC targets, with live reachability + lag. Apps reference an endpoint by
name (the `endpoint` field), so the URL can change without redeploying.

```sh
nockd endpoint add mainnet-rpc http://<host>:5555
nockd endpoint ls                 # NAME · REACH · URL · LAG · APPS
nockd endpoint rm mainnet-rpc
```

They're also managed from the dashboard's **ENDPOINTS** screen (add / remove / live
reachability tiles).

`nockchain-wallet` is **not** a fit: it's a one-shot command tool (pokes once, exits), not
a long-lived service to supervise.

- **Build/run split (principle 7):** `--project` shells out to `nockup`; the daemon only
  runs artifacts and needs no toolchain.
- **Health (DESIGN §5.3, OQ3):** with `--health-addr`, nockd probes the app's private gRPC
  health and surfaces `serving`/`unreachable`. The *swap-gate* form (block/rollback on
  unhealthy upgrades) lands with molt in Phase 2.

Not yet wired (DESIGN §12 / open questions): molt upgrades, secrets, signed
attestations, Unix-socket control transport, auth. The web dashboard is incoming
separately; `nockd dash` is the interim.

## License

MIT
