# Design System — nockd Dashboard

A Bauhaus / De Stijl system with Windows-Metro flatness. Three rules govern everything:

1. **Paper + black.** A near-white ground and near-black ink/rules carry the whole UI.
   Black rules are structural and **heavy** (this is the De Stijl backbone).
2. **Primaries are signal, never decoration.** Blue / yellow / red appear *only* to encode
   status. A block is colored because it *means* something.
3. **One status grammar.** Four geometric glyphs (● ▲ ▼ ■) mean the same thing on every
   screen.

Type is constant: **Jost** (geometric sans) for voice, **IBM Plex Mono** for all data.

---

## 1. Color tokens

### Ground & ink (neutrals)
| Token | Hex | Use |
|-------|-----|-----|
| `paper` | `#f3efe6` | Primary surface (frames, panels, tiles, stat blocks) |
| `paper-idle` | `#e6e0d2` | Muted surface for stopped/idle tiles |
| `ink` | `#1a1815` | Primary text, **all structural rules**, header bar, black stat block |
| `ink-body` | `#4a4639` | Body data values (mono) |
| `ink-label` | `#6b6557` | Secondary labels |
| `ink-muted` | `#9a937f` | Tertiary / metadata / disabled-ish |
| `ink-idle` | `#7c7665` | Idle/stopped app names |
| `cream` | `#f3ede1` | Text/figures on dark (ink) surfaces |
| `cream-dim` | `#8e887b` | Muted text on dark surfaces (nav, host string) |
| `track` | `#ddd4c0` | Empty track behind bars (lag, progress, metrics) |
| `hairline` | `#cdc4b1` | *Legacy thin rule — superseded by 2px `ink` rules; avoid* |

### Primaries (status signal only)
| Token | Hex | Meaning | On-color text / tints |
|-------|-----|---------|------------------------|
| `blue` | `#2b4c9b` | running · reachable · ok · verified · info · "done" | text `#dbe3f4` / `#bcc8e2`; dark-bg variant `#6f8fd0` |
| `yellow` | `#efc02a` | degraded · high-lag · verifying · warning · "gating" | text on yellow `#1a1815`; dim label `#6b5410`, `#5a4a12` |
| `red` | `#cf3a26` | crashing · unreachable · down · drift · destructive | text on red `#f7dcd5` / `#f0c6bd`; dark-bg variant `#e0654c` |

Yellow uses **black** text/glyphs (never cream). Blue and red use **cream** text.

### Status → color map
`running/ok → blue` · `degraded/warn → yellow` · `crashing/down → red` ·
`stopped/idle → ink or ink-muted square`. Numerals that are themselves a warning (e.g. a
restart count of 12) take the status color; otherwise data is `ink-body`.

---

## 2. Rules & structure (the black backbone)

Rules are `ink` (`#1a1815`), solid, and intentionally heavy:

| Element | Weight |
|---------|--------|
| Table row separator | **2px** |
| Table column-header underline | **4px** |
| Stat-block gaps (black showing between blocks) | **7px** |
| Tile / panel gridlines (gap on a black ground) | **6px** |
| Modal section dividers | **4px** |
| Chip / secondary-button border | **1.5–2px** |
| Left-accent note bar | **4px** |

