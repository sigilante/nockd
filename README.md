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

Early design. The authoritative design reference is **[DESIGN.md](./DESIGN.md)** — read it
first; it is the bedrock truth this codebase follows.

## License

MIT
