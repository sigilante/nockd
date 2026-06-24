# RECIPE — building & deploying `nock-price` end to end

This is the transcript for scaffolding `nock-price` — a NockApp price watcher that polls the
live $NOCK price from multiple HTTPS venues and surfaces a median aggregate USD price as a
`nockd` status metric. It was built by **copying the [`chain-watch`](../chain-watch) pattern**
(read that example's `RECIPE.md` first — this app inherits all of its rough edges). This file
records the deltas: what was the same, what was different, and the one **new** rough edge.

Verified on macOS (darwin, aarch64-apple-darwin) on 2026-06-24.

---

## 0. Environment

```sh
export PATH="$PATH:/Users/neal/zorp/nockd/target/release"   # nockd not on PATH by default
which nockup hoonc nockd
rustup toolchain list | grep nightly-2026-04-03             # must be installed
```

A `nockd serve` daemon with a builder key was already running. The pinned nightly was already
installed (chain-watch needed it too).

---

## 1. Scaffold — COPY chain-watch, don't fight `nockup project init`

`chain-watch`'s RECIPE documents that `nockup project init` nests the project in a `<name>/`
subdir and leaves `rev = ""` in Cargo.toml (ROUGH EDGES 1 & 2). The clean shortcut is to copy
chain-watch's already-correct project files and adapt:

```sh
mkdir -p examples/nock-price
cd examples/chain-watch
rsync -a --exclude target --exclude '.data.*' --exclude out.jam --exclude app.nock \
  --exclude Cargo.lock \
  build.rs Cargo.toml rust-toolchain.toml nockapp.toml nockapp.lock hoon \
  ../nock-price/
```

This carries over for free:
- `rust-toolchain.toml` pinning `nightly-2026-04-03` (chain-watch ROUGH EDGE 4 — `nockup`
  uses the ambient toolchain, which is too old for nockvm's `cold_path`).
- The trivial `basic` Hoon kernel (`hoon/app/app.hoon`, `hoon/common/wrapper.hoon`,
  `hoon/lib/lib.hoon`) — name-agnostic, reused verbatim.
- The known-good nockchain rev `6d29078…`.

Then renamed `nock-price` in `nockapp.toml` and `Cargo.toml` (`[package].name` + `[[bin]]`).

---

## 2. Cargo.toml — swap the gRPC client for an HTTP client

This app does NOT touch the Nockchain chain, so the `nockapp-grpc` dependency was **dropped**.
Kept `nockapp` / `nockvm` / `nockvm_macros` at `6d29078…` (just need the kernel runtime to
boot a supervised NockApp). Added:

```toml
serde_json = "1.0"
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
rustls  = { version = "0.23", features = ["ring"] }   # same fix as chain-watch
```

- `reqwest` with `rustls-tls` and **`default-features = false`** so it does not drag in a
  native-tls stack — we want exactly one TLS path, controlled by us.
- `rustls` feature `ring` + `install_default()` at startup is the SAME fix as chain-watch
  (ROUGH EDGE 6): the nockchain dep graph enables both `ring` and `aws-lc-rs`, so rustls
  panics on the first HTTPS handshake unless we pick one. Every price API here is `https`, so
  this is mandatory, not optional.

---

## 3. main.rs

Replaced chain-watch's gRPC poll with three independent HTTP fetchers + a median aggregator.
Kept the chain-watch skeleton verbatim:
- `boot::default_boot_cli(false)` → `init_default_tracing` → `rustls …ring… install_default()`.
- `boot::setup(&kernel, cli, &[], "nock-price", None)` — note `cli` not `Some(cli)` (chain-watch
  build error B), and `NockApp` (not `NockApp<J>`).
- One demo poke, then keep the kernel handle alive; the real work is the Rust loop.
- `tokio::signal::unix` SIGTERM + `ctrl_c()` clean shutdown.

New logic:
- `fetch_base` / `fetch_kraken` / `fetch_safetrade` each return `Option<f64>` and map ANY
  failure (network error, bad JSON, missing field, non-positive value) to `None` = "skip this
  tick". Kraken's `c[0] == 0` (pre-listing) maps to `None` and self-activates once `> 0`.
- All three run concurrently with `tokio::join!`, then `median()` over the live values.
- Per-source lines `metric: nock_usd_base=…` (or `=skip`) **plus** the aggregate on its own
  line `metric: nock_usd=…`. The aggregate key is a strict prefix of none of the per-source
  keys under an `=`-anchored grep, so the status-cmd isolates it.
- 60s poll interval (CoinGecko rate limits), 20s per-request timeout.

---

## 4. Build — clean on the FIRST try

Because the toolchain pin, rev, and the `Some(cli)`→`cli` / `NockApp<J>` API drift were all
already baked into the copied scaffold, the build had **none** of chain-watch's build errors
A/B/C. Run from the PARENT dir (chain-watch ROUGH EDGE 5 — `nockup project build` resolves the
project as a subdir by name):

```sh
cd examples
nockup project build nock-price
# ✓ Cargo build completed successfully!   (~58s)
# ✓ Hoon compilation completed successfully!
# -> nock-price/target/release/nock-price (12M) + nock-price/out.jam (569K)
```

Harmless noise (same as chain-watch): `warning: unused import: 'fs'` from the template
`build.rs`; a `{{project_name}}` cargo complaint from scanning the nockchain template dir.

---

## 5. Smoke test the binary directly

```sh
cd examples/nock-price
timeout 70 ./target/release/nock-price 2>/dev/null | grep -a metric:
# metric: nock_usd_base=0.03232
# metric: nock_usd_kraken=skip
# metric: nock_usd_safetrade=0.03285
# metric: nock_usd=0.03258
```

NO rustls panic — the `ring` provider install worked first time (the value of copying the fix
up front instead of rediscovering it). Live prices sane (~0.032). Kraken skips cleanly.

---

## 6. Deploy — prebuilt (`--bin`/`--jam`)

Project-mode deploy is broken on this nockd/nockup pair (chain-watch ROUGH EDGE 7 — nockd runs
`nockup project build` with no arg from inside the dir, which nockup rejects). Ship the
`nockd.toml` as intended UX but deploy prebuilt:

```sh
cd examples/nock-price
nockd deploy nock-price \
  --bin ./target/release/nock-price \
  --jam ./out.jam \
  --restart always \
  --status-label USD \
  --status-cmd 'grep -aoE "nock_usd=[0-9.]+" | tail -1 | grep -aoE "[0-9.]+"'
# deployed nock-price
#   artifact b83d6f8a…
#   kernel   869c14ae…
```

No `--endpoint` / no `-- …` trailing args: this app has no chain endpoint; its API URLs live
in the wrapper.

---

## 7. Verify

```sh
nockd ps
# NAME        STATE    HEALTH   VERIFIED  PID    ENDPOINT  STATUS
# nock-price  running  unknown  verified  21777  —         USD 0.03281

nockd logs nock-price | grep -a metric:
# metric: nock_usd_base=0.03277
# metric: nock_usd_kraken=skip
# metric: nock_usd_safetrade=0.03285
# metric: nock_usd=0.03281

nockd restart nock-price        # clean:
# nock_price: received SIGTERM; shutting down cleanly  → reboots → status repopulates
```

- **running + verified** ✅ (self-signed, trusted builder key).
- **USD status** ✅ — live median aggregate, ~0.032 (sanity: not 0, not garbage).
- **resilient sources** ✅ — Base + SafeTrade contribute; Kraken skips cleanly (no crash).
- **clean SIGTERM** ✅ on restart.

The `grep -a` and `[0-9.]+` in the status-cmd are both load-bearing (see below).

---

## 8. Rough edges

### Inherited from chain-watch (apply ALL up front — they bit the pathfinder, not us)
1. `nockup project init` nests + leaves `rev = ""` → **copy chain-watch's files** instead.
2. `nockup` uses the **ambient** toolchain → `cold_path` E0658 → ship `rust-toolchain.toml`
   (`nightly-2026-04-03`).
3. `nockup project build` resolves the project as a **subdir by name** → build from the parent
   (`nockup project build nock-price`) or pass an absolute path.
4. **Both rustls providers** in the dep graph → TLS panic on first HTTPS → `rustls`
   feature `ring` + `install_default()` at startup. (Hit on EVERY tick here — all 3 APIs are
   https — so non-negotiable.)
5. **`nockd deploy --project` is broken** → deploy prebuilt `--bin`/`--jam`.
6. **BSD grep + NUL bytes** in the kernel-boot log → use `grep -aoE` (the `-a`), never plain
   `grep -oE`, or the STATUS column stays blank on macOS.

### NEW rough edge (beyond chain-watch's list)

**7. Decimal prices need `[0-9.]+` AND a distinct aggregate key.** chain-watch's status recipe
scrapes an integer height (`[0-9]+`). Prices are decimal, so the pattern must allow the dot:
`grep -aoE 'nock_usd=[0-9.]+'`. Equally important, the aggregate's metric key
(`nock_usd=`) must not be a grep-substring of any per-source key. We log per-source as
`nock_usd_base=`, `nock_usd_kraken=`, `nock_usd_safetrade=` — under the `=`-anchored pattern
`nock_usd=[0-9.]+`, the underscore after `nock_usd` in the per-source keys means they do NOT
match, so `tail -1` reliably picks the aggregate line and never a per-source price. If the
aggregate had been keyed e.g. `nock_usd_agg=` while a source was `nock_usd=`, the grep would
cross-match. **Design the metric key namespace so the aggregate key is grep-isolable, and
allow `.` in the value pattern for any non-integer metric.** (Also: log a non-numeric token
like `=skip` — not `=0` — for an unavailable source, so it can never be mistaken for a real
$0.00000 price by a looser scrape.)

Everything else round-tripped exactly as chain-watch documented.
