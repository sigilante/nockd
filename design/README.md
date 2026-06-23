# Handoff: nockd Browser Dashboard

## Overview

This package documents the **browser dashboard for `nockd`** — the self-hostable
deployment platform for NockApps. The dashboard is served by `nockd serve` out of the
single static binary (assets embedded via `rust-embed`) and is a pure client of the
`nockd` Control API. It covers fleet overview, per-app detail with live logs, the
endpoints registry, secrets, reproducible-build verification, and the health-gated
deploy flow — i.e. the views in **DESIGN.md §9**.

The visual system is a deliberate **Bauhaus / De Stijl** language with a Windows-Metro
flatness: a near-white paper ground, heavy black rules, three primaries (blue / yellow /
red) used **only** as status signal, geometric sans + monospace type, and a single
status grammar of four glyphs (● ▲ ▼ ■). It is described exhaustively in
[`DESIGN-SYSTEM.md`](./DESIGN-SYSTEM.md).

## About the design files

The files in [`references/`](./references/) are **design references created in HTML** —
hi-fi prototypes that show the intended look and behavior. They are **not** production
code to copy verbatim.

> The `.dc.html` files are authored in a streaming "Design Component" format and require
> the sibling `support.js` runtime to render in a browser. They are included so you can
> open them and inspect exact styles, spacing, and copy. **Do not ship `support.js` or the
> `.dc.html` format** — it is a prototyping runtime, not part of nockd.

Your task is to **recreate these designs inside the `nockd` dashboard's own front-end**,
wired to the real Control API (see [`API-INTEGRATION.md`](./API-INTEGRATION.md)). DESIGN.md
§9.2 calls for a *small SPA or server-rendered + sprinkles* — keep the front end modest and
keep the single-binary ethos (assets embedded, no separate web deploy). Any of vanilla
TS + a tiny renderer, Preact, Svelte, or Leptos/Yew is appropriate; the design uses no
framework-specific behavior. Everything here is plain layout, flat color, and SSE-driven
live data.

## Fidelity

**Hi-fi.** Colors, typography, spacing, rules, and component states are final and exact.
Recreate them pixel-faithfully. All hex values, type sizes, and rule weights are listed in
[`DESIGN-SYSTEM.md`](./DESIGN-SYSTEM.md); per-screen layout and copy are in
[`SCREENS.md`](./SCREENS.md). Two things are deliberately *static* in the mocks and must
become real in implementation: live log streaming and progress (deploy / verify / lag) —
these are SSE-driven (see API doc).

## Screens

Full specs in [`SCREENS.md`](./SCREENS.md). Index:

| # | Screen | Reference file → frame label | API |
|---|--------|------------------------------|-----|
| 1 | Fleet Overview — table | `Nockd Dashboard Patterns.dc.html` → `03 Bauhaus Grid` / "FLEET OVERVIEW" | `GET /api/v1/apps` |
| 2 | Fleet Overview — tiles | same file → "FLEET — TILE VIEW · ALT" | `GET /api/v1/apps` |
| 3 | App Detail | same file → "APP DETAIL — blackjack" | `GET /api/v1/apps/:name` + SSE logs |
| 4 | Endpoints registry | `Nockd Bauhaus Screens.dc.html` → `Endpoints` | `GET /api/v1/endpoints` |
| 5 | Secrets | same file → `Secrets` | `GET /api/v1/secrets` |
| 6 | Verification | same file → `Verification` | `GET /api/v1/verification` |
| 7 | Deploy flow (modal) | same file → `Deploy` | `POST …/deploy` + SSE progress |

> **Canonical vs. exploration.** Only the **`03 Bauhaus Grid`** direction in
> `Nockd Dashboard Patterns.dc.html` is canonical (the other four directions in that file —
> Red-Figure, Jasperware, LCARS, Getty — were earlier exploration and should be ignored).
> `Bauhaus Permutations.dc.html` is palette/composition rationale only; **frame A** in it is
> the approved baseline and matches the canonical direction.

## nockd integration (summary)

The dashboard is a thin client of the Control API — *"a window onto the API, not a second
application with its own state of record"* (DESIGN.md §9.2). It has **no privileged
backdoor**; every action it takes is an API call the CLI could also make.

- **Transport / auth.** Same-origin HTTP+JSON. On localhost the API binds a Unix socket
  (file-permission gated); remote access requires TLS + a bearer token. The dashboard never
  speaks the unauthenticated NockApp private gRPC directly — `nockd` fronts it.
- **Live data over SSE.** Logs, status transitions, deploy progress, verify progress, and
  endpoint reachability are server-pushed.
- **Secrets.** The API returns metadata only; the UI must **never** render a secret value
  and must redact it everywhere (the design encodes this as a black redaction bar).

Full surface, data shapes, auth, and SSE event formats are in
[`API-INTEGRATION.md`](./API-INTEGRATION.md).

## Files

```
design_handoff_nockd_dashboard/
├── README.md                ← this file
├── DESIGN-SYSTEM.md         ← tokens, type, status grammar, components
├── SCREENS.md               ← per-screen layout, components, copy, API binding
├── API-INTEGRATION.md       ← Control API surface, auth, SSE, data model, security
└── references/
    ├── Nockd Dashboard Patterns.dc.html   ← Fleet (table+tiles) + App Detail  (use "03 Bauhaus Grid")
    ├── Nockd Bauhaus Screens.dc.html       ← Endpoints, Secrets, Verification, Deploy
    ├── Bauhaus Permutations.dc.html        ← palette rationale; frame A = approved baseline
    └── support.js                          ← prototyping runtime (DO NOT SHIP)
```

To view a reference: open the `.dc.html` file in a browser with `support.js` in the same
folder (already placed). An internet connection loads the two Google fonts (Jost,
IBM Plex Mono).

## Assets

No raster images, icons, or logos. Everything is CSS: solid color blocks, CSS-drawn
geometric status glyphs (circle / triangles / square via `border-radius` and CSS triangle
borders), and type. Two web fonts only — **Jost** and **IBM Plex Mono** (Google Fonts);
self-host them in the binary to preserve the offline, single-artifact ethos.
