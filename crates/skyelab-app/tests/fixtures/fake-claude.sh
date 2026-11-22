#!/bin/sh
# Stand-in for `claude` (tests and SKYELAB_CLAUDE_BIN): replays a recorded session, reports the
# account dir it got, then echoes every stdin line so tests can see what was sent.
cat "$(dirname "$0")/session.jsonl"
printf '{"type":"fake_env","config_dir":"%s","cwd":"%s"}\n' "$CLAUDE_CONFIG_DIR" "$(pwd -P)"
while IFS= read -r line; do printf '%s\n' "$line"; done
