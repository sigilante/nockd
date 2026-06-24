# RECIPE — building & deploying `echo-grpc` end to end

This is the honest build/deploy transcript for `echo-grpc`, a NockApp that demonstrates the
**private NockApp gRPC surface** with a **poke → peek echo roundtrip**. It is a companion to
[`../chain-watch/RECIPE.md`](../chain-watch/RECIPE.md) — read that first; it covers the
shared gotchas (nockup nesting, the `rev=""` placeholder, the ambient-toolchain `cold_path`
error, the parent-dir build invocation, the rustls double-provider panic, the broken
project-mode deploy, and the BSD-grep / NUL-byte status-cmd issue). This file records only
the things that are **NEW or different** for echo-grpc — almost all of them about the private
gRPC API at rev `6d29078` and the two-binary build.

Verified on macOS (darwin, aarch64-apple-darwin) on 2026-06-24. **Clean build rev:
`6d29078e69b64febabe3d8d20a64c06b969a16ed`** (all five nockchain crates).

---

## 0. What we built

- **Server (`listen`)** — boots the `listen` Hoon kernel and attaches
  `grpc_server_driver(addr)`, exposing the kernel's `+poke`/`+peek` over a private, local,
  plaintext gRPC endpoint on `127.0.0.1:5561`. Kernel state is one `val=@t`:
  poke `[%echo val]` stores it; peek `/echo` returns it.
- **Client (`talk`)** — connects with `PrivateNockAppGrpcClient`, pokes a value, peeks it
  back, prints the echoed value, and self-checks the roundtrip.

The base template was `grpc` (`crates/nockup/templates/grpc/`), which already has the
two-binary `listen`/`talk` split and depends on `nockapp-grpc`. But its source uses the OLD
gRPC API; the bulk of the work was porting it to the current `services::private_nockapp`
layout (see §2).

---

## 1. Scaffold (same shape as chain-watch)

The `grpc` template's pinned rev (`485e914`) is too old, and `nockup project init` nests +
leaves `rev=""` (chain-watch ROUGH EDGEs 1–3). Easiest path, as with chain-watch: create the
dir, copy `rust-toolchain.toml` + `build.rs` + `hoon/common/wrapper.hoon` from known-good
sources, and hand-write `nockapp.toml` / `Cargo.toml` / the `hoon/app/*.hoon` / `src/*.rs`.

`rust-toolchain.toml` (pin `nightly-2026-04-03`) is REQUIRED — without it you get the
`cold_path` E0658 from deep in `nockvm` (chain-watch ROUGH EDGE 4).

---

## 2. ⚠️ NEW: the private gRPC API moved (rev 485e914 → 6d29078)

The `grpc` template's `src/listen.rs` / `src/talk.rs` import from the OLD nockapp-grpc
layout and **will not compile** against a current rev. At `6d29078` the API is:

| template (old, 485e914)                              | current (6d29078)                                                        |
| ---------------------------------------------------- | ------------------------------------------------------------------------ |
| `nockapp_grpc::driver::grpc_server_driver` (no addr) | `nockapp_grpc::services::private_nockapp::grpc_server_driver(addr: SocketAddr)` |
| `nockapp_grpc::client::NockAppGrpcClient`            | `nockapp_grpc::services::private_nockapp::PrivateNockAppGrpcClient`       |
| `nockapp_grpc::NockAppGrpcServer`                    | `…::private_nockapp::PrivateNockAppGrpcServer`                            |
| `grpc::string_to_atom`, `nockapp::utils::make_tas`   | `make_tas` is `nockapp::utils::make_tas` (re-export of `nockvm::ext::make_tas`); build cords with `Atom::from_bytes` |

Key real signatures (read from
`crates/nockapp-grpc/src/services/private_nockapp/{driver,client,server}.rs`):

```rust
// driver
pub fn grpc_server_driver(addr: SocketAddr) -> IODriverFn;       // ← takes a SocketAddr now

// client
impl PrivateNockAppGrpcClient {
    pub async fn connect<T: AsRef<str>>(address: T) -> Result<Self>;     // address e.g. "http://127.0.0.1:5561"
    pub async fn poke(&mut self, pid: i32, wire: Wire, payload: Vec<u8>) -> Result<bool>;  // payload = JAM of the cause
    pub async fn peek(&mut self, pid: i32, path: Vec<u8>) -> Result<Vec<u8>>;              // path = JAM of the path noun; returns JAM of the peek result
}
```

`Wire` is `nockapp_grpc::pb::common::v1::Wire`; build a plain one with
`nockapp_grpc::wire_conversion::create_grpc_wire()`.

### ⚠️ NEW: how poke/peek map to the kernel (the load-bearing detail)

This is the thing to get right and is **not** documented anywhere; verified by reading the
server + serf:

