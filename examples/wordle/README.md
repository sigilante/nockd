# wordle

A playable **Wordle** (word-guessing game) served over HTTP — with all the game logic
(target selection, per-letter green/yellow/grey feedback with correct letter
**multiplicity**, win/loss detection) living in the **Hoon kernel**. The Rust wrapper only
boots the kernel and runs the library HTTP driver. It demonstrates a stateful, interactive
NockApp whose behaviour is pure Hoon.

- Guess a 5-letter word in **6 tries**.
- Each tile is colored like real Wordle: 🟩 **green** (right letter, right spot),
  🟨 **yellow** (in the word, wrong spot), ⬛ **grey** (not in the word).
- Yellow respects letter multiplicity (two-pass scoring), so guessing `eaten` against
  `steel` lights up only as many `E`s as `steel` actually has.
- Target is drawn from a curated pool of ~400 common 5-letter words, seeded from the poke's
  entropy.
- Any 5 ASCII letters are accepted as a guess (no dictionary check) — see Notes.
- State persists across `nockd restart` (the game is kernel state, checkpointed by nockd).

## Deploy

```sh
nockd deploy -f nockd.toml      # project-mode: nockd builds via nockup, ships, runs
nockd restart wordle            # swap the live process onto the new artifact
nockd ps                        # wordle · running · verified · GUESSES <n>
```

Serves on **http://127.0.0.1:8088/**.

## Play

Open `http://127.0.0.1:8088/` in a browser, type a 5-letter word, and press **Guess** —
the row colors in. Or drive it with curl:

```sh
curl http://127.0.0.1:8088/                          # render the grid
curl -X POST "http://127.0.0.1:8088/guess?w=crane"   # submit a guess (lowercase)
curl -X POST  http://127.0.0.1:8088/new              # new target, clear guesses
```

A guess may also be sent in the POST body (`w=crane`); the browser form does this.

## How to see it work

`nockd ps` shows the **GUESSES** status (cumulative accepted guesses this session, scraped
from the `metric: guesses=<N>` log line). Submit the right word to flip the status to
**YOU WON in N!**; run out of 6 guesses to see **Out of guesses. The word was …**.

## Notes

- HTTP port is set via the `HTTP_PORT` env var (nockchain PR #134); no proxy. If nockd
  exports `NOCKD_APP_PORT`, the wrapper honors it. Built against nockchain rev `07577127…`.
- `EXPIRE_CACHE=1` (not 0): `0` panics this rev's HTTP driver (`Duration::ZERO`). See
  `RECIPE.md`.
- **Input validation:** guesses are accepted as any 5 ASCII letters — there is no
  allowed-word dictionary, only the answer pool. This keeps the kernel small; the trade-off
  is you can "guess" non-words. Malformed input (not exactly 5 letters) is rejected
  gracefully by re-rendering the current grid unchanged.
