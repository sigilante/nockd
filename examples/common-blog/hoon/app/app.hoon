::  common-blog: a minimal self-hosted blog served straight from the Hoon kernel.
::
::    A NockApp reimplementation of the Urbit "Common Blog" idea: the data model
::    (slug -> {title, body} map) and the publish/read UX, rebuilt on the NockApp
::    %req/%res request/response shape. CRUD over a map + HTML rendering.
::
::    Routes (all driven by the http_driver poke -> [%res ...] effect):
::      GET  /              -> index page: list of post titles linking to /post/<slug>
::      GET  /post/<slug>   -> render that post (title + body), or a 404 page
::      GET  /new           -> an HTML form (title + body textarea) POSTing to /publish
::      POST /publish       -> slugify the title, store the post, redirect to /post/<slug>
::      POST /unpublish     -> remove the post named by the `slug` field, redirect to /
::
::    Body format choice: to keep scope tiny we DO NOT implement a Markdown parser.
::    The submitted body is treated as PLAIN TEXT: it is HTML-escaped and wrapped in a
::    <pre> block so whitespace/newlines are preserved verbatim and no injection is
::    possible. (If you want raw HTML posts instead, drop the escape in +render-post.)
::
/+  *http
/=  *  /common/wrapper
=>
|%
::  $post: a single blog post.
::
+$  post  [title=@t body=@t]
::  $server-state: the whole versioned blog state.
::
::    posts: slug -> post.   order: slugs newest-first, for the index.
::
+$  server-state  [%0 posts=(map @t post) order=(list @t)]
::
::  +css: one built-in stylesheet, inlined into every page.
::
++  css
  ^-  tape
  %-  trip
  '''
  body{font-family:-apple-system,system-ui,sans-serif;max-width:42rem;
        margin:3rem auto;padding:0 1rem;line-height:1.6;color:#1a1a1a}
  h1{color:#5b21b6;margin-bottom:.2rem}
  a{color:#5b21b6;text-decoration:none}
  a:hover{text-decoration:underline}
  ul.posts{list-style:none;padding:0}
  ul.posts li{margin:.4rem 0;font-size:1.1rem}
  nav{margin:1rem 0;border-bottom:1px solid #eee;padding-bottom:1rem}
  nav a{margin-right:1rem}
  form.editor input,form.editor textarea{display:block;width:100%;
        margin:.4rem 0 1rem;padding:.5rem;font:inherit;
        border:1px solid #ccc;border-radius:.3rem;box-sizing:border-box}
  textarea{min-height:14rem}
  button{background:#5b21b6;color:#fff;border:0;border-radius:.3rem;
        padding:.5rem 1rem;font:inherit;cursor:pointer}
  pre.body{white-space:pre-wrap;word-wrap:break-word;background:#faf9ff;
        padding:1rem;border-radius:.4rem}
  footer{margin-top:2rem;color:#888;font-size:.85rem}
  '''
