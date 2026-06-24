/+  *http
/=  *  /common/wrapper
=>
|%
+$  server-state  [%0 value=@]
++  page
  ^-  tape
  %-  trip
  '''
  <!doctype html>
  <html>
    <head><title>http-counter</title></head>
    <body>
      <h1>http-counter</h1>
      <p>A NockApp whose counter PERSISTS across restarts (kernel-state checkpointing).</p>
      <div class="counter-display">
        Count: COUNT
      </div>

      <form method="POST" action="/increment" style="display: inline;">
        <button type="submit" class="increment-button">Increment Counter</button>
      </form>

      <form method="POST" action="/reset" style="display: inline;">
        <button type="submit" class="reset-button">Reset Counter</button>
      </form>
    </body>
  </html>
  '''
::  +render: render the HTML page with the given count spliced in for COUNT
::
++  render
  |=  v=@
  ^-  (unit octs)
  %-  to-octs
  %-  crip
  ^-  tape
  =/  index  (find "COUNT" page)
  ;:  weld
    (scag (need index) page)
    (scow %ud v)
    (slag (add (need index) ^~((lent "COUNT"))) page)
  ==
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
    ::
    ?+    method  [~[[%res ~ %400 ~ ~]] state]
        %'GET'
      ::  emit one greppable metric line carrying the current count, then respond
      ~>  %slog.[0 leaf+"metric: count={<value.state>}"]
      :_  state
      :_  ~
      ^-  effect
      [%res id=id %200 ['content-type' 'text/html']~ (render value.state)]
      ::
        %'POST'
      ?:  =('/increment' uri)
        =/  new-value=@  +(value.state)
        ~>  %slog.[0 leaf+"metric: count={<new-value>}"]
        :_  state(value new-value)
        :_  ~
        ^-  effect
        [%res id=id %200 ['content-type' 'text/html']~ (render new-value)]
      ::
      ?>  =('/reset' uri)
      ~>  %slog.[0 leaf+"metric: count=0"]
      :_  state(value 0)
      :_  ~
      ^-  effect
      [%res id=id %200 ['content-type' 'text/html']~ (render 0)]
    ==
  --
--
((moat |) inner)
