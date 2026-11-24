#!/bin/sh
# Stand-in for `claude` that takes its time: one turn per stdin message, each answered with a result after a
# pause, so a follow-up can be queued behind a running turn. Every result after the first waits for a `release` file
# in the folder too (so a loaded machine can't skip past the state between two results).
printf '{"type":"system","subtype":"init","cwd":"%s","session_id":"fake-turns","tools":[],"model":"fake","permissionMode":"default"}\n' "$(pwd -P)"
n=0
while IFS= read -r line; do
  sleep 1
  n=$((n + 1))
  if [ "$n" -gt 1 ]; then
    while [ ! -e release ]; do sleep 0.1; done
  fi
  printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":1,"num_turns":1,"result":"done","session_id":"fake-turns","total_cost_usd":0}\n'
done
