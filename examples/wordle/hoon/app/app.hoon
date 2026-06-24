/+  *http
/=  *  /common/wrapper
=>
|%
::  +$ status: the game phase.
::
+$  status  ?(%playing %won %lost)
::  +$ mark: per-letter feedback for one tile of a guess.
::    %hit   = right letter, right position (green)
::    %near  = letter present elsewhere in the target (yellow), multiplicity-aware
::    %miss  = letter absent (grey)
::
+$  mark  ?(%hit %near %miss)
::  +$ scored: a single submitted guess plus its per-letter feedback.
::    word is exactly 5 lowercase ASCII letters; marks is 5 marks, aligned.
::
+$  scored  [word=tape marks=(list mark)]
::  +$ game: the full game state.
::
::    target  = the 5-letter answer (tape, lowercase)
::    guesses = scored guesses so far, in submission order (most recent last)
::    =status = the game phase
::    total   = cumulative count of accepted guesses across this session (the metric)
::
+$  game
  $:  target=tape
      guesses=(list scored)
      =status
      total=@ud
  ==
+$  server-state  [%0 game=game]
::  +max-rows: classic Wordle gives you 6 guesses.
::
++  max-rows  6
::  +word-len: 5-letter words.
::
++  word-len  5
::  +answers: the curated answer pool (common 5-letter words). The target is drawn from
::  this list. ~400 entries; all lowercase, exactly 5 letters.
::
++  answers
  ^-  (list tape)
  :~  "apple"  "beach"  "brain"  "bread"  "brush"  "chair"  "chest"  "chord"
      "click"  "clock"  "cloud"  "crane"  "crash"  "crown"  "dance"  "diary"
      "drink"  "earth"  "field"  "flame"  "flock"  "flour"  "fruit"  "ghost"
      "glass"  "grape"  "grass"  "green"  "heart"  "honey"  "horse"  "house"
      "juice"  "knife"  "lemon"  "light"  "money"  "mouse"  "music"  "night"
      "ocean"  "olive"  "paint"  "panel"  "paper"  "party"  "peach"  "pearl"
      "phone"  "piano"  "pizza"  "plant"  "plate"  "pound"  "power"  "queen"
      "quiet"  "radio"  "river"  "robot"  "round"  "salad"  "sauce"  "scale"
      "shape"  "share"  "sheep"  "shell"  "shine"  "shirt"  "shoes"  "smile"
      "smoke"  "snake"  "space"  "spice"  "spoon"  "stage"  "stair"  "stand"
      "stark"  "steam"  "stone"  "storm"  "story"  "sugar"  "sweet"  "table"
      "teeth"  "tiger"  "toast"  "tooth"  "torch"  "tower"  "train"  "trees"
      "truck"  "trust"  "uncle"  "vapor"  "video"  "voice"  "watch"  "water"
      "wheel"  "white"  "woman"  "world"  "youth"  "zebra"  "actor"  "adobe"
      "aglow"  "alarm"  "album"  "alert"  "alike"  "alive"  "alley"  "alpha"
      "amber"  "angel"  "anger"  "angle"  "ankle"  "apron"  "arbor"  "arena"
      "armor"  "arrow"  "asset"  "audio"  "avoid"  "award"  "aware"  "badge"
      "baker"  "basic"  "basin"  "begin"  "berry"  "blade"  "blank"  "blaze"
      "blend"  "bloom"  "board"  "boost"  "booth"  "brave"  "brick"  "bride"
      "brief"  "bring"  "broad"  "brook"  "build"  "bunch"  "cabin"  "cable"
      "candy"  "cargo"  "carve"  "catch"  "cause"  "cedar"  "chalk"  "charm"
      "cheek"  "cheer"  "chief"  "chili"  "civic"  "claim"  "clamp"  "clean"
      "clear"  "cliff"  "climb"  "close"  "coast"  "color"  "couch"  "could"
      "count"  "court"  "cover"  "crack"  "craft"  "cream"  "creek"  "crisp"
      "curve"  "daily"  "dairy"  "delta"  "depth"  "dingo"  "dizzy"  "dodge"
      "donut"  "dough"  "dozen"  "draft"  "drama"  "dream"  "dress"  "drive"
      "eagle"  "early"  "elbow"  "elder"  "email"  "ember"  "enjoy"  "entry"
      "equal"  "essay"  "event"  "exact"  "extra"  "fable"  "faith"  "fancy"
      "fault"  "feast"  "ferry"  "fever"  "fiber"  "final"  "first"  "flash"
      "fleet"  "float"  "flute"  "focus"  "force"  "forge"  "found"  "frame"
      "fresh"  "front"  "frost"  "funny"  "gauge"  "giant"  "glide"  "globe"
      "glory"  "glove"  "grace"  "grade"  "grand"  "grant"  "great"  "grill"
      "groom"  "group"  "guard"  "guess"  "guide"  "habit"  "happy"  "harsh"
      "haven"  "hello"  "hobby"  "honor"  "hotel"  "hound"  "humor"  "ideal"
      "image"  "index"  "inner"  "input"  "ivory"  "jelly"  "jewel"  "joint"
      "jolly"  "judge"  "knock"  "label"  "labor"  "ladle"  "lance"  "large"
      "laser"  "later"  "latch"  "layer"  "learn"  "lease"  "ledge"  "lemur"
      "level"  "lever"  "limit"  "linen"  "lodge"  "logic"  "loyal"  "lucky"
      "lunch"  "magic"  "major"  "maple"  "march"  "marsh"  "match"  "medal"
      "melon"  "mercy"  "metal"  "metro"  "midst"  "miner"  "minor"  "model"
      "moist"  "month"  "moral"  "motor"  "mound"  "mount"  "mouth"  "mover"
      "movie"  "mural"  "nerve"  "newer"  "noble"  "north"  "novel"  "nurse"
      "nylon"  "oasis"  "offer"  "often"  "onion"  "opera"  "orbit"  "order"
      "organ"  "otter"  "ought"  "outer"  "owner"  "patch"  "pause"  "peace"
      "perch"  "petal"  "photo"  "pilot"  "pinch"  "pitch"  "pixel"  "place"
      "plain"  "plead"  "plumb"  "point"  "polar"  "pride"  "prime"  "prize"
      "proof"  "proud"  "pulse"  "punch"  "pupil"  "purse"  "quart"  "quest"
      "quick"  "quilt"  "quote"  "raise"  "rally"  "ranch"  "range"  "rapid"
      "ratio"  "raven"  "reach"  "ready"  "realm"  "rebel"  "relax"  "relay"
      "reply"  "ridge"  "rinse"  "rival"  "roast"  "robin"  "rocky"  "rouge"
      "royal"  "ruler"  "rumor"  "rural"  "sandy"  "scarf"  "scene"  "scent"
      "scope"  "score"  "scout"  "scrub"  "serve"  "seven"  "shade"  "shaft"
      "shark"  "sharp"  "shawl"  "shelf"  "shore"  "short"  "shout"  "shown"
      "sight"  "silly"  "siren"  "skate"  "skill"  "skirt"  "slate"  "sleep"
      "slice"  "slide"  "slope"  "small"  "smart"  "smith"  "solar"  "solid"
      "sound"  "south"  "spare"  "spark"  "speak"  "speed"  "spell"  "spend"
      "spike"  "spine"  "spire"  "split"  "spray"  "spree"  "squad"  "stack"
      "staff"  "stamp"  "start"  "state"  "steal"  "steel"  "steep"  "stern"
      "stick"  "stiff"  "still"  "sting"  "stock"  "stool"  "stoop"  "store"
      "stout"  "strap"  "straw"  "strip"  "stuck"  "study"  "stuff"  "stump"
      "sunny"  "super"  "surge"  "swamp"  "swarm"  "swear"  "sweat"  "sweep"
      "swept"  "swift"  "swing"  "sword"  "syrup"  "tally"  "tango"  "taper"
      "taupe"  "teach"  "tease"  "tempo"  "tenor"  "tense"  "thank"  "theme"
      "thick"  "thief"  "thing"  "thorn"  "those"  "three"  "throw"  "thumb"
      "tidal"  "tight"  "title"  "today"  "token"  "tonic"  "topic"  "total"
      "touch"  "tough"  "trace"  "track"  "trade"  "trail"  "treat"  "trend"
      "trial"  "tribe"  "trick"  "trout"  "tulip"  "tunic"  "turbo"  "tutor"
      "twice"  "twist"  "ultra"  "unite"  "unity"  "until"  "upper"  "urban"
      "usage"  "usher"  "usual"  "valet"  "valid"  "value"  "vault"  "venue"
      "verge"  "verse"  "vigor"  "villa"  "vinyl"  "viola"  "viper"  "vista"
      "vital"  "vivid"  "vocal"  "vodka"  "vowel"  "wagon"  "waist"  "waltz"
      "waste"  "weary"  "weave"  "wedge"  "weigh"  "weird"  "whale"  "wharf"
      "wheat"  "whisk"  "whole"  "widen"  "widow"  "width"  "windy"  "wiser"
      "witch"  "woven"  "wrist"  "wrong"  "yacht"  "yearn"  "yeast"  "yield"
      "young"  "zesty"  "zonal"
  ==
