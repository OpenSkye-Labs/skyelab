#!/bin/sh
# Stand-in for `claude` (tests and SKYELAB_CLAUDE_BIN): replays a recorded session, reports the
# account dir and args it got, then echoes every stdin line so tests can see what was sent.
# A prompt starting "permit" asks to run `make deploy` (a can_use_tool control request), waits for the answer,
# reports it as `fake_answer` and ends the turn; one starting "crash" writes to stderr and exits 3.
# "question" asks an AskUserQuestion and "plan" an ExitPlanMode, each reported like "permit". "timeline" streams a
# turn with every rich row (thinking, text deltas, todos, an edit, bash output, usage), then a usage limit, a proposed
# plan and a question (left waiting). With a .fake-session-gone file in the folder, a --resume start fails the way
# Claude Code does for a session it doesn't have; a fresh start then reports session fake-sess-new.
if [ -f .fake-session-gone ]; then
  case " $* " in
    *" --resume "*)
      printf '{"type":"result","subtype":"error_during_execution","is_error":true,"duration_ms":0,"num_turns":0,"errors":["No conversation found with session ID: fake-sess-1"],"total_cost_usd":0}\n'
      printf 'No conversation found with session ID: fake-sess-1\n' >&2
      exit 1 ;;
  esac
fi
if [ -f .fake-agent-commits ]; then
  # An agent that commits its work.
  printf 'by the agent\n' > agent-commit.txt
  git add agent-commit.txt >/dev/null 2>&1
  git -c user.name=fake -c user.email=fake@example.com -c commit.gpgsign=false commit -q -m "agent commit" -- agent-commit.txt >/dev/null 2>&1
fi
cat "$(dirname "$0")/session.jsonl"
if [ -f .fake-session-gone ]; then
  printf '{"type":"system","subtype":"init","cwd":"/tmp/x","session_id":"fake-sess-new","tools":[],"model":"claude-opus-5-5"}\n'
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
    *'"content":"question'*)
      printf '{"type":"control_request","request_id":"q-1","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","input":{"questions":[{"question":"Which database?","header":"DB","options":[{"label":"Postgres","description":"SQL server"},{"label":"SQLite","description":"One file"}],"multiSelect":false},{"question":"Which extras?","header":"Extras","options":[{"label":"Cache","description":"In memory"},{"label":"Queue","description":"Jobs"}],"multiSelect":true}]},"tool_use_id":"toolu_q"}}\n'
      IFS= read -r reply
      printf '{"type":"fake_answer","reply":%s}\n' "$reply"
      printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":5,"total_cost_usd":0,"result":"done"}\n' ;;
    *'"content":"plan'*)
      printf '{"type":"control_request","request_id":"p-1","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","input":{"plan":"## Plan\\n\\n1. Add the parser\\n2. Test it"},"tool_use_id":"toolu_x"}}\n'
      IFS= read -r reply
      printf '{"type":"fake_answer","reply":%s}\n' "$reply"
      printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":5,"total_cost_usd":0,"result":"done"}\n' ;;
    *'"content":"timeline'*)
      sh "$(dirname "$0")/fake-timeline.sh" ;;
    *'"content":"crash'*)
      printf 'fake-claude: lost the session\n' >&2
      exit 3 ;;
  esac
done
