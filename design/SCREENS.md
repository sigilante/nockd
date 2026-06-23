# Screens — nockd Dashboard

Seven views. Each entry gives purpose, layout, the components used (cross-referenced to
[`DESIGN-SYSTEM.md`](./DESIGN-SYSTEM.md)), the exact demo copy, the live/empty/error states,
and the **API binding** (full surface in [`API-INTEGRATION.md`](./API-INTEGRATION.md)).

Demo fleet used throughout (7 apps): `blackjack`, `wallet`, `treasury`, `name-registry`
(running) · `oracle-feed` (degraded) · `proof-relay` (crashing) · `faucet` (stopped).
`wallet` is a standard app like any other.

---

## 1. Fleet Overview — table view
**Ref:** `Nockd Dashboard Patterns.dc.html` → direction `03 Bauhaus Grid`, frame "FLEET OVERVIEW".
**Purpose:** every app at a glance; the default landing view.

**Layout (top→bottom):** header bar → 4 stat blocks → data table → legend footer.

- **Header bar:** nav active = `FLEET`.
- **Stat blocks:** `APPS 7` (paper) · `RUNNING 4` (blue) · `DEGRADED 1` (yellow) ·
  `CRASHING 1 · 1 STOPPED` (red).
- **Table columns:** `[glyph] APP · ARTIFACT · ENDPOINT · UPTIME · RST · STATE · STATUS`.
  Header underline 4px; rows 2px. Glyph encodes status; STATUS word repeats it in the
  status color. Restart counts that are elevated take the status color (e.g. `12` red).
- **Legend footer** (`ink` bar): ● RUNNING ▲ DEGRADED ▼ CRASHING ■ STOPPED + reconciler
  state (`RECONCILER · converged 0.4s ago`).

**Row data (per app):** name, status, short artifact (`a3f2…9c`), endpoint name, uptime,
restart count, state size, chain-attach (`✓ / ⚠ / ✗`), verified (`✓ / —`).

**Binding:** `GET /api/v1/apps`. Poll or subscribe to `GET /api/v1/events?follow=1`
(SSE) to live-update status pills and the reconciler line. Row click → App Detail.

---

## 2. Fleet Overview — tile view
**Ref:** same file → frame "FLEET — TILE VIEW · ALT".
**Purpose:** the same fleet as flat Metro tiles; a view toggle alternative to the table.

**Layout:** header → toggle/summary row (`7 APPS · 4 RUNNING · 1 DEGRADED · 1 CRASHING ·
1 STOPPED` on the left; a **TABLE / TILES** segmented toggle on the right, TILES active) →
4×2 tile grid on a 6px black ground.

- **Tile:** status band (color + glyph + `RUNNING`/etc. + nothing on right) over a paper
  body: big name, `blake3:…` + endpoint·template mono lines, and a footer with big uptime
  (or `12 RST` for crashing) + state + `chain ✓ · vfy ✓`.
- **8th cell:** `+ DEPLOY APP` dashed tile → opens Deploy (new app).
- Stopped tile uses `paper-idle` body and an `ink` band with a muted square.

**Binding:** same `GET /api/v1/apps`; the table↔tile toggle is pure client state.

---

## 3. App Detail
**Ref:** same file → frame "APP DETAIL — blackjack".
**Purpose:** everything about one app — live logs, history, resources, artifacts, attach.

**Layout:** header (back ‹ FLEET · name · status · `http-server · up 18d` · action buttons
`DEPLOY` `ROLLBACK` `RESTART` `STOP`) → 2-column body on a 6px black grid:

- **Left (≈1.55fr):**
  - **LIVE LOG · SSE** panel (`ink` background, mono lines, colored verbs `poke`/`peek`/
    `chain`/`snap`, a blinking caret at the tail). Header shows `● FOLLOWING`.
  - **EVENT TIMELINE** (paper): dot + timestamp + kind + detail rows
    (`DEPLOY blake3:a3f2…9c · health ✓ 3.1s`, `RESTART on-failure · backoff 2s`,
    `ROLLBACK ← blake3:88c1…41`, `DEPLOY initial`).
