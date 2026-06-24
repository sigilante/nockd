# nockd examples

A gallery of small, *working* NockApps you can deploy under nockd in one command. Each
example lives in its own directory with a `nockd.toml` and a README.

```sh
cd examples/<name>
nockd deploy -f nockd.toml
nockd ps          # running · verified · healthy
nockd logs <name>
```

## Building a new example

1. Read [`GOTCHAS.md`](./GOTCHAS.md) first — it has the toolchain workarounds you'll
   otherwise rediscover.
2. Copy [`_skeleton/`](./_skeleton/) and follow its README: it's a known-good starting point
   (toolchain pin, `nockd.toml`, the TLS/rustls pin, the metric-log convention).
3. Close the loop per example — build → deploy → verify → commit — before starting the next.

## Targets (each demos a distinct nockd feature)

| Example | Demonstrates |
|---|---|
| `chain-watch` | endpoint-by-name + status metric + verified deploy (the canonical "talks to Nockchain" app) |
| `http-counter` | http-server + state persistence across restart |
| `balance-api`  | reading chain state over gRPC (explorer/wallet backend) |
| `echo-grpc`    | the private-gRPC poke/peek surface |
| `oracle`       | signing + posting — **needs nockd secrets (not built yet)**; build it to pull that feature |

See [`../NOCKUP-TODO.md`](../NOCKUP-TODO.md) for the upstream nockup issues these examples
surfaced.
