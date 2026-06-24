/+  *http
/=  *  /common/wrapper
=>
|%
::  Board geometry: a bounded WIDTH x HEIGHT grid. Cells off-grid are always dead.
::
++  width   25
++  height  25
::  +$ board: the game state.
::
::    live is the set of [x y] coordinates that are currently alive. gen is the
::    generation counter (number of +step calls since the last reset).
::
+$  board
  $:  live=(set [x=@ud y=@ud])
      gen=@ud
  ==
+$  server-state  [%0 game=board]
::  +new-board: a fresh, empty board at generation 0.
::
++  new-board
  ^-  board
  [live=~ gen=0]
::  +in-bounds: is [x y] on the grid?
::
++  in-bounds
  |=  [x=@ud y=@ud]
  ^-  ?
  &((lth x width) (lth y height))
::  +neighbors: the up-to-8 in-bounds neighbors of [x y]. Cells off-grid are dead,
::  so we simply omit them (a missing neighbor contributes 0 to the live count).
::
++  neighbors
  |=  [x=@ud y=@ud]
  ^-  (list [@ud @ud])
  ::  the 8 offset pairs around a cell, as signed [dx dy]
  =/  offs=(list [@s @s])
    :~  [-1 -1]  [--0 -1]  [--1 -1]
        [-1 --0]           [--1 --0]
        [-1 --1]  [--0 --1]  [--1 --1]
    ==
  %+  murn  offs
  |=  [dx=@s dy=@s]
  ^-  (unit [@ud @ud])
  =/  nx=@s  (sum:si (sun:si x) dx)
  =/  ny=@s  (sum:si (sun:si y) dy)
  ?:  |(=(-1 (cmp:si nx --0)) =(-1 (cmp:si ny --0)))  ~   ::  off the left/top
  =/  ux=@ud  (abs:si nx)
  =/  uy=@ud  (abs:si ny)
  ?.  (in-bounds ux uy)  ~                                ::  off the right/bottom
  `[ux uy]
::  +live-neighbors: count of live neighbors of [x y] in set `s`.
::
++  live-neighbors
  |=  [s=(set [x=@ud y=@ud]) x=@ud y=@ud]
  ^-  @ud
  %+  roll  (neighbors x y)
  |=  [c=[@ud @ud] acc=@ud]
  ?:  (~(has in s) c)  +(acc)  acc
::  +step: compute the next generation as a pure function over the live set.
::
::    Conway's rules on a bounded grid:
::    - a live cell with 2 or 3 live neighbors survives;
::    - a dead cell with exactly 3 live neighbors becomes alive;
::    - every other cell is dead next generation.
::
::    We scan every cell on the grid once, counting its live neighbors, and keep
::    the cells that are alive next turn. O(w*h) per step.
::
++  step
  |=  s=(set [x=@ud y=@ud])
  ^-  (set [@ud @ud])
  =/  ys  (gulf 0 (dec height))
  =/  xs  (gulf 0 (dec width))
  %-  ~(gas in *(set [@ud @ud]))
  %-  zing
  %+  turn  ys
  |=  y=@ud
  ^-  (list [@ud @ud])
  %+  murn  xs
  |=  x=@ud
  ^-  (unit [@ud @ud])
  =/  n=@ud  (live-neighbors s x y)
  =/  alive  (~(has in s) [x y])
  ?:  alive
    ?:  |(=(2 n) =(3 n))  `[x y]  ~
  ?:  =(3 n)  `[x y]  ~
::  +random-board: seed a board from entropy `eny`, filling ~25% of cells.
::
::    NOTE: the `og` rng door is STUBBED to !! in this stdlib (it crashes when
::    called), so we roll our own PRNG from sha-256 (`shax`, which IS implemented):
::    hash [eny cell-index] -> reduce mod 4 -> alive iff result is 0 (~25% fill).
::
++  random-board
  |=  eny=@
  ^-  (set [@ud @ud])
  =/  ys  (gulf 0 (dec height))
  =/  xs  (gulf 0 (dec width))
  %-  ~(gas in *(set [@ud @ud]))
  %-  zing
  %+  turn  ys
  |=  y=@ud
  ^-  (list [@ud @ud])
  %+  murn  xs
  |=  x=@ud
  ^-  (unit [@ud @ud])
  =/  idx=@  (add (mul y width) x)
  =/  draw=@  (mod (shax (add (mul eny 1.000.003) idx)) 4)
  ?:  =(0 draw)  `[x y]  ~