- **Right (≈1fr):**
  - **RESOURCES** (paper): CPU % and RSS as small bar charts.
  - **ARTIFACT** (paper): current `blake3:a3f2…9c` `VERIFIED ✓` + previous (rollback target).
  - **ATTACHMENT** (blue): endpoint name + lag, `→ http://…:5555`, then **SECRET**
    `wallet_key → WALLET_KEY ••••` (redacted).

**Bindings:**
- `GET /api/v1/apps/:name` → header, artifacts, resources snapshot, attachment, secret refs.
- `GET /api/v1/apps/:name/logs?follow=1` → **SSE** log lines (append + autoscroll).
- `GET /api/v1/apps/:name/events` → timeline.
- `GET /api/v1/apps/:name/metrics` → CPU/RSS series.
- Action buttons → `POST …/deploy | rollback | restart | stop`.
- **Health/chain status** comes from the supervisor probing the app's private gRPC
  (`MonitoringService.Ping` / gRPC health); chain-attach reachability is surfaced
  separately. Never expose the private gRPC to the browser — read it through this API.

---

## 4. Endpoints registry
**Ref:** `Nockd Bauhaus Screens.dc.html` → frame `Endpoints`.
**Purpose:** the named Nockchain RPC targets and their live reachability/lag.

**Layout:** header (nav `ENDPOINTS`) → 4 stat blocks (`ENDPOINTS 4` · `REACHABLE 2` blue ·
`HIGH LAG 1` yellow · `UNREACHABLE 1` red) → 2×2 endpoint tiles.

- **Endpoint tile:** status band (● REACHABLE blue / ▲ HIGH LAG yellow / ▼ UNREACHABLE red,
  with the lag or `timeout` on the right) over a body: name + `REMOTE`/`LOCAL-SOCKET` chip,
  `http://host:port · …` URL, a **lag bar** (fill % = lag/threshold; over-threshold turns
  yellow; unreachable shows the red hazard-gradient bar), and an `ATTACHED · N APPS` label
  with app chips (or `— no instances attached —`).

**Demo data:** `mainnet-rpc` 240ms (4 apps) · `archive-rpc` 410ms (proof-relay) ·
`testnet-rpc` 880ms over-threshold (oracle-feed, faucet) · `local-node` unreachable,
connection refused, 0 apps.

**Binding:** `GET /api/v1/endpoints`. Reachability/lag are live — subscribe via SSE
(`GET /api/v1/endpoints?follow=1`) or poll. `+ ADD ENDPOINT` → `POST /api/v1/endpoints`.
An endpoint is a **public-gRPC URL** (`http://host:port`), referenced by apps **by name**
so the URL can change without redeploying.

---

## 5. Secrets
**Ref:** same file → frame `Secrets`.
**Purpose:** manage named secrets by **metadata only** — never cleartext.

**Layout:** header (nav `SECRETS`) → **yellow security banner**
(`ENCRYPTED AT REST · RESOLVED INTO THE PROCESS AT LAUNCH · NEVER WRITTEN TO ARTIFACTS OR
LOGS · NEVER SHOWN IN CLEARTEXT`) → 4 stat blocks (`SECRETS 5` · `IN USE 5` blue ·
`ROTATED <30d 2` yellow · `STALE 1` ink) → table.

- **Table columns:** `NAME · USED BY · ENV KEY · CREATED · LAST USED · VALUE · ACTION`.
  VALUE is always the **black redaction bar** (`••••••••`). ACTION is a `ROTATE` button
  (stale rows show it in red). A `+ SET SECRET` primary button sits above the table.
- Stale secret (`relay-key`, last used 41d) flags its LAST USED and ROTATE in red.

**Binding:** `GET /api/v1/secrets` returns **metadata only** (`name, used_by, env_key,
created_at, last_used`) — the value field must not exist in the response. `POST
/api/v1/secrets` sets/rotates (value is write-only, encrypted at rest). The UI must have
no code path that displays a value.

