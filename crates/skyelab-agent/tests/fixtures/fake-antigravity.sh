#!/bin/sh
# Stand-in for `agy` (Antigravity ACP CLI) used in tests and SKYELAB_ANTIGRAVITY_BIN.
# Speaks ACP JSON-RPC 2.0 NDJSON over stdio.
[ "$1" = --version ] && { echo '1.5.17'; exit 0; }

# Parse argv for --gemini-dir and --login.
gemini_dir="${GEMINI_HOME-}"
is_login=0
prev=
for a in "$@"; do
  if [ "$prev" = "--gemini-dir" ]; then
    gemini_dir="$a"
  fi
  [ "$a" = "--login" ] && is_login=1
  prev="$a"
done

if [ "$is_login" = 1 ]; then
  mkdir -p "$gemini_dir/antigravity-acp"
  printf 'fake@example.com\n' > "$gemini_dir/fake-login"
  echo "fake antigravity login for $gemini_dir"
  exit 0
fi

# Hostile probe modes driven by marker files in GEMINI_HOME (for acp_bounds.rs).
if [ -f "$gemini_dir/fake-probe-oversized" ]; then
  awk 'BEGIN{printf "{\"jsonrpc\":\"2.0\",\"method\":\"pad\",\"params\":\""; for(i=0;i<524288;i++) printf "aaaaaaaaaa"; print "\"}"}'
  exit 0
fi
if [ -f "$gemini_dir/fake-probe-flood" ]; then
  i=0
  while [ "$i" -lt 600 ]; do
    printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"x"}}}}\n'
    i=$((i + 1))
  done
  exit 0
fi
if [ -f "$gemini_dir/fake-probe-hang" ]; then
  sleep 30
  exit 0
fi

v() { eval "printf '%s' \"\${$1-<unset>}\""; }
esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }

sess="acp_sess_fake"
pending_prompt_id=""

