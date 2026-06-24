# http-counter

A minimal but complete **HTTP NockApp** deployed under [`nockd`](../../): it serves an HTML
page with a counter and Increment / Reset buttons, and **the count PERSISTS across
restarts**. That persistence is the headline feature — the counter lives in the Hoon kernel
state, which `nockd` checkpoints (PMA + event log), so it survives `nockd restart` and full
process restarts for free.

It is adapted from the `http-server` template (which already implements the counter) plus
the proven build/deploy pattern from [`chain-watch`](../chain-watch/RECIPE.md). The honest,
copyable transcript is in [`RECIPE.md`](./RECIPE.md).

## What it does

- Boots a Hoon kernel from `out.jam` via `nockapp::kernel::boot::setup`, so it is a real,
  supervised NockApp with its own state dir.
- The kernel (`hoon/app/app.hoon`) holds the counter in its state (`[%0 value=@]`):
  - `GET /` renders the HTML page with the current count.
  - `POST /increment` bumps the count and re-renders.
  - `POST /reset` sets it to 0.
- On **every** request the kernel logs one clean, greppable line `metric: count=<N>`, so
  `nockd`'s status command can scrape a live `COUNT` metric (visible in `nockd ps`).
- Serves on **`http://127.0.0.1:8081`** (set via the `HTTP_PORT` env var in `src/main.rs`; the
  stock library driver binds it directly as of PR #134, so this coexists with `http-static`).
- Handles `SIGTERM`/Ctrl-C cleanly (nockd SIGTERMs on stop/restart).

## Persistence — the headline feature

The count is kernel state. `nockd` snapshots kernel state to the app's state dir
(`event-log.sqlite3` + `pma/`), so it is restored on boot. Demo:

```sh
curl -X POST http://127.0.0.1:8081/increment   # Count: 1
curl -X POST http://127.0.0.1:8081/increment   # Count: 2
curl -X POST http://127.0.0.1:8081/increment   # Count: 3

nockd restart http-counter                     # new PID, same state

curl http://127.0.0.1:8081/                     # Count: 3  ← survived the restart
curl -X POST http://127.0.0.1:8081/increment   # Count: 4  ← continues from 3, not reset
```

Verified end to end: count `5` survived a `nockd restart` (and a hand-run SIGTERM/restart),
then continued incrementing from the persisted value.

## Architecture note — port binding via `HTTP_PORT`

The batteries-included way to drive HTTP from a NockApp is the library's
`nockapp::http_driver()`, which speaks the `%req`/`%res` noun protocol this kernel expects.
As of **PR #134** (rev `07577127…`, the rev pinned here) the local-mode driver reads the
**`HTTP_PORT`** env var and binds `127.0.0.1:<HTTP_PORT>` **directly** — so `src/main.rs` just
sets `HTTP_PORT=8081` before the driver starts. **No proxy.** Because each app now binds its
own port (no shared `:8080` backend), http-counter (8081) and
[`http-static`](../http-static/) (8083) **run at the same time** — which was impossible at the
old rev, where the driver hardcoded `127.0.0.1:8080`.

`src/main.rs` also sets `EXPIRE_CACHE=0` so the library driver does **not** serve cached GET
responses — every request re-pokes the kernel, keeping the displayed count fresh and emitting
a `metric: count=<N>` line per request.

## Build

`nockup` resolves the project from the **parent** directory by package name (see RECIPE.md),
so build it like this:

```sh
cd examples            # the PARENT of http-counter/
nockup project build http-counter
```

This produces `http-counter/target/release/http-counter` and `http-counter/out.jam`. Built
clean against nockchain rev `07577127958db94be12e95ea816f31bc7582aa2c` (PR #134, which adds
the `HTTP_PORT` override) with the nightly pinned in `rust-toolchain.toml`.

## Deploy

```sh
export PATH="$PATH:/path/to/nockd/target/release"
nockd serve &        # if not already running
nockd key gen        # once: builder identity → "verified"
```

`nockd.toml` ships with `project = "."` (the intended real-toolchain UX): nockd shells out to
nockup with the absolute project path to build the artifact, then ships it.

```sh
cd examples/http-counter
nockd deploy -f nockd.toml      # registers + builds the app via nockup
nockd restart http-counter      # swaps in the freshly built artifact
```

This app has **no** Nockchain endpoint, so there is no `--endpoint`.

## See it work

```sh
nockd ps                  # http-counter → running · verified · COUNT <N>
curl http://127.0.0.1:8081/                       # HTML page, "Count: <N>"
curl -X POST http://127.0.0.1:8081/increment      # bumps the count, re-renders
curl -X POST http://127.0.0.1:8081/reset          # back to 0
nockd logs http-counter | grep -a 'metric: count' # one line per request
```

The `COUNT` column in `nockd ps` tracks the latest count (e.g. `COUNT 5`) and the rendered
page agrees.

## Files

- `nockapp.toml` — project manifest (package + template + Hoon deps).
- `Cargo.toml` — Rust deps; pins the nockchain crates to rev `07577127…` (PR #134, which adds
  the `HTTP_PORT` override so the driver binds 8081 directly).
- `rust-toolchain.toml` — pins the nightly the nockchain crates require (`cold_path` fix).
- `src/main.rs` — the wrapper: sets `HTTP_PORT=8081`, boots the kernel, attaches
  `http_driver()`, handles SIGTERM.
- `hoon/app/app.hoon` — the counter kernel (GET/increment/reset + the `metric: count` slog).
- `hoon/lib/http.hoon`, `hoon/common/wrapper.hoon` — HTTP nouns/helpers and the kernel
  wrapper (from the http-server template).
- `nockd.toml` — the declarative deploy manifest (status recipe in `[deploy.status]`).
- `RECIPE.md` — the honest build/deploy transcript with every error + fix.
