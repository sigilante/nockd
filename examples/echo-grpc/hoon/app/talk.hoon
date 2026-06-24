::  echo-grpc / talk — placeholder kernel for the CLIENT binary.
::
::  The `talk` Rust binary is a pure gRPC client: it connects to the `listen` server's
::  private gRPC port, pokes a value, peeks it back, and exits. It never boots this kernel.
::  But `nockup project build` compiles one hoon entrypoint per binary, so this trivial
::  kernel exists solely to satisfy the build for the `talk` bin.
::
/+  *lib
/=  *  /common/wrapper
::
=>
|%
+$  versioned-state  $:(%v0 ~)
+$  cause  $%([%noop ~])
+$  effect  $%([%noop ~])
--
|%
++  moat  (keep versioned-state)
::
++  inner
  |_  state=versioned-state
  ++  load  |=(old=versioned-state ^-(_state old))
  ++  peek  |=(=path ^-((unit (unit *)) ~))
  ++  poke  |=(=ovum:moat ^-([(list effect) _state] `state))
  --
--
((moat |) inner)
