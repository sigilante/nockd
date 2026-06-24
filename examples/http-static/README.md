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
Built clean against nockchain rev `6d29078e69b64febabe3d8d20a64c06b969a16ed` with the
`rust-toolchain.toml` nightly (`nightly-2026-04-03`).

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

The app serves on **8083** by default (override with the `HTTP_PORT` env var).

⚠️ At the pinned rev, the library HTTP driver hardcodes its local backend to `127.0.0.1:8080`,
so this app runs the driver on 8080 and exposes 8083 via a tiny in-process TCP proxy (see
`src/main.rs`). A side effect: **two library-driver NockApps cannot run in local mode at the
same time** — both want port 8080. Run `http-static` *or* `http-counter`, not both. (Upstream
PR #134 adds an `HTTP_PORT` override that removes the proxy and the collision; the suite will
adopt it when all revs are bumped together.)

See [`RECIPE.md`](RECIPE.md) for the full build/deploy transcript and rough edges.
