# nock-price

A standalone, long-lived **NockApp** service that polls the live **$NOCK** token price from
multiple venues and surfaces a median **aggregate USD price** as a `nockd` status metric.

It is a sibling of the [`chain-watch`](../chain-watch) example and has the same shape: a Rust
wrapper boots a trivial Hoon kernel (so it is a real, supervised NockApp), then runs a poll
loop that hits external HTTPS APIs and logs one greppable metric line per source per tick,
plus one aggregate line. `nockd` scrapes the aggregate line into the `USD` column of
`nockd ps`.

## What it does

Every **60 seconds** (respecting public-API rate limits) it fetches the $NOCK price from
three venues, **independently and resiliently** — any source that errors, times out, or
returns 0/missing is skipped for that tick and never crashes the loop:

| Source | Endpoint | Field | Notes |
|---|---|---|---|
| **BASE** (anchor) | DexScreener `tokens/0x9B5E…1722` | highest-liquidity Base pair `.priceUsd` | Deepest pool is Aerodrome NOCK/USDC. No key. |
| **KRAKEN** | `Ticker?pair=NOCKUSD` | `result.NOCKUSD.c[0]` | Returns `0` until the listing trades; **self-activates** once `c[0] > 0`. No key. |
| **SAFETRADE** | CoinGecko `coins/nockchain/tickers` | ticker `market.name == "SafeTrade"` → `converted_last.usd` | Via the **CoinGecko mirror** because `safe.trade` is Cloudflare-blocked to non-browser/server clients. USDT ≈ USD. |

The **aggregate** is the **median** of the live (non-skipped) sources — robust to one venue
being an outlier or temporarily down.

This app talks only to these price APIs (baked into the wrapper). It does **not** use a
Nockchain RPC endpoint, so there is no `endpoint` field / no `--endpoint` arg.

### Metric lines (what `nockd` scrapes)

Each tick logs clean, greppable lines:

```
metric: nock_usd_base=0.03232
metric: nock_usd_kraken=skip        # "skip" (not a number) until Kraken trades
metric: nock_usd_safetrade=0.03285
metric: nock_usd=0.03258            # <- the median aggregate, on its OWN line
```

`nock_usd=` does **not** match `nock_usd_base=` under an `=`-anchored grep, so the status
command isolates the aggregate cleanly. Prices are decimal, so the scrape allows `.`.

## Build

```sh
export PATH="$PATH:/Users/neal/zorp/nockd/target/release"
# nockup resolves the project as a subdir by NAME, so build from the PARENT dir:
cd examples
nockup project build nock-price
# -> examples/nock-price/target/release/nock-price  and  examples/nock-price/out.jam
```

(See `RECIPE.md` for why it must be run from the parent, the pinned toolchain, etc.)

## Deploy (prebuilt — the working path)

Project-mode deploy (`nockd deploy -f nockd.toml`) is currently **broken** on this
nockd/nockup pair (inherited from chain-watch — see `RECIPE.md`). Deploy the prebuilt
artifacts instead:

```sh
cd examples/nock-price
nockd deploy nock-price \
  --bin ./target/release/nock-price \
  --jam ./out.jam \
  --restart always \
  --status-label USD \
  --status-cmd 'grep -aoE "nock_usd=[0-9.]+" | tail -1 | grep -aoE "[0-9.]+"'
```

The shipped `nockd.toml` documents the intended project-mode UX (and carries the same
`[deploy.status]` recipe) for when project-mode is fixed.

## See it running

```sh
nockd ps
# NAME        STATE    HEALTH   VERIFIED  PID    ENDPOINT  STATUS
# nock-price  running  unknown  verified  …      —         USD 0.03258

nockd logs nock-price | grep -a 'metric:'
# metric: nock_usd_base=0.03232
# metric: nock_usd_kraken=skip
# metric: nock_usd_safetrade=0.03285
# metric: nock_usd=0.03258
```

The `USD` status should read ~0.03 (the live $NOCK price), driven at minimum by the Base
anchor. Kraken will show `skip` until its listing actually trades, then it contributes
automatically. SafeTrade contributes via the CoinGecko mirror.

## Lifecycle

Use `nockd` to manage it — **do not** `pkill`:

```sh
nockd restart nock-price
nockd stop nock-price
```

The wrapper handles `SIGTERM` (and Ctrl-C) cleanly; `nockd` SIGTERMs on stop/restart.

## Status-cmd gotcha (`grep -a`)

The `-a` in the status command is **load-bearing**: the nockapp kernel-boot log contains NUL
bytes, and BSD `grep` (macOS) treats NUL-bearing stdin as binary and silently suppresses
`-o`. `grep -a` forces text mode (correct on both BSD and GNU grep). Plain `grep -oE` leaves
the `USD` column blank. See `RECIPE.md`.
