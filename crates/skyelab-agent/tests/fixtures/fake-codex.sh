#!/bin/sh
# `--version` prints a fixed version (the Welcome page's agent list).
[ "$1" = --version ] && { echo 'codex-cli 0.99.0'; exit 0; }
# Stand-in for `codex app-server` (tests and SKYELAB_CODEX_BIN). Speaks just enough of the app-server
# protocol: answers initialize, thread/start and thread/resume, replays codex-turn.jsonl for each
# turn/start, and ends a turn as interrupted on turn/interrupt. Every line it reads comes back as a
# `fake/request` notification, and thread/start/resume also report the env and args as `fake/env`.
# A folder with a .fake-codex-refuse-open file gets its thread/start refused after a 1 s pause.
# A folder with a .fake-session-gone file has no rollouts: thread/resume is refused as real codex refuses it.
# A prompt containing "hang" starts a turn that only an interrupt ends; "approve" first asks for an approval;
# one starting "usage" first reports rate limits (5h at 47%, weekly at 83%).
# model/list lists one model (gpt-fake, low/medium/high); turn/steer is accepted.
here=$(dirname "$0")
# `codex exec` (a one-shot call): logs CODEX_HOME, its args and stdin to .fake-title-call, answers in the
# --output-last-message file (a commit message or PR for those prompts, else a title).
if [ "$1" = exec ]; then
  input=$(cat)
  out=
  prev=
  for a in "$@"; do
    [ "$prev" = --output-last-message ] && out=$a
    prev=$a
  done
  {
    printf 'codex_home=%s\n' "${CODEX_HOME-<unset>}"
    for a in "$@"; do printf 'arg=[%s]\n' "$a"; done
    printf '%s\n---\n' "$input"
  } >> .fake-title-call
  case "$input" in
    "Write a git commit message"*) printf 'fix(codex): commit from codex\n' > "$out" ;;
    "Write a pull request title"*) printf 'fix(codex): pull request\n\nCodex PR body.\n' > "$out" ;;
    *) printf 'Codex fake title\n' > "$out" ;;
  esac
  exit 0
fi
v() { eval "printf '%s' \"\${$1-<unset>}\""; }
while IFS= read -r line; do
  printf '{"method":"fake/request","params":%s}\n' "$line"
  id=$(printf '%s' "$line" | sed -n 's/^{"id":\([0-9]*\),.*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"id":%s,"result":{"userAgent":"fake-codex/0"}}\n' "$id" ;;
    *'"method":"model/list"'*)
      printf '{"id":%s,"result":{"data":[{"model":"gpt-fake","displayName":"GPT Fake","isDefault":true,"defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"low"},{"reasoningEffort":"medium"},{"reasoningEffort":"high"}]}],"nextCursor":null}}\n' "$id" ;;
    *'"method":"turn/steer"'*) printf '{"id":%s,"result":{"turnId":"turn_hang"}}\n' "$id" ;;
    *'"method":"thread/start"'*|*'"method":"thread/resume"'*)
      if [ -f .fake-codex-refuse-open ]; then
        sleep 1
        printf '{"id":%s,"error":{"code":-32600,"message":"no rollout found for thread"}}\n' "$id"
        continue
      fi
      thread=$(printf '%s' "$line" | sed -n 's/.*"threadId":"\([^"]*\)".*/\1/p')
      if [ -f .fake-session-gone ] && [ -n "$thread" ]; then
        printf '{"id":%s,"error":{"code":-32600,"message":"no rollout found for thread id %s"}}\n' "$id" "$thread"
        continue
      fi
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
            *'"text":"usage'*)
              now=$(date +%s)
              printf '{"method":"account/rateLimits/updated","params":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":47,"windowDurationMins":300,"resetsAt":%s},"secondary":{"usedPercent":83,"windowDurationMins":10080,"resetsAt":%s},"rateLimitReachedType":null}}}\n' "$((now + 7200))" "$((now + 300000))" ;;
          esac
          cat "$here/codex-turn.jsonl" ;;
      esac ;;
    *'"method":"turn/interrupt"'*)
      printf '{"id":%s,"result":{}}\n' "$id"
      printf '{"method":"turn/completed","params":{"threadId":"thr_fake","turn":{"id":"turn_hang","items":[],"status":"interrupted","error":null}}}\n' ;;
  esac
done