- **gRPC poke** (`server.rs::poke`): the server **cues `req.payload`** and calls
  `handle.poke(wire, payload_slab)`. The serf then builds the Arvo job
  `[event_num [%poke wire] eny our now CAUSE]` where **`CAUSE` is exactly your cued payload
  noun**. The moat/wrapper matches `[[%poke *] *]` and hands `+poke` an `ovum` whose
  `cause.input.ovum` is that payload. **So the gRPC poke payload must be the kernel's `cause`
  directly** — here, `[%echo val]`. (No extra wrapping; don't wrap it in `[%poke ...]`
  yourself.)
- **gRPC peek** (`server.rs::peek`): the server cues `req.path` and calls `handle.peek(slab)`,
  which slams the kernel's `+peek` with that noun **as the `path`**. So jam a real path noun
  (`~[%echo]` == `[%echo 0]`), not a string.
- **peek return value**: `handle.peek` (the *driver* method, `driver.rs:185`) returns the
  **raw** kernel `+peek` output, which is a `(unit (unit *))`. The server jams that whole
  thing. So the bytes the client gets back cue to `[~ ~ val]` == `[0 0 val]`. The client must
  walk `[0 [0 val]]` to extract `val`. (Note: this is the *unmunged* path; the separate
  `NockApp::peek_handle` does strip the outer units, but the private gRPC server does **not**
  use it.)

`src/talk.rs` builds the cause with `Atom::from_bytes` + `T(&[D(tas!(b"echo")), val])`, jams
it, pokes; builds the path with `make_tas(&mut slab,"echo")` + `T(&[knot, D(0)])`, jams it,
peeks; then cues `[~ ~ val]` and decodes `val` with `noun_serde::String::from_noun`.

---

## 3. ⚠️ NEW: two binaries ⇒ no `out.jam` (per-bin `<name>.jam` instead)

`nockup project build` with **multiple `[[bin]]` entries** compiles `hoon/app/<binname>.hoon`
for each bin and then **renames `out.jam` to `<binname>.jam`** (verified in
`crates/nockup/src/commands/build/build.rs:174`). So a two-bin project produces
`listen.jam` + `talk.jam` and **no `out.jam`**. Consequences:

- Every bin needs its own `hoon/app/<binname>.hoon` or the build errors with
  `Hoon app file not found`. The `talk` client never boots a kernel, but you must still ship a
  trivial `hoon/app/talk.hoon` to satisfy the build.
- For the prebuilt deploy you pass `--jam ./out.jam`, and the server reads `out.jam` at boot.
  So **copy the server kernel into place first**: `cp listen.jam out.jam`. (`nockd` then stages
  whatever `--jam` you give it into the app state dir as `out.jam`, which is the server's cwd —
  confirmed in `nockd/src/store.rs::stage_jam`.) The `listen` binary also falls back to reading
  `listen.jam` if `out.jam` is absent, so a by-hand `./target/release/listen` works from the
  project dir too.

---

## 4. Build error iterations (all in our own `src/*.rs`)

After the API port, the remaining errors were ordinary Rust:

- **`boot::setup` takes `cli`, not `Some(cli)`** — same drift chain-watch hit; the `grpc`
  template still passes `Some(cli)`. Pass `cli`.
- **`no method named noun_space` on `NounSlab`** — `noun_space()` is a `NounAllocator` trait
  method; add `use nockvm::noun::NounAllocator;`.
- **`type annotations needed for NounSlab<_>`** — annotate `let mut slab: NounSlab =
  NounSlab::new();` (the `J` jammer param is otherwise unconstrained in the client, which does
  no booting).
- **error-type juggling in `main`** — `NockApp::run()` returns `Result<(), NockAppError>`
  (NOT boxed), while `boot::setup` returns `Result<_, Box<NockAppError>>`. Making `main`
  return `anyhow::Result<()>` and mapping the stringly errors with `anyhow::anyhow!` is the
  least-friction fix.

Build succeeds (from the PARENT dir, chain-watch ROUGH EDGE 5):

```sh
cd examples && nockup project build echo-grpc
# ✓ Cargo build completed successfully!
# Compiling Hoon app file at: echo-grpc/hoon/app/listen.hoon  → listen.jam
# Compiling Hoon app file at: echo-grpc/hoon/app/talk.hoon    → talk.jam
ls echo-grpc/{listen,talk}.jam echo-grpc/target/release/{listen,talk}
cp echo-grpc/listen.jam echo-grpc/out.jam
```

(Harmless build noise: `invalid character '{' in package name: '{{project_name}}'` — cargo
scanning the template dir in the git checkout — and `warning: unused import: 'fs'` from the
template `build.rs`. Both cosmetic; build still succeeds. Same as chain-watch.)

---

## 5. ⚠️ NEW: clean SIGTERM exit code

