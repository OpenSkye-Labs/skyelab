#!/bin/sh
# Stand-in for `claude` that calls SkyeLab's MCP tools (tests and SKYELAB_CLAUDE_BIN). A prompt
# `mcp:<tool>:<json args>` makes one tools/call through the proxy named in --mcp-config, shown as a
# tool_use + tool_result pair; `delegate <project id>` is short for a start_thread there (easy to type
# in /loop-verify-ui; `delegate <project id> hold` starts a child that never finishes), `browse <url>` for a browser_open, `title` for a browser_eval of document.title and `shot` for a browser_screenshot (echoed as text plus a base64 image block, as real claude does); `hold` never finishes its turn; `usage` reports two usage windows (rate_limit_event); `limit` ends its turn, then reports the account limited; any other prompt gets a short answer. Never contacts a real account.
head -n 1 "$(dirname "$0")/session.jsonl"
cfg=
while [ $# -gt 0 ]; do
  [ "$1" = "--mcp-config" ] && cfg=$2
  shift
done
started=
n=0
start_proxy() {
  cmd=$(sed -n 's/^ *"command": "\(.*\)",*$/\1/p' "$cfg")
  sock=$(sed -n 's/^ *"SKYELAB_MCP_SOCKET": "\(.*\)",*$/\1/p' "$cfg")
  token=$(sed -n 's/^ *"SKYELAB_MCP_TOKEN": "\(.*\)",*$/\1/p' "$cfg")
  fifo=$(mktemp -d)
  mkfifo "$fifo/in" "$fifo/out"
  set -- # the proxy's args, one per line in the pretty-printed config
  while IFS= read -r a; do [ -n "$a" ] && set -- "$@" "$a"; done <<ARGS
$(sed -n '/"args": \[/,/\]/p' "$cfg" | sed -n 's/^ *"\(.*\)",*$/\1/p')
ARGS
  env SKYELAB_MCP_SOCKET="$sock" SKYELAB_MCP_TOKEN="$token" "$cmd" "$@" <"$fifo/in" >"$fifo/out" &
  exec 3>"$fifo/in" 4<"$fifo/out"
  rpc '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"fake","version":"1"}}}' >/dev/null
  printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}' >&3
  started=1
}
# One request, its answer; the proxy's other stdout lines (a test harness's) are skipped.
rpc() {
  printf '%s\n' "$1" >&3
  while IFS= read -r l <&4; do
    case $l in '{'*) printf '%s\n' "$l"; return ;; esac
  done
}
done_turn() {
  printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":5,"num_turns":1,"result":"%s","session_id":"fake-sess-1","total_cost_usd":0}\n' "$1"
}
while IFS= read -r line; do
  case $line in *'"type":"user"'*) ;; *) continue ;; esac
  # A JSON string value: ([^"\\]|\\.)* stops at its closing quote, whatever key comes next.
  case $line in *'"content":"hold"'*) continue ;; esac # stays mid-turn
  case $line in *'"content":"limit"'*)
    # Claude's order: the refused turn's result first, then the rejected limit.
    done_turn "limit"
    printf '{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","rateLimitType":"five_hour","resetsAt":%s}}\n' "$(( $(date +%s) + 8040 ))"
    continue ;;
  esac
  case $line in *'"content":"usage"'*)
    printf '%s\n' '{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","rateLimitType":"seven_day","utilization":0.62}}' \
      '{"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","rateLimitType":"five_hour","utilization":0.91}}'
    done_turn "usage"
    continue ;;
  esac
  call=$(printf '%s' "$line" | sed -nE 's/.*"content":"mcp:([a-z_]+):(([^"\\]|\\.)*)".*/\1 \2/p')
  project=$(printf '%s' "$line" | sed -nE 's/.*"content":"delegate ([0-9]+)( hold)?".*/\1/p')
  child=$(printf '%s' "$line" | sed -nE 's/.*"content":"delegate [0-9]+ hold".*/hold/p')
  browse=$(printf '%s' "$line" | sed -nE 's/.*"content":"browse (http[^"\\ ]*)".*/\1/p')
  [ -n "$browse" ] && call="browser_open {\\\"url\\\":\\\"$browse\\\"}"
  case $line in *'"content":"title"'*) call='browser_eval {\"expression\":\"document.title\"}' ;; esac
  case $line in *'"content":"shot"'*) call='browser_screenshot {}' ;; esac
  [ -n "$project" ] && call="start_thread {\\\"project_id\\\":$project,\\\"prompt\\\":\\\"${child:-Write the release notes}\\\"}"
  if [ -z "$call" ] || [ -z "$cfg" ]; then
    printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"Done with the delegated task."}]}}'
    done_turn "done"
    continue
  fi
  [ -n "$started" ] || start_proxy
  n=$((n + 1))
  tool=${call%% *}
  json=$(printf '%s' "${call#* }" | sed 's/\\"/"/g')
  printf '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_mcp%s","name":"mcp__skyelab__%s","input":%s}]}}\n' "$n" "$tool" "$json"
  resp=$(rpc "{\"jsonrpc\":\"2.0\",\"id\":$n,\"method\":\"tools/call\",\"params\":{\"name\":\"$tool\",\"arguments\":$json}}")
  text=$(printf '%s' "$resp" | sed -nE 's/.*"text":"(([^"\\]|\\.)*)".*/\1/p')
  err=$(printf '%s' "$resp" | sed -nE 's/.*"isError":(true|false).*/\1/p')
  # An image block comes back the way real claude echoes it: text then the image as a base64 source.
  data=$(printf '%s' "$resp" | sed -nE 's/.*"data":"([A-Za-z0-9+/=]*)".*/\1/p')
  if [ -n "$data" ]; then
    printf '{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_mcp%s","content":[{"type":"text","text":"%s"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"%s"}}],"is_error":%s}]}}\n' "$n" "$text" "$data" "${err:-true}"
  else
    printf '{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_mcp%s","content":"%s","is_error":%s}]}}\n' "$n" "$text" "${err:-true}"
  fi
  done_turn "called $tool"
done
