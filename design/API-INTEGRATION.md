# API Integration — nockd Dashboard

How the dashboard talks to `nockd`. This documents the **shipped** `/api/v1` surface (the
dashboard and CLI are both clients of it; the dashboard has no privileged backdoor). Sections
marked **PLANNED** are not built yet — they're the forward shape from DESIGN.md.

> Two surfaces: **`/api/v1/*`** is the versioned surface the browser dashboard uses.
> **`/api/*`** (unversioned) is the legacy surface the CLI/TUI use, including **deploy**
> (artifact upload). Both are served by the same `nockd serve` process.

---

## 1. Transport & auth

- **Same binary, same origin.** `nockd serve` embeds the dashboard assets (`rust-embed`) and
  serves them from the same HTTP server as the Control API. No separate deploy, no CORS.
- **Current (Phase-0/1):** a **localhost TCP listener**, default **`http://127.0.0.1:4490`**,
  with **no auth**. Keep it local-only. This is a tracked deviation from the bedrock design.
- **PLANNED hardening:** a Unix-socket default (file-permission gated) and, for opt-in remote
  access, TLS + a bearer token (`Authorization: Bearer …` on every request and SSE).
- **Never** let the browser speak a NockApp's **private/admin gRPC** (no TLS/auth, §5.3/§10).
  `nockd` keeps each app's private gRPC bound to localhost, is its only speaker, and
  re-exports a safe view through this API. The dashboard reads health/control **only** through
  `nockd`.

---

## 2. Data model

### App — `GET /api/v1/apps`, `GET /api/v1/apps/:name`

Desired + observed, merged for the UI. Timestamps are **Unix seconds** (ints). Hashes are raw
blake3 hex (no `blake3:` prefix). `null` = not set / not yet sampled.

```jsonc
{
  "name": "minesweeper",
  "status": "running",          // running | degraded | stopped | crashing  (derived)
  "desired_status": "running",  // running | stopped
  "artifact_hash": "e43a20…",
  "kernel_hash": "3251c7…",     // null for binary-only apps (kernel embedded in the binary)
  "prev_artifact": "b18408…",   // rollback target; null if only one artifact has run
  "endpoint_name": "mainnet-rpc", // null if unattached
  "restart_policy": "always",   // always | on-failure | never
  "restart_count": 0,
  "uptime_s": 1555200,          // null if not running
  "state_size_bytes": null,     // not yet sampled
  "template": null,             // not yet recorded
  "health": "serving",          // serving | notserving | unreachable | unknown
  "chain_attach": null,         // not yet probed
  "verified": "verified",       // verified | unverified | drift
  "status_label": "MOVES",      // custom status metric label; null if none
  "status_line": "9",           // custom status metric value; null if none
  "port": 8084,                 // HTTP port (relay link → localhost:<port>); null if none
  "manifest_path": "/…/nockd.toml", // deployed-from manifest; enables Reload; null otherwise
  "has_icon": true,             // true → GET /api/v1/apps/:name/icon
  "cpu_pct": 0.4,               // % of one core, sampled ~5s; null if not running
  "rss_bytes": 16601088,        // resident memory; the OOM-watch metric; null if not running
  "pid": 26364,                 // null if not running
  "created_at": 1718900000,
  "updated_at": 1718900500
}
```

### Deploy history — `GET /api/v1/apps/:name/history`
```jsonc
// newest first; consecutive duplicate artifacts collapsed
[{ "artifact_hash": "e43a20…", "kernel_hash": "3251c7…",
   "verified_status": "verified", "deployed_at": 1718900000, "current": true }]
```

### Endpoint — `GET /api/v1/endpoints`
```jsonc
{ "name": "mainnet-rpc", "url": "http://1.2.3.4:5555", "kind": "remote", // remote | local-socket
  "reachable": true, "lag_ms": 240,   // real gRPC handshake + health check RTT
  "height": 93095, "behind": 0,       // chain tip (Nockchain metrics) + blocks behind the leader
  "attached_apps": ["chain-watch"] }
```

### Event — `GET /api/v1/apps/:name/events`, SSE `GET /api/v1/events`
```jsonc
{ "id": 4471, "ts": 1718900000, "app_name": "minesweeper",
  "kind": "deploy",  // deploy | start | stop | crash | error | restart | reload | rollback
  "detail": "artifact e43a20…" }
```

### Secret — **PLANNED** (the SECRETS screen is a placeholder)
Metadata only; values never appear in any response.

> **Observed vs. durable.** Live PIDs, health, status line, and CPU/RSS are held in memory by
> the supervisor (ephemeral, rebuilt on restart), not SQLite. Treat status/health/metrics as
> live (poll + SSE); desired-state/artifacts/endpoints/history as durable (fetch + refresh on
> mutation).

---

## 3. Routes