::  +list-len: count of the answer pool, computed once.
::
++  list-len  ^~((lent answers))
::  +pick-target: choose a target from the answer pool, seeded by entropy `eny`.
::
::    The `og` rng door is STUBBED to !! in this stdlib, so we derive a draw from
::    sha-256 (`shax`, which IS implemented) reduced modulo the pool size, and pull
::    that index out of the list.
::
++  pick-target
  |=  eny=@
  ^-  tape
  =/  idx  (mod (shax eny) list-len)
  (snag idx answers)
::  +score: the heart of Wordle. Compute per-letter feedback for `guess` against
::  `target`, both exactly `word-len` lowercase letters.
::
::    Real Wordle is a TWO-PASS algorithm so letter multiplicity is honored:
::
::    Pass 1 (greens): walk the two words in lockstep. Where guess[i] == target[i],
::    flag a %hit and DO NOT add that target letter to the pool of "available" letters
::    -- a green consumes its target letter.
::
::    Pass 2 (yellows/greys): walk the guess again. For each non-green letter, if a
::    copy of it still remains in the available pool, emit %near and remove ONE copy
::    from the pool; otherwise emit %miss. Removing on use is what stops a guess of
::    "lulls" against target "level" from lighting up more L's than the target holds.
::
++  score
  |=  [guess=tape target=tape]
  ^-  (list mark)
  ::  pass 1: collect greens (aligned to guess) and the pool of unmatched target letters.
  =/  pg
    =/  gl  guess
    =/  tl  target
    =|  pool=(list @t)
    =|  greens=(list ?)
    |-  ^-  [pool=(list @t) greens=(list ?)]
    ?~  gl  [(flop pool) (flop greens)]
    ?~  tl  [(flop pool) (flop greens)]
    ?:  =(i.gl i.tl)
      ::  green: record the hit, do NOT add this target letter to the pool
      $(gl t.gl, tl t.tl, greens [& greens])
    ::  not a green here: target letter is still available for a later yellow
    $(gl t.gl, tl t.tl, greens [| greens], pool [i.tl pool])
  ::  pass 2: emit marks, consuming one pool copy per yellow.
  =/  g  guess
  =/  gr  greens.pg
  =/  bag  pool.pg
  |-  ^-  (list mark)
  ?~  g  ~
  ?~  gr  ~
  ?:  i.gr
    [%hit $(g t.g, gr t.gr)]
  ::  non-green: is a copy of this letter still in the bag?
  ?:  (lien bag |=(c=@t =(c i.g)))
    [%near $(g t.g, gr t.gr, bag (del-one bag i.g))]
  [%miss $(g t.g, gr t.gr)]
