#!/bin/sh
# Stand-in for `claude` in tests: replays a recorded stream, reports its env and cwd, then echoes stdin lines.
# A prompt with "permit" asks for permission (a can_use_tool control request) and reports the reply it gets as
# `fake_answer`; "withdraw" asks and then withdraws the ask; "stderr-flood" writes 200 KiB and then END to stderr;
# "crash" writes to stderr and exits 3.
cat "$(dirname "$0")/session.jsonl"
printf '{"type":"fake_env","config_dir":"%s","api_key":"%s","cwd":"%s","args":"%s","env_keys":"%s"}\n' "$CLAUDE_CONFIG_DIR" "$ANTHROPIC_API_KEY" "$(pwd -P)" "$*" "$(env | sed -n 's/^\([A-Za-z_][A-Za-z0-9_]*\)=.*/\1/p' | sort | tr '\n' ' ')"
# A grandchild holding stdout open, like a tool or MCP server would: kill() must reach it.
sleep 30 &
ask() {
  printf '{"type":"control_request","request_id":"%s","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"make deploy"},"permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"make deploy"}],"behavior":"allow","destination":"localSettings"}],"tool_use_id":"toolu_p"}}\n' "$1"
}
while IFS= read -r line; do
  printf '%s\n' "$line"
  case "$line" in
    *'"content":"'permit*)
      ask perm-1
      IFS= read -r reply
      printf '{"type":"fake_answer","reply":%s}\n' "$reply" ;;
    *'"content":"'withdraw*)
      ask perm-2
      printf '{"type":"control_cancel_request","request_id":"perm-2"}\n' ;;
    *'"content":"'stderr-flood*)
      head -c 204800 /dev/zero | tr '\0' 'e' >&2
      printf 'END' >&2 ;;
    *'"content":"'crash*)
      printf 'fake-claude: boom\n' >&2
      exit 3 ;;
  esac
done
