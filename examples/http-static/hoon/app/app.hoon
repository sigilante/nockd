/+  *http
/=  *  /common/wrapper
=>
|%
::  $server-state: just a request tally for the metric; the SERVED CONTENT is static.
::
+$  server-state  [%0 requests=@]
::  +css: a little shared stylesheet, inlined into every page.
::
++  css
  ^-  tape
  %-  trip
  '''
  body{font-family:-apple-system,system-ui,sans-serif;max-width:42rem;
        margin:3rem auto;padding:0 1rem;line-height:1.5;color:#1a1a1a}
  h1{color:#5b21b6}
  code{background:#f3f0ff;padding:.1rem .3rem;border-radius:.2rem}
  nav a{margin-right:1rem;color:#5b21b6}
  footer{margin-top:2rem;color:#888;font-size:.85rem}
  '''
::  +home: the static landing page served at /.
::
++  home
  ^-  tape
  ;:  weld
    "<!doctype html><html><head><title>http-static</title><style>"
    css
    "</style></head><body>"
    "<h1>http-static</h1>"
    "<p>A NockApp that serves <strong>static content</strong> straight from the "
    "Hoon kernel &mdash; no mutable state, just a page.</p>"
    "<p>Every <code>GET /</code> returns this exact HTML.</p>"
    "<nav><a href=\"/\">home</a><a href=\"/about\">about</a></nav>"
    "<footer>served by the Hoon kernel via nockd</footer>"
    "</body></html>"
  ==
::  +about: a second static route served at /about.
::
++  about
  ^-  tape
  ;:  weld
    "<!doctype html><html><head><title>about &middot; http-static</title><style>"
    css
    "</style></head><body>"
    "<h1>About this NockApp</h1>"
    "<p>This is the simplest possible &ldquo;serve a page&rdquo; demo: a Hoon "
    "kernel that answers HTTP <code>GET</code> requests with fixed HTML.</p>"
    "<p>Unlike <code>http-counter</code>, there is no counter and no state to "
    "mutate &mdash; the response is the same every time. The kernel boots from "
    "<code>out.jam</code> and nockd keeps it running.</p>"
    "<nav><a href=\"/\">home</a><a href=\"/about\">about</a></nav>"
    "<footer>served by the Hoon kernel via nockd</footer>"
    "</body></html>"
  ==
::  +not-found: 404 body.
::
++  not-found
  ^-  tape
  ;:  weld
    "<!doctype html><html><head><title>404 &middot; http-static</title><style>"
    css
    "</style></head><body>"
    "<h1>404</h1><p>No such page. Try "
    "<a href=\"/\">/</a> or <a href=\"/about\">/about</a>.</p>"
    "</body></html>"
  ==
::  +page-for: pick the static body for a request path.
::
++  page-for
  |=  uri=@t
  ^-  [status=@ud body=tape]
  ?:  ?|(=('/' uri) =('/index.html' uri))
    [200 home]
  ?:  =('/about' uri)
    [200 about]
  [404 not-found]
::  +render: tape -> (unit octs) response body.
::
++  render
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
    ::  Tally this request and emit a greppable metric line.
    =/  new-requests=@  +(requests.state)
    ~>  %slog.[0 leaf+"metric: requests={<new-requests>}"]
    ?+    method  [~[[%res id=id %405 ~ ~]] state(requests new-requests)]
        %'GET'
      =/  res=[status=@ud body=tape]  (page-for uri)
      :_  state(requests new-requests)
      :_  ~
      ^-  effect
      [%res id=id status.res ['content-type' 'text/html']~ (render body.res)]
    ==
  --
--
((moat |) inner)
