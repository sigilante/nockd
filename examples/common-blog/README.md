# common-blog

A minimal **self-hosted blog** NockApp: publish posts and read them back over HTTP, with
**all logic and storage living in the Hoon kernel**. The Rust binary just boots the kernel
from `out.jam` and attaches the library HTTP driver; everything else — the post store, the
slugifier, URI/form parsing, and HTML rendering — is Hoon.

This is a NockApp **reimplementation** of the Urbit ["Common Blog"][cb] app's core *idea*, not
a port of its Hoon. We keep its **data model** (a map of `slug -> {title, body}`) and the
**publish/read UX**, rebuilt in the NockApp `%req`/`%res` request/response shape. Everything
Gall/Urbit-specific in the original (sss-path syndication, Clay export, the React/TSX editor
SPA, ship-auth) is dropped. What's left is CRUD-over-a-map plus HTML rendering — a natural fit
for the `http_driver` poke/effect contract.

[cb]: https://github.com/thecommons-urbit/blog

## What it does

State is a single versioned noun in the kernel:

```hoon
+$  post          [title=@t body=@t]
+$  server-state  [%0 posts=(map @t post) order=(list @t)]
```

`posts` maps a url-safe **slug** (derived from the title) to the post; `order` is the slug
list newest-first, for the index. There's one built-in CSS string inlined into every page.

### Routes

| Method + path        | Behaviour                                                         |
|----------------------|------------------------------------------------------------------|
| `GET /`              | Index page: post titles as links to `/post/<slug>`.              |
| `GET /post/<slug>`   | Render that post (title + body), or a styled **404** if missing. |
| `GET /new`           | An HTML form (title + body textarea) POSTing to `/publish`.      |
| `POST /publish`      | Slugify the title, store the post, **303 redirect** to its page. |
| `POST /unpublish`    | Remove the post named by the `slug` form field, redirect to `/`. |

The slug is parsed from the request `uri` (`/post/<slug>`, query string stripped). Form fields
are parsed from the body `octs` as `application/x-www-form-urlencoded` (split on `&` then `=`,
with `+`→space and `%XX` percent-decoding). All of this is done in Hoon — see
[`hoon/app/app.hoon`](hoon/app/app.hoon).

### Body format: plain text (no Markdown)

To keep scope tiny, **the post body is treated as plain text** — there is no Markdown parser.
The submitted title and body are **HTML-escaped** (`& < > "`), and the body is wrapped in a
`<pre>` block so whitespace and newlines are preserved verbatim and no markup injection is
possible. (If you'd rather accept raw HTML posts, drop the escape in `+render-post` /
`+render-index` in `app.hoon`.)

### Posts persist across restart

Posts live entirely in the Hoon kernel state, and nockd checkpoints kernel state (PMA + event
log). So **published posts survive `nockd restart` / process restart for free** — verified
below.

## Build

```sh
nockup project build common-blog      # -> out.jam + target/release/common-blog
```

Requires the pinned nightly in [`rust-toolchain.toml`](rust-toolchain.toml)
(`nightly-2026-04-03`). The crates are pinned to nockchain rev
`07577127958db94be12e95ea816f31bc7582aa2c`, which carries PR #134's `HTTP_PORT` override so
the stock `http_driver()` binds `127.0.0.1:<port>` directly — no proxy. The port is declared
once as `port = 8085` in `nockd.toml`; nockd exports `NOCKD_APP_PORT` and `main.rs` bridges it to
`HTTP_PORT` (falling back to 8085 when run standalone).

## Deploy (project mode)

```sh
export PATH="$PATH:/Users/neal/zorp/nockd/target/release"
nockd deploy -f nockd.toml      # nockd builds via nockup and ships the artifact
nockd restart common-blog       # start / swap in the freshly built artifact
nockd ps                        # common-blog: running / verified / POSTS <n>
```

The app serves on **`http://127.0.0.1:8085`** (the `port` from `nockd.toml`), and the
dashboard shows an **"Open app ↗"** link to it. `nockd.toml` also sets the `POSTS` status
metric, grepped from the kernel's `metric: posts=<N>` slog line (logged on every request).

## Use it — publish and read over HTTP

```sh
# publish a post (form-encoded title + body)
curl -s -X POST http://127.0.0.1:8085/publish \
  --data-urlencode 'title=My First Post' \
  --data-urlencode 'body=Hello from the Hoon kernel!'
# -> HTTP 303, Location: /post/my-first-post

# the index now lists it
curl -s http://127.0.0.1:8085/ | grep '<li>'
# <li><a href="/post/my-first-post">My First Post</a></li>

# read the post back
curl -s http://127.0.0.1:8085/post/my-first-post
# ...<h1>My First Post</h1><pre class="body">Hello from the Hoon kernel!</pre>...

# remove it
curl -s -X POST http://127.0.0.1:8085/unpublish --data-urlencode 'slug=my-first-post'
# -> HTTP 303, Location: /
```

Or just open <http://127.0.0.1:8085/new> in a browser and use the form.

### Persistence proof

```sh
curl -s http://127.0.0.1:8085/ | grep '<li>'   # your posts
nockd restart common-blog                       # new PID
curl -s http://127.0.0.1:8085/ | grep '<li>'   # SAME posts — survived the restart
```

See [`RECIPE.md`](RECIPE.md) for the full build/deploy transcript and the rough edges hit
along the way (especially Hoon URI-path + POST form-body parsing and map CRUD).