---

## 6. Verification
**Ref:** same file → frame `Verification`.
**Purpose:** show whether each deployed artifact is reproducible-verified against source.

**Layout:** header (nav `VERIFY`) → 4 stat blocks (`DEPLOYED 7` · `VERIFIED 5` blue ·
`VERIFYING 1` yellow · `UNVERIFIED 1` ink) → **statement strip** explaining the rebuild +
a legend (● VERIFIED ▲ VERIFYING ▼ DRIFT) → per-app rows.

- **Row columns:** `[glyph] APP · ARTIFACT · TOOLCHAIN (hoonc x · rust y) · STATUS`.
- **Status states:** `VERIFIED ✓ · hash match · <time>` (blue) · `VERIFYING… rebuilding
  64%` with a yellow progress bar · `UNVERIFIED · cannot rebuild` (ink, no source pin) ·
  `DRIFT · deployed ≠ source` (red — defined in the legend; render it when the rebuilt hash
  mismatches).

**Binding:** `GET /api/v1/verification` → `{app, artifact, hoonc_version, toolchain_pin,
status, last_verified, progress?}`. `POST /api/v1/apps/:name/verify` triggers a rebuild;
stream progress over SSE and animate the bar. Status enum: `verified | verifying |
unverified | drift`.

---

## 7. Deploy flow (modal)
**Ref:** same file → frame `Deploy`.
**Purpose:** the health-gated rollout (DESIGN.md §8.1) as a live modal.

**Layout:** a centered modal over a dimmed fleet + scrim.

- **Title bar** (`ink`): `DEPLOY · blackjack` + `✕`.
- **Artifact swap** (4px rule under it): `CURRENT blake3:a3f2…9c → NEW blake3:b7e4…2a`
  (the NEW half sits in a blue block).
- **Pipeline** — numbered step rows, each: status-colored number square + Jost label +
  mono detail + status word:
  1. `BUILD` — hoonc + cargo · 3.1s — **DONE ✓** (blue)
  2. `HASH` — blake3:b7e4…2a · content-addressed — **DONE ✓**
  3. `UPLOAD` — dedup · 1 new layer shipped — **DONE ✓**
  4. `START` — new instance · against existing state dir — **DONE ✓**
  5. `HEALTH GATE` — *active*, full **yellow band**: `probing private-gRPC · Ping ok · gRPC
     health SERVING · poke %ping…` with a progress bar — **GATING…**
  6. `RETIRE OLD` — old instance still serving until gate passes — **PENDING** (grey)
- **Note** (left 4px rule): "The previous instance keeps serving until the new one proves
  healthy — a bad deploy never mutates state and rolls back in one click."
- **Footer** (4px rule): `↩ ROLLBACK to a3f2…9c available` · `CANCEL` (secondary) ·
  `DEPLOYING…` (primary, disabled while in flight).

**Binding:**
- `POST /api/v1/apps/:name/deploy` (body: existing artifact hash, or multipart upload of a
  freshly built artifact) → returns `{deploy_id}`.
- `GET /api/v1/deploys/:id/events` → **SSE** stream of pipeline transitions; map each event
  `{step, state, detail}` onto the six rows. The **health gate** is the load-bearing step:
  the old instance is retired only after it passes; on failure the old instance is
  untouched and state is not mutated.
- `CANCEL` → `DELETE /api/v1/deploys/:id`. `ROLLBACK` → `POST /api/v1/apps/:name/rollback`
  (re-points to the previous artifact; same health gating).

---

## States to implement (not shown statically)

- **Loading / empty:** zero apps → a `+ DEPLOY APP` empty state (reuse the dashed tile).
- **Live updates:** logs, deploy/verify progress, endpoint lag, status pills — all SSE.
- **Errors:** deploy health-gate failure (step 5 turns red ▼ "GATE FAILED — old instance
  retained"); endpoint unreachable (already designed); API/socket disconnect → a banner.
- **Confirms:** STOP and ROLLBACK should confirm (destructive/stateful) — use the modal
  shell with the same chrome.