::
::  +esc: HTML-escape a tape (so plaintext bodies/titles can't inject markup).
::
++  esc
  |=  t=tape
  ^-  tape
  ?~  t  ~
  =/  rest  $(t t.t)
  ?:  =('&' i.t)  (weld "&amp;" rest)
  ?:  =('<' i.t)  (weld "&lt;" rest)
  ?:  =('>' i.t)  (weld "&gt;" rest)
  ?:  =('"' i.t)  (weld "&quot;" rest)
  [i.t rest]
::
::  +slugify: title cord -> url-safe slug cord.
::
::    Lowercase; keep [a-z0-9]; turn any run of other chars into a single '-';
::    trim leading/trailing '-'. Empty result falls back to "post".
::
++  slugify
  |=  title=@t
  ^-  @t
  =/  in=tape  (cass (trip title))      ::  cass: tape -> lowercase tape
  =|  out=tape                          ::  built reversed
  =|  dash=?                            ::  pending separator?
  |-  ^-  @t
  ?~  in
    ::  drop trailing dash, reverse, fall back to "post" if empty
    =/  res=tape  (flop out)
    ?~(res 'post' (crip res))
  =/  c=@t  i.in
  ?:  ?|  &((gte c '0') (lte c '9'))
          &((gte c 'a') (lte c 'z'))
      ==
    ::  alnum: flush a pending dash (only if we already have chars), then keep
    =/  out2  ?:(&(dash ?=(^ out)) [c '-' out] [c out])
    $(in t.in, out out2, dash %.n)
  ::  non-alnum: mark a pending separator (collapses runs)
  $(in t.in, dash %.y)
::
::  +decode: percent- + plus-decode one url-encoded form field value.
::
::    application/x-www-form-urlencoded: '+' means space, '%XX' is a byte.
::    Anything malformed is passed through literally.
::
++  decode
  |=  t=tape
  ^-  tape
  ?~  t  ~
  ?:  =('+' i.t)  [' ' $(t t.t)]       ::  '+' -> space
  ?.  =('%' i.t)  [i.t $(t t.t)]
  ::  '%XX': need two hex digits
  ?~  t.t   [i.t ~]
  ?~  t.t.t  [i.t i.t.t ~]
  =/  hi  (from-hex i.t.t)
  ?~  hi  [i.t $(t t.t)]                    ::  not hex -> pass '%' through
  =/  lo  (from-hex i.t.t.t)
  ?~  lo  [i.t $(t t.t)]
  =/  byte=@  (add (mul 16 u.hi) u.lo)
  [`@t`byte $(t t.t.t.t)]
::
::  +from-hex: a single hex digit char -> (unit @), else ~.
::
++  from-hex
  |=  c=@t
  ^-  (unit @)
  ?:  &((gte c '0') (lte c '9'))  `(sub c '0')
  ?:  &((gte c 'a') (lte c 'f'))  `(add 10 (sub c 'a'))
  ?:  &((gte c 'A') (lte c 'F'))  `(add 10 (sub c 'A'))
  ~
::
::  +split: split a tape on a single delimiter char into a list of tapes.
::
++  split
  |=  [del=@t t=tape]
  ^-  (list tape)
  =|  cur=tape          ::  current field, reversed
  =|  acc=(list tape)   ::  finished fields, reversed
  |-  ^-  (list tape)
  ?~  t
    (flop [(flop cur) acc])
  ?:  =(del i.t)
    $(t t.t, cur ~, acc [(flop cur) acc])
  $(t t.t, cur [i.t cur])
::
::  +parse-form: url-encoded form body -> (map @t @t) of field -> decoded value.
::
++  parse-form
  |=  body=(unit octs)
  ^-  (map @t @t)
  ?~  body  ~
  =/  raw=tape  (trip q.u.body)
  =/  pairs=(list tape)  (split '&' raw)
  =|  out=(map @t @t)
  |-  ^-  (map @t @t)
  ?~  pairs  out
  =/  kv=(list tape)  (split '=' i.pairs)
  ?~  kv  $(pairs t.pairs)
  =/  key=@t   (crip (decode i.kv))
  =/  val=@t   ?~(t.kv '' (crip (decode i.t.kv)))
  $(pairs t.pairs, out (~(put by out) key val))
::
::  +slug-from-uri: pull the slug out of a "/post/<slug>" path.
::
::    Strips a leading "/post/", then drops any query string after "?".
::    Returns ~ if the path isn't under /post/.
::
++  slug-from-uri
  |=  uri=@t
  ^-  (unit @t)
  =/  t=tape  (trip uri)
  =/  pfx=tape  "/post/"
  ?.  =(pfx (scag (lent pfx) t))  ~
  =/  rest=tape  (slag (lent pfx) t)
  =/  q  (find "?" rest)            ::  drop ?query if present
  =/  slug=tape  ?~(q rest (scag u.q rest))
  ?~(slug ~ `(crip slug))
::
::  +page: wrap a title + inner-html body tape in the full HTML document.
::
++  page
  |=  [title=tape inner=tape]
  ^-  tape
  ;:  weld
    "<!doctype html><html><head><meta charset=\"utf-8\"><title>"
    title
    " &middot; common-blog</title><style>"
    css
    "</style></head><body>"
    "<nav><a href=\"/\">common-blog</a><a href=\"/new\">+ new post</a></nav>"
    inner
    "<footer>a NockApp blog &mdash; posts live in the Hoon kernel, "
    "checkpointed by nockd</footer>"
    "</body></html>"
  ==
::
::  +render-index: the home page listing all posts (newest first).
::
++  render-index
  |=  st=server-state
  ^-  tape
  =/  items=tape
    ?:  =(~ order.st)
      "<p>No posts yet. <a href=\"/new\">Write the first one.</a></p>"
    =/  lis=tape
      =/  slugs=(list @t)  order.st
      |-  ^-  tape
      ?~  slugs  ~
      =/  pst  (~(get by posts.st) i.slugs)
      =/  rest  $(slugs t.slugs)
      ?~  pst  rest                       ::  skip dangling slugs
      ;:  weld
        "<li><a href=\"/post/"  (trip i.slugs)  "\">"
        (esc (trip title.u.pst))  "</a></li>"
        rest
      ==
    (weld "<ul class=\"posts\">" (weld lis "</ul>"))
  (page "home" (weld "<h1>common-blog</h1>" items))
