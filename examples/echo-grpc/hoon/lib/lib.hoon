::  echo-grpc / lib — shared types for the echo demo.
::
::  Kept intentionally minimal; the listen kernel defines its own state/cause/effect inline.
::  This exists so `/+  *lib` resolves for both the listen and talk hoon entrypoints.
|%
+$  echo-value  @t
--
