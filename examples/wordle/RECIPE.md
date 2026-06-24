# wordle — build/deploy recipe

Builds on the suite's shared recipe — see `../minesweeper/RECIPE.md` and
`/Users/neal/zorp/nockd/examples/GOTCHAS.md` for the toolchain gotchas. This file covers
only what's specific to wordle.

## Build

- nockchain crates pinned at rev **`07577127958db94be12e95ea816f31bc7582aa2c`**
  (current origin/master — includes PR #134's `HTTP_PORT`).
- `rust-toolchain.toml` → `nightly-2026-04-03` (avoids the `cold_path` E0658).
- `nockup project build "$(pwd)"` → `target/release/wordle` + `out.jam`.
  - **Pass the absolute project path.** Bare `nockup project build` (no arg) here fails with
    `Project directory '<package>' not found` — nockup misreads the package name as a subdir.
    nockd's project-mode deploy passes the absolute path itself, so `nockd deploy -f` is fine.
  - **`nockup project build` prints `✓` even when the Hoon crashes.** The *real* signal is
    whether `out.jam` was rewritten (check the timestamp/size). A Hoon `mint-nice` type error
    leaves the old `out.jam` in place while still printing a green check.
  - Ignore the stray `invalid character '{' in package name: '{{project_name}}'` line — that's
    nockup's internal template cache, not this project. The cargo + Hoon builds still succeed.

## HTTP serving (PR #134)

The library `http_driver()` binds the port from `HTTP_PORT`. `main.rs` sets it before the
driver starts (honoring nockd's `NOCKD_APP_PORT` if present) — no TCP proxy. Port **8088**.

```rust
std::env::set_var("EXPIRE_CACHE", "1");   // NOT 0 — see below
std::env::set_var("HTTP_PORT", "8088");
```

### EXPIRE_CACHE=0 PANICS at this rev — use 1

The spec asked for `EXPIRE_CACHE=0`, but at rev `07577127` that **panics the HTTP driver on
the first cache tick**:

```
thread 'tokio-rt-worker' panicked at .../drivers/http/http.rs:396:40:
`period` must be non-zero.
```

The driver does `tokio::time::interval(Duration::from_secs(EXPIRE_CACHE))`, and
`Duration::ZERO` is rejected by tokio. Set `EXPIRE_CACHE=1` (the smallest non-panicking
value): GET `/` re-pokes at least once a second; every guess/`new` is a POST whose response
is never cached, so the grid is always fresh regardless.

## Deploy (project-mode)

```sh
nockd deploy -f nockd.toml      # project = "." → nockd builds via nockup + ships
nockd restart wordle            # deploy registers the artifact; restart swaps the live process
```

Verified under nockd: `wordle running verified`, GET `/` renders the grid, a POST guess
returns the colored feedback row, and `nockd ps` shows the `GUESSES` metric after the first
request. (The metric is empty until the first request logs a `metric: guesses=<N>` line.)

## Hoon game logic (app.hoon) — points of note

### The yellow-letter multiplicity logic (the interesting bit)

Naive scoring ("is this letter anywhere in the target?") double-counts repeated letters:
guessing `eaten` against `steel` would wrongly light up **both** E's as present even though
one is already green. Real Wordle is a **two-pass** algorithm, implemented in `++score`:

1. **Pass 1 (greens):** walk guess and target in lockstep. At each position where
   `guess[i] == target[i]`, flag a `%hit` and do **NOT** add that target letter to a "pool"
   of available letters — a green *consumes* its target letter. Every non-matching target
   letter goes into the pool (a `(list @t)` bag).
2. **Pass 2 (yellows/greys):** walk the guess again. For each non-green letter, if a copy is
   still in the pool, emit `%near` and **delete one copy** (`++del-one` removes the first
   occurrence); otherwise emit `%miss`. Deleting on use is what caps yellows at the target's
   real letter count.

Verified: `steel` target → `eaten` scores `E`=near, `A`=miss, `T`=near, `E`=**hit**, `N`=miss
(only one E is yellow because the other is green); `steep` → `STEE`=hit, `P`=miss; `steel` →
all hit (win).

### Other notes

- State is one versioned noun `[%0 game]` where `game` = `[target=tape guesses=(list scored)
  status total=@ud]`; nockd checkpoints it, so an in-progress game survives `restart`.
- The kernel boots with a **bunt** game (empty target, bunt-of-`$?` status). `++poke`
  normalizes: an empty `target` means "seed a fresh game from `eny`". The target is picked
  with `++pick-target` via `shax` mod pool-size — the `og` RNG door is stubbed to `!!` in
  this stdlib, same as minesweeper.
- **`{N}` inside a `"..."` tape is interpolation, not a literal.** An HTML `pattern="...{5}"`
  attribute crashed the build (`have @ud, need tape`) because Hoon read `{5}` as interpolating
  the number 5. Spell the regex out (`[A-Za-z][A-Za-z]...`) or use a `'''`-literal block.
- The guess arrives in either the query string (`/guess?w=…`) or the POST body (`w=…` from the
  form). `++poke` concatenates `uri & body` and `++grab-word` scans the run of letters after
  `w=`, lowercasing — so both transports work.
- `metric: guesses=<N>` is slogged on every request → the `GUESSES` status.

## Gotchas reused (not re-derived here)

HTTP_PORT, the `++inner` door taking exactly `load`/`peek`/`poke` (helpers like
`render`/`score` live in the prelude core), and the `grep -a` NUL-strip in `nockd.toml` — see
the shared GOTCHAS.