### Fleet & apps (`/api/v1`)
| Method | Path | Does |
|--------|------|------|
| `GET` | `/api/v1/apps` | `App[]` — Fleet table/tiles + Metrics |
| `GET` | `/api/v1/apps/:name` | `App` — App detail |
| `GET` | `/api/v1/apps/:name/events` | `Event[]` — detail timeline |
| `GET` | `/api/v1/apps/:name/history` | `DeployHistory[]` — Artifact panel |
| `GET` | `/api/v1/apps/:name/icon` | the app's icon (image bytes + `Cache-Control`); `404` if none |
| `GET` | `/api/v1/apps/:name/logs` | **SSE** live log (see §4) |
| `POST` | `/api/v1/apps/:name/restart` | graceful restart |
| `POST` | `/api/v1/apps/:name/reload` | re-read the manifest, re-apply config, restart (no rebuild); `400` if no manifest |
| `POST` | `/api/v1/apps/:name/rollback` | revert to `prev_artifact` + restart (no body); `409` if none |
| `POST` | `/api/v1/apps/:name/start` | set `desired_status=running` |
| `POST` | `/api/v1/apps/:name/stop` | set `desired_status=stopped` |
| `POST` | `/api/v1/down` · `/api/v1/up` | stop / start the whole fleet → `{changed,total}` |

### Endpoints & trust (`/api/v1`)
| Method | Path | Does |
|--------|------|------|
| `GET` · `POST` | `/api/v1/endpoints` | list · register `{name,url,kind}` |
| `DELETE` | `/api/v1/endpoints/:name` | remove |
| `GET` · `POST` | `/api/v1/trust` | list · add `{pubkey}` trusted builders |
| `DELETE` | `/api/v1/trust/:pubkey` | stop trusting |
| `GET` | `/api/v1/events` | **SSE** audit-event stream (see §4) |

### Deploy (legacy `/api/*`, used by the CLI)
| Method | Path | Does |
|--------|------|------|
| `POST` | `/api/apps` | deploy: body is the full artifact (base64 binary + optional jam) + config + optional attestation → `{name, artifact_hash, kernel_hash}` |

Deploy is **synchronous** (build happens client-side; the daemon stores + reconciles), so
there's no deploy-pipeline id/SSE. A deploy of an already-running app restarts it to apply.

### PLANNED (not built)
`/api/v1/secrets` (metadata + write-only set), reproducible-build verify
(`/api/v1/apps/:name/verify`, verification status + progress SSE).

---

## 4. Live data over SSE

`text/event-stream`. Two streams are implemented:

| Stream | Drives |
|--------|--------|
| `GET /api/v1/apps/:name/logs` | App-detail **LIVE LOG** — seeds the recent tail, then follows appends |
| `GET /api/v1/events` | Fleet/timeline refresh trigger |

**Log lines are raw text** (one `data:` per line), with **ANSI color preserved** and NUL bytes
stripped; the dashboard parses ANSI → spans and highlights NockApp verbs (`poke|peek|chain|
snap`) client-side. There is no structured `{ts,verb,msg}` log JSON.

The events SSE pushes on registry changes; the dashboard treats it as a "re-fetch now" nudge
(Fleet/Metrics poll every ~2.5s as a fallback). Endpoint tiles poll (~4s); no SSE.

**PLANNED:** deploy-pipeline and verification progress streams (deploy is synchronous today).

---

## 5. Security rules the UI honors

- **No NockApp gRPC from the browser** — only through this API (see §1).
- **Secrets (when they land):** never request, cache, log, or render a value; the list returns
  metadata only; always show the redaction bar; rotation posts a value but never reads one
  back.
- **Audit:** privileged actions (deploy, reload, rollback, stop, restart, …) are appended to
  the event log by `nockd`; the timelines surface them so the dashboard is a faithful window
  onto that log.
- **Token handling (when remote auth lands):** keep the bearer token in memory, TLS only,
  never in a URL/query (SSE included); don't persist to `localStorage`.
- **Verification semantics:** `verified` = trusted-builder signature + hash-bound;
  `drift` = mismatch (tamper / out-of-band) → red; `unverified` = no/again attestation → grey.

---

## 6. Front-end shape

- **Embed assets** (`rust-embed`) and serve from the same HTTP server — single artifact, zero
  install. **Self-host the fonts** (Jost, IBM Plex Mono) so the dashboard works offline.
- **Vanilla, no build step** (`web/app.js` ES module): a tiny hash router over the screens
  (Fleet, Metrics, Endpoints, + placeholders for Secrets/Verify) + App Detail, with SSE for
  live logs/events. No framework, no client state of record — fetch and react; re-fetch after
  a mutation rather than mutating local caches.

---

## 7. Status → color quick map

| API value | Glyph / color |
|-----------|---------------|
| `status: running` / `health: serving` / `reachable: true` / `verified` | ● blue |
| `status: degraded` / lag over threshold | ▲ yellow |
| `status: crashing` / `health: unreachable` / `reachable: false` / `drift` | ▼ red |
| `status: stopped` / `health: unknown` / `unverified` / idle | ■ ink / muted |

See [`DESIGN-SYSTEM.md`](./DESIGN-SYSTEM.md) §4 for glyph construction and exact hexes.
