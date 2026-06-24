# RECIPE — building & deploying `chain-watch` end to end

This is the **canonical transcript** for scaffolding a chain-interacting NockApp service,
building it with the real `nockup` toolchain, and deploying it under `nockd` with a live
status metric. It is written to be copied by other examples. It is honest: every error hit
along the way is recorded with its fix, and every `nockd`/`nockup` rough edge is flagged.

Verified on macOS (darwin, aarch64-apple-darwin) on 2026-06-24.

---

## 0. Environment

```sh
export PATH="$PATH:/Users/neal/zorp/nockd/target/release"   # nockd is NOT on PATH by default
which nockup hoonc        # /Users/neal/.nockup/bin/{nockup,hoonc}
```

- `nockup` + `hoonc` are the client-side toolchain (build happens here; the daemon never compiles).
- The public RPC endpoint used: `https://rpc.nockchain.net` (TLS gRPC, reads only).
- A `nockd serve` daemon was already running on `127.0.0.1:4490`. If not, start one:
  `nockd serve &` and give it a second.

---

## 1. Scaffold

`nockup project init` requires a `nockapp.toml` in the current directory **first**, then it
generates the project.

```sh
mkdir -p examples/chain-watch && cd examples/chain-watch
# Write nockapp.toml (package name + template + deps):
cat > nockapp.toml <<'TOML'
[package]
name = "chain-watch"
version = "0.1.0"
authors = ["sigilante"]
description = "..."
license = "MIT"
template = "basic"

[dependencies]
TOML

nockup project init
```

### ⚠️ ROUGH EDGE 1 — `nockup project init` creates a NESTED subdirectory

`init` does **not** initialize in place. Run inside `examples/chain-watch/`, it created
`examples/chain-watch/chain-watch/` (a subdir named after the package) and put the whole
project there. Fix was to flatten it back up one level:

```sh
rm nockapp.toml                       # the outer stub
mv chain-watch/* chain-watch/nockapp.lock chain-watch/nockapp.toml .   # move everything up
mv chain-watch/build.rs chain-watch/Cargo.toml chain-watch/README.md chain-watch/hoon chain-watch/src .
rmdir chain-watch
```

(There is a matching gotcha in `nockup project build` — see ROUGH EDGE 5.)

### ⚠️ ROUGH EDGE 2 — `init` leaves the git rev placeholder EMPTY

The generated `Cargo.toml` had:

```toml
nockapp = { git = "https://github.com/nockchain/nockchain.git", rev = "" }
```

The `{{nockapp_commit_hash}}` template variable was **not substituted** — `rev = ""`. This
fails to build. You must fill in a real rev yourself (next step).

---

## 2. Cargo.toml — pin the right rev and add the gRPC client

The `basic` template has no chain client. The `grpc` template pins
`rev = "485e914b389a1e518d4aaaa24f5f079d0ad894be"` and depends on `nockapp-grpc` — but:

### ⚠️ ROUGH EDGE 3 — the `grpc` template's pinned rev is TOO OLD for the v2 chain client

At `485e914…` the `nockapp-grpc` crate has the OLD layout (`client.rs` with a private
`NockAppGrpcClient` only). There is **no** `services::public_nockchain` module and **no**
`explorer_heaviest_height` — the method we need for the chain tip. (Verified by reading the
cargo checkout at that rev.)

The v2 public client (`PublicNockchainGrpcClient::explorer_heaviest_height`) lives in newer
revs. The local nockchain source checkout (`/Users/neal/zorp/nockchain-new`) was at
`6d29078e69b64febabe3d8d20a64c06b969a16ed`, which **does** have it:

```
crates/nockapp-grpc/src/services/public_nockchain/v2/client.rs
  pub struct PublicNockchainGrpcClient
  pub async fn connect(...)
  pub async fn explorer_heaviest_height(&mut self) -> Result<u64>   # ← the chain tip
```

So all four nockchain crates were pinned to `6d29078…`:

```toml
nockapp        = { git = "...nockchain.git", rev = "6d29078e69b64febabe3d8d20a64c06b969a16ed" }
nockvm         = { git = "...nockchain.git", rev = "6d29078e69b64febabe3d8d20a64c06b969a16ed" }
nockvm_macros  = { git = "...nockchain.git", rev = "6d29078e69b64febabe3d8d20a64c06b969a16ed" }
nockapp-grpc   = { git = "...nockchain.git", rev = "6d29078e69b64febabe3d8d20a64c06b969a16ed" }
```

Import path used in `main.rs`:

```rust
use nockapp_grpc::services::public_nockchain::v2::client::PublicNockchainGrpcClient;
// (a shorter re-export also exists: nockapp_grpc::services::public_nockchain::PublicNockchainGrpcClient)
```

