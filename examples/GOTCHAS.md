# Building NockApp examples for nockd — gotchas

Hard-won rough edges, with the exact workaround for each. Read this before building an
example so you don't re-discover them. Most are **upstream nockup/toolchain** issues, not
nockd — see [`../NOCKUP-TODO.md`](../NOCKUP-TODO.md).

> Status legend: **[nockd-fixed]** handled by nockd now · **[app-workaround]** you must do
> this in your example · **[upstream]** a nockup/template bug you route around.

---

## Toolchain (do these for every example)

### 1. Pin the Rust toolchain — **[app-workaround]**
nockup builds with the *ambient* Rust toolchain, not the one the pinned rev needs. The
symptom is a cryptic `error[E0658]` about `cold_path` from `nockvm`. Drop a
`rust-toolchain.toml` in the project root (copy from [`_skeleton/`](./_skeleton/)):

```toml
[toolchain]
channel = "nightly-2026-04-03"
```

### 2. Pin one rustls crypto provider (TLS apps) — **[app-workaround]**
Any app that opens an `https://` connection (chain readers, anything dialing
`rpc.nockchain.net`) can pull **two** rustls providers (`ring` + `aws-lc-rs`) into the graph,
which **panics on the first TLS handshake**. Force one and install it. In `Cargo.toml`:

```toml
rustls = { version = "0.23", default-features = false, features = ["ring"] }
```

…and at the very top of `main()`, before any TLS:

```rust
let _ = rustls::crypto::ring::default_provider().install_default();
```

(nockd itself isn't affected — its TLS already works against `rpc.nockchain.net`. This is
purely the app dependency graph.)

### 3. Use a current nockchain rev — **[upstream]**
The `grpc` template pins an old rev (`485e914`) that predates the v2 public client /
`explorer_heaviest_height`. Use **`6d29078`** (matches typhoon's `nockchain` workspace).
Bumping brings API drift you must follow: `boot::setup` takes `cli` (not `Some(cli)`), and
`NockApp<J>` is now generic.

### 4. `nockup project init` quirks — **[upstream]**
It nests the project under a `<name>/` subdir and leaves `rev = ""` unsubstituted in the
manifest. After init: flatten the dir if needed and set the rev to `6d29078`.

---

## nockd conventions (so the example deploys + is observable)

### Project-mode build now works — **[nockd-fixed]**
`nockd deploy --project <dir>` (and `project = "<dir>"` in `nockd.toml`) builds via nockup
and ships the artifact. (Earlier this was broken — nockd ran `nockup project build` no-arg
and nockup misread the package name as a subdir. Fixed: nockd passes the absolute path.)
The prebuilt path still works too: `nockd deploy <name> --bin … --jam …`.

### Multi-bin projects: name the bin target — **[nockd-fixed]**
nockup's multi-bin convention compiles `hoon/app/<bin>.hoon` → `target/release/<bin>` +
`<bin>.jam` per `[[bin]]` (e.g. the grpc template / echo-grpc's `listen` + `talk`) — there is
**no `out.jam`** in multi-bin mode. A single-bin project stays `target/release/<package>` +
`out.jam`. Since one nockd app is one process, tell nockd which bin to ship:

```toml
[deploy]
app        = "echo-grpc"
project    = "."
bin_target = "listen"   # → target/release/listen + listen.jam
```

or `--bin-target listen` on the CLI. Omit it for single-bin apps. (Without it, nockd looked
for `out.jam` and failed on multi-bin projects.)

### Log a clean, greppable metric line
If your app has a key number (height, requests, balance), **log it on one line** like
`metric: requests=42`. nockd's `--status-cmd` receives the **ANSI- and NUL-stripped** recent
log on **stdin**, so the recipe is just a grep — no perl, no `$NOCKD_LOG`, no platform
quirks:

```toml
[deploy.status]
label = "REQ"
cmd   = "grep -oE 'requests=[0-9]+' | tail -1 | grep -oE '[0-9]+'"
```

(NUL bytes in kernel-boot logs used to silently blank the metric on macOS — nockd strips
them now, so the recipe above works on every platform.)

### Serves a web page? Declare the port (not a URL) — **[nockd feature]**
Don't hardcode a port in your app. Declare it **once** in `nockd.toml` (`port = 8085`, or
`--web-port`); nockd exports it as **`NOCKD_APP_PORT`** (and substitutes `{port}` in args), so your
app reads the port from the environment and binds it — single source of truth, no duplication.
The dashboard then derives an **"Open app ↗"** relay link to `localhost:<port>` (plus a ↗ next
to the name in the table). Example bridge for an `HTTP_PORT`-style driver:

```rust
let port = std::env::var("NOCKD_APP_PORT").unwrap_or_else(|_| "8085".into()); // 8085 = standalone fallback
std::env::set_var("HTTP_PORT", &port);
```

### Edited nockd.toml? Reload, don't redeploy — **[nockd feature]**
Config (`port`, `args`, `status`, `endpoint`, `restart`) is written to the registry at deploy
time. After editing `nockd.toml`, click **Reload** on the dashboard status page (or
`nockd reload <app>`): the daemon re-reads the manifest it deployed from and re-applies the
config in place, then restarts — **no rebuild**. (Changed the *code*? That needs a real
`nockd deploy -f nockd.toml` to rebuild + ship a new artifact; the daemon never compiles.)
Only works for apps deployed with `-f` (the daemon needs a manifest path to re-read).

### Reference an endpoint by name
Chain apps take the RPC URL via an arg with the `{endpoint}` placeholder; nockd substitutes
the registered URL and also sets `NOCKD_ENDPOINT_URL`. Set `endpoint = "mainnet-rpc"` in
`nockd.toml` and register it once: `nockd endpoint add mainnet-rpc https://rpc.nockchain.net`.
That endpoint is **read-only** gRPC (TLS) and works today.

### Handle SIGTERM
nockd stops/restarts apps with SIGTERM (then SIGKILL after a grace period). Flush state on
SIGTERM for a clean shutdown — stateful nodes that don't can corrupt their PMA.

### Other facts
- nockd **never compiles** — you build client-side, it runs the artifact.
- **Binary-only** apps (embed the kernel, like nockchain) have **no `out.jam`** → deploy
  with `--bin` only. Template apps ship `out.jam`.
- An app runs with **cwd = its state dir**, so cwd-relative state (`./.data.<name>`) is
  isolated automatically.
- Don't `pkill -f <appname>` — nockd runs apps from the **artifact path**, not the name.

---

## Definition of done (per example)
1. `nockup project build` succeeds clean.
2. `nockd deploy -f nockd.toml` → `nockd ps` shows **running**, **verified** (self-attested),
   and **healthy** where relevant.
3. Observable output: a metric in `ps`/dashboard, an HTTP response, or a clear log line.
4. A `README.md`: what it does, `nockd deploy -f nockd.toml`, how to see it work.
5. Self-contained in its own dir under `examples/`; commit when green.
