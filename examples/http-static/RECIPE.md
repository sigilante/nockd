# RECIPE — building & deploying `http-static` end to end

The honest transcript for scaffolding a **stateless, static-content HTTP NockApp** from the
`http-static` template, building it with the real `nockup` toolchain, and deploying it under
`nockd` in **project mode** with a live REQ status metric.

Verified on macOS (darwin, aarch64-apple-darwin) on 2026-06-24, against nockchain rev
`6d29078e69b64febabe3d8d20a64c06b969a16ed`.

This recipe builds directly on [`http-counter/RECIPE.md`](../http-counter/RECIPE.md). All of
http-counter's rough edges (and chain-watch's underneath it) still apply: init nesting, empty
rev, ambient-toolchain `cold_path`, build-as-subdir / `{{project_name}}` cargo noise, broken
naive project-mode (now fixed in this nockd), BSD grep + NUL bytes, the hardcoded-8080 HTTP
driver, the GET cache, and the `++inner` door arm-shape constraint. Read that first. Below are
only the things that were NEW or different for a **static** app.

---

## 0. Environment

```sh
export PATH="$PATH:/Users/neal/zorp/nockd/target/release"   # nockd not on PATH by default
which nockup hoonc        # /Users/neal/.nockup/bin/{nockup,hoonc}
rustup toolchain list | grep 2026-04-03   # must be installed (rust-toolchain.toml pins it)
```

A `nockd serve` daemon with a builder key + the project-mode fix was already running. We picked
HTTP port **8083** (8081 is http-counter, 8082 balance-api, 5561 echo-grpc).

---

## 1. Scaffold — copy the template idea + http-counter's proven glue

`nockup project init` nests and leaves `rev=""` (chain-watch rough edge 1), so we assembled by
hand. We took the *static* kernel shape from the `http-static` template but reused
http-counter's proven build glue, toolchain pin, lockfile, wrapper, and HTTP lib:

```sh
mkdir -p examples/http-static/{hoon/app,hoon/lib,hoon/common,src}
cp examples/http-counter/build.rs            examples/http-static/
cp examples/http-counter/rust-toolchain.toml examples/http-static/
cp examples/http-counter/nockapp.lock        examples/http-static/
cp examples/http-counter/hoon/common/wrapper.hoon examples/http-static/hoon/common/
cp examples/http-counter/hoon/lib/{http,lib}.hoon examples/http-static/hoon/lib/
```

Then wrote `hoon/app/app.hoon`, `Cargo.toml`, `nockapp.toml`, `.gitignore`, `nockd.toml`,
`src/main.rs`.

---

## 2. Rev choice — `6d29078` (the template's `336f744` is too old)

The `http-static` template pins `rev = "336f744…"`, which predates the toolchain/API used by
this nockd suite. We pinned all three runtime crates to http-counter's proven
`6d29078…` instead. With `rust-toolchain.toml`'s `nightly-2026-04-03`, the Rust + Hoon build was
clean. No `rustls`/`nockapp-grpc` needed — this app has no chain endpoint and makes no TLS calls.

API at `6d29078`: `boot::setup` takes `cli: Cli` (not `Some(cli)`) and `NockApp` is generic
`NockApp<J>` (inferred) — same as http-counter, so no new drift.

---

## 3. The Hoon kernel — static pages, helper arms in the prelude

The kernel serves fixed HTML. Two design points carried over from http-counter's rough edges:

- **Door arm shape (http-counter rough edge C).** The `(keep)` wrapper's `fort` mold expects
  the `++inner` door to have EXACTLY `load`/`peek`/`poke`. So all page-rendering helpers
  (`++home`, `++about`, `++not-found`, `++css`, `++page-for`, `++render`) live in the **prelude
  core** (`=> |% … --`), NOT inside the door. `poke` just calls `(page-for uri)`.
- **Routing is a pure function of the URI.** `++page-for` maps `/` (and `/index.html`) → home,
  `/about` → about, everything else → a 404 page. No state is consulted for content.

The only state is `requests=@`, a tally bumped on every poke purely to drive the metric:

```hoon
=/  new-requests=@  +(requests.state)
~>  %slog.[0 leaf+"metric: requests={<new-requests>}"]
```

`++render` is `(to-octs (crip t))` — tape → `(unit octs)` response body.

---

## ⚠️ NEW ROUGH EDGE D — two library-driver apps can't share local mode (both hardcode :8080)

http-counter rough edge A established that the local HTTP driver hardcodes `127.0.0.1:8080`
with no override, worked around with an in-process TCP proxy (public port → 8080). For a single
app that's fine. The NEW observation for a *second* such app:

