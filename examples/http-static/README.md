# http-static

The simplest **"serve a page"** NockApp: an HTTP server that serves **static HTML straight
from the Hoon kernel**. No mutable state, no counter — every request returns the same page.

It is the static counterpart to [`http-counter`](../http-counter/): same HTTP plumbing, but the
kernel just answers `GET` requests with fixed HTML.

## What it does

- `GET /` (and `/index.html`) → a static landing page describing the app.
- `GET /about` → a static "about this NockApp" page.
- Anything else → a static `404` page.
- On **every** request the kernel logs `metric: requests=<N>` (a monotonically increasing
  request tally), which nockd surfaces as the **REQ** column in `nockd ps`. The tally is the
  only state; the *served content* never changes.

The HTML (and a little inline CSS) lives entirely in the Hoon kernel:
[`hoon/app/app.hoon`](hoon/app/app.hoon). The Rust binary
([`src/main.rs`](src/main.rs)) just boots the kernel from `out.jam` and attaches the library
HTTP driver.

## Build

From the `examples/` workspace directory (the one with `nockapp.toml` projects under it):

```sh
nockup project build http-static
```

This produces `out.jam` (the compiled kernel) and `target/release/http-static` (the runtime).
Built clean against nockchain rev `07577127958db94be12e95ea816f31bc7582aa2c` (PR #134, which
adds the `HTTP_PORT` override) with the `rust-toolchain.toml` nightly (`nightly-2026-04-03`).

## Deploy under nockd (project mode)

`nockd.toml` ships `project = "."`, so nockd builds the artifact via nockup and runs it.
This is a single-bin project, so no `bin_target` is needed.

```sh
export PATH="$PATH:/path/to/nockd/target/release"
nockd deploy -f nockd.toml      # registers + builds the app
nockd restart http-static       # swaps in the freshly built artifact
nockd ps                        # http-static → running / verified / REQ <n>
```

(There is no Nockchain endpoint, so there is no `endpoint` field and no `--endpoint` flag.)

## See it work

```sh
curl http://127.0.0.1:8083/          # the static landing page
curl http://127.0.0.1:8083/about     # the static about page
curl -i http://127.0.0.1:8083/nope   # 404 static page

# Each request bumps the tally; watch REQ climb:
for i in 1 2 3; do curl -s -o /dev/null http://127.0.0.1:8083/; done
nockd ps | grep http-static          # REQ increased by 3
```

Example response for `GET /` (formatting collapsed):

```html
<!doctype html><html><head><title>http-static</title><style>…</style></head>
<body><h1>http-static</h1>
<p>A NockApp that serves <strong>static content</strong> straight from the Hoon kernel
   &mdash; no mutable state, just a page.</p>
<p>Every <code>GET /</code> returns this exact HTML.</p>
<nav><a href="/">home</a><a href="/about">about</a></nav>
<footer>served by the Hoon kernel via nockd</footer></body></html>
```

## Port

The app serves on **8083**, set via the `HTTP_PORT` env var in `src/main.rs`.

As of **PR #134** (rev `07577127…`, the rev pinned here) the library HTTP driver reads
`HTTP_PORT` and binds `127.0.0.1:<HTTP_PORT>` **directly** — no proxy, and no shared `:8080`
backend. Because each app binds its own port, **http-static (8083) and
[`http-counter`](../http-counter/) (8081) run at the same time** — which was impossible at the
old rev, where the driver hardcoded `127.0.0.1:8080` and only one app could hold it.

See [`RECIPE.md`](RECIPE.md) for the full build/deploy transcript and rough edges.
