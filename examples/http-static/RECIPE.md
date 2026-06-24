# RECIPE — building & deploying `http-static` end to end

The honest transcript for scaffolding a **stateless, static-content HTTP NockApp** from the
`http-static` template, building it with the real `nockup` toolchain, and deploying it under
`nockd` in **project mode** with a live REQ status metric.

Verified on macOS (darwin, aarch64-apple-darwin) on 2026-06-24, against nockchain rev
`07577127958db94be12e95ea816f31bc7582aa2c` (PR #134, one commit past `6d29078`, adding the
`HTTP_PORT` override — see ROUGH EDGE D below).

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

## 2. Rev choice — `07577127…` (PR #134, for the `HTTP_PORT` override)

The `http-static` template pins `rev = "336f744…"`, which predates the toolchain/API used by
this nockd suite. We pin all three runtime crates to http-counter's
`07577127958db94be12e95ea816f31bc7582aa2c` — the merge of **PR #134**, exactly ONE commit past
`6d29078`, whose ONLY diff is `crates/nockapp/src/drivers/http/http.rs` adding the `HTTP_PORT`
env var. With `rust-toolchain.toml`'s `nightly-2026-04-03`, the Rust + Hoon build was clean. No
`rustls`/`nockapp-grpc` needed — this app has no chain endpoint and makes no TLS calls.

API: `boot::setup` takes `cli: Cli` (not `Some(cli)`) and `NockApp` is generic `NockApp<J>`
(inferred) — same as `6d29078`, so **no new drift**. Bumping from `6d29078` to `07577127`
required ZERO source changes other than deleting the proxy and adding the `HTTP_PORT` line.

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

## ✅ FORMER ROUGH EDGE D — two library-driver apps can now share local mode (PR #134)

http-counter's old ROUGH EDGE A: the local HTTP driver hardcoded `127.0.0.1:8080` with no
override, worked around with an in-process TCP proxy (public port → 8080). The NEW problem this
recipe originally documented: because the *backend* port was hardcoded, **two library-driver
NockApps could not run in local mode simultaneously** — both tried to bind `127.0.0.1:8080` and
the second failed. We had to `nockd stop http-counter` before bringing http-static up.

**Resolved by PR #134 (this rev, `07577127…`):** the local driver now reads `HTTP_PORT` and
binds `127.0.0.1:<HTTP_PORT>` **directly**. http-static sets `HTTP_PORT=8083`, http-counter sets
`HTTP_PORT=8081`, and they bind their own ports with no shared `:8080` — so they **run at the
same time**. Verified directly: `lsof -nP -iTCP:8081 -iTCP:8083 -sTCP:LISTEN` shows two procs on
two ports, nothing on 8080, and concurrent `curl localhost:8081/` + `curl localhost:8083/` both
return their pages while `nockd ps` shows both `running`/`verified`. The proxy code is gone.

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

No need to free any port — the driver binds 8083 directly (rough edge D resolved), so it does
not touch 8080 and does not collide with http-counter:

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

## 8. Verify — including SIMULTANEOUS with http-counter

```sh
nockd ps
# NAME          STATE    HEALTH   VERIFIED  PID    ENDPOINT  STATUS
# http-counter  running  unknown  verified  11045  —         COUNT 5
# http-static   running  unknown  verified  10984  —         REQ 29

curl -s localhost:8083/        # full static landing page (h1 "http-static")
curl -s localhost:8083/about   # static about page (h1 "About this NockApp")
for i in 1 2 3 4; do curl -s -o /dev/null localhost:8083/; done
nockd ps | grep http-static    # REQ climbed (e.g. 23 → 29): increments per request

# === SIMULTANEITY PROOF (the point of the HTTP_PORT bump) ===
curl -s localhost:8083/ | grep -o '<title>[^<]*'      # http-static on :8083
curl -s localhost:8081/ | grep -io 'Count: *[0-9]*'   # http-counter on :8081, concurrently
lsof -nP -iTCP:8081 -iTCP:8083 -sTCP:LISTEN           # two procs, two ports, none on :8080
```

`running + verified` (self-signed by the trusted builder key). REQ status populated and climbs
one per request. The served HTML is identical every time — the headline feature (static
content from the kernel) — AND **http-static and http-counter served concurrently** on
8083/8081, which was impossible before PR #134. Both confirmed.

---

## 9. Summary of rough edges (vs http-counter)

- **D. (RESOLVED by PR #134.)** Two library-driver apps used to be unable to share local mode —
  both hardcoded `127.0.0.1:8080`. The driver now reads `HTTP_PORT` and binds it directly, so
  each app binds its own port and they run simultaneously. No more `nockd stop` dance, no proxy.

Everything from http-counter's RECIPE (rough edges A-resolved/B/C) and chain-watch's (1–8) still
applies unchanged — and project-mode deploy works on this nockd, so we use it.
