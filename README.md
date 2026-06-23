# nockd

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

The **Phase 0 spine** runs: `nockd serve` supervises content-addressed artifacts with
crash-restart and a SQLite registry, exposes an HTTP control API + a minimal browser
dashboard, and `nockd deploy/ps/logs/restart/stop` drive it.

```sh
cargo build
nockd serve &                                  # daemon + dashboard on http://127.0.0.1:4490
nockd deploy myapp --bin ./target/release/myapp --jam ./out.jam --restart always
nockd ps
nockd logs myapp
```

Not yet wired (see DESIGN §12 / open questions): client-side `nockup` build, gRPC health
gate, molt upgrades, secrets, Unix-socket control transport, auth. Phase 0 deploy takes a
**prebuilt** binary + kernel.

## License

MIT
