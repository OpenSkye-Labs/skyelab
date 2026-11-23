#!/bin/sh
# Stand-in for `claude` (tests and SKYELAB_CLAUDE_BIN): replays a recorded session, reports the
# account dir and args it got, then echoes every stdin line so tests can see what was sent.
# A prompt starting "permit" asks to run `make deploy` (a can_use_tool control request), waits for the answer,
# reports it as `fake_answer` and ends the turn; one starting "crash" writes to stderr and exits 3.
if [ -f .fake-agent-commits ]; then
  # An agent that commits its work.
  printf 'by the agent\n' > agent-commit.txt
  git add agent-commit.txt >/dev/null 2>&1
  git -c user.name=fake -c user.email=fake@example.com -c commit.gpgsign=false commit -q -m "agent commit" -- agent-commit.txt >/dev/null 2>&1
fi
if [ -f .fake-agent-edits ]; then
  # An agent that edits a file every turn (one turn per run).
  printf 'turn\n' >> agent-edit.txt
fi
if [ -f .fake-agent-no-result ]; then
  # A turn that never ends (no result event), so tests can act mid-turn.
  grep -v '"type":"result"' "$(dirname "$0")/session.jsonl"
else
  cat "$(dirname "$0")/session.jsonl"
fi
printf '{"type":"fake_env","config_dir":"%s","cwd":"%s","args":"%s","pid":%s}\n' "$CLAUDE_CONFIG_DIR" "$(pwd -P)" "$*" "$$"
while IFS= read -r line; do
  printf '%s\n' "$line"
  case "$line" in
    *'"content":"permit'*)
      printf '{"type":"control_request","request_id":"perm-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"make deploy","description":"Deploy the site"},"tool_use_id":"toolu_p"}}\n'
      IFS= read -r reply
      printf '{"type":"fake_answer","reply":%s}\n' "$reply"
      printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":5,"total_cost_usd":0,"result":"done"}\n' ;;
    *'"content":"crash'*)
      printf 'fake-claude: lost the session\n' >&2
      exit 3 ;;
  esac
done
