# RECIPE — building & deploying `http-counter` end to end

The honest transcript for scaffolding a **stateful HTTP NockApp** from the `http-server`
template, building it with the real `nockup` toolchain, and deploying it under `nockd` with a
live status metric — and **proving the counter persists across restarts**.

Verified on macOS (darwin, aarch64-apple-darwin) on 2026-06-24, against nockchain rev
`07577127958db94be12e95ea816f31bc7582aa2c` (PR #134, one commit past `6d29078`, adding the
`HTTP_PORT` override — see ROUGH EDGE A below).

This recipe builds on [`chain-watch/RECIPE.md`](../chain-watch/RECIPE.md). The eight rough
edges documented there (init nesting, empty rev, ambient toolchain `cold_path`, build-as-
subdir, rustls, broken project-mode deploy, BSD grep + NUL bytes) **all still apply**. Read
that first. Below are only the things that were NEW or different for an HTTP, stateful app.

---

## 0. Environment

```sh
export PATH="$PATH:/Users/neal/zorp/nockd/target/release"   # nockd not on PATH by default
which nockup hoonc        # /Users/neal/.nockup/bin/{nockup,hoonc}
rustup toolchain list | grep 2026-04-03   # must be installed (rust-toolchain.toml pins it)
```

A `nockd serve` daemon with a builder key was already running. We picked HTTP port **8081**
to avoid collisions with the other example apps.

---

## 1. Scaffold — copy the template + chain-watch's proven files (don't fight `init`)

Per chain-watch ROUGH EDGE 1, `nockup project init` nests and leaves `rev=""`. We skipped it
entirely and assembled the project by hand:

```sh
mkdir -p examples/http-counter
# Hoon from the http-server template (it ALREADY implements the counter):
cp templates/http-server/hoon/app/app.hoon        examples/http-counter/hoon/app/
cp templates/http-server/hoon/lib/{http,lib}.hoon examples/http-counter/hoon/lib/
cp templates/http-server/hoon/common/wrapper.hoon examples/http-counter/hoon/common/
# Build glue + toolchain pin + lockfile from chain-watch (proven):
cp examples/chain-watch/{build.rs,rust-toolchain.toml,nockapp.lock} examples/http-counter/
```

Then wrote `Cargo.toml`, `nockapp.toml`, `.gitignore`, `nockd.toml`, `src/main.rs`.

---

## 2. Rev choice — `07577127…` (PR #134, for the `HTTP_PORT` override)

The `http-server` template pins `rev = "336f744b6b83448ec2b86473a3dec29b15858999"` (too old).
We pin all three runtime crates (`nockapp`, `nockvm`, `nockvm_macros`) to
`07577127958db94be12e95ea816f31bc7582aa2c` — the merge of **PR #134**, exactly ONE commit
past `6d29078`, whose ONLY diff is `crates/nockapp/src/drivers/http/http.rs` adding the
`HTTP_PORT` env var. With the `rust-toolchain.toml` nightly (`nightly-2026-04-03`) in place,
the Rust + Hoon build was clean. (We did NOT need `nockapp-grpc` or `rustls` — this app has
no chain endpoint and makes no TLS calls.)

API drift: **none** beyond what chain-watch already flagged. `boot::setup` takes `cli`
directly (not `Some(cli)`) and `NockApp` is generic `NockApp<J>` (inferred) — identical to
`6d29078`, since PR #134 touches only the HTTP driver. The bump from `6d29078` to `07577127`
required ZERO source changes other than deleting the proxy and adding the `HTTP_PORT` line.

---

## ✅ FORMER ROUGH EDGE A — port binding (was: hardcoded 8080 + proxy; now: `HTTP_PORT`)

The template's `main.rs` does `nockapp.add_io_driver(http_driver()).await`. `http_driver` is
`pub use http::http::http as http_driver` — the full HTTP driver.

