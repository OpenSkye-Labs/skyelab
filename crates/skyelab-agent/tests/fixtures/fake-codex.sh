#!/bin/sh
# Stand-in for `codex app-server` (tests and SKYELAB_CODEX_BIN). Speaks just enough of the app-server
# protocol: answers initialize, thread/start and thread/resume, replays codex-turn.jsonl for each
# turn/start, and ends a turn as interrupted on turn/interrupt. Every line it reads comes back as a
# `fake/request` notification, and thread/start/resume also report the env and args as `fake/env`.
# A prompt containing "hang" starts a turn that only an interrupt ends; "approve" first asks for an approval.
here=$(dirname "$0")
v() { eval "printf '%s' \"\${$1-<unset>}\""; }
while IFS= read -r line; do
  printf '{"method":"fake/request","params":%s}\n' "$line"
  id=$(printf '%s' "$line" | sed -n 's/^{"id":\([0-9]*\),.*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"id":%s,"result":{"userAgent":"fake-codex/0"}}\n' "$id" ;;
    *'"method":"thread/start"'*|*'"method":"thread/resume"'*)
      thread=$(printf '%s' "$line" | sed -n 's/.*"threadId":"\([^"]*\)".*/\1/p')
      [ -n "$thread" ] || thread=thr_fake
      printf '{"method":"fake/env","params":{"codex_home":"%s","claude_config_dir":"%s","openai_api_key":"%s","openai_base_url":"%s","openai_org_id":"%s","codex_api_key":"%s","codex_access_token":"%s","cwd":"%s","args":"%s"}}\n' \
        "$(v CODEX_HOME)" "$(v CLAUDE_CONFIG_DIR)" "$(v OPENAI_API_KEY)" "$(v OPENAI_BASE_URL)" "$(v OPENAI_ORG_ID)" \
        "$(v CODEX_API_KEY)" "$(v CODEX_ACCESS_TOKEN)" "$(pwd -P)" "$(printf '%s' "$*" | sed 's/\\/\\\\/g; s/"/\\"/g')"
      printf '{"id":%s,"result":{"thread":{"id":"%s"},"model":"gpt-fake","cwd":"%s"}}\n' "$id" "$thread" "$(pwd -P)" ;;
    *'"method":"turn/start"'*)
      case "$line" in
        *hang*)
          printf '{"id":%s,"result":{"turn":{"id":"turn_hang","status":"inProgress","items":[]}}}\n' "$id"
          printf '{"method":"turn/started","params":{"threadId":"thr_fake","turn":{"id":"turn_hang","status":"inProgress","items":[]}}}\n' ;;
        *)
          printf '{"id":%s,"result":{"turn":{"id":"turn_fake","status":"inProgress","items":[]}}}\n' "$id"
          case "$line" in
            *approve*)
              printf '{"id":99,"method":"item/commandExecution/requestApproval","params":{"threadId":"thr_fake","turnId":"turn_fake","itemId":"item_9","command":"make deploy"}}\n'
              IFS= read -r reply
              printf '{"method":"fake/request","params":%s}\n' "$reply" ;;
          esac
          cat "$here/codex-turn.jsonl" ;;
      esac ;;
    *'"method":"turn/interrupt"'*)
      printf '{"id":%s,"result":{}}\n' "$id"
      printf '{"method":"turn/completed","params":{"threadId":"thr_fake","turn":{"id":"turn_hang","items":[],"status":"interrupted","error":null}}}\n' ;;
  esac
done
