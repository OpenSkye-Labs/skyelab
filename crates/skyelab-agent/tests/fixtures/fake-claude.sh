#!/bin/sh
# Stand-in for `claude` in tests: replays a recorded stream, reports its env and cwd, then echoes stdin lines.
cat "$(dirname "$0")/session.jsonl"
printf '{"type":"fake_env","config_dir":"%s","cwd":"%s","args":"%s"}\n' "$CLAUDE_CONFIG_DIR" "$(pwd -P)" "$*"
while IFS= read -r line; do printf '%s\n' "$line"; done
