# RECIPE — building & deploying `token-price` end to end

`token-price` is an HTTP API: `GET /price/<base-token-address>` returns the token's live USD
price read from DexScreener. This recipe is the honest transcript of building it with the real
`nockup` toolchain and deploying it under `nockd` in project-mode.

It is the [`balance-api`](../balance-api/RECIPE.md) pattern — a **pure-Rust `axum` HTTP server
that calls an external API per request**, plus a **trivial Hoon kernel** booted only to be a
valid supervised NockApp. All the balance-api / chain-watch rough edges (toolchain pin,
rustls dual-provider, rev `6d29078`, build-from-parent-dir) apply here and are not re-derived
— read those recipes first. Below are the deltas and the NEW edges.

Verified on macOS (darwin, aarch64) on 2026-06-24. Built clean at nockchain rev
`6d29078e69b64febabe3d8d20a64c06b969a16ed`.

---

## 0. Environment

```sh
export PATH="$PATH:/Users/neal/.nockup/bin"                 # nockup + hoonc
export PATH="$PATH:/Users/neal/zorp/nockd/target/release"   # nockd
```

A `nockd serve` daemon was already running with a builder key and the project-mode fix.
HTTP port **8086** (8081–8085 are other http apps, 8082 balance-api, 5561 echo-grpc).

---

## 1. Scaffold

Copied the proven scaffold from `balance-api` (same architecture) and renamed it:

```sh
cd examples
mkdir -p token-price
cp -r balance-api/hoon token-price/
cp balance-api/build.rs balance-api/nockapp.lock balance-api/rust-toolchain.toml token-price/
mkdir -p token-price/src
# then: write nockapp.toml / Cargo.toml / nockd.toml / .gitignore / src/main.rs with the
# token-price name.
```

Kept verbatim from balance-api: `rust-toolchain.toml` (`nightly-2026-04-03`), the trivial
`hoon/` `basic` kernel, `build.rs`.

---

## 2. Cargo.toml

Three nockchain crates (`nockapp`, `nockvm`, `nockvm_macros`) pinned to `6d29078…`, plus
`axum`, `serde_json`, and `rustls` (feature `ring`). The deltas vs balance-api:

```toml
# DROPPED: nockapp-grpc — we never touch the chain.
# ADDED:  the HTTP client for the external data source.
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
```

`reqwest` with `rustls-tls` (not the default native-tls) keeps the TLS stack on rustls/ring,
matching the dual-provider fix. Everything was already in the nockchain dep graph, so no new
toolchain friction.

---

## 3. main.rs — the price read

The data source (verified live): `GET https://api.dexscreener.com/latest/dex/tokens/<addr>`
returns `{"pairs": [ {chainId, dexId, priceUsd, liquidity:{usd}, baseToken:{symbol},
quoteToken:{symbol}}, … ] }`. The read:

```rust
let body: Value = client.get(&url).send().await?.json().await?;
let best = body["pairs"].as_array()                       // null for unknown token -> 404
  .iter().flatten()
  .filter(|p| p["chainId"] == "base")                     // Base only
  .max_by_key(|p| p["liquidity"]["usd"]);                 // deepest pool
let price: f64 = best["priceUsd"].as_str()?.parse()?;     // priceUsd is a STRING, already USD
```

(In the actual code the `max_by` uses `partial_cmp` because liquidity is `f64`.)

### HTTP shape

`axum` router: `GET /price/:token` and `GET /` (an HTML help page). Shared state is
`{ reqwest::Client, AtomicU64 requests }`. Each request bumps the counter and prints
`metric: requests=<N>`. A cheap `0x` + 40-hex check rejects garbage with **400** before an
HTTP round-trip; a token with no Base pool is **404**; an upstream/transport/parse failure is
**502**.

### Boot + shutdown

Identical to balance-api: install `rustls::crypto::ring` provider first, boot the kernel from
`out.jam` (`boot::setup(&kernel, cli, &[], "token-price", None)` — note `cli`, not
`Some(cli)`, at this rev), fire the demo poke, keep the handle alive, then run the server with
`axum::serve(...).with_graceful_shutdown(...)` wired to SIGTERM + Ctrl-C. There is **no**
endpoint resolution — the DexScreener base URL is a `const`.

---

## 4. Build

Per the build-from-parent-dir edge, build from `examples/` with the project NAME:

