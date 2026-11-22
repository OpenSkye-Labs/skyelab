#!/bin/sh
# Like fake-claude.sh, but a tool that left claude's process group holds stdout for 3 s after a kill.
cat "$(dirname "$0")/session.jsonl"
perl -e 'setpgrp(0,0); sleep 3' &
while IFS= read -r line; do printf '%s\n' "$line"; done
