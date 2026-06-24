# minesweeper — build/deploy recipe

Builds on the suite's shared recipe — see `../chain-watch/RECIPE.md` and
`/Users/neal/zorp/nockd/examples/GOTCHAS.md` for the toolchain gotchas. This file
covers only what's specific to minesweeper.

> Note: the original build agent compiled the kernel and verified the game serving
> locally, but stopped before the nockd deploy + docs; the coordinator finished the
> deploy (project-mode), gameplay verification under nockd, and these docs.

## Build

- nockchain crates pinned at rev **`07577127958db94be12e95ea816f31bc7582aa2c`**
  (current origin/master — includes PR #134's `HTTP_PORT`).
- `rust-toolchain.toml` → `nightly-2026-04-03` (avoids the `cold_path` E0658).
- `nockup project build` → `target/release/minesweeper` + `out.jam`.

## HTTP serving (PR #134)

The library `http_driver()` binds the port from the `HTTP_PORT` env var. `main.rs`
sets it before the driver starts — no TCP proxy:

```rust
std::env::set_var("HTTP_PORT", "8084");
std::env::set_var("EXPIRE_CACHE", "0");   // else GET is cached forever + the poke is skipped
```

## Deploy (project-mode)

```sh
nockd deploy -f nockd.toml      # project = "." → nockd builds via nockup + ships
nockd restart minesweeper       # deploy registers the artifact; restart swaps the live process
```

Verified under nockd: `minesweeper running verified`, GET `/` renders the board,
`POST /reveal?x=2&y=2` placed mines safely and showed `moves: 1`, `/flag` and `/new` work.

## Hoon game logic (app.hoon) — points of note

- Board state is sets of `[x=@ud y=@ud]` (`mine`/`shown`/`flag`) plus `status` and a
  `moves` counter; one versioned noun, so nockd checkpoints it across restart.
- Neighbour math uses signed integers (`si`): offsets as `(list [@s @s])`, bounds-checked
  back to `@ud`.
- Mines are seeded from the poke's `eny` via the `og` RNG (`~(. og eny)` + `rads`),
  placed lazily on the first reveal and never on the clicked cell (safe first click).
- Flood-reveal is an explicit BFS worklist over a `(list [@ud @ud])` (no recursion depth
  worries); enqueues neighbours only for zero-adjacent cells.
- `metric: moves=<N>` is slogged on every request → the `MOVES` status. (Status `tail -1`
  can momentarily lag a burst of requests; the rendered page reflects authoritative state.)

## Gotchas reused (not re-derived here)

HTTP_PORT + `EXPIRE_CACHE=0` + the `++inner` door taking exactly `load`/`peek`/`poke`
(helpers like `render`/`cell-html` live in the prelude core) — see the shared GOTCHAS.
