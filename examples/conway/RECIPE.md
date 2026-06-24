# conway — build/deploy recipe

Builds on the suite's shared recipe — see `../minesweeper/RECIPE.md` and
`/Users/neal/zorp/nockd/examples/GOTCHAS.md` for the toolchain gotchas. This file
covers only what's specific to conway, plus the **new rough edges** discovered building it.

## Build

- nockchain crates pinned at rev **`07577127958db94be12e95ea816f31bc7582aa2c`**
  (includes PR #134's `HTTP_PORT`).
- `rust-toolchain.toml` → `nightly-2026-04-03` (avoids the `cold_path` E0658).
- Build with **`nockup project build conway`** — see the new rough edge below.
- Output: `target/release/conway` + `out.jam`.

## HTTP serving (PR #134)

The library `http_driver()` binds the port from the `HTTP_PORT` env var. `main.rs`
sets it before the driver starts — no TCP proxy. We serve on **8089**.

## Deploy (project-mode)

```sh
nockd deploy -f nockd.toml      # project = "." → nockd builds via nockup + ships
nockd restart conway            # deploy registers the artifact; restart swaps the live process
```

Verified under nockd: `conway running verified`, GET `/` renders the grid, the blinker
oscillates correctly, `/random` + `/step` advance the generation, GEN shows in `nockd ps`.

## Hoon game logic (app.hoon) — points of note

- Board state is a `(set [x=@ud y=@ud])` of live cells plus a `gen=@ud` counter — one
  versioned noun, so nockd checkpoints it across restart.
- `++step` is a **pure** function of the live set: it scans every one of the 625 cells,
  counts live neighbors (signed `si` offset math, bounds-checked back to `@ud`), and keeps
  the survivors/births per Conway's rules. Cells off the bounded grid are simply dead
  (neighbor offsets that fall out of bounds are dropped by `murn`, contributing 0).
- `++random-board` seeds ~25% fill from the poke's `eny`. NOTE the `og` RNG door is
  **stubbed to `!!`** in this stdlib, so (as in minesweeper) we roll our own PRNG from
  `shax`: `(mod (shax (add (mul eny k) idx)) 4)` per cell, alive iff `0`.
- `metric: gen=<N>` is slogged on every request → the `GEN` status column.

## NEW rough edges (not in the shared GOTCHAS)

### `EXPIRE_CACHE=0` PANICS at this rev — use `1`, not `0` — **[app-workaround]**
The HTTP driver does `tokio::time::interval(Duration::from_secs(EXPIRE_CACHE))`. With
`EXPIRE_CACHE=0` that's `interval(Duration::ZERO)`, which panics:

```
thread 'tokio-rt-worker' panicked at .../drivers/http/http.rs:384:
`period` must be non-zero.
```

This is **not cosmetic**: the panic kills a tokio worker, the process dies, and **nockd
restarts it** — dropping the in-memory board (the `/step` you just did reverts). The fix
is `EXPIRE_CACHE=1` (1-second TTL). Mutations are POSTs whose responses are never cached
and already carry the freshly stepped board, so the page stays effectively fresh.
(minesweeper's recipe noted the panic; here we confirmed it actually crash-loops the
deployed app, not just the standalone binary.)

### Cached GET can read stale during the 1s window — read POST bodies for proofs — **[gotcha]**
With `EXPIRE_CACHE=1`, a *bare* `GET /` issued within ~1s of a mutation can return the
**cached** previous page. A browser is fine (each control is a POST, whose response is
never cached and shows the new board). But a curl/scripted correctness proof that does
`POST /step` then a **separate** `GET /` can observe the stale pre-step board. Read the
**POST response body** directly (it's the freshly rendered, never-cached board) — that's
exactly what the browser renders after a click. The README's blinker proof does this.

### `nockup project build` needs the project NAME as an argument — **[upstream]**
`nockup project build` (no arg) and `nockup project build .` both fail with
`Error: Project directory '<name>' not found` — it resolves the package name relative to
the **parent** directory. Run it from the examples dir as
**`nockup project build conway`** (the directory name). `nockd deploy -f nockd.toml`
(project-mode) handles this correctly on its own — it passes the absolute path.

### Template-cargo warning during `nockd deploy` is harmless — **[upstream, cosmetic]**
project-mode deploy prints
`error: invalid character "{" in package name: "{{project_name}}"` from the nockup
`basic` template's un-substituted `Cargo.toml`. The build still completes
(`✓ Cargo build completed successfully!`), Hoon compiles (`no panic!`), `out.jam` is
written, and `deployed conway` succeeds. Ignore it.

## DoD checklist (verified)

1. `nockup project build conway` clean — `out.jam` rewritten (checked mtime, not just the ✓).
2. `nockd deploy -f nockd.toml` + `nockd restart conway` → `conway running verified`, GEN shown.
3. Blinker proof: horizontal `(1,2)(2,2)(3,2)` → `/step` → vertical `(2,1)(2,2)(2,3)` →
   `/step` → horizontal again (gen 0→1→2→3), via never-cached POST bodies. `/random`+`/step`
   advances gen.
4. Process PID stable across the run (no `EXPIRE_CACHE` crash-loop).
