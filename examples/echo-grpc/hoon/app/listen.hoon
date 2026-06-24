::  echo-grpc / listen — the SERVER kernel for the private-gRPC echo demo.
::
::  This kernel is the simplest possible illustration of the private NockApp gRPC surface:
::
::    * a gRPC POKE carries a cause `[%echo val=@t]`; we store `val` in kernel state.
::    * a gRPC PEEK on path `/echo` returns the stored value.
::
::  The Rust `listen` binary boots this kernel and attaches `grpc_server_driver`, which
::  exposes +poke / +peek over the private gRPC service. A gRPC poke's payload becomes the
::  `cause` here; a gRPC peek's path becomes the `path` here. So the whole echo roundtrip is:
::  client poke [%echo 'hi'] -> +poke stores 'hi' -> client peek /echo -> +peek returns 'hi'.
::
/+  *lib
/=  *  /common/wrapper
::
=>
|%
+$  versioned-state
  $:  %v0
      val=@t          ::  the last value echoed in via gRPC poke (empty until first poke)
  ==
::
::  Causes accepted by +poke. The gRPC poke payload is cued straight into this `cause`.
+$  cause
  $%  [%echo val=@t]    ::  store `val` as the echo value
  ==
::
::  On each poke we emit `[%echoed val]`. The Rust `listen` process watches for this effect
::  to count served echoes and print the greppable `metric: pokes=<N>` line for nockd status.
+$  effect
  $%  [%echoed val=@t]
  ==
--
|%
++  moat  (keep versioned-state)
::
++  inner
  |_  state=versioned-state
  ::
  ++  load
    |=  old=versioned-state
    ^-  _state
    old
  ::
  ::  +peek: path /echo returns the stored value as `[~ ~ val]` (a loobean-free (unit (unit @t))).
  ::  The private gRPC server jams the *entire* peek result and ships it back to the client,
  ::  which cues it and pulls out `val`.
  ++  peek
    |=  =path
    ^-  (unit (unit *))
    ?+    path  ~
        [%echo ~]
      ~>  %slog.[0 leaf+"echo-grpc: peek /echo -> {<val.state>}"]
      ``val.state
    ==
  ::
  ::  +poke: store the echoed value. The gRPC poke payload is the `cause` directly.
  ++  poke
    |=  =ovum:moat
    ^-  [(list effect) _state]
    =/  cause  ((soft cause) cause.input.ovum)
    ?~  cause
      ~>  %slog.[3 leaf+"echo-grpc: invalid cause {<cause.input.ovum>}"]
      `state
    ~>  %slog.[0 leaf+"echo-grpc: poke %echo <- {<val.u.cause>}"]
    :_  state(val val.u.cause)
    ^-  (list effect)
    ~[[%echoed val.u.cause]]
  --
--
((moat |) inner)