while IFS= read -r line; do
  [ -z "$line" ] && continue
  printf '{"jsonrpc":"2.0","method":"fake/request","params":%s}\n' "$line"
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"promptCapabilities":{"image":true,"audio":false,"embeddedContext":true},"sessionCapabilities":{"resume":{},"load":{},"list":{},"close":{}}},"agentInfo":{"name":"Antigravity ACP Server","version":"1.5.17"},"authMethods":[{"id":"oauth-personal","name":"Log in with Google"}]}}\n' "$id"
      ;;
    *'"method":"session/new"'*|*'"method":"session/resume"'*|*'"method":"session/load"'*)
      printf '{"jsonrpc":"2.0","method":"fake/env","params":{"gemini_home":"%s","claude_config_dir":"%s","codex_home":"%s","gemini_api_key":"%s","google_api_key":"%s","google_application_credentials":"%s","agy_force_file":"%s","agy_disable_update":"%s","ssh_connection":"%s","cwd":"%s","args":"%s"}}\n' \
        "$(esc "$(v GEMINI_HOME)")" "$(esc "$(v CLAUDE_CONFIG_DIR)")" "$(esc "$(v CODEX_HOME)")" \
        "$(esc "$(v GEMINI_API_KEY)")" "$(esc "$(v GOOGLE_API_KEY)")" "$(esc "$(v GOOGLE_APPLICATION_CREDENTIALS)")" \
        "$(esc "$(v AGY_ACP_FORCE_FILE_STORAGE)")" "$(esc "$(v AGY_CLI_DISABLE_AUTO_UPDATE)")" \
        "$(esc "$(v SSH_CONNECTION)")" "$(esc "$(pwd -P)")" "$(esc "$*")"
      if [ -f .fake-acp-refuse-open ]; then
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32603,"message":"refused open"}}\n' "$id"
        continue
      fi
      if [ ! -f "$gemini_dir/fake-login" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"Authentication required"}}\n' "$id"
        continue
      fi
      req_sess=$(printf '%s' "$line" | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p')
      if [ -f .fake-session-gone ] && [ -n "$req_sess" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32002,"message":"session not found: %s"}}\n' "$id" "$req_sess"
        continue
      fi
      [ -n "$req_sess" ] && sess="$req_sess"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"%s","configOptions":[{"id":"model","name":"Model","type":"select","currentValue":"gemini-3-flash","options":[{"value":"gemini-3-flash","name":"Gemini 3 Flash"},{"value":"gemini-2.5-pro","name":"Gemini 2.5 Pro"},{"value":"claude-sonnet-4-5","name":"Claude Sonnet 4.5"}]},{"id":"thought_level","name":"Thinking","category":"thought_level","type":"select","currentValue":"medium","options":[{"value":"low","name":"Low"},{"value":"medium","name":"Medium"},{"value":"high","name":"High"}]}],"modes":{"currentModeId":"default","availableModes":[{"id":"default","name":"Ask"},{"id":"plan","name":"Plan"}]}}}\n' "$id" "$sess"
      ;;
    *'"method":"session/set_config_option"'*|*'"method":"session/set_model"'*|*'"method":"session/set_mode"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id"
      ;;
    *'"method":"session/cancel"'*)
      if [ -n "$pending_prompt_id" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$pending_prompt_id"
        pending_prompt_id=""
      fi
      ;;
    *'"method":"session/prompt"'*)
      case "$line" in
        *hang*)
          pending_prompt_id="$id"
          ;;
        *flood_asks*)
          n=1
          while [ "$n" -le 70 ]; do
            printf '{"jsonrpc":"2.0","id":%s,"method":"session/request_permission","params":{"sessionId":"%s","toolCall":{"toolCallId":"call_flood_%s","title":"Run echo %s?","kind":"execute","rawInput":{"command":"echo %s"}},"options":[{"optionId":"allow_once","name":"Allow once","kind":"allow_once"},{"optionId":"reject_once","name":"Deny","kind":"reject_once"}]}}\n' "$((200 + n))" "$sess" "$n" "$n" "$n"
            n=$((n + 1))
          done
          # Read the 6 auto-cancelled replies for asks 65..70 and echo them back.
          r=1
          while [ "$r" -le 6 ]; do
            if IFS= read -r reply; then
              printf '{"jsonrpc":"2.0","method":"fake/request","params":%s}\n' "$reply"
            fi
            r=$((r + 1))
          done
          # Exit with 64 asks still open so EOF withdrawal is exercised too.
          exit 0
          ;;
        *review_plan*)
          # agy's Plan mode: writes plan.md into its brain dir (outside the folder) asking for feedback, links it
          # as file:///…, and ends the turn.
          plan="$gemini_dir/antigravity-acp/brain/$sess/plan.md"
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call","toolCallId":"call_plan","title":"Write plan.md","kind":"edit","status":"in_progress","rawInput":{"ArtifactMetadata":"{\\"RequestFeedback\\":true,\\"Summary\\":\\"Plan for subtract\\",\\"UserFacing\\":true}","Content":"# Plan\\n\\n1. Add subtract(a, b) to calc.py\\n2. Test it","Overwrite":"true","Path":"%s"}}}}\n' "$sess" "$plan"
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call_update","toolCallId":"call_plan","status":"completed","rawOutput":{"text":"written"}}}}\n' "$sess"
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"I wrote [plan.md](file://%s). Review and approve it."}}}}\n' "$sess" "$plan"
          printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
          ;;
        *"plan is approved"*)
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Implementing the plan."}}}}\n' "$sess"
          printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
          ;;
        *slow_turn*)
          # agy's real result: a stop reason, no duration and a null usage.
          sleep 0.3
          printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn","usage":null}}\n' "$id"
          ;;
        *huge_plan*)
          entries=$(awk 'BEGIN{printf "["; for(i=1;i<=300;i++){if(i>1)printf ","; printf "{\"content\":\"Step %d\",\"status\":\"pending\"}", i}; printf "]"}')
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"plan","entries":%s}}}\n' "$sess" "$entries"
          printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
          ;;
        *giant_tool*)
          big=$(awk 'BEGIN{for(i=0;i<20000;i++) printf "0123456789"}')
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call","toolCallId":"call_giant","title":"Giant","kind":"execute","status":"in_progress","rawInput":{"command":"%s","deep":{"a":{"b":{"c":{"d":{"e":{"f":{"g":{"h":{"i":{"j":{"k":{"l":{"m":{"n":"bottom"}}}}}}}}}}}}}}}}}}\n' "$sess" "$big"
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call_update","toolCallId":"call_giant","status":"completed","rawOutput":{"output":"%s"}}}}\n' "$sess" "$big"
          printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
          ;;
        *oversized_line*)
          awk 'BEGIN{printf "{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{\"sessionId\":\"s\",\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\""; for(i=0;i<1800000;i++) printf "abcdefghij"; print "\"}}}}"}'
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"after oversized"}}}}\n' "$sess"
          printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
          ;;
        *flood_text*)
          chunk=$(awk 'BEGIN{for(i=0;i<1024;i++) printf "0123456789"}')
          i=1
          while [ "$i" -le 520 ]; do
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$sess" "$chunk"
            i=$((i + 1))
          done
          printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
          ;;
        *one_shot_ask*)
          printf '{"jsonrpc":"2.0","id":301,"method":"session/request_permission","params":{"sessionId":"%s","toolCall":{"toolCallId":"call_os_1","title":"Run rm -rf /?","kind":"execute","rawInput":{"command":"rm -rf /"}},"options":[{"optionId":"allow_once","name":"Allow once","kind":"allow_once"},{"optionId":"reject_once","name":"Deny","kind":"reject_once"}]}}\n' "$sess"
          IFS= read -r reply
          printf '%s\n' "$reply" > "$gemini_dir/fake-oneshot-perm"
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Safe title"}}}}\n' "$sess"
          printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
          ;;
        *"Write a title for this conversation"*)
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Antigravity fake title"}}}}\n' "$sess"
          printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
          ;;
        *approve*)
          printf '{"jsonrpc":"2.0","id":99,"method":"session/request_permission","params":{"sessionId":"%s","toolCall":{"toolCallId":"call_perm_1","title":"Run make deploy?","kind":"execute","rawInput":{"command":"make deploy"}},"options":[{"optionId":"allow_once","name":"Allow once","kind":"allow_once"},{"optionId":"allow_always","name":"Allow always","kind":"allow_always"},{"optionId":"reject_once","name":"Deny","kind":"reject_once"}]}}\n' "$sess"
          IFS= read -r reply
          printf '{"jsonrpc":"2.0","method":"fake/request","params":%s}\n' "$reply"
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Approved done."}}}}\n' "$sess"
          printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
          ;;
        *question*)
          printf '{"jsonrpc":"2.0","id":100,"method":"session/request_permission","params":{"sessionId":"%s","toolCall":{"toolCallId":"interaction_q1","title":"Pick a database"},"options":[{"optionId":"pg","name":"PostgreSQL","kind":"allow_once"},{"optionId":"sqlite","name":"SQLite","kind":"allow_once"}]}}\n' "$sess"
          IFS= read -r reply
          printf '{"jsonrpc":"2.0","method":"fake/request","params":%s}\n' "$reply"
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Question answered."}}}}\n' "$sess"
          printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
          ;;
        *)
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Thinking..."}}}}\n' "$sess"
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call","toolCallId":"call_1","title":"Read README.md","kind":"read","status":"in_progress","rawInput":{"path":"README.md"}}}}\n' "$sess"
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call_update","toolCallId":"call_1","status":"completed","rawOutput":{"text":"ok"}}}}\n' "$sess"
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"plan","entries":[{"content":"Inspect code","status":"completed"},{"content":"Write reply","status":"in_progress"}]}}}\n' "$sess"
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"usage_update","used":1200,"size":200000}}}\n' "$sess"
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Hello from "}}}}\n' "$sess"
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"fake Antigravity."}}}}\n' "$sess"
          printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
          ;;
      esac
      ;;
  esac
done
