# RECIPE — building & deploying `common-blog` end to end

The honest transcript for building a **stateful, CRUD-over-HTTP NockApp** — a minimal blog —
from the `http-server` template, building it with the real `nockup` toolchain, deploying it
under `nockd` in **project mode**, and **proving published posts persist across restarts**.

Verified on macOS (darwin, aarch64-apple-darwin) on 2026-06-24, against nockchain rev
`07577127958db94be12e95ea816f31bc7582aa2c` (origin/master; carries PR #134's `HTTP_PORT`).

This builds directly on [`http-static/RECIPE.md`](../http-static/RECIPE.md) and
[`http-counter/RECIPE.md`](../http-counter/RECIPE.md). Everything they document still applies:
the toolchain pin (`rust-toolchain.toml` → `nightly-2026-04-03`), the `HTTP_PORT` +
`EXPIRE_CACHE=0` env vars set in `main.rs` before the driver starts, the harmless
`invalid character '{' in package name` + `unused import: fs` build noise, and the
`++inner` door having **exactly** `load`/`peek`/`poke` (helpers go in the prelude core).
Below are only the things that were **NEW** for a CRUD blog — chiefly **Hoon-side URI-path and
POST form-body parsing**, and **map CRUD**.

---

## 0. Scaffold

`http-static` was the cleanest base (single GET, `HTTP_PORT`, no proxy). Assembled by hand
(don't fight `nockup project init`):

```sh
mkdir -p common-blog/hoon/{app,lib,common} common-blog/src
cp http-static/build.rs common-blog/
cp http-static/rust-toolchain.toml common-blog/
cp http-static/nockapp.lock common-blog/
cp http-static/hoon/lib/{http,lib}.hoon common-blog/hoon/lib/
cp http-static/hoon/common/wrapper.hoon common-blog/hoon/common/
# then wrote: Cargo.toml, nockapp.toml, .gitignore, nockd.toml, src/main.rs, hoon/app/app.hoon
```

`src/main.rs` is `http-static`'s verbatim, with the port changed to **8085** and the names
changed to `common-blog`. No new Rust — all the blog logic is in `hoon/app/app.hoon`.

---

## 1. The build: five Hoon type errors, in order

`out.jam` is only written when the Hoon compile *fully* succeeds. nockup prints
`✓ Hoon compilation completed successfully!` **even when the compile crashed** (it reports the
*step* ran, not that it produced a kernel) — so the real signal is **"did `out.jam` appear?"**
To read the actual error you must strip nockup's ANSI/line-trace spew:

```sh
nockup project build common-blog 2>&1 \
  | LC_ALL=C tr -cd '\11\12\15\40-\176' | sed -E 's/\x1b\[[0-9;]*m//g' \
  | grep -iE 'mint-vain|nest-fail|syntax error|find-fork|crash|app.hoon::\[' | grep -v hoonc
```

The error class is the first token (`nest-fail`, `mint-vain`, `find-fork`, …); the innermost
`...app.hoon::[L C].[L C]` span that follows points at the offending source. The five we hit,
in build order — all generically useful when writing NockApp Hoon:

1. **`syntax error` in `?+`** — a `?+ i.t [default] '&' (...) '<' (...) ==` switch with cord
   literals as cases choked the parser. Rewrote as a `?:` ladder (`?: =('&' i.t) ...`). For a
   handful of cases the `?:` ladder is more robust than `?+` anyway.
2. **`nest-fail` on `(cass title)`** — `++cass` (lowercase) takes a **`tape`**, not `@t`. It's
   `(cass (trip title))`, not `(trip (cass title))`. (Same shape bites you with `crip`/`trip`
   constantly — `cass`/`cuss`/`turn` are tape→tape.)
3. **`nest-fail` on `++css`** — a `'''…'''` triple-cord block is a **`@t` cord**, but the arm
   was `^- tape`. Wrap it: `%- trip '''…'''`. (Every multi-line string literal is a cord.)
4. **`mint-vain` in the index loop** — after `?~ order.st` narrowed `order.st` to non-null,
   the inner `|-` reused that face, so its `?~ slugs ~` base case was *provably* dead
   (`mint-vain` = a branch can never be taken). Fix: rebind a fresh face for the loop
   (`=/ slugs=(list @t) order.st |- ?~ slugs ~ …`). **Lesson: don't recurse over a face that
   an enclosing `?~` already narrowed** — give the loop its own variable.
5. **`find-fork` on `u.hi`** — guarding with `?: |(=(~ hi) =(~ lo))` does **not** narrow the
   `(unit @)`s, so `u.hi` afterward is a type error. Only `?~`/`?=` narrow. Fix: sequential
   `?~ hi … ?~ lo …` before touching `u.hi`/`u.lo`.

After those five, `out.jam` (≈617K) + `target/release/common-blog` (≈16M) built clean.

---

## ⚠️ NEW ROUGH EDGE — parsing the URI path + the POST form body, in Hoon

The counter/static examples only ever switched on a whole-string `uri` (`=('/' uri)`). A blog
needs to (a) pull a **slug** out of `/post/<slug>`, and (b) read **form fields** out of the
POST body. There's no `de-purl:html` in the kernel's `hoon.hoon` (that lives in `zuse`), so
both are hand-rolled in `app.hoon`. The patterns, for reuse:

**Slug from path** — strip a literal prefix, drop any `?query`:

```hoon
++  slug-from-uri
  |=  uri=@t  ^-  (unit @t)
  =/  t=tape  (trip uri)
  ?.  =("/post/" (scag 6 t))  ~              ::  not under /post/
  =/  rest  (slag 6 t)
  =/  q  (find "?" rest)
  =/  slug  ?~(q rest (scag u.q rest))
  ?~(slug ~ `(crip slug))
```

**Form body** — `application/x-www-form-urlencoded` is split on `&` then `=`, with each half
percent/plus-decoded, into a `(map @t @t)`:

```hoon
++  split       |=([del=@t t=tape] ...)        ::  split a tape on one delimiter char
++  decode      |=(t=tape ...)                 ::  '+' -> ' ', '%XX' -> byte, else literal
++  parse-form  |=(body=(unit octs) ...)       ::  (split '&') -> (split '=') -> decode -> map
```

Two real gotchas writing these:
- The body arrives as `(unit octs)` = `[p=@ q=@]` (byte length + payload atom). `(trip q.u.body)`
  gives you the raw query string as a tape. Then you `(~(gut by form) 'title' '')` to read a
  field with a default.
- Percent-decoding needs **sequential `?~` narrowing** on the two `(unit @)` hex digits (see
  build error #5) — and `curl --data-urlencode` is the easy way to exercise it.

Once the form map exists, the rest is just **map CRUD** with the standard `++by` engine:
`(~(put by posts) slug post)`, `(~(get by posts) slug)`, `(~(del by posts) slug)`,
`(~(has by posts) slug)`, `~(wyt by posts)` for the count. The `order` list (newest-first, for
the index) is maintained alongside: prepend the slug on a *new* publish, `skip` it out on
unpublish. None of this needed anything beyond the base `hoon.hoon` — `cass`, `find`, `scag`,
`slag`, `flop`, `weld`, `turn`, `by`.

> `POST /publish` and `/unpublish` reply with an HTTP **303** + `location` (and an explicit
> `content-length: 0` header so the redirect body is empty) so a browser form does the
> post-redirect-get dance. `curl` follows it with `-L`; without `-L` you just see the 303.

---

## 2. Smoke test the binary directly (before nockd)

```sh
cd examples/common-blog
rm -rf .data.common-blog
./target/release/common-blog &        # serves 127.0.0.1:8085 (HTTP_PORT)

curl -s localhost:8085/ | grep -i 'No posts'                 # empty index
curl -s -X POST localhost:8085/publish \
  --data-urlencode 'title=Hello, NockApp World!' \
  --data-urlencode 'body=line one
line two'                                                     # -> 303 /post/hello-nockapp-world
curl -s localhost:8085/ | grep '<li>'                        # post listed
curl -s localhost:8085/post/hello-nockapp-world              # title + <pre>body</pre>
```

Verified along the way: the **slugifier** (`Hello, NockApp World!` → `hello-nockapp-world`;
collapses runs of punctuation/spaces to single `-`, trims, lowercases, digits kept — e.g.
`<script>alert(1)</script> & "quotes"` → `script-alert-1-script-quotes`); **HTML escaping**
(a `<script>` title/body renders as inert `&lt;script&gt;`); **newlines preserved** in the
`<pre>` body; **404** for both unknown `/post/<slug>` and unknown paths; **unpublish** removes
the post and re-renders the index; and a `metric: posts=<N>` slog line on **every** request.

`kill -TERM <pid>` → clean `received SIGTERM; shutting down cleanly` log.

---

## 3. Deploy — project mode works now

Unlike when the earlier recipes were written, **project-mode deploy works** on this
nockd/nockup pair (nockd passes nockup the absolute project path):

```sh
export PATH="$PATH:/Users/neal/zorp/nockd/target/release"
cd examples/common-blog
rm -rf .data.common-blog                  # let nockd own the state dir
nockd deploy -f nockd.toml                # nockd shells out to nockup, builds, ships
#   deployed common-blog
#     artifact a63e1fd4…
#     kernel   5f12f0b7…
nockd restart common-blog                 # start it
```

`nockd.toml` is the whole manifest — `app`, `project = "."`, `restart = "always"`, and the
`POSTS` status metric:

```toml
[deploy.status]
label = "POSTS"
cmd   = "grep -oE 'posts=[0-9]+' | tail -1 | grep -oE '[0-9]+'"
```

(No `-a` needed on this grep — nockd strips NULs from the boot log before piping it to the
status cmd. No `endpoint` — no chain.)

---

## 4. Verify + the persistence proof

```sh
nockd ps
# NAME         STATE    HEALTH   VERIFIED  PID    ENDPOINT  STATUS
# common-blog  running  unknown  verified  34384  —         POSTS 2

curl -s -X POST localhost:8085/publish --data-urlencode 'title=My First Post'    --data-urlencode 'body=Hello from the Hoon kernel!'
curl -s -X POST localhost:8085/publish --data-urlencode 'title=Second Thoughts' --data-urlencode 'body=Posts persist across restarts.'
curl -s localhost:8085/ | grep '<li>'
# <li><a href="/post/second-thoughts">Second Thoughts</a></li>
# <li><a href="/post/my-first-post">My First Post</a></li>

# === PERSISTENCE PROOF ===
nockd restart common-blog                 # new PID 34384
curl -s localhost:8085/ | grep '<li>'     # SAME two posts — survived the restart
curl -s localhost:8085/post/second-thoughts   # body "Posts persist across restarts." intact
```

`running + verified` (self-attested by the trusted builder key). `POSTS` populated and matches
the rendered index. **Both posts — titles, bodies, and index order — survived `nockd restart`
with a fresh PID**, because the post map lives in the checkpointed Hoon kernel state. The
headline feature, confirmed.

---

## 5. Summary of what was NEW (vs http-static / http-counter)

- **Hand-rolled URI-path + POST form-body parsing in Hoon** (no `de-purl:html` in the kernel
  `hoon.hoon`): prefix-strip + query-drop for the slug; `&`/`=` split + percent/plus-decode
  for the form, into a `(map @t @t)`. Body comes in as `(unit octs)`; `(trip q.u.body)`.
- **Map CRUD** via `++by` (`put`/`get`/`del`/`has`/`gut`/`wyt`) for the post store, plus a
  parallel `order` list for the index (prepend on new publish, `skip` on unpublish).
- **Five ordinary Hoon type errors** worth internalizing: `?+` cord-case parse fragility,
  `cass` is tape→tape, `'''…'''` is a cord (needs `trip`), `mint-vain` from recursing over an
  outer-`?~`-narrowed face, and `find-fork` because `|(=(~ a) =(~ b))` doesn't narrow units
  (only `?~`/`?=` do).
- **Project-mode deploy works** here (`nockd deploy -f nockd.toml` builds via nockup) — the
  earlier recipes' "project-mode broken, use prebuilt `--bin/--jam`" caveat no longer applies.
