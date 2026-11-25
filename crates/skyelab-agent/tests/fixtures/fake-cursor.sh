#!/bin/sh
# Stand-in for `cursor-agent` / `agent` (tests and SKYELAB_CURSOR_BIN).
# Supports:
#   --version                  prints a fixed Cursor CLI version
#   status / whoami            signed in while $CURSOR_CONFIG_DIR/fake-login exists (prints its email), else Not logged in (exit 1)
#   login                      creates $CURSOR_CONFIG_DIR/fake-login and exits 0
#   models / --list-models     lists fake models (auto, composer-1, claude-4-sonnet, plus a bad flag-like entry)
#   -p --output-format text    one-shot mode (title / commit / PR), logs call to .fake-title-call
#   -p --output-format stream-json  streaming turn mode; reads prompt from stdin:
#     - emits fake_env event with CURSOR_CONFIG_DIR, CURSOR_API_KEY, CURSOR_AUTH_TOKEN, CLAUDE_CONFIG_DIR, CODEX_HOME, cwd, args, prompt
#     - prompt containing "hang": sleeps until SIGINT/SIGTERM (for interrupt test)
#     - prompt containing "oversized": emits a >16 MiB line then a normal result
#     - prompt containing "many_blocks": emits an assistant message with 120 blocks
#     - prompt containing "flood": emits 120000 thinking deltas (exceeding event cap)
#     - prompt containing "flat_tool": emits flat tool_call schema (tool_call_id/tool_name/arguments)
#     - prompt containing "bad_json": emits non-JSON line then normal result
#     - .fake-session-gone with --resume: fails with "No conversation found" error
#     - default: streams system/init, thinking delta+completed, nested tool_call started+completed,
#                timestamped assistant deltas, final assistant message, and result.

[ "$1" = --version ] && { echo 'cursor-agent 0.99.0'; exit 0; }

if [ "$1" = status ] || [ "$1" = whoami ]; then
  if [ -n "$CURSOR_CONFIG_DIR" ] && [ -f "$CURSOR_CONFIG_DIR/fake-login" ]; then
    who=$(tr -d '\r\n' < "$CURSOR_CONFIG_DIR/fake-login")
    if [ -n "$who" ]; then
      printf 'Logged in as %s\n' "$who"
    else
      printf 'Logged in as cursor@example.com\n'
    fi
    exit 0
  fi
  echo "Not logged in. Run 'cursor-agent login' to authenticate." >&2
  exit 1
fi

if [ "$1" = login ] && [ $# -eq 1 ]; then
  mkdir -p "$CURSOR_CONFIG_DIR" && printf 'cursor@example.com\n' > "$CURSOR_CONFIG_DIR/fake-login"
  echo "fake cursor-agent login for $CURSOR_CONFIG_DIR"
  exit 0
fi

if [ "$1" = models ] || [ "$1" = --list-models ]; then
  printf 'Available models:\n'
  printf '  auto - Auto (default)\n'
  printf '  composer-1 - Composer 1\n'
  printf '  claude-4-sonnet - Claude 4 Sonnet\n'
  printf '  --evil-flag - Should Be Skipped\n'
  exit 0
fi

# Parse flags
output_format="text"
resume_id=""
model=""
mode=""
force="false"
prev=""
for a in "$@"; do
  case "$prev" in
    --output-format) output_format="$a" ;;
    --resume) resume_id="$a" ;;
    --model) model="$a" ;;
    --mode) mode="$a" ;;
  esac
  [ "$a" = "--force" ] && force="true"
  prev="$a"
done

prompt=$(cat)

if [ "$output_format" = "text" ]; then
  {
    printf 'cursor_config_dir=%s\n' "${CURSOR_CONFIG_DIR-<unset>}"
    for a in "$@"; do printf 'arg=[%s]\n' "$a"; done
    printf '%s\n---\n' "$prompt"
  } >> .fake-title-call
  case "$prompt" in
    "Write a git commit message"*) printf 'fix(cursor): commit from cursor\n' ;;
    "Write a pull request title"*) printf 'fix(cursor): pull request\n\nCursor PR body.\n' ;;
    *) printf 'Cursor fake title\n' ;;
  esac
  exit 0
fi

v() { eval "printf '%s' \"\${$1-<unset>}\""; }
esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g; s/	/\\t/g' | tr '\n' ' '; }

session_id="${resume_id:-cur_fake_sess_1}"
active_model="${model:-composer-1}"

printf '{"type":"fake_env","cursor_config_dir":"%s","cursor_api_key":"%s","cursor_auth_token":"%s","cursor_backend_url":"%s","claude_config_dir":"%s","codex_home":"%s","cwd":"%s","args":"%s","prompt":"%s","mode":"%s","force":%s}\n' \
  "$(esc "$(v CURSOR_CONFIG_DIR)")" \
  "$(esc "$(v CURSOR_API_KEY)")" \
  "$(esc "$(v CURSOR_AUTH_TOKEN)")" \
  "$(esc "$(v CURSOR_BACKEND_URL)")" \
  "$(esc "$(v CLAUDE_CONFIG_DIR)")" \
  "$(esc "$(v CODEX_HOME)")" \
  "$(esc "$(pwd -P)")" \
  "$(esc "$*")" \
  "$(esc "$prompt")" \
  "$(esc "$mode")" \
  "$force"

