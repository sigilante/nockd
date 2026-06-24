# minesweeper

A playable **Minesweeper** game served over HTTP — with all the game logic
(board, mine placement, adjacency counts, flood-reveal, win/loss) living in the
**Hoon kernel**. The Rust wrapper only boots the kernel and runs the library HTTP
driver. It demonstrates a stateful, interactive NockApp whose behaviour is pure Hoon.

- 9×9 board, 10 mines ("beginner").
- **Safe first click:** mines are placed lazily on the first reveal, never under it.
- Classic **flood-reveal** of zero-adjacent regions.
- Flagging, win detection (all safe cells revealed), and loss (hit a mine → board exposed).
- State persists across `nockd restart` (the board is kernel state, checkpointed by nockd).

## Deploy

```sh
nockd deploy -f nockd.toml      # project-mode: nockd builds via nockup, ships, runs
nockd ps                        # minesweeper · running · verified · MOVES <n>
```

Serves on **http://127.0.0.1:8084/**.

## Play

Open `http://127.0.0.1:8084/` in a browser and click cells (each hidden cell has a
`?` reveal button and an `f` flag toggle), or drive it with curl:

```sh
curl http://127.0.0.1:8084/                       # render the board
curl -X POST "http://127.0.0.1:8084/reveal?x=2&y=2"   # reveal a cell
curl -X POST "http://127.0.0.1:8084/flag?x=0&y=0"     # toggle a flag
curl -X POST  http://127.0.0.1:8084/new               # new game
```

## How to see it work

`nockd ps` shows the `MOVES` status (cumulative reveals + flag toggles, scraped from
the `metric: moves=<N>` log line). Reveal a mine to see the board flip to **BOOM!**;
clear all safe cells to see **YOU WON!**.

## Notes

- HTTP port is set directly via the `HTTP_PORT` env var (nockchain PR #134); no proxy.
  Built against nockchain rev `07577127…`. See `RECIPE.md`.
