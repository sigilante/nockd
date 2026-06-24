# conway

**Conway's Game of Life** served over HTTP — with all the simulation logic (the
live-cell set, neighbor counting, and the next-generation step rule) living in the
**Hoon kernel**. The Rust wrapper only boots the kernel and runs the library HTTP
driver. It demonstrates a stateful, interactive NockApp whose behaviour is pure Hoon.

- 25×25 bounded grid (cells off the grid are always dead).
- Life rules per step: a live cell with 2 or 3 live neighbors survives; a dead cell
  with exactly 3 live neighbors is born; everything else dies / stays dead. The next
  generation is computed as a pure function over the live set.
- Click any cell to toggle it, then **Step** to advance one generation. A generation
  counter tracks how many steps you've taken since the last reset.
- State persists across `nockd restart` (the board is kernel state, checkpointed by nockd).

## Deploy

```sh
nockd deploy -f nockd.toml      # project-mode: nockd builds via nockup, ships, runs
nockd restart conway            # deploy registers the artifact; restart swaps the live process
nockd ps                        # conway · running · verified · GEN <n>
```

Serves on **http://127.0.0.1:8089/**.

## Controls

Open `http://127.0.0.1:8089/` in a browser:

- **Click a cell** — toggles it alive (filled) / dead (empty). Draw any pattern.
- **Step** — advance one generation (gen += 1).
- **Random** — seed a fresh ~25%-full board from the poke's entropy (gen reset to 0).
- **Clear** — empty the grid (gen reset to 0).

Or drive it with curl:

```sh
curl http://127.0.0.1:8089/                          # render the grid
curl -X POST "http://127.0.0.1:8089/toggle?x=1&y=2"  # toggle a cell
curl -X POST  http://127.0.0.1:8089/step             # advance one generation
curl -X POST  http://127.0.0.1:8089/random           # random ~25% board, gen 0
curl -X POST  http://127.0.0.1:8089/clear            # empty grid, gen 0
```

## How to see it work — the blinker oscillator

A **blinker** (three live cells in a row) is the simplest period-2 oscillator: it flips
between horizontal and vertical every generation. POST responses carry the freshly
rendered board, so read them directly:

```sh
curl -X POST  http://127.0.0.1:8089/clear
curl -X POST "http://127.0.0.1:8089/toggle?x=1&y=2"
curl -X POST "http://127.0.0.1:8089/toggle?x=2&y=2"
curl -X POST "http://127.0.0.1:8089/toggle?x=3&y=2"   # horizontal: (1,2)(2,2)(3,2)
curl -X POST  http://127.0.0.1:8089/step              # vertical:   (2,1)(2,2)(2,3)
curl -X POST  http://127.0.0.1:8089/step              # horizontal again
```

`nockd ps` shows the `GEN` status (current generation, scraped from the
`metric: gen=<N>` log line the kernel slogs on every request).

## Notes

- HTTP port is set directly via the `HTTP_PORT` env var (nockchain PR #134); no proxy.
  Built against nockchain rev `07577127…`. See `RECIPE.md` for the toolchain + cache gotchas.
- The GET `/` page has a 1-second cache TTL (`EXPIRE_CACHE=1`); POST responses (every
  control) are never cached and always show the freshly stepped board.
