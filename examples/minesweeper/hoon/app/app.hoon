/+  *http
/=  *  /common/wrapper
=>
|%
::  Board geometry: 9x9 grid with 10 mines (classic "beginner").
::
++  width   9
++  height  9
++  mines   10
::  +$ status: the game phase.
::
+$  status  ?(%playing %won %lost)
::  +$ board: the game state.
::
::    mine/reveal/flag are sets of [x y] coordinates. moves counts every
::    mutating action (reveal or flag toggle) for the metric.
::
+$  board
  $:  mine=(set [x=@ud y=@ud])
      shown=(set [x=@ud y=@ud])
      flag=(set [x=@ud y=@ud])
      =status
      moves=@ud
  ==
+$  server-state  [%0 game=board]
::  +cells: every [x y] coordinate on the board, row-major.
::
++  cells
  ^-  (list [@ud @ud])
  =/  ys  (gulf 0 (dec height))
  =/  xs  (gulf 0 (dec width))
  %-  zing
  %+  turn  ys
  |=  y=@ud
  %+  turn  xs
  |=  x=@ud
  [x y]
::  +neighbors: the up-to-8 in-bounds neighbors of [x y].
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
  ?:  |((gte ux width) (gte uy height))  ~                ::  off the right/bottom
  `[ux uy]
::  +adjacent: count of mines adjacent to [x y].
::
++  adjacent
  |=  [g=board x=@ud y=@ud]
  ^-  @ud
  %+  roll  (neighbors x y)
  |=  [c=[@ud @ud] acc=@ud]
  ?:  (~(has in mine.g) c)  +(acc)  acc
::  +place-mines: seed `count` distinct mines from entropy `eny`, never placing
::  one on the player's first-revealed cell `safe` (classic safe first click).
::
::    NOTE: the `og` rng door (`rad`/`rads`/`raw`) is STUBBED to !! in this stdlib
::    (it crashes when called), so we roll our own PRNG from sha-256 (`shax`, which
::    IS implemented): hash [eny counter] -> reduce mod (w*h) -> [x y]. Bumping the
::    counter on every draw (hit-or-miss) gives a fresh, well-distributed value.
::
++  place-mines
  |=  [eny=@ count=@ud safe=[x=@ud y=@ud]]
  ^-  (set [@ud @ud])
  =/  area  (mul width height)
  =|  out=(set [@ud @ud])
  =/  ctr=@  0
  |-  ^-  (set [@ud @ud])
  ?:  =(count ~(wyt in out))  out
  =/  draw=@  (mod (shax (add (mul eny 1.000.003) ctr)) area)
  =/  c=[@ud @ud]  [(mod draw width) (div draw width)]
  ?:  |(=(c safe) (~(has in out) c))
    $(ctr +(ctr))                          ::  collision: redraw with next counter
  $(out (~(put in out) c), ctr +(ctr))
::  +flood: reveal [x y]; if it has 0 adjacent mines, flood to neighbors.
::  Returns the updated shown set. Never reveals flagged cells.
::
++  flood
  |=  [g=board x=@ud y=@ud]
  ^-  (set [@ud @ud])
  =/  shown  shown.g
  =/  todo=(list [@ud @ud])  ~[[x y]]
  |-  ^-  (set [@ud @ud])
  ?~  todo  shown
  =/  c=[@ud @ud]  i.todo
  ?:  (~(has in shown) c)  $(todo t.todo)
  ?:  (~(has in flag.g) c)  $(todo t.todo)
  =.  shown  (~(put in shown) c)
  ?:  !=(0 (adjacent g(shown shown) -.c +.c))
    $(todo t.todo)
  ::  zero adjacent: enqueue all neighbors
  $(todo (weld (neighbors -.c +.c) t.todo))
::  +safe-cells: number of non-mine cells (the win condition target).
::
++  safe-cells
  ^~((sub (mul width height) mines))
::  +route-is: does request uri `t` name route `pfx`? `find` returns a (unit @ud)
::  (the index), NOT a bare @, so we must test the unit -- a route matches when its
::  literal appears at index 0 (the path always starts the uri the driver gives us).
::
++  route-is
  |=  [pfx=tape t=tape]
  ^-  ?
  =(`0 (find pfx t))
::  +parse-query: pull x= and y= from a query string / form body tape.
::  Accepts "x=3&y=5" style. Returns [x y] as @ud (0 on miss).
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
::  +cell-html: render one <td> for cell [x y].
::
++  cell-html
  |=  [g=board x=@ud y=@ud]
  ^-  tape
  =/  c=[@ud @ud]  [x y]
  =/  is-shown  (~(has in shown.g) c)
  =/  is-flag   (~(has in flag.g) c)
  =/  is-mine   (~(has in mine.g) c)
  =/  over  ?=(?(%won %lost) status.g)
  =/  xs  (scow %ud x)
  =/  ys  (scow %ud y)
  ?:  &(over is-mine)
    ::  game over: expose all mines
    "<td class=\"mine\">*</td>"
  ?:  is-shown
    =/  n  (adjacent g x y)
    ?:  =(0 n)
      "<td class=\"open\"></td>"
    (weld (weld "<td class=\"open n{(scow %ud n)}\">" (scow %ud n)) "</td>")
  ?:  &(is-flag !over)
    ::  flagged & still playing: show a flag button that un-flags on click
    ;:  weld
      "<td class=\"flag\"><form method=\"POST\" action=\"/flag?x={xs}&y={ys}\">"
      "<button type=\"submit\" class=\"flagbtn\">F</button></form></td>"
    ==
  ?:  is-flag
    "<td class=\"flag\">F</td>"
  ?:  over
    "<td class=\"hidden\"></td>"
  ::  hidden & playing: reveal button (left); flag toggle via separate tiny form
  ;:  weld
    "<td class=\"hidden\">"
    "<form method=\"POST\" action=\"/reveal?x={xs}&y={ys}\" style=\"display:inline\">"
    "<button type=\"submit\" class=\"cellbtn\">?</button></form>"
    "<form method=\"POST\" action=\"/flag?x={xs}&y={ys}\" style=\"display:inline\">"
    "<button type=\"submit\" class=\"flagsm\">f</button></form>"
    "</td>"
  ==
