#!/bin/sh
# `--version` prints a fixed version (the Welcome page's agent list).
[ "$1" = --version ] && { echo '2.1.0 (Claude Code)'; exit 0; }
# `auth login` signs the account in: it writes fake-login (fake@example.com) in its config dir. `auth status` answers
# like Claude Code's pretty-printed JSON: signed in, as fake-login's content, while that file is there.
[ "$1 $2" = "auth login" ] && { echo "fake claude auth login for $CLAUDE_CONFIG_DIR"; mkdir -p "$CLAUDE_CONFIG_DIR" && echo fake@example.com > "$CLAUDE_CONFIG_DIR/fake-login"; exit 0; }
# `mcp login -- <name>` writes the name and its folder to fake-mcp-login in its config dir and exits 0.
[ "$1 $2 $3" = "mcp login --" ] && { echo "fake claude mcp login $4 for $CLAUDE_CONFIG_DIR"; mkdir -p "$CLAUDE_CONFIG_DIR" && printf '%s\n%s\n' "$4" "$(pwd -P)" > "$CLAUDE_CONFIG_DIR/fake-mcp-login"; exit 0; }
if [ "$1 $2" = "auth status" ]; then
  if [ -f "$CLAUDE_CONFIG_DIR/fake-login" ]; then
    printf '{\n  "loggedIn": true,\n  "authMethod": "claude.ai",\n  "email": "%s"\n}\n' "$(cat "$CLAUDE_CONFIG_DIR/fake-login")"
    exit 0
  fi
  printf '{\n  "loggedIn": false,\n  "authMethod": "none"\n}\n'
  exit 1
fi
# Stand-in for `claude` (tests and SKYELAB_CLAUDE_BIN): replays a recorded session, reports the
# account dir and args it got, then echoes every stdin line so tests can see what was sent.
# A prompt starting "permit" asks to run `make deploy` (a can_use_tool control request), waits for the answer,
# reports it as `fake_answer` and ends the turn ("permit write" asks a 2,000-line Write, "permit edit" an Edit and "permit multi" a two-edit MultiEdit
# instead); one starting "crash" writes to stderr and exits 3, and one starting
# "partial" streams half an answer first (text deltas, no final message), then exits 3 the same way.
# "hang" runs until Stop, then ends as Claude Code ends an aborted turn ("hang fail": a real API error instead).
# "turn" ends a plain turn ($0.0125, 1200 in, 80 out) with Claude's running totals.
# "question" asks an AskUserQuestion and "plan" an ExitPlanMode, each reported like "permit". "timeline" streams a
# turn with every rich row (thinking, text deltas, todos, an edit, bash output, usage), then a usage limit, a proposed
# plan and a question (left waiting). "stream" streams a long answer fast (fake-stream.sh, ~30 s). "subagent" runs two subagents (fake-subagent.sh), and "login" fails the turn
# the way a signed-out Claude Code does. "context" ends a turn whose last call held 170k tokens of a 200k window
# (85%: the meter warns and hints /compact). `auth login` prints which config dir it logs in to and exits 0. With a .fake-session-gone file in the folder, a --resume start fails the way
# Claude Code does for a session it doesn't have; a fresh start then reports session fake-sess-new.
# A `get_usage` control request (the usage poll) answers with the account dir's fake-usage.json (else signed out) and
# appends the dir to fake-usage.calls there.
# A title call (`-p ... --no-session-persistence`) appends its config dir, args (one per line) and stdin to
# .fake-title-call in its folder and answers "Fake title from <config dir name>"; a commit message prompt gets
# "feat(fake): commit from <config dir name>" plus a body, a pull request prompt a title and a body that says
# whether the prompt held a <template>.
case " $* " in
  *" --no-session-persistence "*)
    input=$(cat)
    {
      printf 'config_dir=%s\n' "$CLAUDE_CONFIG_DIR"
      for a in "$@"; do printf 'arg=[%s]\n' "$a"; done
      printf '%s\n---\n' "$input"
    } >> .fake-title-call
    who=$(basename "$CLAUDE_CONFIG_DIR")
    case "$input" in
      "Write a git commit message"*) printf 'feat(fake): commit from %s\n\nWritten by the fake agent.\n' "$who" ;;
      "Write a pull request title"*)
        printf 'feat(fake): pull request from %s\n\n' "$who"
        case "$input" in
          *"<template>"*) printf '## Summary\nFilled from the template.\n' ;;
          *) printf 'Fake PR body.\n' ;;
        esac ;;
      *) printf 'Fake title from %s\n' "$who" ;;
    esac
    exit 0 ;;
esac
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
if [ -f .fake-agent-edits ]; then
  # An agent that edits a file every turn (one turn per run).
  printf 'turn\n' >> agent-edit.txt
