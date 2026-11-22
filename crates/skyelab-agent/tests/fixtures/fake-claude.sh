#!/bin/sh
# Stand-in for `claude` in tests: replays a recorded stream, reports its env and cwd, then echoes stdin lines.
cat "$(dirname "$0")/session.jsonl"
printf '{"type":"fake_env","config_dir":"%s","api_key":"%s","cwd":"%s","args":"%s"}\n' "$CLAUDE_CONFIG_DIR" "$ANTHROPIC_API_KEY" "$(pwd -P)" "$*"
# A grandchild holding stdout open, like a tool or MCP server would: kill() must reach it.
sleep 30 &
while IFS= read -r line; do printf '%s\n' "$line"; done
