# chain-watch

A minimal but complete, **long-lived NockApp service** deployed under
[`nockd`](../../). It connects to the Nockchain public gRPC endpoint, polls the chain tip
(heaviest block height) every ~10 seconds, and logs one clean, greppable line per poll so
`nockd`'s status command can scrape it into a live `HEIGHT` metric.

It is the **pathfinder example**: it exercises two specific `nockd` features end to end —
**endpoint-by-name** and a **custom status metric** — and the build/deploy transcript in
[`RECIPE.md`](./RECIPE.md) is the canonical recipe other examples copy.

## What it does

- Boots a (trivial) Hoon kernel from `out.jam` via `nockapp::kernel::boot::setup`, so it is
  a real, supervised NockApp with its own state dir — not just a loose Rust process.
- Runs a Rust poll loop (`src/main.rs`): every 10s it calls
  `PublicNockchainGrpcClient::explorer_heaviest_height()` against the configured endpoint
  and prints `metric: height=<N>` on its own line.
- Resolves its endpoint **by name**: `nockd` injects the registry URL by substituting
  `{endpoint}` in the app's args and by setting `NOCKD_ENDPOINT_URL`. The app prefers an
  explicit `--endpoint <url>`, then `NOCKD_ENDPOINT_URL`, then a public default.
- Handles `SIGTERM`/Ctrl-C cleanly (nockd SIGTERMs on stop/restart).

## Architecture

A NockApp = a Rust wrapper (`src/main.rs`) that reads `out.jam` and boots a Hoon kernel,
plus `hoon/app/app.hoon` wrapped by `hoon/common/wrapper.hoon`. This example keeps the
kernel as the stock `basic` template kernel — all the chain logic lives in Rust. The chain
client is the `nockapp-grpc` crate's v2 `PublicNockchainGrpcClient`.

## Build

`nockup` resolves the project from the **parent** directory by package name (see the
gotcha in `RECIPE.md`), so build it like this:

```sh
cd examples            # the PARENT of chain-watch/
nockup project build chain-watch
```

This produces `chain-watch/target/release/chain-watch` and `chain-watch/out.jam`.

## Deploy

```sh
export PATH="$PATH:/path/to/nockd/target/release"
nockd serve &                                   # if not already running
nockd key gen                                    # once: builder identity → "verified"
nockd endpoint add mainnet-rpc https://rpc.nockchain.net

# Canonical one-command deploy (see RECIPE.md for the project-mode caveat):
cd examples/chain-watch
nockd deploy -f nockd.toml
```

> **Heads-up (see RECIPE.md):** `nockd.toml` uses `project = "."` (real-toolchain build via
> `nockup`), which is the intended UX, but **project-mode is currently broken** because
> `nockd` invokes `nockup project build` in a way `nockup` doesn't accept (details +
> reproduction in `RECIPE.md`). Until that's fixed, deploy the prebuilt artifact:
>
> ```sh
> nockd deploy chain-watch \
>   --bin ./target/release/chain-watch --jam ./out.jam \
>   --restart always --endpoint mainnet-rpc \
>   --status-label HEIGHT \
>   --status-cmd "grep -aoE 'height=[0-9]+' | tail -1 | grep -aoE '[0-9]+'" \
>   -- --endpoint '{endpoint}'
> ```

## See it work

```sh
nockd ps                  # chain-watch → running · verified · mainnet-rpc · HEIGHT <N>
nockd logs chain-watch    # metric: height=<N> lines, tracking the live tip
nockd endpoint list       # mainnet-rpc shows APPS=1
```

`<N>` is the live Nockchain heaviest height (~92,800+ as of writing) and increases as new
blocks arrive.

## Deploy one per endpoint (a fleet)

Because endpoints are referenced **by name** and each app gets its own state dir, the same
artifact can run as several supervised instances — one per RPC endpoint — distinguished only
by app name + endpoint name. This turns chain-watch into a fleet that shows each endpoint's
tip side by side in `nockd ps` (and lets you spot a lagging node at a glance).

```sh
cd examples/chain-watch

# Register the endpoints (idempotent; updates the URL if it already exists):
nockd endpoint add mainnet-rpc https://rpc.nockchain.net
nockd endpoint add nockbox     https://rpc.nockbox.org
nockd endpoint add zorp        http://23.252.122.18:5556

# Deploy one instance per endpoint. Same --bin/--jam; unique app name + --endpoint.
for ep in mainnet-rpc nockbox zorp; do
  nockd deploy "chain-watch-$ep" \
    --bin ./target/release/chain-watch \
    --jam ./out.jam \
    --restart always \
    --endpoint "$ep" \
    --status-label HEIGHT \
    --status-cmd "grep -aoE 'height=[0-9]+' | tail -1 | grep -aoE '[0-9]+'" \
    -- --endpoint '{endpoint}'
done

nockd ps          # chain-watch-mainnet-rpc / -nockbox / -zorp, each with its own HEIGHT
nockd endpoint list   # APPS column shows the instance count per endpoint
```

Each instance is independent: restart, stop, or roll back one without touching the others.
To re-point an instance at a different node, update the endpoint URL
(`nockd endpoint add <name> <new-url>`) and restart that app — no rebuild, no redeploy.

(The same fleet can be expressed declaratively: copy `nockd.toml` to
`nockd.<endpoint>.toml`, change `app` and `endpoint`, and `nockd deploy -f` each — once
project-mode is fixed, see RECIPE.md ROUGH EDGE 7.)

## Files

- `nockapp.toml` — project manifest (package + template + Hoon deps).
- `Cargo.toml` — Rust deps; pins the nockchain crates to a rev that has the v2
  public-gRPC client, and adds `rustls` with the `ring` provider (see RECIPE.md).
- `rust-toolchain.toml` — pins the nightly the nockchain crates require.
- `src/main.rs` — the wrapper: boots the kernel, runs the poll loop.
- `hoon/` — the stock `basic` kernel (`app.hoon`, `common/wrapper.hoon`, `lib/lib.hoon`).
- `nockd.toml` — the declarative deploy manifest.
- `RECIPE.md` — the canonical, honest build/deploy transcript with every error + fix.
