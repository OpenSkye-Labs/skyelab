#!/bin/sh
printf '{"type":"system","subtype":"init","cwd":"%s","session_id":"fake-chatty","tools":[],"model":"fake","permissionMode":"default"}\n' "$(pwd -P)"
IFS= read -r line
( while :; do printf '{"type":"fake_tick"}\n'; sleep 0.05; done ) &
echo $! > .holder-pid
exit 0