::  +status-line: human-readable status text.
::
++  status-line
  |=  g=board
  ^-  tape
  =/  remaining  (sub mines (min mines ~(wyt in flag.g)))
  ?-  status.g
    %playing  "Playing - mines left: {(scow %ud remaining)}, moves: {(scow %ud moves.g)}"
    %won      "YOU WON! moves: {(scow %ud moves.g)}"
    %lost     "BOOM! You hit a mine. moves: {(scow %ud moves.g)}"
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
      "<title>minesweeper</title><style>"
      css
      "</style></head><body>"
      "<h1>NockApp Minesweeper</h1>"
      "<p>{width-by-height-line}, all game logic in the Hoon kernel.</p>"
      "<p class=\"status\">{(status-line g)}</p>"
      "<table>"
      rows
      "</table>"
      "<form method=\"POST\" action=\"/new\"><button type=\"submit\">New game</button></form>"
      "<p style=\"color:#666\">Each cell has two buttons: <b>?</b> reveals, <b>f</b> toggles a flag.</p>"
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
  table{border-collapse:collapse;margin:1em 0}
  td{width:28px;height:28px;text-align:center;border:1px solid #999;padding:0}
  td.hidden,td.flag{background:#bbb}
  td.open{background:#ddd}
  td.mine{background:#f55;font-weight:bold}
  .cellbtn,.flagsm,.flagbtn{width:100%;height:28px;border:0;background:transparent;cursor:pointer;font-family:monospace}
  .flagsm{font-size:9px;color:#900}
  h1{margin-bottom:0}.status{font-weight:bold;margin:0.5em 0}
  '''
::  +width-by-height-line: descriptive header text.
::
++  width-by-height-line
  ^-  tape
  "{(scow %ud width)}x{(scow %ud height)} board, {(scow %ud mines)} mines"
::  +new-board: a fresh, unseeded board (mines placed on first reveal).
::
++  new-board
  ^-  board
  [mine=~ shown=~ flag=~ status=%playing moves=0]
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
    ::  Normalize the board. The kernel boots with a BUNT board whose `status` is not
    ::  necessarily %playing (the bunt of a $? union is implementation-defined). A board
    ::  with no revealed cells cannot have been won or lost, so force %playing -- this both
    ::  fixes the boot default and is semantically correct.
    =/  g=board
      =/  raw=board  game.state
      ?:  =(0 ~(wyt in shown.raw))  raw(status %playing)
      raw
    =/  ok  |=(b=board ^-([(list effect) server-state] [~[[%res id %200 ['content-type' 'text/html']~ (render b)]] state(game b)]))
    ?+    method  [~[[%res id %400 ~ ~]] state]
        %'GET'
      ~>  %slog.[0 leaf+"metric: moves={<moves.g>}"]
      [~[[%res id %200 ['content-type' 'text/html']~ (render g)]] state]
    ::
        %'POST'
      =/  uri-tape  (trip uri)
      =/  q=[x=@ud y=@ud]  (parse-query uri-tape)
      ::  /new: fresh board
      ?:  (route-is "/new" uri-tape)
        =/  b  new-board
        ~>  %slog.[0 leaf+"metric: moves={<moves.b>}"]
        (ok b)
      ::  ignore mutations once the game is over (except /new, handled above)
      ?.  ?=(%playing status.g)
        ~>  %slog.[0 leaf+"metric: moves={<moves.g>}"]
        (ok g)
      ::  /flag: toggle a flag (only on hidden cells)
      ?:  (route-is "/flag" uri-tape)
        =/  c=[@ud @ud]  [x.q y.q]
        ?:  (~(has in shown.g) c)
          (ok g)
        =/  b=board
          ?:  (~(has in flag.g) c)
            g(flag (~(del in flag.g) c), moves +(moves.g))
          g(flag (~(put in flag.g) c), moves +(moves.g))
        ~>  %slog.[0 leaf+"metric: moves={<moves.b>}"]
        (ok b)
      ::  /reveal: reveal a cell
      ?:  (route-is "/reveal" uri-tape)
        =/  c=[@ud @ud]  [x.q y.q]
        ::  no-op if already shown or flagged
        ?:  |((~(has in shown.g) c) (~(has in flag.g) c))
          (ok g)
        ::  lazily place mines on the FIRST reveal (safe first click)
        =/  g2=board
          ?:  =(0 ~(wyt in mine.g))
            g(mine (place-mines eny.input.ovum mines c))
          g
        ::  did we hit a mine?
        ?:  (~(has in mine.g2) c)
          =/  b  g2(status %lost, shown (~(put in shown.g2) c), moves +(moves.g2))
          ~>  %slog.[0 leaf+"metric: moves={<moves.b>}"]
          (ok b)
        ::  safe: flood-reveal, then check for a win
        =/  new-shown  (flood g2 x.q y.q)
        =/  b1  g2(shown new-shown, moves +(moves.g2))
        =/  b
          ?:  =(safe-cells ~(wyt in shown.b1))
            b1(status %won)
          b1
        ~>  %slog.[0 leaf+"metric: moves={<moves.b>}"]
        (ok b)
      ::  unknown POST
      [~[[%res id %404 ~ ~]] state]
    ==
  --
--
((moat |) inner)
