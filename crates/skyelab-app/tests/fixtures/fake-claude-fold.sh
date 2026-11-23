#!/bin/sh
# Stand-in for a claude that folds a message sent mid-turn into the running turn, as claude 2.1 does: the first
# two prompts get one result that lists both prompts' uuids in user_message_uuids. Each later prompt gets a
# background "wake" result that answers none of our prompts, then its own result.
printf '{"type":"system","subtype":"init","cwd":"%s","session_id":"fake-fold","tools":[],"model":"fake","permissionMode":"default"}\n' "$(pwd -P)"
uuid() { printf '%s' "$1" | sed -n 's/.*"uuid":"\([^"]*\)".*/\1/p'; }
result() { printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":1,"num_turns":1,"result":"%s","session_id":"fake-fold","total_cost_usd":0,"user_message_uuids":[%s]}\n' "$1" "$2"; }
IFS= read -r first
sleep 1
IFS= read -r second
result both "\"$(uuid "$first")\",\"$(uuid "$second")\""
while IFS= read -r line; do
  result wake '"not-one-of-yours"'
  sleep 1
  result done "\"$(uuid "$line")\""
done