::
::  +render-post: a single post page (plaintext body, escaped + <pre>-wrapped).
::
++  render-post
  |=  [slug=@t =post]
  ^-  tape
  ;:  weld
    "<h1>"  (esc (trip title.post))  "</h1>"
    "<pre class=\"body\">"  (esc (trip body.post))  "</pre>"
    "<form method=\"POST\" action=\"/unpublish\" "
    "onsubmit=\"return confirm('Delete this post?')\">"
    "<input type=\"hidden\" name=\"slug\" value=\""  (trip slug)  "\">"
    "<button type=\"submit\">Delete post</button>"
    "</form>"
  ==
::
::  +render-not-found: 404 body.
::
++  render-not-found
  ^-  tape
  %+  page  "404"
  ;:  weld
    "<h1>404</h1><p>No such post. "
    "<a href=\"/\">Back to the index.</a></p>"
  ==
::
::  +render-new: the publish form.
::
++  render-new
  ^-  tape
  %+  page  "new post"
  ;:  weld
    "<h1>new post</h1>"
    "<form class=\"editor\" method=\"POST\" action=\"/publish\">"
    "<label>Title</label>"
    "<input type=\"text\" name=\"title\" placeholder=\"My first post\" required>"
    "<label>Body (plain text)</label>"
    "<textarea name=\"body\" placeholder=\"Write here...\" required></textarea>"
    "<button type=\"submit\">Publish</button>"
    "</form>"
  ==
::
::  +to-body: tape -> (unit octs) response body.
::
++  to-body
  |=  t=tape
  ^-  (unit octs)
  (to-octs (crip t))
--
::
=>
|%
++  moat  (keep server-state)
::
++  inner
  |_  state=server-state
  ::
  ::  +load: upgrade from previous state
  ::
  ++  load
    |=  arg=server-state
    ^-  server-state
    arg
  ::
  ::  +peek: external inspect
  ::
  ++  peek
    |=  =path
    ^-  (unit (unit *))
    ~>  %slog.[0 'Peeks awaiting implementation']
    ~
  ::
  ::  +poke: external apply
  ::
  ++  poke
    |=  =ovum:moat
    ^-  [(list effect) server-state]
    =/  sof-cau=(unit cause)  ((soft cause) cause.input.ovum)
    ?~  sof-cau
      ~&  "cause incorrectly formatted!"
      ~&  now.input.ovum
      !!
    ::  Parse request into components.
    =/  [id=@ uri=@t =method headers=(list header) body=(unit octs)]  +.u.sof-cau
    ::  Emit one greppable metric line carrying the live post count.
    ~>  %slog.[0 leaf+"metric: posts={<~(wyt by posts.state)>}"]
    ::  Helper closures for building responses (capture id).
    =/  html
      |=  [status=@ud t=tape]
      ^-  effect
      [%res id=id status ['content-type' 'text/html']~ (to-body t)]
    =/  redirect
      |=  loc=@t
      ^-  effect
      [%res id=id %303 ~['location'^loc 'content-length'^'0'] ~]
    ::
    ?+    method  [~[(html 405 "method not allowed")] state]
        %'GET'
      ?:  ?|(=('/' uri) =('/index.html' uri))
        [~[(html 200 (render-index state))] state]
      ?:  =('/new' uri)
        [~[(html 200 render-new)] state]
      =/  slug  (slug-from-uri uri)
      ?~  slug
        [~[(html 404 render-not-found)] state]
      =/  pst  (~(get by posts.state) u.slug)
      ?~  pst
        [~[(html 404 render-not-found)] state]
      :_  state
      :_  ~
      %+  html  200
      (page (trip title.u.pst) (render-post u.slug u.pst))
    ::
        %'POST'
      =/  form  (parse-form body)
      ?:  =('/publish' uri)
        =/  title=@t  (~(gut by form) 'title' '')
        =/  pbody=@t  (~(gut by form) 'body' '')
        ?:  =('' title)
          [~[(html 400 (page "error" "<h1>400</h1><p>Title required.</p>"))] state]
        =/  slug=@t  (slugify title)
        ::  New slug goes to the front of the index order; replacing an existing
        ::  slug keeps its position.
        =/  exists  (~(has by posts.state) slug)
        =/  new-order
          ?:(exists order.state [slug order.state])
        =/  new-posts  (~(put by posts.state) slug [title pbody])
        :_  state(posts new-posts, order new-order)
        :_  ~
        (redirect (crip (weld "/post/" (trip slug))))
      ::
      ?:  =('/unpublish' uri)
        =/  slug=@t  (~(gut by form) 'slug' '')
        =/  new-posts  (~(del by posts.state) slug)
        =/  new-order  (skip order.state |=(s=@t =(s slug)))
        :_  state(posts new-posts, order new-order)
        :_  ~
        (redirect '/')
      ::
      [~[(html 404 render-not-found)] state]
    ==
  --
--
((moat |) inner)