if [ -f .fake-session-gone ] && [ -n "$resume_id" ]; then
  printf '{"type":"result","subtype":"error","is_error":true,"duration_ms":5,"result":"No conversation found with session ID: %s","session_id":"%s"}\n' "$(esc "$resume_id")" "$(esc "$resume_id")"
  exit 0
fi

printf '{"type":"system","subtype":"init","session_id":"%s","model":"%s","cwd":"%s"}\n' \
  "$(esc "$session_id")" "$(esc "$active_model")" "$(esc "$(pwd -P)")"

case "$prompt" in
  *hang*)
    trap 'printf "{\"type\":\"result\",\"subtype\":\"interrupted\",\"is_error\":true,\"duration_ms\":50,\"result\":\"interrupted\",\"session_id\":\"%s\"}\n" "'"$session_id"'"; exit 130' INT TERM
    printf '{"type":"thinking","subtype":"delta","text":"Working on long task...","timestamp_ms":1000}\n'
    i=0
    while [ "$i" -lt 600 ]; do
      sleep 0.1
      i=$((i + 1))
    done
    exit 0
    ;;
  *oversized*)
    # Emit a >16 MiB single JSON line using awk/dd without huge shell args.
    printf '{"type":"assistant","pad":"'
    dd if=/dev/zero bs=65536 count=260 2>/dev/null | tr '\0' 'a'
    printf '"}\n'
    printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"after oversized"}]}}\n'
    printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":10,"result":"after oversized","session_id":"%s"}\n' "$(esc "$session_id")"
    exit 0
    ;;
  *many_blocks*)
    printf '{"type":"assistant","message":{"role":"assistant","content":['
    i=1
    while [ "$i" -le 120 ]; do
      [ "$i" -gt 1 ] && printf ','
      printf '{"type":"text","text":"block-%s"}' "$i"
      i=$((i + 1))
    done
    printf ']}}\n'
    printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":10,"result":"many","session_id":"%s"}\n' "$(esc "$session_id")"
    exit 0
    ;;
  *flood*)
    awk 'BEGIN { for (i = 0; i < 105000; i++) printf "{\"type\":\"thinking\",\"subtype\":\"delta\",\"text\":\"x\"}\n" }'
    printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":10,"result":"flooded","session_id":"%s"}\n' "$(esc "$session_id")"
    exit 0
    ;;
  *bad_json*)
    printf 'not json at all\n'
    printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"recovered"}]}}\n'
    printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":10,"result":"recovered","session_id":"%s"}\n' "$(esc "$session_id")"
    exit 0
    ;;
  *flat_tool*)
    printf '{"type":"tool_call","subtype":"started","tool_call_id":"call_flat_1","tool_name":"grep_search","arguments":{"query":"Provider::Cursor"},"timestamp_ms":1001}\n'
    printf '{"type":"tool_call","subtype":"completed","tool_call_id":"call_flat_1","tool_name":"grep_search","arguments":{"query":"Provider::Cursor"},"result":"found 1 match","timestamp_ms":1002}\n'
    printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Done with flat tool."}]}}\n'
    printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":25,"result":"Done with flat tool.","session_id":"%s"}\n' "$(esc "$session_id")"
    exit 0
    ;;
esac

printf '{"type":"user","message":{"role":"user","content":[{"type":"text","text":"%s"}]}}\n' "$(esc "$prompt")"
printf '{"type":"thinking","subtype":"delta","text":"Inspecting ","timestamp_ms":1001}\n'
printf '{"type":"thinking","subtype":"delta","text":"the repo.","timestamp_ms":1002}\n'
printf '{"type":"thinking","subtype":"completed","timestamp_ms":1003}\n'
printf '{"type":"tool_call","subtype":"started","call_id":"call_1","tool_call":{"shellToolCall":{"args":{"command":"ls -la"}}},"timestamp_ms":1004}\n'
printf '{"type":"tool_call","subtype":"completed","call_id":"call_1","tool_call":{"shellToolCall":{"args":{"command":"ls -la"},"result":{"success":{"stdout":"README.md\\n","exitCode":0}}}},"timestamp_ms":1005}\n'
printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hello from "}]},"timestamp_ms":1006}\n'
printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"fake cursor."}]},"timestamp_ms":1007}\n'
printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hello from fake cursor."}]}}\n'
printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":42,"result":"Hello from fake cursor.","session_id":"%s","usage":{"input_tokens":120,"output_tokens":30}}\n' "$(esc "$session_id")"
