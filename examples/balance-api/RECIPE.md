# RECIPE — building & deploying `balance-api` end to end

`balance-api` is an HTTP explorer backend: `GET /balance/<pubkey>` returns the pubkey's
balance read live from Nockchain over gRPC. This recipe is the honest transcript of building
it with the real `nockup` toolchain and deploying it under `nockd` in project-mode.

It is the [`chain-watch`](../chain-watch/RECIPE.md) pattern (Rust chain client + trivial Hoon
kernel) with an **HTTP listener instead of a poll loop**. All the chain-watch rough edges
(toolchain pin, rustls dual-provider, rev 6d29078, endpoint-by-name) apply here too and are
not re-derived — read that recipe first. Below are the deltas and the NEW edges.

Verified on macOS (darwin, aarch64) on 2026-06-24. Built clean at nockchain rev
`6d29078e69b64febabe3d8d20a64c06b969a16ed`.

---

## 0. Environment

```sh
export PATH="$PATH:/Users/neal/.nockup/bin"                 # nockup + hoonc
export PATH="$PATH:/Users/neal/zorp/nockd/target/release"   # nockd
```

A `nockd serve` daemon was already running with a builder key and the project-mode fix.

---

## 1. Scaffold

Rather than re-run `nockup project init` (which nests under a `<name>/` subdir and leaves
`rev = ""` — chain-watch ROUGH EDGE 1 & 2), I copied the proven scaffold from chain-watch and
renamed it:

```sh
mkdir -p examples/balance-api && cd examples/chain-watch
cp -r hoon ../balance-api/
cp build.rs nockapp.toml nockapp.lock rust-toolchain.toml ../balance-api/
# then: rename the package to "balance-api" in nockapp.toml, write Cargo.toml / nockd.toml /
# .gitignore / src/main.rs.
```

Kept verbatim from chain-watch: `rust-toolchain.toml` (`nightly-2026-04-03`), the trivial
`hoon/` kernel, `build.rs`.

---

## 2. Cargo.toml

Same four nockchain crates pinned to `6d29078…` plus `rustls` (feature `ring`). NEW deps for
this example:

```toml
axum       = "0.7"      # pure-Rust HTTP server
serde_json = "1.0"      # JSON response bodies
```

`axum` 0.7 is in the existing dep graph (the nockchain workspace already pulls axum/tonic),
so it added no new toolchain friction.

---

## 3. main.rs — the chain read

The chain client API (verified by reading
`nockchain-new/.../public_nockchain/v2/client.rs`):

```rust
use nockapp_grpc::services::public_nockchain::v2::client::{BalanceRequest, PublicNockchainGrpcClient};
let mut client = PublicNockchainGrpcClient::connect(endpoint).await?;
let balance = client.wallet_get_balance(&BalanceRequest::Address(pubkey)).await?;
```

### Surfacing a total from the `Balance` type (the main NEW work)

`wallet_get_balance` returns `pb::common::v2::Balance`, which is **per-note UTXOs, not a
scalar**:

```
Balance { notes: Vec<BalanceEntry>, height: Option<BlockHeight>, block_id, page }
BalanceEntry { name, note: Option<Note> }
Note { note_version: Option<note::NoteVersion> }            // a oneof!
note::NoteVersion::Legacy(common::v1::Note)  -> .assets: Option<Nicks>  (proto tag 6)
note::NoteVersion::V1(common::v2::NoteV1)    -> .assets: Option<Nicks>  (proto tag 5)
Nicks { value: u64 }
```

So the total balance is: **sum of `assets.value` over every note**, matching on the
`NoteVersion` oneof to reach `assets` in either variant:

```rust
use nockapp_grpc::pb::common::v2::note::NoteVersion;
balance.notes.iter()
  .filter_map(|e| e.note.as_ref())
  .filter_map(|n| match n.note_version.as_ref() {
      Some(NoteVersion::Legacy(x)) => x.assets.as_ref().map(|a| a.value as u128),
      Some(NoteVersion::V1(x))     => x.assets.as_ref().map(|a| a.value as u128),
      None => None,
  })
  .sum::<u128>()
```

Real wallets today return the **`Legacy`** (v1::Note) variant — that's where the funded
demo's nicks live. I sum as `u128` to be safe against a many-note wallet overflowing `u64`,
and `serde_json` serializes it fine.

### HTTP shape

`axum` router: `GET /balance/:pubkey` and `GET /` (with optional `?pubkey=`). Shared state is
`{ endpoint, AtomicU64 requests }`. Each request bumps the counter and prints
`metric: requests=<N>`. A cheap base58/length check rejects garbage with 400 before a gRPC
round-trip; the chain's own rejection of a format-valid-but-bad address is surfaced as 400,
and a connect failure as 502.

### Boot + shutdown

Identical to chain-watch: install `rustls::crypto::ring` provider first, boot the kernel from
`out.jam` (`boot::setup(&kernel, cli, …)` — note `cli`, not `Some(cli)`, at this rev), fire
the demo poke, keep the handle alive, then run the server with
`axum::serve(...).with_graceful_shutdown(...)` wired to SIGTERM + Ctrl-C.

---

## 4. Build

Per chain-watch ROUGH EDGE 5, build from the PARENT dir with the project NAME:

```sh
cd examples
nockup project build balance-api
# ... Cargo build completed successfully!
# ... hoonc: output written successfully to '.../balance-api/out.jam'
# ... ✓ Hoon compilation completed successfully!
```

