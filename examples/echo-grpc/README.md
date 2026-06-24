# echo-grpc

A minimal but complete NockApp that demonstrates the **private NockApp gRPC surface** with a
**poke → peek echo roundtrip**: send a value into kernel state via a gRPC *poke*, then read
it back via a gRPC *peek*. The server side (`listen`) is deployed and supervised under
[`nockd`](../../); a small client (`talk`) drives the roundtrip.

It is a companion to [`chain-watch`](../chain-watch/) — where chain-watch exercises the
*public* Nockchain gRPC (reads the chain), echo-grpc exercises the *private/admin* NockApp
gRPC (poke/peek into your own kernel). The canonical build/deploy gotchas live in
[`chain-watch/RECIPE.md`](../chain-watch/RECIPE.md); this example's NEW findings are in
[`RECIPE.md`](./RECIPE.md).

## What it does

Two binaries (mirroring the `grpc` template):

- **`listen`** — the **server** NockApp you deploy. It boots the `listen` Hoon kernel from
  `out.jam` and attaches `nockapp_grpc::services::private_nockapp::grpc_server_driver`, which
  serves the kernel's `+poke` / `+peek` over a **private, local, plaintext gRPC** endpoint
  (default `127.0.0.1:5561`). The kernel state holds a single `val=@t`:
  - a gRPC **poke** carrying the cause `[%echo val]` stores `val` and acks;
  - a gRPC **peek** on path `/echo` returns the stored value.
  On each poke the kernel emits `[%echoed val]`; a tiny effect-counting driver prints
  `metric: pokes=<N>` so `nockd`'s status command surfaces the echo count as a `POKES` column.

- **`talk`** — the **client** that proves the roundtrip. It connects to the server's private
  gRPC address, pokes a value, peeks `/echo`, prints the echoed value, and exits non-zero if
  the peeked value doesn't match what it poked (so it doubles as a self-test).

### The gRPC address

The private gRPC surface is the app's **private/admin** path — local plaintext, intended for
a trusted host (use an SSH tunnel / VPN to reach it remotely). This demo uses
**`127.0.0.1:5561`**. The server binds it (`--grpc-addr`), and `nockd` health-probes the same
address (`--health-addr`) via the tonic health service the private gRPC server exposes — so
the app reports **`serving`** health.

## Architecture

A NockApp = a Rust wrapper that reads `out.jam` and boots a Hoon kernel, plus the kernel
itself (`hoon/app/listen.hoon` wrapped by `hoon/common/wrapper.hoon`). The whole echo
protocol is the kernel's `+poke`/`+peek`; the private gRPC server driver is the bridge:

```
talk:  poke [%echo "hi"]  --gRPC poke-->  server  --handle.poke-->  +poke  (stores "hi")
talk:  peek /echo         --gRPC peek-->  server  --handle.peek-->  +peek  (returns "hi")
                          <----------- jammed [~ ~ "hi"] -----------
```

A gRPC poke's JAM payload *is* the `cause` the kernel sees; a gRPC peek's JAM path *is* the
`path` the kernel sees. The peek result is the full kernel `+peek` output `[~ ~ val]`, which
the server jams and ships back; `talk` cues it and pulls out `val`.

## Build

`nockup` resolves the project from the **parent** directory by package name (see the gotcha
in `chain-watch/RECIPE.md`), so build it like this:

```sh
cd examples            # the PARENT of echo-grpc/
nockup project build echo-grpc
```

This produces `echo-grpc/target/release/{listen,talk}` and — because there are **two
binaries** — `echo-grpc/listen.jam` and `echo-grpc/talk.jam` (there is **no** `out.jam` in
multi-bin mode; see RECIPE.md).

## Deploy

The intended path is project-mode with `nockd.toml`, which names `bin_target = "listen"` so
nockd builds via nockup and ships `target/release/listen` + `listen.jam`:

```sh
export PATH="$PATH:/path/to/nockd/target/release"
nockd serve &        # if not already running
nockd key gen        # once: builder identity → "verified"

cd examples/echo-grpc
nockd deploy -f nockd.toml
```

Equivalent without the manifest — note `--bin-target` selects the bin, no `out.jam` copy:

```sh
nockd deploy echo-grpc \
  --project . --bin-target listen \
  --restart always \
  --health-addr 127.0.0.1:5561 \
  --status-label POKES \
  --status-cmd "grep -aoE 'pokes=[0-9]+' | tail -1 | grep -aoE '[0-9]+'" \
  -- --grpc-addr 127.0.0.1:5561
```

To deploy a prebuilt artifact instead, stage the server kernel as `out.jam`
(`cp listen.jam out.jam`) and pass `--bin ./target/release/listen --jam ./out.jam`.

## The poke → peek roundtrip (the proof)

With `listen` deployed and serving on `127.0.0.1:5561`, drive the roundtrip with `talk`:

```sh
cd examples/echo-grpc
./target/release/talk --grpc-addr http://127.0.0.1:5561 --value "hello over grpc"
```

Sample output:

```
POKE  [%echo "hello over grpc"] -> acked=true
PEEK  /echo -> "hello over grpc"
ROUNDTRIP OK: poked "hello over grpc" == peeked "hello over grpc"
```

Poke a different value and peek it back to see the echo update:

```sh
./target/release/talk --grpc-addr http://127.0.0.1:5561 --value "second value 42"
# POKE  [%echo "second value 42"] -> acked=true
# PEEK  /echo -> "second value 42"
# ROUNDTRIP OK: poked "second value 42" == peeked "second value 42"
```

Each poke increments the `POKES` status. Verify the whole thing under `nockd`:

```sh
nockd ps                # echo-grpc → running · serving · verified · POKES <N>
nockd logs echo-grpc    # echo-grpc: poke %echo <- '...' / peek /echo -> '...' / metric: pokes=<N>
```

Observed:

```
NAME       STATE    HEALTH   VERIFIED  PID    ENDPOINT  STATUS
echo-grpc  running  serving  verified  40630  —         POKES 2
```

- **running + serving + verified** — the private gRPC health gate reports `serving`, the
  builder key self-signed the artifact (`verified`).
- **POKES** — increments by one per gRPC poke served.
- `SIGTERM` (nockd stop/restart) is handled cleanly — the log shows
  `received SIGTERM; shut down cleanly`. The kernel state persists across restarts.

## Files

- `nockapp.toml` — project manifest (package + `grpc` template).
- `Cargo.toml` — Rust deps; two bins (`listen`, `talk`); pins the nockchain crates to the
  `6d29078` rev that matches the current `services::private_nockapp` gRPC API, plus `rustls`
  with the `ring` provider (see RECIPE.md).
- `rust-toolchain.toml` — pins the nightly the nockchain crates require.
- `src/listen.rs` — the server wrapper: boots the kernel, attaches the private gRPC server
  driver + the poke-counting metric driver, handles SIGTERM cleanly.
- `src/talk.rs` — the client: poke a value, peek it back, assert the roundtrip.
- `hoon/app/listen.hoon` — the echo kernel (`+poke` stores, `+peek` returns).
- `hoon/app/talk.hoon` — trivial placeholder kernel (the `talk` bin needs one to build).
- `hoon/lib/lib.hoon`, `hoon/common/wrapper.hoon` — shared lib + the standard moat wrapper.
- `nockd.toml` — the declarative deploy manifest (intended UX; deploy prebuilt for now).
- `RECIPE.md` — NEW rough edges hit building this example (private gRPC API, two-bin jam).