Also added to `[dependencies]`: `tracing`, the `"time"` feature on `tokio`, and `rustls`
(see ROUGH EDGE 6).

---

## 3. main.rs

Replace the template's one-shot poke-and-exit `main.rs` with: boot the kernel (so `out.jam`
is consumed and we are a real supervised NockApp), then run a tokio poll loop. See
`src/main.rs` in this directory for the final version. Key points:

- `let cli = boot::default_boot_cli(false);` — this builds a `Cli` struct directly; it does
  **not** parse `argv`, so our own `--endpoint` flag does not collide with the boot CLI.
- Endpoint resolution: scan `std::env::args()` for `--endpoint <url>` (nockd substitutes
  `{endpoint}` here), else `NOCKD_ENDPOINT_URL` (nockd also sets this), else a default.
- Print `metric: height=<N>` per poll — the one greppable line the status command scrapes.
- `tokio::signal::unix` SIGTERM handler + `ctrl_c()` for clean shutdown.

---

## 4. Build (and the iterations it took)

`nockup`'s build path:

### ⚠️ ROUGH EDGE 5 — `nockup project build` resolves the project as a SUBDIR by name

From inside the project, **all of these FAIL**:

```sh
nockup project build           # Error: Project directory 'chain-watch' not found
nockup project build .         # Error: Project directory 'chain-watch' not found
```

`nockup project build` reads the package name from `nockapp.toml` and looks for a
**subdirectory** of that name. It works only when invoked from the PARENT dir with the name,
or with an ABSOLUTE path:

```sh
cd examples
nockup project build chain-watch        # ✅ works (subdir of cwd)
# or, from inside the project:
nockup project build "$PWD"             # ✅ works (absolute path)
```

This same disagreement is what breaks `nockd deploy --project` — see ROUGH EDGE 7.

### Build error A — unstable `cold_path` feature (toolchain too old)

```
error[E0658]: use of unstable library feature `cold_path`
  --> .../nockvm/.../unifying_equality.rs:194:13
     core::hint::cold_path();
   = note: this compiler was built on 2025-11-04 ...
```

The ambient toolchain `nockup` used was `rustc 1.93.0-nightly (2025-11-04)`. `nockvm` at
rev `6d29078…` needs `core::hint::cold_path`, stabilized/available only in a newer nightly.
The nockchain workspace pins `nightly-2026-04-03` (`rustc 1.96.0-nightly`), which was
already installed via `rustup`.

**Fix:** add a `rust-toolchain.toml` to the project so cargo (and thus `nockup`) selects it:

```toml
[toolchain]
channel = "nightly-2026-04-03"
```

> ⚠️ ROUGH EDGE 4: `nockup` builds with the **ambient** Rust toolchain, not the one the
> pinned nockchain rev expects. Without a project `rust-toolchain.toml` you get a confusing
> `cold_path` error from deep inside `nockvm`. Any example pinning a recent nockchain rev
> needs this file. Make sure the pinned nightly is installed (`rustup toolchain list`).

### Build error B — `boot::setup` signature drift

```
error[E0308]: mismatched types
  68 | boot::setup(&kernel, Some(cli), &[], "chain-watch", None)
     |                      ^^^^^^^^^ expected `Cli`, found `Option<Cli>`
```

The `basic` template's `main.rs` passes `Some(cli)`, but at rev `6d29078…` `boot::setup`
takes `cli: Cli` (and `NockApp` is generic `NockApp<J: Jammer>`). **Fix:** pass `cli`
directly:

```rust
let mut nockapp: NockApp = boot::setup(&kernel, cli, &[], "chain-watch", None).await?;
```

(`NockApp` with no explicit `J` infers fine here.)

### Build error C — `type annotations needed`

```
error[E0282]: type annotations needed
  119 | match c.explorer_heaviest_height().await {
```

Resolved by binding the result: `let height: u64 = height;` inside the `Ok(height)` arm.

### Build success

```sh
cd examples
nockup project build chain-watch
# ... Cargo build completed successfully!
# ... hoonc: output written successfully to '.../chain-watch/out.jam'
# ... ✓ Hoon compilation completed successfully!

ls -la chain-watch/out.jam chain-watch/target/release/chain-watch
# out.jam ~569K ; chain-watch  ~11M Mach-O arm64
```

> Harmless noise during build: `error: invalid character '{' in package name:
> '{{project_name}}'` — this is cargo scanning the **template** dir inside the nockchain git
> checkout (`crates/nockup/templates/basic/Cargo.toml`), not your project. The build still
> succeeds. Also a `warning: unused import: 'fs'` from the template's `build.rs` (cosmetic).

