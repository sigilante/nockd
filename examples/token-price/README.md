# token-price

An HTTP API NockApp: given a token's contract address on **Base**, it returns that token's
**live USD price**, read from [DexScreener](https://dexscreener.com). No API key.

```
GET /price/<base-token-address>   -> {"token":"0x…","price_usd":<f64>,"pair":"<dex/pair>","liquidity_usd":<f64>}
GET /                             -> a tiny HTML help page
```

- `price_usd` comes from the token's **deepest Base liquidity pool** (max `liquidity.usd`),
  which is the most reliable on-chain price.
- `pair` is `<dexId>/<base>-<quote>` for that pool (e.g. `aerodrome/NOCK-USDC`).
- `liquidity_usd` is that pool's USD liquidity.
- Data is **read-only** from the public DexScreener token API. The app never signs or sends
  anything on-chain.

## Architecture

This is the [`balance-api`](../balance-api/) shape — a **pure-Rust `axum` HTTP server that
calls an external API per request**, with a **trivial Hoon kernel**:

- The HTTP server listens on **127.0.0.1:8086**. On each request it calls
  `https://api.dexscreener.com/latest/dex/tokens/<address>`, filters the returned `pairs[]`
  to `chainId == "base"`, picks the pool with the most liquidity, reads its `priceUsd`
  (a string, already in USD), and replies with JSON.
- The Hoon kernel is just the `basic` template kernel, booted from `out.jam` so the process
  is a valid supervised NockApp the way nockd expects. **All logic is in Rust.**
- Unlike `balance-api`, the data source is **not** a Nockchain RPC, so there is no
  endpoint-by-name and no `--endpoint` arg — the DexScreener base URL is hardcoded.

## Build

```sh
export PATH="$PATH:/Users/neal/.nockup/bin"
cd examples
nockup project build token-price        # run from the PARENT dir with the project NAME
```

(Building as `nockup project build .` or no-arg from inside the project fails — nockup
resolves the package name as a *subdirectory*. See RECIPE.md.)

## Deploy (project-mode, under nockd)

```sh
export PATH="$PATH:/Users/neal/zorp/nockd/target/release"

cd examples/token-price
nockd deploy -f nockd.toml      # builds via nockup, registers the artifact
nockd restart token-price       # swaps the live process onto the new artifact

nockd ps                        # token-price  running  verified  ...  REQ <n>
```

`nockd.toml` uses `project = "."` (real client-side toolchain build) and passes `--port 8086`.
The status line scrapes `metric: requests=<N>` into the **REQ** column.

## Query it

```sh
# The $NOCK token on Base (deepest pool: Aerodrome NOCK/USDC):
curl http://127.0.0.1:8086/price/0x9B5E262cF9bb04869ab40b19AF91D2dc85761722
# {"liquidity_usd":1220824.69,"pair":"aerodrome/NOCK-USDC","price_usd":0.03611,"token":"0x9B5E262cF9bb04869ab40b19AF91D2dc85761722"}

# Malformed address -> 400:
curl -i http://127.0.0.1:8086/price/notanaddress
# {"detail":"expected an EVM address: 0x followed by 40 hex digits","error":"invalid token address","token":"notanaddress"}

# Valid format but no Base pool -> 404:
curl -i http://127.0.0.1:8086/price/0x1234567890abcdef1234567890abcdef12345678
# {"detail":"DexScreener has no Base (chainId=base) trading pair for this token","error":"no Base pool found",...}

# Help page:
curl http://127.0.0.1:8086/
```

## Status & shutdown

- Every lookup logs one greppable line `metric: requests=<N>`; nockd surfaces the cumulative
  count as `REQ <n>` in `nockd ps`.
- SIGTERM (and Ctrl-C) trigger a graceful `axum` shutdown, so `nockd stop`/`nockd restart`
  are clean. Use those — never `pkill -f token-price` (nockd runs the app from its artifact
  path, not by name).

See [RECIPE.md](./RECIPE.md) for the full build/deploy transcript and rough edges.