Built clean on the first try (no API-drift surprises — the boot/setup signatures were
already correct from copying chain-watch). Artifacts: `out.jam` (~569K) and
`target/release/balance-api` (~12M Mach-O arm64). The only warnings are the cosmetic
`unused import: fs` from the template `build.rs` and the harmless `{{project_name}}` noise
from cargo scanning the template dir inside the nockchain checkout.

---

## 5. Smoke test (before nockd)

```sh
cd examples/balance-api
./target/release/balance-api --endpoint https://rpc.nockchain.net --port 8082 &
# log: balance-api starting; ... listening on http://127.0.0.1:8082

PK=2bc9h9E8zBHeJCyp9QWEmwGdX9uLGDRwJJMJMe8GEeSKKkPmoBx4Kq5ME8mic9WrhjfRmGeruy56zfWVZnqwrxChyRSHGUxDCgJzRd7RmH4qM7JGmGUpRypYJtK7yVEWTu1e
curl http://127.0.0.1:8082/balance/$PK
# {"balance":33116464,"height":92952,"notes":104,"pubkey":"2bc9h9E8...","unit":"nicks"}
#   33116464 nicks = 505.317… NOCK   ← matches the funded wallet
curl http://127.0.0.1:8082/balance/notapubkey   # -> 400 invalid pubkey
curl http://127.0.0.1:8082/                      # -> 400 missing pubkey
kill -TERM %1                                     # -> "received SIGTERM; shutting down cleanly", exits
```

SIGTERM exits cleanly. No rustls panic (the `ring` provider install handles the dual-provider
trap on the very first TLS handshake to `rpc.nockchain.net`).

---

## 6. Deploy (project-mode — now works for single-bin)

```sh
nockd endpoint add mainnet-rpc https://rpc.nockchain.net   # one-time (already registered here)

cd examples/balance-api
nockd deploy -f nockd.toml
# ✓ Cargo build completed successfully!  ✓ Hoon compilation completed successfully!
# deployed balance-api
#   artifact eddddef9...
#   kernel   d22cd157...
nockd restart balance-api
# restarted balance-api
```

Project-mode deploy **round-tripped cleanly** — this is the chain-watch ROUGH EDGE 7 that is
now fixed in nockd (it passes the absolute project path to `nockup project build`).
`balance-api` is single-bin, so no `bin_target` is needed (artifact is
`target/release/balance-api` + `out.jam`). Per the prompt's model: **deploy registers,
restart swaps** the live process onto the new artifact.

---

## 7. Verify

```sh
nockd ps
# NAME         STATE    HEALTH   VERIFIED  PID    ENDPOINT     STATUS
# balance-api  running  unknown  verified  62499  mainnet-rpc  REQ 0

PK=2bc9h9E8...WTu1e
curl http://127.0.0.1:8082/balance/$PK
# {"balance":33116464,"height":92953,"notes":104,"pubkey":"...","unit":"nicks"}   ← live, under nockd

nockd logs balance-api | grep -aoE 'requests=[0-9]+' | tail
# requests=1  requests=2  requests=3

nockd ps   # (after a status tick)
# balance-api  running  verified  ...  REQ 3
```

- **running + verified** ✅ (self-signed attestation, trusted builder key).
- **endpoint-by-name** ✅ — the app dialed the URL injected from `mainnet-rpc`.
- **REQ metric** ✅ — cumulative lookup count, populated from the live request counter.
- **funded balance** ✅ — 33,116,464 nicks (≈505.3 NOCK) across 104 notes, read live.

---

## 8. NEW rough edges (beyond chain-watch's list)

1. **`wallet_get_balance` returns per-note UTXOs, not a scalar balance.** You must sum
   `Balance.notes[].note.{Legacy|V1}.assets.value` yourself. The amount lives behind a
   `Note.note_version` **oneof** (`pb::common::v2::note::NoteVersion::{Legacy, V1}`) — both
   arms carry an `assets: Option<Nicks>` but at different proto tags. Real wallets today
   return the `Legacy` (`common::v1::Note`) arm, so a V1-only match would silently report 0.
   Sum as `u128`, not `u64`, to be safe for large/many-note wallets.

2. **Finding a funded pubkey is the hard part of the demo, not the code.** Block explorers
   are scripting-gated (nockblocks.com needs a Cloudflare Turnstile + API key; others refuse
   non-browser clients). gRPC `GetTransactionDetails` is currently broken on the live server
   (NounDecode "Expected v1 raw-tx version 1"). The reliable path is a known wallet pubkey
   queried directly. The funded one used here came from a dev's public mining-wallet output
   and was confirmed live. Note: many widely-copied "example" pubkeys from mining guides are
   **format-valid but empty** — verify before using one in a demo.

3. **Pubkey format:** base58, **132 chars** for these v0/schnorr "cheetah point" keys. The
   chain rejects malformed input with `ERROR_CODE_INVALID_REQUEST: "Address is improperly
   formatted"`; balance-api surfaces that as a 400. (My syntactic pre-check only requires
   base58 + length 40–256, deliberately loose, and lets the chain be the real authority.)

4. **Status metric `tail -1` is a window, not a max.** `nockd`'s status command sees only the
   recent log tail, so `grep ... | tail -1` shows the latest `requests=N` line *in that
   window*. Right after a burst it can momentarily read a lower N than the true total until
   the newest line is in-window; it converges within a tick. (The cumulative counter in the
   process is always correct; this is purely a display artifact of the tail-based status.)

5. The chain-watch NUL-byte `grep -a` edge no longer applies here — nockd now strips NUL
   bytes before piping to the status command (per GOTCHAS.md), so the plain
   `grep -oE 'requests=[0-9]+' | tail -1 | grep -oE '[0-9]+'` recipe works as written.