::  +route-is: does request uri `t` name route `pfx`? `find` returns a (unit @ud)
::  (the index), NOT a bare @, so we must test the unit -- a route matches when its
::  literal appears at index 0 (the path always starts the uri the driver gives us).
::
++  route-is
  |=  [pfx=tape t=tape]
  ^-  ?
  =(`0 (find pfx t))
::  +parse-query: pull x= and y= from a query string / form body tape.
::
++  parse-query
  |=  t=tape
  ^-  [x=@ud y=@ud]
  :-  (grab-num "x=" t)
  (grab-num "y=" t)
::  +grab-num: find `key` in tape, parse the run of digits after it.
::
++  grab-num
  |=  [key=tape t=tape]
  ^-  @ud
  =/  idx  (find key t)
  ?~  idx  0
  =/  rest  (slag (add u.idx (lent key)) t)
  =/  digs  |-(?~(rest ~ ?:((gte i.rest '0') ?:((lte i.rest '9') [i.rest $(rest t.rest)] ~) ~)))
  ?~  digs  0
  (scan digs dem)
::  +cell-html: render one <td> for cell [x y]. Every cell is a tiny form button
::  POSTing /toggle so you can draw on the grid by clicking.
::
++  cell-html
  |=  [g=board x=@ud y=@ud]
  ^-  tape
  =/  is-live  (~(has in live.g) [x y])
  =/  xs  (scow %ud x)
  =/  ys  (scow %ud y)
  =/  klass  ?:(is-live "live" "dead")
  ;:  weld
    "<td class=\"{klass}\">"
    "<form method=\"POST\" action=\"/toggle?x={xs}&y={ys}\">"
    "<button type=\"submit\" class=\"cellbtn\"></button></form>"
    "</td>"
  ==
::  +render: full HTML page for the current board.
::
++  render
  |=  g=board
  ^-  (unit octs)
  =/  rows=tape
    %+  roll  (gulf 0 (dec height))
    |=  [y=@ud acc=tape]
    ^-  tape
    =/  row=tape
      %+  roll  (gulf 0 (dec width))
      |=  [x=@ud racc=tape]
      ^-  tape
      (weld racc (cell-html g x y))
    ;:  weld  acc  "<tr>"  row  "</tr>"  ==
  =/  doc=tape
    ;:  weld
      "<!doctype html><html><head><meta charset=\"utf-8\">"
      "<title>conway</title><style>"
      css
      "</style></head><body>"
      "<h1>NockApp Conway's Game of Life</h1>"
      "<p>{(scow %ud width)}x{(scow %ud height)} grid, all rules in the Hoon kernel.</p>"
      "<p class=\"status\">Generation: {(scow %ud gen.g)} &mdash; live cells: {(scow %ud ~(wyt in live.g))}</p>"
      "<div class=\"controls\">"
      "<form method=\"POST\" action=\"/step\"><button type=\"submit\">Step</button></form>"
      "<form method=\"POST\" action=\"/random\"><button type=\"submit\">Random</button></form>"
      "<form method=\"POST\" action=\"/clear\"><button type=\"submit\">Clear</button></form>"
      "</div>"
      "<table>"
      rows
      "</table>"
      "<p style=\"color:#666\">Click any cell to toggle it alive/dead, then press <b>Step</b> to advance one generation.</p>"
      "</body></html>"
    ==
  (to-octs (crip doc))
::  +css: the page stylesheet. A '''-block cord is LITERAL (no { } tape
::  interpolation), so CSS braces are safe here -- unlike "..." tapes.
::
++  css
  ^-  tape
  %-  trip
  '''
  body{font-family:monospace;background:#eee}
  table{table-layout:fixed;border-collapse:collapse;margin:1em 0}
  td{width:20px;height:20px;box-sizing:border-box;border:1px solid #ccc;padding:0}
  td.dead{background:#fff}
  td.live{background:#222}
  td form{display:block;margin:0;padding:0;width:100%;height:100%}
  .cellbtn{width:100%;height:100%;border:0;background:transparent;cursor:pointer;padding:0}
  h1{margin-bottom:0}.status{font-weight:bold;margin:0.5em 0}
  .controls{margin:0.5em 0}
  .controls form{display:inline-block;margin-right:0.5em}
  .controls button{font-size:16px;padding:0.3em 0.8em;cursor:pointer}
  '''
--
::
=>
|%
++  moat  (keep server-state)
::
++  inner
  |_  state=server-state
  ::
  ++  load
    |=  arg=server-state
    ^-  server-state
    arg
  ::
  ++  peek
    |=  =path
    ^-  (unit (unit *))
    ~
  ::
  ++  poke
    |=  =ovum:moat
    ^-  [(list effect) server-state]
    =/  sof-cau=(unit cause)  ((soft cause) cause.input.ovum)
    ?~  sof-cau
      ~&  "cause incorrectly formatted!"
      !!
    =/  [id=@ uri=@t =method headers=(list header) body=(unit octs)]  +.u.sof-cau
    =/  g=board  game.state
    =/  ok  |=(b=board ^-([(list effect) server-state] [~[[%res id %200 ['content-type' 'text/html']~ (render b)]] state(game b)]))
    ?+    method  [~[[%res id %400 ~ ~]] state]
        %'GET'
      ~>  %slog.[0 leaf+"metric: gen={<gen.g>}"]
      [~[[%res id %200 ['content-type' 'text/html']~ (render g)]] state]
    ::
        %'POST'
      =/  uri-tape  (trip uri)
      ::  /toggle?x=&y=: flip a single cell alive/dead
      ?:  (route-is "/toggle" uri-tape)
        =/  q=[x=@ud y=@ud]  (parse-query uri-tape)
        =/  c=[@ud @ud]  [x.q y.q]
        ?.  (in-bounds x.q y.q)
          ~>  %slog.[0 leaf+"metric: gen={<gen.g>}"]
          (ok g)
        =/  b=board
          ?:  (~(has in live.g) c)
            g(live (~(del in live.g) c))
          g(live (~(put in live.g) c))
        ~>  %slog.[0 leaf+"metric: gen={<gen.b>}"]
        (ok b)
      ::  /step: advance one generation
      ?:  (route-is "/step" uri-tape)
        =/  b=board  g(live (step live.g), gen +(gen.g))
        ~>  %slog.[0 leaf+"metric: gen={<gen.b>}"]
        (ok b)
      ::  /random: seed a ~25%-full board from the poke's entropy, gen 0
      ?:  (route-is "/random" uri-tape)
        =/  b=board  [live=(random-board eny.input.ovum) gen=0]
        ~>  %slog.[0 leaf+"metric: gen={<gen.b>}"]
        (ok b)
      ::  /clear: empty grid, gen 0
      ?:  (route-is "/clear" uri-tape)
        =/  b=board  new-board
        ~>  %slog.[0 leaf+"metric: gen={<gen.b>}"]
        (ok b)
      ::  unknown POST
      [~[[%res id %404 ~ ~]] state]
    ==
  --
--
((moat |) inner)