Because the backend port is hardcoded, **two library-driver NockApps cannot run in local mode
simultaneously** — both try to bind `127.0.0.1:8080` and the second fails. While http-counter
was running it held 8080, so http-static could not start its driver. We confirmed this directly
(`lsof -nP -iTCP:8080 -sTCP:LISTEN` showed http-counter's `bin` owning 8080).

**Workaround for now:** run **one** library-driver app at a time. We `nockd stop http-counter`
before bringing http-static up; the proxy then forwards 8083 → 8080 as usual. The public-port
override (`HTTP_PORT`) only moves the *proxy* port, not the backend, so it does not resolve the
collision.

**Real fix:** upstream PR #134 adds an `HTTP_PORT` env var to the local driver so it binds the
chosen port directly — no proxy, no shared 8080, no collision. The example suite will adopt it
when all revs are bumped together (kept on `6d29078` + proxy here to match http-counter).

---

## 4. The metric line (kernel-side)

Logged from Hoon inside `++poke`, on the tally bump, so it fires on EVERY request:

```hoon
~>  %slog.[0 leaf+"metric: requests={<new-requests>}"]
```

Status recipe in `nockd.toml` (the kernel boot log here did NOT need `-a`; a plain `grep -oE`
worked because nockd strips NULs on this build — kept the plain form the brief specified):

```toml
[deploy.status]
label = "REQ"
cmd   = "grep -oE 'requests=[0-9]+' | tail -1 | grep -oE '[0-9]+'"
```

---

## 5. Build

```sh
cd examples
nockup project build http-static     # ✅ Cargo + hoonc clean
ls -la http-static/out.jam http-static/target/release/http-static
# out.jam ~590K ; http-static ~16M Mach-O arm64
```

Same harmless noise as http-counter: `invalid character '{' in package name` (cargo scanning
the template dir) and a `build.rs` `unused import: fs` warning. Build still succeeds.

---

## 6. Smoke test the binary directly (before nockd)

Free 8080 first (`nockd stop http-counter` — see rough edge D), then:

```sh
cd examples/http-static
rm -rf .data.http-static
./target/release/http-static &                       # serves 127.0.0.1:8083
curl -s localhost:8083/        | grep -o '<title>[^<]*'   # http-static
curl -s localhost:8083/about   | grep -o '<h1>[^<]*'      # About this NockApp
curl -s -o /dev/null -w '%{http_code}\n' localhost:8083/nope   # 404
grep -a 'metric: requests' <logs>                    # one line per request
kill -TERM <pid>                                     # "received SIGTERM; shutting down cleanly"
```

All passed; SIGTERM logged a clean shutdown and freed both ports.

---

## 7. Deploy — PROJECT MODE (works on this nockd)

Unlike http-counter's recipe (which predated the fix and used the prebuilt path), project-mode
deploy works on this nockd/nockup pair:

```sh
cd examples/http-static
nockd deploy -f nockd.toml      # nockd shells out to nockup with the ABSOLUTE project path
nockd restart http-static       # swaps in the freshly built artifact
# deployed http-static
#   artifact bb51a296…
#   kernel   372d0a46…
```

`nockd.toml` carries `project = "."` (single-bin, so no `bin_target`). No `--endpoint`.

---

## 8. Verify

```sh
nockd ps
# NAME         STATE    HEALTH   VERIFIED  PID    ENDPOINT  STATUS
# http-static  running  unknown  verified  27042  —         REQ 10

curl -s localhost:8083/        # full static landing page (h1 "http-static")
curl -s localhost:8083/about   # static about page (h1 "About this NockApp")
for i in 1 2 3 4; do curl -s -o /dev/null localhost:8083/; done
nockd ps | grep http-static    # REQ climbed (e.g. 10 → 17): increments per request
```

`running + verified` (self-signed by the trusted builder key). REQ status populated and climbs
one per request. The served HTML is identical every time — the headline feature (static
content from the kernel), confirmed.

---

## 9. Summary of NEW rough edges (vs http-counter)

- **D. Two library-driver apps can't share local mode** — both hardcode `127.0.0.1:8080`, so
  only one can bind it. Stop the other (`nockd stop http-counter`) to run this one; the real
  fix is upstream PR #134's `HTTP_PORT` override (bump later, suite-wide).

Everything from http-counter's RECIPE (rough edges A/B/C) and chain-watch's (1–8) still applies
unchanged — except that **project-mode deploy now works** on this nockd, so we use it instead of
the prebuilt fallback.
