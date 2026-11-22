#!/bin/sh
# Stand-in for `claude` that calls SkyeLab's MCP tools (tests and SKYELAB_CLAUDE_BIN). A prompt
# `mcp:<tool>:<json args>` makes one tools/call through the proxy named in --mcp-config, shown as a
# tool_use + tool_result pair; `delegate <project id>` is short for a start_thread there (easy to type
# in /loop-verify-ui); `hold` never finishes its turn; any other prompt gets a short answer. Never contacts a real account.
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
  call=$(printf '%s' "$line" | sed -nE 's/.*"content":"mcp:([a-z_]+):(([^"\\]|\\.)*)".*/\1 \2/p')
  project=$(printf '%s' "$line" | sed -nE 's/.*"content":"delegate ([0-9]+)".*/\1/p')
  [ -n "$project" ] && call="start_thread {\\\"project_id\\\":$project,\\\"prompt\\\":\\\"Write the release notes\\\"}"
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
  printf '{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_mcp%s","content":"%s","is_error":%s}]}}\n' "$n" "$text" "${err:-true}"
  done_turn "called $tool"
done
