#!/bin/sh
# Stand-in for a `claude` that stopped reading stdin: reports its pid (its process group id), keeps a grandchild, never reads.
sleep 60 &
printf '{"type":"fake_pid","pid":%s}\n' "$$"
sleep 60