Construction pattern for gridlines: a black (`ink`) container with `gap` and matching
`padding`; the children (`paper`/colored) sit on top, so the black shows only as lines.
No border-radius anywhere except the **●** status circle and the 2px frame corner of a
mock (drop the 2px in production — it's just the prototype card edge).

---

## 3. Typography

Two families, loaded 400/500/600/700.

- **Jost** — display & UI. Geometric, Futura-like.
- **IBM Plex Mono** — every number, hash, URL, timestamp, status word, and caps label.

| Role | Family | Size | Weight | Tracking |
|------|--------|------|--------|----------|
| Wordmark `nockd` | Jost | 28px | 700 | 0 |
| Stat-block number | IBM Plex Mono | 38–46px | 700 | 0 |
| Section / poster headline | Jost | 24–130px | 700 | -0.01 to -0.03em |
| App name (table) | Jost | 16–19px | 600 | 0 |
| App name (tile) | Jost | 23–26px | 700 | 0 |
| Step label (deploy) | Jost | 15px | 600–700 | 0.04em |
| Nav item | IBM Plex Mono | 12px | 400/500 | 0.12em |
| Caps label / column head | IBM Plex Mono | 10–11px | 400/500 | 0.10–0.18em |
| Data value | IBM Plex Mono | 12.5–14px | 400/500 | 0 |
| Status word | IBM Plex Mono | 11–12px | 400/500 | 0.06–0.10em |

All caps labels and status words are uppercase. Nav active item = `yellow`; inactive =
`cream-dim`.

---

## 4. Status grammar (the four glyphs)

The heart of the system. Same shapes, same meanings, every screen. CSS-drawn — no icon
assets.

| Glyph | Shape | Meaning | CSS |
|-------|-------|---------|-----|
| ● | filled circle | running / reachable / ok / verified | `width/height; border-radius:50%; background:<blue>` |
| ▲ | up-triangle | degraded / high-lag / verifying / warn | `width:0;height:0;border-left/right:<n> solid transparent;border-bottom:<2n> solid <yellow>` |
| ▼ | down-triangle | crashing / unreachable / down / drift | `…border-top:<2n> solid <red>` (mirror of ▲) |
| ■ | square | stopped / idle / unverified | `width/height; background:<ink or ink-muted>` |

Sizes: **13–18px** in tables/lists, **22–26px** as tile-band heroes. On a colored band the
glyph flips to the contrasting neutral (e.g. a cream ● on a blue band; a cream ▼ on a red
band; a black ▲ on a yellow band).

> Earlier iteration note: crashing is the **red down-triangle** specifically — the exact
> mirror of the yellow degraded up-triangle. Do not substitute a stemmed arrow.

---

## 5. Components

### Header bar
- Background `ink`; padding `20px 34px`; full width.
- Left: red 22px square + `nockd` (Jost 28/700, `cream`).
- Center/right: nav — mono caps items, `gap:28px`; **active = `yellow`**, rest `cream-dim`.
- Far right: host string, mono 12px `cream-dim` (e.g. `localhost:7777`).

### Stat-block row (Mondrian header)
- 4-column grid, **7px** black gaps, `padding:7px 0` (black top/bottom hairline).
- Block 1 = `paper` (neutral total). Blocks 2–4 = `blue` / `yellow` / `red` (or `ink` for a
  neutral-but-emphatic count like "STALE" / "UNVERIFIED").
- Each block: big mono number (38–46/700) + caps label (mono 11/0.14em). Number color
  contrasts the block (ink on paper/yellow, cream on blue/red/ink).

### Data table
- Column header row: mono caps labels, `ink-label`, `border-bottom:4px ink`.
- Rows: `border-bottom:2px ink`; padding `14–17px 0`; grid columns.
- Cell order: status glyph · app name (Jost) · mono data columns · status word.
- Idle/stopped row dims name to `ink-idle` and data to `ink-muted`.

### Metro tile
- `paper` body in a 6px black gridded grid.
- **Status band** (top, 54–60px) in the status color: glyph + caps status label on the
  left, a key metric on the right (lag ms, uptime, etc.).
- Body: big Jost name + optional `REMOTE`/kind chip; mono meta lines; an optional bar; and
  at the bottom a caps label + chips (e.g. attached apps).
- `+ NEW` tile: `paper` with a 3px dashed border and a CSS-drawn black `+`.

### Chips
- `paper` bg, **1.5px** `ink` border, no radius, mono 11px, padding `3px 9px`. Used for
  attached-app lists and `REMOTE` / `LOCAL-SOCKET` tags.

### Buttons
| Variant | Style |
|---------|-------|
| Primary | `ink` bg, `cream` text, mono caps, padding `~11px 20px` |
| Secondary | `paper` bg, **2px** `ink` border, `ink` text |
| Destructive | `paper` bg, `red` border + `red` text (e.g. STOP, stale ROTATE) |
| Nav/segmented | flat; active filled `ink` w/ `yellow` text, inactive `paper` w/ border |

### Bars
- Track `track` (`#ddd4c0`); fill in the semantic color, width = value%.
- Over-threshold lag fills `yellow`; healthy `blue`.
- **Unreachable / no-route**: a `repeating-linear-gradient(45deg, red, red 4px, track 4px, track 8px)` hazard bar.
- **Redaction** (secrets value): a solid `ink` bar containing dim mono dots — reads as a
  redacted field. Never put a real value here.

### Modal (deploy, etc.)
- Centered over a `rgba(26,24,21,0.62)` scrim on a dimmed (opacity ~0.45) backdrop of the
  underlying screen.
- `ink` title bar (Jost 18/600 cream + `✕`); `paper` body; **4px** `ink` section dividers.
- Pipeline steps: a numbered square (status-colored, cream/ink numeral) + Jost label +
  mono detail + status word. Active step sits in a full `yellow` band.

### Banner (security)
- Full-width `yellow` strip, padding `13px 34px`: black 16px square + mono caps text.
  Used on Secrets to state the encryption/redaction guarantee.

---

## 6. Spacing & sizing

- Frame (mock): 1360 × 864 design units; production is fluid — preserve the **proportions
  and rules**, not the literal frame.
- Outer screen padding: `20–42px` horizontal (`34px` is the table/header default).
- Stat numbers 38–46px; section labels 10–11px. Keep the large-number / tiny-label
  contrast — it is core to the Metro feel.
- Minimum hit target for any action (button, ROTATE, nav) ≥ 40px tall in production.

---

## 7. Do / Don't

- **Do** keep color scarce — a calm screen is mostly paper + black, with primaries only
  where status demands.
- **Do** keep all numbers/hashes/timestamps in IBM Plex Mono.
- **Don't** add gradients (except the one hazard bar), shadows (except the modal lift),
  rounded corners (except ●), or a fourth accent hue.
- **Don't** ever render a secret value; redact with the black bar.
- **Don't** soften the rules — the heavy black lines are the identity.
