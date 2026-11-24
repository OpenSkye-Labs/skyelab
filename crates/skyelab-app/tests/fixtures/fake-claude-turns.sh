#!/bin/sh
# Stand-in for `claude` that takes its time: one turn per stdin message, each answered with a result after a
# pause, so a follow-up can be queued behind a running turn.
printf '{"type":"system","subtype":"init","cwd":"%s","session_id":"fake-turns","tools":[],"model":"fake","permissionMode":"default"}\n' "$(pwd -P)"
n=0
while IFS= read -r line; do
  n=$((n + 1))
  # Later answers wait for the test's go-ahead (a `.fake-go` file in the folder), so it sees the state in between.
  if [ "$n" -gt 1 ]; then
    while [ ! -e .fake-go ]; do sleep 0.1; done
    rm -f .fake-go
  fi
  sleep 1
  printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":1,"num_turns":1,"result":"done","session_id":"fake-turns","total_cost_usd":0}\n'
done
