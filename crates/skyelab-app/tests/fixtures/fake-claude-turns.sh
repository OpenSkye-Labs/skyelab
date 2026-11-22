#!/bin/sh
# Stand-in for `claude` that takes its time: one turn per stdin message, each answered with a result after a
# pause, so a follow-up can be queued behind a running turn.
printf '{"type":"system","subtype":"init","cwd":"%s","session_id":"fake-turns","tools":[],"model":"fake","permissionMode":"default"}\n' "$(pwd -P)"
while IFS= read -r line; do
  sleep 1
  printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":1,"num_turns":1,"result":"done","session_id":"fake-turns","total_cost_usd":0}\n'
done
