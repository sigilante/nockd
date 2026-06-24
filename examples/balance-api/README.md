# balance-api

An HTTP "explorer backend" NockApp: given a Nockchain pubkey, it returns that pubkey's
balance **read live from the chain over gRPC**.

```
GET /balance/<pubkey>      -> {"pubkey":"...","balance":<nicks>,"unit":"nicks","notes":<n>,"height":<h>}
GET /?pubkey=<pubkey>      -> same
GET /                      -> 400 with usage help
```

- `balance` is the total across all of the pubkey's unspent notes (UTXOs), in **nicks**
  (1 NOCK = 65536 nicks).
- `notes` is how many UTXOs the pubkey holds; `height` is the chain height the snapshot was
  computed at.
- Chain access is **READ-ONLY** — it uses the public `wallet_get_balance` gRPC method. The
  app never signs or sends anything.

## Architecture

This is the [`chain-watch`](../chain-watch/) shape — a **Rust chain client + a trivial Hoon
kernel** — but with an **HTTP listener instead of a poll loop**:

- A pure-Rust HTTP server (`axum`) listens on **127.0.0.1:8082**. On each request it dials
  the Nockchain public RPC and calls `PublicNockchainGrpcClient::wallet_get_balance`, sums
  the per-note `assets`, and replies with JSON.
- The Hoon kernel is just the `basic` template kernel, booted from `out.jam` so the process
  is a valid supervised NockApp the way nockd expects. **All logic is in Rust.** The chain
  read is *not* routed through the kernel — nockd's bundled `http_driver` is hardcoded to
  `:8080` with crate-private helpers (see `../../../nockd/examples/GOTCHAS.md`), so serving
  HTTP directly in Rust is the clean path.

## Build

```sh
export PATH="$PATH:/Users/neal/.nockup/bin"
cd examples
nockup project build balance-api        # run from the PARENT dir with the project NAME
```

(Building as `nockup project build .` or no-arg from inside the project fails — nockup
resolves the package name as a *subdirectory*. See RECIPE.md.)

## Deploy (project-mode, under nockd)

```sh
export PATH="$PATH:/Users/neal/zorp/nockd/target/release"

# One-time: register the read-only public RPC endpoint by name.
nockd endpoint add mainnet-rpc https://rpc.nockchain.net

cd examples/balance-api
nockd deploy -f nockd.toml      # builds via nockup, registers the artifact
nockd restart balance-api       # swaps the live process onto the new artifact

nockd ps                        # balance-api  running  verified  ...  REQ <n>
```

`nockd.toml` uses `project = "."` (real client-side toolchain build), `endpoint =
"mainnet-rpc"` (nockd substitutes the URL into `--endpoint {endpoint}`), and passes
`--port 8082`. The status line scrapes `metric: requests=<N>` into the **REQ** column.

## Query it

```sh
# A real, funded mainnet pubkey (≈505 NOCK across 104 notes at the time of writing):
PK=2bc9h9E8zBHeJCyp9QWEmwGdX9uLGDRwJJMJMe8GEeSKKkPmoBx4Kq5ME8mic9WrhjfRmGeruy56zfWVZnqwrxChyRSHGUxDCgJzRd7RmH4qM7JGmGUpRypYJtK7yVEWTu1e

curl http://127.0.0.1:8082/balance/$PK
# {"balance":33116464,"height":92954,"notes":104,"pubkey":"2bc9h9E8...WTu1e","unit":"nicks"}
#   33116464 nicks = 505.317… NOCK

# Query-string form:
curl "http://127.0.0.1:8082/?pubkey=$PK"

# Malformed pubkey -> 400:
curl -i http://127.0.0.1:8082/balance/notapubkey
# {"detail":"expected a base58 Nockchain pubkey","error":"invalid pubkey","pubkey":"notapubkey"}

# Missing pubkey -> 400 with usage:
curl -i http://127.0.0.1:8082/
# {"error":"missing pubkey","usage":"GET /balance/<pubkey>  or  GET /?pubkey=<pubkey>"}
```

A syntactically valid but unfunded pubkey returns a well-formed `200` with `"balance":0`,
`"notes":0`.

## Status & shutdown

- Every lookup logs one greppable line `metric: requests=<N>`; nockd surfaces the cumulative
  count as `REQ <n>` in `nockd ps`.
- SIGTERM (and Ctrl-C) trigger a graceful axum shutdown, so `nockd stop`/`nockd restart`
  are clean. Use those — never `pkill -f balance-api` (nockd runs the app from its artifact
  path, not by name).

See [RECIPE.md](./RECIPE.md) for the full build/deploy transcript and rough edges.
