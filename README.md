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

# …or ship a prebuilt artifact:
nockd deploy myapp --bin ./target/release/myapp --jam ./out.jam --restart always

nockd ps                                       # fleet + state + health
nockd dash                                     # live TUI (↑/↓ select · r/s/x · q quit)
nockd logs myapp
```

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