::  +del-one: remove the FIRST occurrence of `c` from list `l`.
::
++  del-one
  |=  [l=(list @t) c=@t]
  ^-  (list @t)
  ?~  l  ~
  ?:  =(i.l c)  t.l
  [i.l $(l t.l)]
::  +all-hit: did every tile come back green?
::
++  all-hit
  |=  marks=(list mark)
  ^-  ?
  (levy marks |=(m=mark =(%hit m)))
::  +valid-guess: is `t` exactly word-len lowercase ASCII letters (a-z)?
::
++  valid-guess
  |=  t=tape
  ^-  ?
  ?.  =(word-len (lent t))  |
  (levy t |=(c=@t &((gte c 'a') (lte c 'z'))))
::  +route-is: does request uri `t` name route `pfx`? `find` returns a (unit @ud)
::  (the index of the match), so a route matches when its literal sits at index 0.
::
++  route-is
  |=  [pfx=tape t=tape]
  ^-  ?
  =(`0 (find pfx t))
::  +grab-word: pull the value of `w=` from a query string / form body tape, taking
::  the run of ASCII letters after it and lowercasing them. Returns "" on miss.
::
++  grab-word
  |=  t=tape
  ^-  tape
  =/  idx  (find "w=" t)
  ?~  idx  ""
  =/  rest  (slag (add u.idx 2) t)
  ::  take letters (A-Z or a-z) until a non-letter (e.g. '&' or end), lowercasing.
  =|  out=tape
  |-  ^-  tape
  ?~  rest  (flop out)
  =/  c  i.rest
  ?:  &((gte c 'A') (lte c 'Z'))
    $(rest t.rest, out [(add c 32) out])    ::  upper -> lower
  ?:  &((gte c 'a') (lte c 'z'))
    $(rest t.rest, out [c out])
  (flop out)
::  +upper: ASCII-uppercase a tape (for display).
::
++  upper
  |=  t=tape
  ^-  tape
  %+  turn  t
  |=  c=@t
  ?:  &((gte c 'a') (lte c 'z'))  (sub c 32)
  c
::  +tile-html: render one feedback tile for letter `c` with mark `m`.
::
++  tile-html
  |=  [c=@t m=mark]
  ^-  tape
  =/  cls
    ?-  m
      %hit   "tile hit"
      %near  "tile near"
      %miss  "tile miss"
    ==
  =/  up  ?:(&((gte c 'a') (lte c 'z')) (sub c 32) c)
  "<div class=\"{cls}\">{(trip up)}</div>"
::  +row-html: render one scored guess as a row of 5 feedback tiles.
::
++  row-html
  |=  s=scored
  ^-  tape
  =/  cells=tape
    =/  w  word.s
    =/  ms  marks.s
    |-  ^-  tape
    ?~  w  ""
    ?~  ms  ""
    (weld (tile-html i.w i.ms) $(w t.w, ms t.ms))
  ;:(weld "<div class=\"row\">" cells "</div>")
::  +empty-row-html: an empty (unfilled) grid row of 5 blank tiles.
::
++  empty-row-html
  ^-  tape
  =/  cell  "<div class=\"tile empty\"></div>"
  =/  cells  ;:(weld cell cell cell cell cell)
  ;:(weld "<div class=\"row\">" cells "</div>")
::  +status-line: human-readable status text.
::
++  status-line
  |=  g=game
  ^-  tape
  =/  used  (lent guesses.g)
  ?-  status.g
    %playing  "Guess the word - attempt {(scow %ud +(used))} of {(scow %ud max-rows)}"
    %won      "YOU WON in {(scow %ud used)}! Press New game to play again."
    %lost     "Out of guesses. The word was {(upper target.g)}."
  ==
::  +render: full HTML page for the current game.
::
++  render
  |=  g=game
  ^-  (unit octs)
  =/  used  (lent guesses.g)
  ::  rows: the scored guesses, then blank rows padding out to max-rows.
  =/  filled=tape
    %+  roll  guesses.g
    |=  [s=scored acc=tape]
    (weld acc (row-html s))
  =/  blanks=@ud  (sub max-rows (min max-rows used))
  =/  empties=tape
    =/  n  blanks
    |-  ^-  tape
    ?:  =(0 n)  ""
    (weld empty-row-html $(n (dec n)))
  =/  over  ?=(?(%won %lost) status.g)
  =/  form=tape
    ?:  over  ""
    ;:  weld
      "<form method=\"POST\" action=\"/guess\" class=\"guessform\">"
      "<input name=\"w\" maxlength=\"5\" minlength=\"5\" pattern=\"[A-Za-z][A-Za-z][A-Za-z][A-Za-z][A-Za-z]\" "
      "autocomplete=\"off\" autofocus placeholder=\"guess\" required>"
      "<button type=\"submit\">Guess</button></form>"
    ==
  =/  doc=tape
    ;:  weld
      "<!doctype html><html><head><meta charset=\"utf-8\">"
      "<title>wordle</title><style>"
      css
      "</style></head><body>"
      "<h1>NockApp Wordle</h1>"
      "<p>Guess the 5-letter word in 6 tries. All game logic runs in the Hoon kernel.</p>"
      "<p class=\"status\">{(status-line g)}</p>"
      "<div class=\"grid\">"
      filled
      empties
      "</div>"
      form
      "<form method=\"POST\" action=\"/new\" class=\"newform\"><button type=\"submit\">New game</button></form>"
      "<p class=\"legend\">"
      "<span class=\"tile hit\">A</span> right spot &nbsp; "
      "<span class=\"tile near\">B</span> wrong spot &nbsp; "
      "<span class=\"tile miss\">C</span> not in word"
      "</p>"
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
  body{font-family:system-ui,monospace;background:#121213;color:#fff;text-align:center;padding:1em}
  h1{margin-bottom:0}
  p{color:#d7dadc}
  .status{font-weight:bold;margin:0.6em 0}
  .grid{display:inline-flex;flex-direction:column;gap:6px;margin:1em 0}
  .row{display:flex;gap:6px;justify-content:center}
  .tile,.empty{width:54px;height:54px;line-height:54px;font-size:28px;font-weight:bold;text-transform:uppercase;border:2px solid #3a3a3c;box-sizing:border-box}
  .tile.empty{background:#121213}
  .tile.hit{background:#538d4e;border-color:#538d4e}
  .tile.near{background:#b59f3b;border-color:#b59f3b}
  .tile.miss{background:#3a3a3c;border-color:#3a3a3c}
  .guessform{margin:1em 0}
  .guessform input{font-size:24px;text-transform:uppercase;padding:6px 10px;letter-spacing:6px;width:7em;text-align:center;border:2px solid #565758;background:#121213;color:#fff;border-radius:4px}
  button{font-size:18px;padding:8px 16px;margin:4px;cursor:pointer;border:0;border-radius:4px;background:#538d4e;color:#fff}
  .newform button{background:#565758}
  .legend{margin-top:1.5em;color:#888;font-size:14px}
  .legend .tile{display:inline-block;width:24px;height:24px;line-height:24px;font-size:14px;border-width:1px}
  '''
::  +new-game: a fresh game with a freshly-seeded target.
::
++  new-game
  |=  eny=@
  ^-  game
  [target=(pick-target eny) guesses=~ status=%playing total=0]
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
    ::  Normalize the game. The kernel boots with a BUNT game whose target is empty and
    ::  whose status is the bunt of a $? union (implementation-defined). Seed a real game
    ::  on first contact (empty target) and force %playing in that case.
    =/  g=game
      =/  raw=game  game.state
      ?:  =(0 (lent target.raw))
        (new-game eny.input.ovum)
      raw
    =/  ok
      |=  b=game
      ^-  [(list effect) server-state]
      [~[[%res id %200 ['content-type' 'text/html']~ (render b)]] state(game b)]
    ?+    method  [~[[%res id %400 ~ ~]] state]
        %'GET'
      ~>  %slog.[0 leaf+"metric: guesses={<total.g>}"]
      [~[[%res id %200 ['content-type' 'text/html']~ (render g)]] state(game g)]
    ::
        %'POST'
      =/  uri-tape  (trip uri)
      ::  /new: fresh target, clear guesses
      ?:  (route-is "/new" uri-tape)
        =/  b  (new-game eny.input.ovum)
        ~>  %slog.[0 leaf+"metric: guesses={<total.g>}"]
        (ok b)
      ::  /guess: submit a guess
      ?:  (route-is "/guess" uri-tape)
        ::  ignore guesses once the game is over
        ?.  ?=(%playing status.g)
          ~>  %slog.[0 leaf+"metric: guesses={<total.g>}"]
          (ok g)
        ::  the guess can arrive in the query string OR the POST body; check both.
        =/  body-tape  ?~(body "" (trip q.u.body))
        =/  w  (grab-word :(weld uri-tape "&" body-tape))
        ::  reject malformed input gracefully: re-render unchanged.
        ?.  (valid-guess w)
          ~>  %slog.[0 leaf+"metric: guesses={<total.g>}"]
          (ok g)
        =/  marks  (score w target.g)
        =/  s=scored  [w marks]
        =/  g2=game  g(guesses (snoc guesses.g s), total +(total.g))
        =/  b=game
          ?:  (all-hit marks)
            g2(status %won)
          ?:  =(max-rows (lent guesses.g2))
            g2(status %lost)
          g2
        ~>  %slog.[0 leaf+"metric: guesses={<total.b>}"]
        (ok b)
      ::  unknown POST
      [~[[%res id %404 ~ ~]] state]
    ==
  --
--
((moat |) inner)
