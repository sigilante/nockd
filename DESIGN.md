# nockd — Design Document

> **Status:** Bedrock truth. This document is the authoritative design reference for
> `nockd`. Code follows this doc; when they disagree, fix one of them deliberately.
>
> **Last revised:** 2026-06-22

---

## 0. One-paragraph summary

`nockd` is a self-hostable deployment platform for [NockApps](https://github.com/nockchain/nockchain).
It takes the clean, content-addressed artifact that the Nockup toolchain already
produces — a Rust wrapper binary plus a Nock kernel (`out.jam`) — and runs it as a
supervised, stateful, long-lived service that attaches to a Nockchain node over gRPC.
It ships as a single static Rust binary that is both a **daemon** (`nockd serve`) and a
**client** (`nockd deploy`, `nockd ps`, ...), and it serves a **browser dashboard** out
of that same binary. It is useful on one box with zero control plane, and scales to a
fleet later. The goal is Fly.io-grade developer experience, Nock-native, and
self-hosted.

---

## 1. Motivation

The Nockup project made *building* NockApps feasible: scaffold from a template, manage
Hoon library dependencies, wrap `hoonc` + `cargo`, produce a binary. But Nockup stops at
`nockup run`, which is literally `cargo run --release` in a local directory. There is no
deployment story: no supervision, no persistence, no remote target, no lifecycle, no
secrets, no observability. The one cloud gesture (a Replit template) died on memory
limits.

The original effort to close this gap (Zorp Nockup) stalled for various reasons. The
lesson we take from that is **not** "build a managed PaaS." It is the
opposite: build an **open, self-hostable engine**  and let hosted convenience be an
optional layer on top.

This is feasible *because the NockApp artifact is unusually deployment-friendly* (§3).

---

## 2. Goals and non-goals

### 2.1 Goals

- **G1 — One-command deploy.** `nockd deploy <app>` builds, ships, and runs a NockApp as
  a supervised service. Solo dev on a $5 VPS or a Raspberry Pi is a first-class target.
- **G2 — Self-host-first, OSS-first.** The engine runs standalone on one box with no
  control plane and no external dependency. Anyone can run it; nothing phones home.
- **G3 — Stateful by default.** NockApps are event-sourced. `nockd` owns and persists
  app state (jam snapshot + event log), backs it up, and restores it.
- **G4 — Browser dashboard.** A web UI, served from the `nockd` binary, for fleet
  overview, per-app status, live logs, chain-attach health, and lifecycle actions
  (deploy / rollback / restart / stop).
- **G5 — Verifiable deploys.** Because `hoonc` is byte-reproducible (confirmed) and we
  pin the Rust toolchain, an artifact is content-addressed. `nockd` can answer "does what
  is deployed match this source?" with a hash comparison.
- **G6 — Light chain-interacting apps are the default tier.** The default deployment is a
  small process dialing a **remote** Nockchain gRPC endpoint (confirmed: remote gRPC is
  the default for Nockchain instances). No colocated node required.
- **G7 — State-preserving upgrades.** Swap an app's kernel while preserving its state
  (Urbit-style "molt"). This is the hardest and most valuable capability; it is the thing
  `nockd` should do better than anyone (phase 2).

### 2.2 Non-goals (for now)

- **N1 — Not a managed multi-tenant PaaS** in phase 0–2. Billing, tenancy isolation, and
  a hosted fleet are explicitly later/optional. (This is the surface area that sank the
  prior effort.)
- **N2 — Not a Kubernetes operator / CRD model.** Overkill for the "feasible for users"
  goal. A single-node reconciler gives us declarative behavior without the weight.
- **N3 — Not optimized for heavy proving/mining workloads** in the default tier. Those
  are RAM/CPU-intensive (this is what killed Replit) and get an explicit opt-in tier
  later (§11, phase 3). The default design must not be distorted to carry them.
- **N4 — Not a fork of Nockup.** Nockup now lives in `nockchain/nockchain`. `nockd`
  *consumes* the toolchain (`hoonc`, the Nockup build path); it does not reimplement it.

### 2.3 Design principles

1. **Single static binary.** Daemon, client, and dashboard ship as one artifact. The
   dashboard's static assets are embedded. No runtime install dance.
2. **Boring core, clever edges.** SQLite, plain process supervision, HTTP+JSON. Save the
   cleverness for content-addressing, state migration, and verification.
3. **Declarative desired state.** You declare what should run; `nockd` reconciles reality
   to match and keeps it matched. No imperative "start this PID" thinking.
4. **Standalone-complete.** Every layer must be useful with the layer above absent. One
   `nockd` with no control plane is a finished product.
5. **Secrets never touch artifacts or logs.** Wallet keys are referenced, resolved at
   launch, redacted everywhere.
6. **State is sacred.** Never destroy an app's state dir implicitly. Upgrades and
   rollbacks always preserve a recoverable snapshot.

---

## 3. Why NockApps are special (the leverage)

Three properties of the artifact drive the whole design and make `nockd` better than
"just use Docker/Fly":

1. **The artifact is clean and content-addressable.** A NockApp is one Rust wrapper
   binary + a Nock kernel (`out.jam`). `hoonc` is byte-reproducible, so the same Hoon
   yields the same jam yields the same hash. With a pinned Rust toolchain and
   `--locked`, the whole artifact gets a stable BLAKE3 identity. This buys **verifiable
   deploys**, trivial **rollback**, and **dedup** — none of which a normal container
   build gives you.

2. **State is a portable jam.** App state is a snapshot (jammed noun) plus an event log.
   Backup, restore, and host-to-host migration are *file copies*. This is a structural
   advantage over stateful containers, and it makes G3/G7 tractable.

3. **The hard, valuable problem is the stateful kernel upgrade ("molt").** Every
   long-lived chain app eventually needs to swap kernel logic without losing state, and
   nobody wants to hand-roll it. `nockd` owning this is the moat.

And the topology fact that makes the default tier light: **NockApps do not embed a
node.** They dial a Nockchain node over gRPC (`--nockchain-socket` / gRPC client; see the
`chain` template). Most apps only poke/peek the chain and are tiny; only proving/mining is
heavy. So the default deploy is "small process + remote RPC URL," not "provision a node."

---

## 4. Core concepts and vocabulary

| Term | Meaning |
|---|---|
| **Artifact** | An immutable, content-addressed bundle: the Rust wrapper binary, the `out.jam` kernel, and a provenance manifest (source manifest, `hoonc` version, toolchain pin, build host). Identified by `blake3:<hex>`. |
| **App** | A named, long-lived deployment. Has desired state: which artifact, config, secrets refs, nockchain attachment, restart policy. The unit a user reasons about. |
| **Instance** | A running OS process supervised by `nockd` for an App. Light/stateful apps are single-instance by definition (state is local and singular). |
| **State dir** | The persistent directory `nockd` owns for an App: jam snapshots, event log, app-written files. Never destroyed implicitly. |
| **Endpoint** | A named Nockchain attachment target = a **public-gRPC URL** `http://host:port` of a node (e.g. `mainnet-rpc → http://1.2.3.4:5555`). Apps reference endpoints by name so the URL can change without redeploying. See §5.3. |
| **Admin address** | An app's own inbound **private-gRPC** `host:port` (`Peek`/`Poke`/`Ping`), bound to localhost, that `nockd` uses for health and control. See §5.3. |
| **Secret** | A named, encrypted-at-rest value (e.g. a wallet key). Apps reference secrets by name; resolved into the process environment/files at launch, redacted everywhere else. |
| **Desired state** | The declarative record (in SQLite) of what should be running. |
| **Observed state** | What is actually running, as seen by the supervisor. |
| **Reconciler** | The loop that drives observed state toward desired state and keeps it there. |

---

## 5. Architecture

```
                         ┌──────────────────────────────────────────────┐
   nockd CLI (client) ── │  Control API  (HTTP+JSON over Unix socket;    │
   browser dashboard  ── │                optional TCP+TLS)              │
                         ├──────────────────────────────────────────────┤
                         │  Reconciler  (desired → observed)             │
                         ├───────────────┬───────────────┬──────────────┤
                         │  Supervisor   │  Artifact      │  State       │
                         │  (proc lifecycle, │  store      │  manager     │
                         │   health, restart)│ (content-   │ (snapshots,  │
                         │                │   addressed)   │  backup)     │
                         ├───────────────┼───────────────┼──────────────┤
                         │  Registry (SQLite: apps, artifacts,           │
                         │            endpoints, secrets-meta, events)   │
                         ├──────────────────────────────────────────────┤
                         │  Log/metrics collector   │  Secrets store     │
                         │  (ring buffer + files)   │  (encrypted)       │
                         └──────────────────────────────────────────────┘
                                         │
                                         ▼  supervised processes
                            ┌──────────────┐   ┌──────────────┐
                            │ NockApp inst │   │ NockApp inst │  ──gRPC──▶  Nockchain node
                            │  + state dir │   │  + state dir │            (remote, default)
                            └──────────────┘   └──────────────┘
```

### 5.1 Components

- **Supervisor.** A tiny, specialized init for NockApps: spawn, monitor, restart per
  policy (`always` / `on-failure` / `never`, with backoff), graceful shutdown
  (SIGTERM → grace → SIGKILL), healthcheck. It must let the app reach a clean snapshot
  before kill where possible.
- **Artifact store.** Content-addressed local store at `~/.nockd/artifacts/blake3/<hash>/`
  holding `bin`, `out.jam`, and `provenance.toml`. Uploads that already exist are no-ops
  (dedup). Unreferenced artifacts are GC'd on a retention policy, never if they are the
  current or previous artifact of any app.
- **State manager.** Owns each App's state dir. Schedules snapshots, performs backups
  (local dir, later object storage), and restores. Knows how to copy/migrate a state dir
  between hosts (jam portability).
- **Registry.** SQLite holds desired state and metadata: apps, their current/previous
  artifact, endpoints, secret metadata (not values), and an append-only event log
  (deploys, restarts, health transitions). SQLite because it is boring, single-file,
  backup-friendly, and zero-ops.
- **Reconciler.** Periodically (and on every API mutation) compares desired vs observed
  and acts: start missing instances, restart crashed ones per policy, stop removed apps,
  roll to a new artifact, gate on health.
- **Control API.** HTTP+JSON. Binds a **Unix socket by default** (local, file-permission
  gated). Optional TCP listener with TLS + token auth for remote CLI and the dashboard.
  This single API is the only way in: CLI and dashboard are both just clients.
- **Dashboard.** Static assets embedded in the binary (`rust-embed`), served by the same
  HTTP server, talking to the same API. Live logs stream over SSE/WebSocket. See §9.
- **Log/metrics collector.** Captures per-instance stdout/stderr to a bounded ring buffer
  (for live tail) and rotated files (for history). Collects basic metrics: CPU, RSS,
  restart count, uptime, last-health, chain-attach status, state size.
- **Secrets store.** Encrypted-at-rest named values. Resolved into a launched process's
  environment or a tmpfs file at start; never written to artifacts, logs, or the
  dashboard in cleartext.

### 5.2 The one-binary, two-modes model

```
nockd serve                 # run the daemon (supervisor + API + dashboard)
nockd deploy <app>          # client: build, ship artifact, set desired state
nockd ps                    # client: list apps + status
nockd logs <app> [-f]       # client: fetch / follow logs
nockd rollback <app>        # client: point app at its previous artifact
nockd restart|stop <app>    # client: lifecycle
nockd secret set <name>     # client: write an encrypted secret
nockd endpoint add <name> <url>   # client: register a Nockchain endpoint
nockd --host <addr> ...     # target a remote daemon (TLS + token)
```

A client subcommand with no `--host` talks to the local daemon over the Unix socket. This
is the `flyctl`-in-one ergonomic: nothing to install separately, the daemon and the tool
are the same program.

> **Build dependency:** `nockd deploy` invokes the upstream Nockup/`hoonc` build path to
> produce the artifact. `nockd` does not reimplement the compiler or the template system
> (N4). It shells out to / links the toolchain and then takes ownership at the artifact
> boundary.

### 5.3 The Nockchain gRPC surface (bedrock — verified against `nockchain/nockchain`)

Confirmed by reading `crates/nockapp-grpc*` and `crates/nockchain/src/config.rs`. These
are facts `nockd` builds on, not assumptions.

**Transport.** gRPC over HTTP/2 via `tonic`, bound to a **TCP `SocketAddr`** (not a Unix
socket). Clients connect with a URL string the tonic `Endpoint` accepts — i.e.
`http://host:port`. **There is no built-in TLS or auth**: the code and its docs explicitly
say "do NOT expose to an untrusted network… use an SSH tunnel or VPN with firewalling."
The kernel acknowledges a bind via a `[%grpc-bind result]` effect.

**Three service surfaces:**

1. **Public — `nockchain.public.v1/v2 NockchainService`.** The node's chain API the app
   *dials outward* to: `WalletGetBalance`, `WalletSendTransaction`, `TransactionAccepted`;
   v2 adds `NockchainBlockService` (blocks/tx lookup) and `NockchainMetricsService`
   (explorer/peer/req-res metrics). A node exposes it with
   `--bind-public-grpc-addr` (**off by default**, recommended `127.0.0.1:5555`). **This is
   the endpoint a deployed NockApp attaches to.**
2. **Private — `nockchain.private.v1 NockAppService`.** `Peek` and `Poke` (JAM-encoded
   path / wire+payload). This is the *admin control channel into a running NockApp*. A node
   exposes it with `--bind-private-grpc-addr` / `--bind-private-grpc-port`
   (**default `5555`, localhost**). Marked "core/admin path — do NOT expose to untrusted
   networks."
3. **Monitoring — `nockchain.monitoring.v1 MonitoringService`.** A `Ping` RPC. In addition,
   the servers register the **standard gRPC health service** (`tonic_health`) and
   reflection.

**What this resolves for `nockd`:**

- **An `endpoint` is a `http://host:port` gRPC URL to a node's *public* service** — not a
  Unix-socket path. The old `chain` template's `--nockchain-socket=PATH` idiom is
  superseded by this gRPC-address model. The endpoint registry stores URLs; the default
  attach is a remote public-gRPC node.
- **Health gating has a real, ready mechanism.** `nockd` can probe a supervised app's
  **private gRPC** via `MonitoringService.Ping` and/or the standard gRPC health protocol,
  and can drive liveness with a `Poke`/`Peek`. The manifest's `health = { poke = "ping" }`
  maps onto an actual private-gRPC call — apps need not implement a custom HTTP health
  endpoint.
- **`nockd` becomes the trusted local front for the unauthenticated private channel.**
  Because the private/admin gRPC has no TLS or auth, the right pattern is: keep each app's
  private gRPC bound to **localhost**, let `nockd` (the local supervisor) be the only thing
  that speaks it, and expose a *safe, authenticated* surface (the `nockd` API + dashboard,
  §9–§10) on top. For remote *public* endpoints, `nockd` documents SSH-tunnel / VPN /
  TLS-terminating reverse-proxy as the supported reach, matching upstream guidance.

So an app deployment involves **two gRPC addresses**: the outbound **public** endpoint it
dials (the chain), and its own inbound **private** admin address that `nockd` binds to
localhost and uses for health and control.

---

## 6. Data model (Registry / SQLite)

Indicative schema; exact columns evolve, but these are the entities of record.

```sql
-- An immutable, content-addressed build output.
artifact(
  hash            TEXT PRIMARY KEY,   -- "blake3:<hex>" over canonical bundle
  created_at      TEXT,
  hoonc_version   TEXT,
  toolchain_pin   TEXT,
  source_manifest TEXT,               -- the project manifest used to build
  provenance      TEXT                -- build host, reproducible? verified?
);

-- A named, long-lived deployment (desired state).
app(
  name              TEXT PRIMARY KEY,
  artifact_hash     TEXT REFERENCES artifact(hash),
  prev_artifact     TEXT REFERENCES artifact(hash),  -- for one-step rollback
  endpoint_name     TEXT REFERENCES endpoint(name),
  restart_policy    TEXT,             -- always | on-failure | never
  health_spec       TEXT,            -- e.g. {"poke":"ping","timeout":"5s"}
  state_path        TEXT,
  desired_status    TEXT,             -- running | stopped
  created_at        TEXT,
  updated_at        TEXT
);

-- Named Nockchain attachment targets.
endpoint(
  name     TEXT PRIMARY KEY,
  url      TEXT,                       -- grpc://host:port  (remote = default)
  kind     TEXT                        -- remote | local-socket
);

-- Secret metadata only; ciphertext lives in the secrets store.
secret_meta(
  name        TEXT PRIMARY KEY,
  created_at  TEXT,
  last_used   TEXT
);

-- Which secrets an app may resolve.
app_secret(app_name TEXT, secret_name TEXT, env_key TEXT);

-- Append-only audit/event log surfaced in the dashboard timeline.
event(
  id        INTEGER PRIMARY KEY,
  ts        TEXT,
  app_name  TEXT,
  kind      TEXT,                      -- deploy|start|stop|crash|health|rollback|upgrade
  detail    TEXT
);
```

Observed state (live PIDs, current health, resource metrics) is held in memory by the
supervisor, not in SQLite — it is ephemeral and reconstructed on daemon restart.

---

## 7. Manifests and configuration

There are two manifests, kept separate but composable.

### 7.1 Project manifest (build-time — already exists, upstream)

Carried by Nockup in `nockchain/nockchain`: name, template, library deps, the pinned
`nockapp_commit_hash`. `nockd` reads it but does not own it.

### 7.2 Deploy manifest (runtime — new, owned by `nockd`)

A `nockd.toml` describing how to run the app. Secrets are referenced, never inlined.

```toml
[deploy]
app       = "blackjack"
restart   = "on-failure"         # always | on-failure | never
health    = { ping = true, timeout = "5s" }   # private-gRPC Ping / health probe (§5.3)
admin_addr = "127.0.0.1:0"       # app's private gRPC; localhost; 0 = nockd picks a port

[deploy.nockchain]
endpoint = "mainnet-rpc"         # name → public-gRPC URL from the endpoint registry
# resolved to http://host:port; remote public gRPC is the default attach (§5.3)

[deploy.state]
path   = "/var/lib/nockd/blackjack"
backup = "daily"                 # off | hourly | daily

[deploy.secrets]
wallet_key = { ref = "blackjack-wallet", env = "WALLET_KEY" }

[deploy.resources]               # advisory in default tier; enforced in heavy tier
memory = "512MiB"
```

Design choices:

- **Endpoint by name, not URL.** Lets you re-point an app at a different Nockchain node
  (RPC migration, failover) without rebuilding or redeploying.
- **Single instance is implied** for stateful apps; there is no `replicas` knob in the
  default tier because local singular state forbids it. Horizontal scale, if ever, is a
  heavy-tier concern with an explicit state-sharing story.
- **Resources are advisory** in the default (light) tier and enforced (cgroups/VM sizing)
  only in the heavy tier.

---

## 8. Key flows

### 8.1 Deploy

```
1. nockd deploy blackjack
2. client builds via Nockup/hoonc  ──▶  (binary + out.jam)
3. client canonicalizes + hashes   ──▶  artifact = blake3:…
4. client uploads to daemon store   (skipped if hash already present — dedup)
5. client sets desired state: app.blackjack.artifact = blake3:…
6. reconciler acts:
     - resolve endpoint + secrets
     - start new instance against the SAME state dir
     - gate on health (must pass health_spec before old retired)
     - on pass: retire old instance, record event(deploy)
     - on fail: leave old running, surface error, do NOT mutate state
7. rollback = set app.artifact = prev_artifact; reconcile (same gating)
```

The health gate makes a bad deploy a non-event: the previous instance keeps serving until
the new one proves itself.

### 8.2 Supervision / crash recovery

The supervisor watches each instance. On exit: if policy permits, restart with
exponential backoff against the existing state dir (the app recovers from its last
snapshot + event log). Health transitions and crashes are written to the event log and
shown in the dashboard timeline.

Liveness is probed over the app's **private gRPC admin address** (§5.3): the standard gRPC
health service and/or `MonitoringService.Ping`, with an optional `Poke`/`Peek` for a
deeper application-level check. This is also the channel the health gate in §8.1 uses to
decide whether a freshly deployed instance is serving before the old one is retired.

### 8.3 State-preserving upgrade — "molt" (phase 2)

This is the hard one and depends on a kernel-side convention (§13, open question).

```
1. snapshot current state  (rollback point, retained)
2. boot new kernel in a staging instance
3. feed old state through the kernel's migration arm (old-state → new-state)
4. gate on health of the staged instance
5. on pass: atomically swap; retain old snapshot for one-step rollback
6. on fail: discard staged instance; old instance untouched
```

The platform defines the contract (a kernel upgrade/`+load`-style arm that maps prior
state to new state); the templates adopt it. Until that contract exists upstream, `nockd`
treats every deploy as a fresh-state or same-kernel-state deploy and does not attempt
migration.

### 8.4 Backup / restore / migrate

Because state is a jam, backup is a copy of the state dir to a backup target; restore is
the reverse; host migration is backup-on-A + restore-on-B + re-point endpoint. The state
manager schedules these per the manifest's `backup` setting.

---

## 9. Dashboard (browser UI)

Served by `nockd serve` from embedded assets. The dashboard is a first-class deliverable,
not an afterthought — it is a big part of how `nockd` out-shines the prior effort.

### 9.1 Views

- **Fleet overview** — every app: status pill (running / degraded / stopped / crashing),
  artifact short-hash, endpoint, uptime, restart count, state size, chain-attach health.
- **App detail** — live log stream (SSE/WebSocket), event timeline (deploys, crashes,
  health flips), resource graphs (CPU/RSS), current vs previous artifact, snapshots list.
- **Actions** — deploy (upload or pick artifact), rollback (one click to previous
  artifact), restart, stop, edit endpoint/secrets refs. All actions go through the same
  Control API the CLI uses; the dashboard has no privileged backdoor.
- **Endpoints** — registry of Nockchain RPC targets with live reachability/lag status.
- **Secrets** — list names only; set/rotate values; never display cleartext.
- **Verification** — show whether the deployed artifact is reproducible-verified against
  its source (§10).

### 9.2 Tech and ethos

- Assets embedded via `rust-embed`; single-binary ethos preserved (no separate web
  deploy).
- Server-pushed live data via SSE (logs, status). Keep the frontend modest — a small SPA
  or server-rendered + sprinkles. The dashboard is a window onto the API, not a second
  application with its own state of record.

### 9.3 Exposure and safety (see also §10)

- **Localhost by default.** Browser access on the same box needs no extra config.
- **Remote exposure is opt-in** and always requires TLS + a bearer token. For convenience
  we document an SSH-tunnel and a reverse-proxy recipe as the recommended paths; direct
  public binding is allowed but gated behind explicit config and a generated token.

---

## 10. Security model

Browser exposure and wallet keys make this load-bearing, not a footnote.

- **Bind localhost / Unix socket by default.** The Control API's primary listener is a
  Unix socket gated by file permissions. Remote (TCP) is opt-in.
- **Contain the unauthenticated NockApp gRPC.** Nockchain's private/admin gRPC has **no
  TLS and no auth** (§5.3) and is explicitly "not for untrusted networks." `nockd` keeps
  every supervised app's private/admin address bound to **localhost**, is the only speaker
  of it, and re-exports a safe, token-authenticated view through its own API/dashboard. For
  remote *public* (chain) endpoints, `nockd` follows upstream guidance: SSH tunnel, VPN, or
  a TLS-terminating reverse proxy — never a raw public bind.
- **Token auth + TLS for remote.** Any non-local access (remote CLI, remote dashboard)
  requires a bearer token (generated by `nockd` on first run, shown once via CLI) over
  TLS (self-signed bootstrap, ACME or reverse-proxy for real certs).
- **Secrets encrypted at rest**, resolved only into the launched process (env or tmpfs
  file), never logged, never in artifacts, redacted in the dashboard and API responses.
- **Least privilege.** `nockd` should not require root. Apps run as a dedicated,
  unprivileged user; the state dir and secrets store are owned tightly.
- **Verifiable artifacts.** Because the build is reproducible, `nockd` can rebuild from
  pinned source and confirm the deployed artifact hash matches — surfacing
  "verified ✓ / unverified" per app. This detects tampered or drifted deploys.
- **Audit log.** Every privileged action (deploy, secret set, endpoint change, rollback)
  is appended to the event log.

---

## 11. Tiers and topology

- **Default tier — light, chain-interacting apps.** Small process dialing a *remote*
  Nockchain gRPC endpoint. Fits a $5 VPS / Pi. Single `nockd`, single box, no node to
  provision. This is what phases 0–2 target.
- **Heavy tier — proving/mining (phase 3, opt-in).** RAM/CPU-intensive ZKVM work. Needs
  real VMs and enforced resource limits, not micro-containers. Explicitly separate so it
  cannot distort the light default. Likely colocated-node attachment (`local-socket`)
  rather than remote RPC.
- **Shared Nockchain RPC.** Even self-host-first, a community/public RPC endpoint means a
  first deploy needs *zero* node provisioning. Anyone can self-host their own and point an
  endpoint at it; the default just works.
- **Fleet (phase 3, optional).** An aggregating control plane over many `nockd` agents,
  and an optional hosted offering. `nockd` stays standalone-complete (principle 4); the
  control plane only aggregates, it is never required.

---

## 12. Phasing / roadmap

### Phase 0 — Spine (MVP)
Prove the whole stack end-to-end on one box, using `blackjack` (`http-server` template) as
the guinea pig.
- `nockd serve`: supervisor + content-addressed artifact store + SQLite registry +
  Control API over Unix socket.
- `nockd deploy / ps / logs / restart / stop` against the local daemon.
- One app, persistent state dir, crash-restart with backoff, basic health gate.
- Remote gRPC endpoint attachment (the confirmed default).
- Minimal read-only dashboard: fleet list + live logs.

**Explicitly out of phase 0:** molt/upgrade, multi-host, full dashboard actions, real
secrets backend (stub with a file ref), heavy tier.

### Phase 1 — Dashboard + lifecycle
- Full browser dashboard: auth, log streaming, deploy/rollback/restart/stop actions,
  event timeline, resource metrics.
- Rollback to previous artifact; snapshot + backup/restore.
- Multiple apps; endpoint registry; encrypted secrets store.

### Phase 2 — Upgrades + verification
- State-preserving kernel upgrade ("molt") against the kernel-side migration contract.
- Reproducible-build verification ("deployed matches source").
- Host-to-host state migration.

### Phase 3 — Fleet + heavy tier
- Aggregating control plane over multiple `nockd` hosts; optional hosted offering.
- Heavy/proving tier with enforced resources and colocated-node attachment.
- Shared public RPC service.

---

## 13. Open questions / dependencies

- **OQ1 — Kernel upgrade contract.** Molt (§8.3) needs a kernel-side convention (a
  `+load`-style arm mapping prior state → new state). This must be designed *with* the
  NockApp templates upstream. Until it exists, `nockd` cannot do live state migration.
- **OQ2 — Artifact canonicalization.** Exactly what bytes go into the BLAKE3 hash (jam +
  binary + which provenance fields) must be pinned so two honest builders agree. The Rust
  binary's reproducibility (toolchain pin + `--locked` + vendored deps) needs to be
  validated empirically even though `hoonc`'s is confirmed.
- **OQ3 — Health semantics.** Resolved into a concrete mechanism (§5.3): probe the app's
  private gRPC (standard gRPC health + `MonitoringService.Ping`, optional `Poke`/`Peek`)
  for process/app liveness, and surface chain-attach reachability separately in the
  dashboard. Remaining detail: the exact "deep" poke each template should answer.
- **OQ4 — gRPC attach detail. RESOLVED (§5.3).** Attachment is a TCP gRPC URL
  `http://host:port` to a node's *public* service (`--bind-public-grpc-addr`, recommended
  `127.0.0.1:5555`), **not** a Unix socket — the old `--nockchain-socket=PATH` idiom is
  superseded. Each app also exposes its own *private* admin gRPC
  (`--bind-private-grpc-addr`/`-port`, default `5555`) which `nockd` binds to localhost for
  health/control. No TLS/auth on either; `nockd` contains them per §10.
- **OQ5 — Secrets backend.** Phase 1 store: a local encrypted file is the boring default.
  Decide the KDF/sealing and whether to support external backends (age, system keyring)
  later.
- **OQ6 — Daemon restart fidelity.** On `nockd` restart, observed state is rebuilt from
  desired state + live process discovery. Define how orphaned child processes are
  re-adopted vs. restarted.

---

## 14. What success looks like

A solo developer writes a Hoon NockApp, runs `nockd deploy myapp` against a fresh VPS,
opens a browser to a dashboard showing it running, attached to a Nockchain node, with live
logs — and when they push a new kernel, `nockd` upgrades it without losing state, and a
bad deploy rolls back in one click. No company needs to exist for any of that to keep
working. That is the bar, and it should mog what came before.