```sh
cd examples
nockup project build token-price
# ... ✓ Cargo build completed successfully!
# ... hoonc: output written successfully to '.../token-price/out.jam'
# ... ✓ Hoon compilation completed successfully!
```

Built clean on the first try. Artifacts: `out.jam` (~569K) and `target/release/token-price`
(~13M Mach-O arm64). Only cosmetic warnings (the template `build.rs` `unused import: fs`).

---

## 5. Smoke test (before nockd)

```sh
cd examples/token-price
./target/release/token-price --port 8086 &
# log: token-price starting; ... listening on http://127.0.0.1:8086

curl http://127.0.0.1:8086/price/0x9B5E262cF9bb04869ab40b19AF91D2dc85761722
# {"liquidity_usd":1220845.12,"pair":"aerodrome/NOCK-USDC","price_usd":0.03611,"token":"0x9B5E2..."}
curl -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8086/price/notanaddress
# 400
curl -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8086/price/0x1234567890abcdef1234567890abcdef12345678
# 404
kill -TERM %1   # -> "received SIGTERM; shutting down cleanly", exits
```

No rustls panic (the `ring` provider install handles the dual-provider trap on the first TLS
handshake to `api.dexscreener.com`). SIGTERM exits cleanly.

---

## 6. Deploy (project-mode)

```sh
cd examples/token-price
nockd deploy -f nockd.toml
# ✓ Cargo build completed successfully!  ✓ Hoon compilation completed successfully!
# deployed token-price
#   artifact 654b89e6...
#   kernel   5ecc3c2c...
nockd restart token-price
# restarted token-price
```

`token-price` is single-bin (no `bin_target`); artifact is `target/release/token-price` +
`out.jam`. No `endpoint` in the manifest — DexScreener isn't a Nockchain RPC, so its base URL
is hardcoded in the binary; `args = ["--port", "8086"]` is the only injected config.

---

## 7. Verify

```sh
nockd ps
# NAME         STATE    HEALTH   VERIFIED  PID    ENDPOINT  STATUS
# token-price  running  unknown  verified  81512  —         REQ 0

curl http://127.0.0.1:8086/price/0x9B5E262cF9bb04869ab40b19AF91D2dc85761722
# {"liquidity_usd":1220824.69,"pair":"aerodrome/NOCK-USDC","price_usd":0.03611,"token":"0x9B5E2..."}  ← live, under nockd
curl -w " [%{http_code}]\n" http://127.0.0.1:8086/price/notanaddress
# {"detail":"expected an EVM address: 0x followed by 40 hex digits",...} [400]

nockd ps   # (after a status tick, having served a few requests)
# token-price  running  verified  ...  REQ 5
```

- **running + verified** ✅ (self-signed attestation, trusted builder key).
- **live USD price** ✅ — $NOCK on Base ≈ **$0.0361**, from the Aerodrome NOCK/USDC pool
  (~$1.22M liquidity), read live from DexScreener.
- **REQ metric** ✅ — cumulative request count, populated from the live counter.

---

## 8. NEW rough edges (beyond balance-api's list)

1. **`priceUsd` is a JSON string, not a number.** DexScreener returns `"priceUsd": "0.03611"`,
   so you must `.as_str()?.parse::<f64>()`, not `.as_f64()`. (Liquidity *is* a number under
   `liquidity.usd`.)

2. **`pairs` is `null` for an unknown token, not `[]`.** An address DexScreener has never
   indexed comes back as `{"pairs": null}` (or `{}`), so match on `Some(Value::Array)` and
   treat anything else as "no pool" → 404, rather than assuming an array.

3. **Filter by `chainId` before picking a pool — a token symbol can exist on many chains.**
   The same contract address can be listed across chains; we hard-filter `chainId == "base"`
   and then take the deepest-liquidity pool so the price is the on-Base price, not whatever
   pool happens to be first.

4. **`reqwest` must use `rustls-tls`, not the default `native-tls`.** `default-features =
   false` + `features = ["json", "rustls-tls"]` keeps TLS on rustls/ring, consistent with the
   dual-provider `ring` install. Letting it pull native-tls would add an OpenSSL build dep and
   sidestep the provider fix.

5. **The zero address (`0x000…0`) is NOT a good 404 fixture** — DexScreener actually has Base
   pools tagged to it, so it returns 200. Use a random unindexed address (e.g.
   `0x1234…5678`) to demonstrate the 404 path.