---

## 5. Smoke test the binary directly (before nockd)

Run from the dir containing `out.jam` (cwd-relative read):

```sh
cd examples/chain-watch
./target/release/chain-watch --endpoint https://rpc.nockchain.net
```

### Runtime error D — rustls "no CryptoProvider" panic

```
thread 'main' panicked at rustls-0.23.41/src/crypto/mod.rs:249:
  Could not automatically determine the process-level CryptoProvider from Rustls crate
  features. Call CryptoProvider::install_default() ... or make sure exactly one of the
  'aws-lc-rs' and 'ring' features is enabled.
```

### ⚠️ ROUGH EDGE 6 — both rustls crypto providers are in the nockchain dep graph

`grep '^name = "(ring|aws-lc-rs)"' Cargo.lock` shows **both** `ring` and `aws-lc-rs` are
pulled in transitively. rustls can't auto-pick, so it panics on the first TLS handshake —
which is exactly when you dial an `https://` endpoint.

**Fix:** depend on `rustls` directly and install a provider at startup, before any gRPC use:

```toml
# Cargo.toml
rustls = { version = "0.23", features = ["ring"] }
```

```rust
// main.rs, right after init_default_tracing:
let _ = rustls::crypto::ring::default_provider().install_default();
```

Rebuild (`cd examples && nockup project build chain-watch`), then re-run. Success:

```
metric: endpoint=https://rpc.nockchain.net
... chain_watch: connected to https://rpc.nockchain.net
metric: height=92864
metric: height=92864
... chain_watch: received SIGTERM; shutting down cleanly
```

The height (92864 here) matches `nockd endpoint list`'s reading of the same endpoint — a
good sanity check that it's the real tip. SIGTERM is handled cleanly.

---

## 6. nockd setup

```sh
nockd key gen                                    # builder identity (idempotent-ish: errors if it exists; that's fine — verified still works)
nockd endpoint add mainnet-rpc https://rpc.nockchain.net
nockd endpoint list
# NAME         REACH  URL                        LAG    HEIGHT  BEHIND  APPS
# mainnet-rpc  ok     https://rpc.nockchain.net  ~95ms  92864   tip     0
```

> `nockd key gen` errors if a builder key already exists ("refusing to overwrite") — that's
> expected and harmless; existing key still self-signs deploys → **verified**.

---

## 7. Deploy

### The intended canonical deploy: `project = "."` (REAL TOOLCHAIN)  — ⚠️ CURRENTLY BROKEN

`nockd.toml` ships with `project = "."` so `nockd deploy` shells out to `nockup` to build
(DESIGN principle 7). The intended command:

```sh
cd examples/chain-watch
nockd deploy -f nockd.toml
```

**Result — it fails:**

```
Error: Project directory 'chain-watch' not found
Error: `nockup project build` failed with status exit status: 1
```

### ⚠️ ROUGH EDGE 7 — `nockd deploy --project` (project-mode) is broken on this nockd/nockup pair

Root cause (verified by reading `nockd/src/buildkit.rs`): nockd's builder runs

```rust
Command::new("nockup").args(["project", "build"]).current_dir(project_dir)
```

i.e. `nockup project build` with **no positional arg, from inside the project dir**. But
`nockup project build` ignores cwd and resolves the package name from `nockapp.toml` as a
**subdirectory** to descend into (ROUGH EDGE 5) — so it looks for `./chain-watch/` inside
the project and fails. The two tools disagree on the calling convention. There is no
`nockd.toml`/`project` value that fixes this from the example side; it needs a fix in nockd
(pass the project dir as an absolute-path arg: `nockup project build <abs path>`, which is
known to work) or in nockup (honor cwd / accept `.`).

**This is the central pathfinder finding.** Project-mode — the whole point of the
build/run split — does not currently round-trip.

### Fallback that closes the loop: prebuilt `--bin` / `--jam`

```sh
cd examples/chain-watch
nockd deploy chain-watch \
  --bin ./target/release/chain-watch \
  --jam ./out.jam \
  --restart always \
  --endpoint mainnet-rpc \
  --status-label HEIGHT \
  --status-cmd "grep -aoE 'height=[0-9]+' | tail -1 | grep -aoE '[0-9]+'" \
  -- --endpoint '{endpoint}'
# deployed chain-watch
#   artifact 557d9df2...
#   kernel   b34e2f8e...
```

Note `-- --endpoint '{endpoint}'`: everything after `--` is passed to the app; nockd
substitutes `{endpoint}` with the resolved `mainnet-rpc` URL and also sets
`NOCKD_ENDPOINT_URL`.

