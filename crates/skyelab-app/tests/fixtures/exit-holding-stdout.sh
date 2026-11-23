#!/bin/sh
# Stand-in for a claude that exits mid-turn while a tool it started (in its process group) still holds stdout.
# The tool's pid goes to .holder-pid in the folder.
printf '{"type":"system","subtype":"init","cwd":"%s","session_id":"fake-exit","tools":[],"model":"fake","permissionMode":"default"}\n' "$(pwd -P)"
IFS= read -r line
sleep 20 &
echo $! > .holder-pid
exit 0