**The old problem (rev `6d29078`):** in local mode (when `HTTPS_DOMAIN` is unset/`localhost`)
the driver bound **`127.0.0.1:8080`**, hardcoded, with **no** port env var. Copying the driver
into the project to change the port did NOT work out-of-crate: the effect-parsing path needs
`NounSlab::noun_space()` and `ptr_ranges()` (both crate-private) to resolve the slab's
offset-form root. The only public-API workaround was a tiny in-process TCP proxy
(public port → 8080) via `tokio::io::copy_bidirectional` — which also meant two library-driver
apps couldn't run at once (they collided on 8080).

**The fix (this rev, `07577127…` / PR #134):** the local-mode driver now reads the
**`HTTP_PORT`** env var and binds `127.0.0.1:<HTTP_PORT>` directly. So `src/main.rs` just does:

```rust
std::env::set_var("HTTP_PORT", "8081");   // before the driver starts
```

No proxy, no `TcpListener`/`copy_bidirectional` glue, no shared `:8080`. Each app binds its own
port, so **http-counter (8081) and http-static (8083) run simultaneously** (verified — see §7).
The bump was purely mechanical: delete the proxy code, add the one `set_var` line.

---

## ⚠️ NEW ROUGH EDGE B — the library HTTP driver caches GET responses forever → stale count + no per-request metric

In local mode with `EXPIRE_CACHE` unset, the driver caches GET responses **per URI, never
expiring**. For a counter that is wrong twice over:

1. After the first `GET /` (count 0) is cached, later `GET /` returns the **stale** cached
   page even after increments.
2. A cached GET is served **without poking the kernel**, so the kernel never runs and never
   emits its `metric: count=<N>` slog line — the `nockd` status would go stale.

**Fix:** set `EXPIRE_CACHE=0` (in `src/main.rs`, before the driver starts). With duration 0,
`is_expired` is always true → the cache is effectively disabled → every request re-pokes the
kernel → fresh count + a metric line per request. (POST is never cached, but GET is, so this
matters for the demo and for the COUNT status.)

---

## ⚠️ NEW ROUGH EDGE C — extra arms in the Hoon door break the `(keep)` wrapper nest

The template's `app.hoon` inlines the page-rendering `weld` three times. We factored it into
a helper to splice the count in one place. First attempt put the helper as an arm **inside**
the `++inner` door:

```hoon
++  inner
  |_  state=server-state
  ++  load  ...
  ++  peek  ...
  ++  body  |=(v=@ ...)   ::  ← NEW arm
  ++  poke  ...
  --
```

This failed to compile with a `nest-fail` at `((moat |) inner)`. The wrapper's `fort` mold
(in `wrapper.hoon`) is a `$_` over a door with **exactly** `load`/`peek`/`poke`; adding a
fourth arm changes the battery shape so `inner` no longer nests against `fort`.

**Fix:** put the helper (`++render`) in the **prelude core** (`=> |% … --`, next to
`++page`), not inside the `++inner` door. Then call `(render value.state)` from `poke`. Door
arms must match the wrapper's expected shape; auxiliary functions go in the prelude.

(Also note: had the helper stayed in the door, the request's destructured `body=(unit octs)`
face would have shadowed a `body` arm — call `^body` to reach the arm. Moving it out avoids
the whole issue.)

---

## 3. The metric line (kernel-side this time)

chain-watch logged `metric: height=<N>` from **Rust**. http-counter has no Rust poll loop, so
the metric is logged from **Hoon**, inside `++poke`, on every branch:

```hoon
~>  %slog.[0 leaf+"metric: count={<value.state>}"]   ::  GET: current value
~>  %slog.[0 leaf+"metric: count={<new-value>}"]     ::  increment: new value
~>  %slog.[0 leaf+"metric: count=0"]                 ::  reset
```

Status recipe (note the `-a`, per chain-watch ROUGH EDGE 8 — BSD grep + NUL bytes):

```sh
grep -aoE 'count=[0-9]+' | tail -1 | grep -aoE '[0-9]+'    # label COUNT
```

---

## 4. Build

```sh
cd examples
nockup project build http-counter        # ✅ Cargo + hoonc clean
ls -la http-counter/out.jam http-counter/target/release/http-counter
# out.jam ~588K ; http-counter ~16M Mach-O arm64
```

(Same harmless noise as chain-watch: `invalid character '{' in package name` from cargo
scanning the template dir, and a `build.rs` `unused import` warning. Build still succeeds.)

---

## 5. Smoke test the binary directly (before nockd)

```sh
cd examples/http-counter
rm -rf .data.http-counter           # start clean
./target/release/http-counter &     # serves 127.0.0.1:8081

curl -s localhost:8081/ | grep -i count          # Count: 0
curl -s -X POST localhost:8081/increment         # Count: 1, 2, 3 …
curl -s localhost:8081/ | grep -i count          # Count: 3  (fresh — EXPIRE_CACHE=0 working)
grep -a 'metric: count' <logs>                   # one line per request
```

**Persistence (hand-run):** `kill -TERM <pid>` (clean "received SIGTERM" log), then re-run
the binary from the same dir → `GET /` still shows `Count: 3`. State lives in
`.data.http-counter/` (`event-log.sqlite3` + `pma/`).

---

## 6. Deploy — PROJECT MODE

```sh
cd examples/http-counter
nockd deploy -f nockd.toml      # nockd shells out to nockup with the ABSOLUTE project path
nockd restart http-counter      # swaps in the freshly built artifact
# deployed http-counter
#   artifact ba4c5be5…
#   kernel   1bd12833…
```

No `--endpoint` (no chain endpoint). `nockd.toml` carries `project = "."` (single-bin, so no
`bin_target`). (Project-mode deploy was broken when this recipe was first written and used the
prebuilt `--bin`/`--jam` path; the fixed nockd builds via nockup directly, so we use that now.)

---

## 7. Verify + the persistence proof + SIMULTANEOUS WITH http-static

```sh
nockd ps
# NAME          STATE    HEALTH   VERIFIED  PID    ENDPOINT  STATUS
# http-counter  running  unknown  verified  11045  —         COUNT 5
# http-static   running  unknown  verified  10984  —         REQ 29

curl -s localhost:8081/ | grep -i count          # Count: 5
for i in 1 2 3 4 5; do curl -s -X POST localhost:8081/increment; done   # → 5
# ps shows COUNT 5 (status populated; the -a grep handles the NUL-bearing boot log)

# === SIMULTANEITY PROOF (the point of the HTTP_PORT bump) ===
# Both apps run at once now that each binds its own port directly (no shared :8080):
curl -s localhost:8081/ | grep -io 'Count: *[0-9]*'   # http-counter on :8081
curl -s localhost:8083/ | grep -o '<title>[^<]*'      # http-static  on :8083, concurrently
lsof -nP -iTCP:8081 -iTCP:8083 -sTCP:LISTEN           # two procs, two ports, none on :8080

# === PERSISTENCE PROOF ===
nockd restart http-counter           # new PID
curl -s localhost:8081/ | grep -i count          # Count: N   ← SURVIVED the restart
curl -s -X POST localhost:8081/increment         # continues from N, not reset
curl -s -X POST localhost:8081/reset             # Count: 0
```

`running + verified` (self-signed by the trusted builder key). COUNT status populated and
matches the rendered page. **The count survived `nockd restart` with its value intact** (the
headline feature) AND **both http-counter and http-static served concurrently** on 8081/8083 —
both confirmed.

---

## 8. Summary of rough edges (vs chain-watch)

- **A. (RESOLVED by PR #134.)** `http_driver()` used to hardcode `127.0.0.1:8080` and couldn't
  be replaced out-of-crate (the effect-parsing helpers `NounSlab::noun_space()`/`ptr_ranges()`
  are private) → we used an in-process TCP proxy. The driver now reads `HTTP_PORT` and binds it
  directly, so we just `set_var("HTTP_PORT", "8081")` — no proxy, and apps coexist.
- **B. The local HTTP driver caches GET responses forever** → stale count + no per-request
  kernel poke (so no metric). Fixed with `EXPIRE_CACHE=0`. (Still applies.)
- **C. Adding an extra arm to the `++inner` door breaks the `(keep)` wrapper nest** — the
  `fort` mold expects exactly load/peek/poke. Put helpers in the prelude core instead.

Everything from chain-watch's RECIPE (1–8) still applies unchanged.