fi
# Claude's init reports the MCP servers it connected: with a fake-mcp-status.json (a JSON list of {name, status})
# in the account dir, the replayed init line carries it as `mcp_servers`.
mcp_status="$CLAUDE_CONFIG_DIR/fake-mcp-status.json"
replay() {
  if [ -f "$mcp_status" ]; then
    sed "s|\"subtype\":\"init\",|\"subtype\":\"init\",\"mcp_servers\":$(tr -d '\n|' < "$mcp_status"),|"
  else
    cat
  fi
}
# A .fake-hold-init file in the folder holds the start (before any line) until it is removed (at most 2 minutes).
n=0
while [ -f .fake-hold-init ] && [ $n -lt 1200 ]; do sleep 0.1; n=$((n + 1)); done
if [ -f .fake-start-hook ]; then
  # A SessionStart hook ahead of init, held until the folder has a .fake-hook-done file (at most 2 minutes).
  hook='"hook_id":"fake-hook-1","hook_name":"SessionStart:startup","hook_event":"SessionStart"'
  printf '{"type":"system","subtype":"hook_started",%s}\n' "$hook"
  n=0
  while [ ! -f .fake-hook-done ] && [ $n -lt 1200 ]; do sleep 0.1; n=$((n + 1)); done
  printf '{"type":"system","subtype":"hook_response",%s,"exit_code":0,"outcome":"success"}\n' "$hook"
fi
if [ -f .fake-agent-no-result ]; then
  # A turn that never ends (no result event), so tests can act mid-turn.
  grep -v '"type":"result"' "$(dirname "$0")/session.jsonl" | replay
else
  replay < "$(dirname "$0")/session.jsonl"
fi
if [ -f .fake-session-gone ]; then
  printf '{"type":"system","subtype":"init","cwd":"/tmp/x","session_id":"fake-sess-new","tools":[],"model":"claude-opus-5-5"}\n'
fi
printf '{"type":"fake_env","config_dir":"%s","cwd":"%s","args":"%s","pid":%s}\n' "$CLAUDE_CONFIG_DIR" "$(pwd -P)" "$*" "$$"
# The --mcp-config file it got, on one line (tests read which servers reached it).
prev=
for a in "$@"; do
  [ "$prev" = --mcp-config ] && [ -f "$a" ] && printf '{"type":"fake_mcp","config":%s}\n' "$(tr -d '\n' < "$a")"
  prev=$a