The NockApp framework installs its own SIGTERM/SIGINT handlers and **exits with `128+signum`
via `NockAppError::Exit`** (143 = SIGTERM, 130 = SIGINT). If you just `return Err(e)` from
`run()`, a normal `nockd stop`/`restart` looks like a crash in your logs
(`kernel loop exited with error: Exit(143)` + a non-zero process exit). Match `Exit(143)` /
`Exit(130)` and treat them as clean:

```rust
match nockapp.run().await {
    Ok(_) => Ok(()),
    Err(NockAppError::Exit(143)) => { info!("received SIGTERM; shut down cleanly"); Ok(()) }
    Err(NockAppError::Exit(130)) => { info!("received SIGINT; shut down cleanly"); Ok(()) }
    Err(other) => Err(anyhow::anyhow!("kernel loop error: {other:?}")),
}
```

(The framework still logs its internal `nockapp: Shutdown triggered with error: Exit(143)` —
that line is not yours and is expected.) After this fix, restart logs show
`received SIGTERM; shut down cleanly`.

---

## 6. Deploy (prebuilt) and the health gate

Project-mode is still broken (chain-watch ROUGH EDGE 7); `nockd.toml` ships `project = "."`
as the intended UX but you deploy prebuilt:

```sh
cd examples/echo-grpc
cp listen.jam out.jam
nockd deploy echo-grpc \
  --bin ./target/release/listen \
  --jam ./out.jam \
  --restart always \
  --health-addr 127.0.0.1:5561 \
  --status-label POKES \
  --status-cmd "grep -aoE 'pokes=[0-9]+' | tail -1 | grep -aoE '[0-9]+'" \
  -- --grpc-addr 127.0.0.1:5561
```

### ✅ NEW: `--health-addr` against the private gRPC works → `serving`

The private gRPC server (`server.rs`) registers a tonic **health service** and sets it
`serving`. Pointing `nockd`'s `--health-addr` at the same `127.0.0.1:5561` the app binds makes
`nockd` health-probe it successfully — so `nockd ps` shows **HEALTH = serving** (not just
`unknown` as for chain-watch, which has no gRPC server to probe). Pass the same address to the
app (`-- --grpc-addr 127.0.0.1:5561`) so bind-addr and health-addr agree. **Use a unique port
per app** to avoid collisions (5561 here).

### Status-cmd: same `grep -a` NUL gotcha

The `POKES` metric uses the chain-watch `-a` recipe (chain-watch ROUGH EDGE 8): the kernel
boot log has NUL bytes, and BSD grep treats NUL-bearing stdin as binary and suppresses `-o`
output. `grep -aoE 'pokes=[0-9]+' | tail -1 | grep -aoE '[0-9]+'` works on BSD + GNU.

---

## 7. Verify — the roundtrip proof

```sh
nockd ps
# echo-grpc  running  serving  verified  40630  —  POKES 2

cd examples/echo-grpc
./target/release/talk --grpc-addr http://127.0.0.1:5561 --value "hello over grpc"
# POKE  [%echo "hello over grpc"] -> acked=true
# PEEK  /echo -> "hello over grpc"
# ROUNDTRIP OK: poked "hello over grpc" == peeked "hello over grpc"

nockd logs echo-grpc | grep -aE 'poke %echo|peek /echo|metric: pokes'
# echo-grpc: poke %echo <- 'hello over grpc'
# metric: pokes=1
# echo-grpc: peek /echo -> 'hello over grpc'
```

Each `talk` invocation pokes a value and peeks the identical value back; `POKES` increments;
the kernel slogs confirm both arms ran inside the deployed kernel. State persists across
`nockd restart` (the snapshot keeps `POKES` and the last `val`).

---

## 8. Summary of NEW rough edges (beyond chain-watch's list)

1. **The private gRPC API moved** between 485e914 and 6d29078: it's now under
   `nockapp_grpc::services::private_nockapp::{grpc_server_driver, PrivateNockAppGrpcClient,
   PrivateNockAppGrpcServer}`, and `grpc_server_driver` takes a `SocketAddr`. The `grpc`
   template's source targets the old layout and must be ported.
2. **poke payload == kernel `cause`; peek path == kernel `path`; peek result == raw
   `(unit (unit *))`.** The private gRPC server does no munging — the client must jam the bare
   cause/path and cue `[~ ~ val]` back out. (Undocumented; from reading server.rs + serf.)
3. **Two `[[bin]]`s ⇒ no `out.jam`** — nockup renames to `<binname>.jam` per bin, and each bin
   needs its own `hoon/app/<binname>.hoon`. Copy `listen.jam` → `out.jam` before the prebuilt
   deploy.
4. **Clean SIGTERM is `NockAppError::Exit(143)`** — match it (and 130 for SIGINT) so
   `nockd stop`/`restart` doesn't look like a crash.
5. **`--health-addr` on the private gRPC actually reports `serving`** — the server exposes a
   tonic health service, so the nockd health gate works (unlike apps with no gRPC server).
   Pin a unique private port per app.
