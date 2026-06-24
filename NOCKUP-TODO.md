# Nockup TODO

Upstream issues in **nockup** (and the templates) surfaced while building NockApp examples
for nockd. These are *not* nockd bugs — nockd's own fixes for the same fan-out are in the
git log. File/track these against `nockchain/nockchain` (crates/nockup) and the typhoon
templates. Workarounds live in [`examples/GOTCHAS.md`](./examples/GOTCHAS.md).

## 1. `nockup project build` can't find its own project
Run with no arg from inside the project dir, nockup interprets the package **name** as a
**subdirectory** (`<dir>/<name>`) and fails: `Project directory '<name>' not found`.
- **Fix one of:** honor cwd (build the project in the current directory), or accept `.`, or
  document that the arg is a path. nockd now works around it by passing the absolute project
  path — but a bare `nockup project build` from inside a project should Just Work.

## 2. `nockup project init` quirks
- Nests the new project under a `<name>/` subdirectory instead of the current directory.
- Leaves `rev = ""` (the upstream pin) **unsubstituted** in the generated manifest.
- **Fix:** scaffold into the current dir (or document the nesting), and substitute the rev
  from the selected template/channel.

## 3. nockup doesn't honor the rev's Rust toolchain pin
Builds use the **ambient** Rust toolchain, not the one the pinned nockchain rev requires.
Symptom: cryptic `error[E0658]` about `cold_path` from `nockvm`.
- **Workaround:** add a project `rust-toolchain.toml` (`nightly-2026-04-03`).
- **Fix:** nockup should pin/select the toolchain for the rev it's building against (or
  scaffold the `rust-toolchain.toml`).

## 4. `grpc` template pins a stale nockchain rev
The `grpc` template pins `485e914`, which predates the v2 public client /
`explorer_heaviest_height`.
- **Workaround:** bump to `6d29078` (matches typhoon's `nockchain` workspace).
- **Note the API drift** consumers must follow: `boot::setup` takes `cli` (not `Some(cli)`),
  and `NockApp<J>` is now generic.
- **Fix:** keep template revs current with typhoon, and bump on a schedule.

## 5. (App-graph, not strictly nockup) rustls dual-provider panic
TLS apps built from the templates pull both `ring` and `aws-lc-rs` rustls providers →
panic on first handshake.
- **Workaround:** pin `rustls`/`ring` + `install_default()` (see GOTCHAS).
- **Fix:** templates that bring in `nockapp-grpc` + TLS should pin a single provider so apps
  don't have to.