done
# Like Claude Code, `total_cost_usd` and `modelUsage` are the process's running totals (the replayed session's $0.0421
# first); `usage` is the turn's own.
total=0.0421 in_total=0 out_total=0
while IFS= read -r line; do
  printf '%s\n' "$line"
  case "$line" in
    *'"subtype":"get_usage"'*)
      # The usage poll: the account dir's fake-usage.json is the answer, else a signed-out one.
      usage='{"rate_limits_available":false,"rate_limits":null}'
      [ -f "$CLAUDE_CONFIG_DIR/fake-usage.json" ] && usage=$(tr -d '\n' < "$CLAUDE_CONFIG_DIR/fake-usage.json")
      # Logged first: SkyeLab kills the CLI once it reads the answer.
      echo "$CLAUDE_CONFIG_DIR" >> "$CLAUDE_CONFIG_DIR/fake-usage.calls"
      printf '{"type":"control_response","response":{"subtype":"success","request_id":"skyelab-usage","response":%s}}\n' "$usage" ;;
    *'"content":"permit write'*)
      # A 2,000-line Write: the approval shows its first lines, not its JSON.
      body=$(awk 'BEGIN { for (i = 1; i <= 2000; i++) printf "fn line_%d() {}\\n", i }')
      printf '{"type":"control_request","request_id":"perm-1","request":{"subtype":"can_use_tool","tool_name":"Write","input":{"file_path":"%s/src/big.rs","content":"%s"},"tool_use_id":"toolu_p"}}\n' "$PWD" "$body"
      IFS= read -r reply
      printf '{"type":"fake_answer","reply":%s}\n' "$reply"
      printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":5,"total_cost_usd":0,"result":"done"}\n' ;;
    *'"content":"permit multi'*)
      printf '{"type":"control_request","request_id":"perm-1","request":{"subtype":"can_use_tool","tool_name":"MultiEdit","input":{"file_path":"%s/src/lib.rs","edits":[{"old_string":"pub fn a() {}","new_string":"pub fn a() -> u8 {\\n    1\\n}"},{"old_string":"// old note","new_string":"// new note"}]},"tool_use_id":"toolu_p"}}\n' "$PWD"
      IFS= read -r reply
      printf '{"type":"fake_answer","reply":%s}\n' "$reply"
      printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":5,"total_cost_usd":0,"result":"done"}\n' ;;
    *'"content":"permit edit'*)
      printf '{"type":"control_request","request_id":"perm-1","request":{"subtype":"can_use_tool","tool_name":"Edit","input":{"file_path":"%s/src/main.rs","old_string":"fn main() {\\n    println!(\\"hi\\");\\n}","new_string":"fn main() {\\n    println!(\\"hello\\");\\n    run();\\n    // a long line that must wrap, never clip, a long line that must wrap, never clip, a long line that must wrap, never clip, a long line that must wrap, never clip, a long line that must wrap, never clip, a long line that must wrap, never clip, END\\n}"},"tool_use_id":"toolu_p"}}\n' "$PWD"
      IFS= read -r reply
      printf '{"type":"fake_answer","reply":%s}\n' "$reply"
      printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":5,"total_cost_usd":0,"result":"done"}\n' ;;
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
    *'"content":"hang'*)
      # Runs until the next stdin line (Stop's interrupt), then ends the way Claude Code ends an aborted turn: an
      # error result with terminal_reason aborted_*. "hang fail" ends in a real API error instead.
      IFS= read -r reply
      printf '%s\n' "$reply"
      case "$line" in
        *'"content":"hang fail'*) printf '{"type":"result","subtype":"error_during_execution","is_error":true,"duration_ms":4200,"result":"API Error: 500 overloaded","total_cost_usd":0}\n' ;;
        *) printf '{"type":"result","subtype":"error_during_execution","is_error":true,"duration_ms":4200,"terminal_reason":"aborted_streaming","errors":["Error: Request was aborted."],"total_cost_usd":0}\n' ;;
      esac ;;
    *'"content":"turn'*)
      total=$(awk "BEGIN{print $total + 0.0125}") in_total=$((in_total + 1200)) out_total=$((out_total + 80))
      printf '{"type":"assistant","message":{"id":"msg_t%s","role":"assistant","model":"claude-opus-5-5","content":[{"type":"text","text":"Turn done."}]},"session_id":"fake-sess-1"}\n' "$in_total"
      printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":900,"num_turns":1,"result":"Turn done.","session_id":"fake-sess-1","total_cost_usd":%s,"usage":{"input_tokens":3,"cache_creation_input_tokens":200,"cache_read_input_tokens":997,"output_tokens":80},"modelUsage":{"claude-opus-5-5":{"inputTokens":%s,"outputTokens":%s,"costUSD":%s}}}\n' "$total" "$in_total" "$out_total" "$total" ;;
    *'"content":"timeline'*)
      sh "$(dirname "$0")/fake-timeline.sh" ;;
    *'"content":"stream'*)
      sh "$(dirname "$0")/fake-stream.sh" ;;
    *'"content":"mermaid'*)
      # A reply with a mermaid diagram, a broken one and a plain code block (the card next to a normal one).
      printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Here is the flow:\\n\\n```mermaid\\nflowchart LR\\n    A[User prompt] --> B{Bound account?}\\n    B -- yes --> C[Start agent]\\n    B -- no --> D[Ask the user]\\n    C --> E[(Thread store)]\\n```\\n\\nAnd one that does not parse:\\n\\n```mermaid\\nnot a diagram ->->\\n```\\n\\nA plain block:\\n\\n```rust\\nfn main() {}\\n```"}]},"parent_tool_use_id":null}\n'
      printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":5,"total_cost_usd":0,"result":"done"}\n' ;;
    *'"content":"table'*)
      # A reply with a markdown table (the copy menu under it).
      printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Here are the accounts:\\n\\n| Account | Provider | Threads |\\n|---|---|---:|\\n| work | Claude | 12 |\\n| personal | Codex | 3 |\\n\\nThat is all."}]},"parent_tool_use_id":null}\n'
      printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":5,"total_cost_usd":0,"result":"done"}\n' ;;
    *'"content":"subagent'*)
      sh "$(dirname "$0")/fake-subagent.sh" ;;
    *'"content":"context'*)
      printf '{"type":"assistant","parent_tool_use_id":null,"message":{"id":"msg_c","role":"assistant","model":"claude-opus-5-5","content":[{"type":"text","text":"Context is filling up."}],"usage":{"input_tokens":2,"cache_creation_input_tokens":1000,"cache_read_input_tokens":168598,"output_tokens":400}},"session_id":"fake-sess-1"}\n'
      printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":50,"result":"Context is filling up.","session_id":"fake-sess-1","total_cost_usd":0,"modelUsage":{"claude-opus-5-5":{"inputTokens":170000,"outputTokens":400,"contextWindow":200000}}}\n' ;;
    *'"content":"login'*)
      printf '{"type":"result","subtype":"success","is_error":true,"duration_ms":40,"total_cost_usd":0,"result":"Not logged in \\u00b7 Please run /login"}\n' ;;
    *'"content":"partial'*)
      for w in "Half an" " answer"; do
        printf '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"%s"}},"parent_tool_use_id":null}\n' "$w"
      done
      sleep 0.3
      printf 'fake-claude: lost the session\n' >&2
      exit 3 ;;
    *'"content":"crash'*)
      printf 'fake-claude: lost the session\n' >&2
      exit 3 ;;
  esac
done