---

## 8. Verify it's working

```sh
nockd ps
# NAME         STATE    HEALTH   VERIFIED  PID    ENDPOINT     STATUS
# chain-watch  running  unknown  verified  83977  mainnet-rpc  HEIGHT 92864

nockd logs chain-watch | grep -a 'metric: height'
# metric: height=92864  (×N, then 92865, 92866, ... as blocks arrive)

nockd endpoint list | grep mainnet-rpc
# mainnet-rpc  ok  https://rpc.nockchain.net  ...  92867  tip  1   ← APPS=1
```

- **running + verified** ✅ (self-signed attestation, trusted builder key).
- **endpoint-by-name** ✅ — app dialed the URL injected from the `mainnet-rpc` registry entry.
- **HEIGHT metric** ✅ — populated from the live chain; observed increasing 92864 → 92870
  over a few minutes.

Restart is clean and the status repopulates:

```sh
nockd restart chain-watch
# logs: received SIGTERM; shutting down cleanly → kernel: starting → connected → metric: height=...
# ps:   chain-watch running verified ... HEIGHT 92870   (new PID)
```

---

## 9. The status-command gotcha (HEIGHT showed, then went BLANK)

After the first deploy, `nockd ps` briefly showed `HEIGHT 92864`, then the STATUS column
went **blank** even though `nockd logs chain-watch | grep -a metric` clearly showed dozens
of `metric: height=...` lines.

### ⚠️ ROUGH EDGE 8 — BSD `grep` treats NUL-bearing input as binary → the docs' status recipe yields nothing

`nockd` runs the status command with the ANSI-stripped recent log piped to stdin (cwd =
state dir). `strip_ansi` removes CSI escapes but **not NUL bytes**. The nockapp kernel-boot
log contains raw NUL bytes (atom dumps, e.g. the build-hash line:
`I (..) [\x00\x00\x00\x00\x00\x00\x00%build-hash ...]`). On macOS, **BSD `grep` treats any
input containing a NUL as binary** and suppresses `-o` output — so the canonical recipe from
the nockd README/DESIGN:

```sh
grep -oE 'height=[0-9]+' | tail -1 | grep -oE '[0-9]+'      # ← silently returns NOTHING on macOS
```

produced no value, and the status column stayed blank. (It worked for the first few seconds
only because the NUL-bearing boot lines hadn't yet entered the tail window.)

**Fix — add `-a` (force text) to every grep in the recipe:**

```sh
grep -aoE 'height=[0-9]+' | tail -1 | grep -aoE '[0-9]+'    # ✅ works on BSD and GNU grep
```

Redeploy with the `-a` recipe (this example's `nockd.toml` already uses it) and the HEIGHT
status populates and stays populated across restarts.

> Recommendation for nockd: either strip NUL bytes (not just ANSI) before piping to the
> status command, or document `grep -a` in the canonical recipe. The current README/DESIGN
> examples use plain `grep` and will silently fail on macOS for any app whose log contains
> NUL bytes (which every nockapp kernel boot does).

---

## 10. Summary of rough edges (for the fan-out agents)

1. **`nockup project init` nests** the project in a `<name>/` subdir; flatten it.
2. **`init` leaves `rev = ""`** in Cargo.toml (`{{nockapp_commit_hash}}` not substituted);
   fill in a real rev.
3. **The `grpc` template's pinned rev (485e914) is too old** for the v2 public-gRPC client /
   `explorer_heaviest_height`; use a newer rev (here `6d29078`).
4. **`nockup` uses the ambient Rust toolchain**, not the rev's pin → `cold_path` E0658; add
   a project `rust-toolchain.toml` pinning `nightly-2026-04-03` (must be installed).
5. **`nockup project build` resolves the project as a subdir by name**; run it from the
   PARENT (`nockup project build <name>`) or pass an absolute path. `build` (no arg) / `.`
   from inside the project both fail.
6. **Both rustls providers (ring + aws-lc-rs) are in the dep graph** → TLS panic; add
   `rustls` (feature `ring`) and `install_default()` at startup.
7. **`nockd deploy --project` (project-mode) is broken**: nockd runs `nockup project build`
   with no arg from inside the dir, which nockup rejects. Fall back to `--bin`/`--jam`. Fix
   belongs in nockd (pass the abs project path) or nockup (honor cwd).
8. **BSD grep + NUL bytes in nockapp logs** breaks the documented status recipe; use
   `grep -a`.

Also note: `boot::setup` and `NockApp` signatures drift between revs — expect to adjust the
template `main.rs` (`Some(cli)` → `cli`) when you bump the pin.
