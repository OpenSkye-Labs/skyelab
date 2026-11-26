#!/bin/sh
# `--version` prints a fixed version (the Welcome page's agent list).
[ "$1" = --version ] && { echo 'codex-cli 0.99.0'; exit 0; }
# Stand-in for `codex app-server` (tests and SKYELAB_CODEX_BIN). Speaks just enough of the app-server
# protocol: answers account/rateLimits/read from CODEX_HOME's fake-usage.json (else signed out), initialize, thread/start and thread/resume, replays codex-turn.jsonl for each
# turn/start, and ends a turn as interrupted on turn/interrupt. Every line it reads comes back as a
# `fake/request` notification, and thread/start/resume also report the env and args as `fake/env`.
# A folder with a .fake-codex-refuse-open file gets its thread/start refused after a 1 s pause.
# A folder with a .fake-session-gone file has no rollouts: thread/resume is refused as real codex refuses it.
# A prompt containing "hang" starts a turn that only an interrupt ends ("hang longcmd" also starts `sleep 40` as a
# running command item, pid in .fake-cmd-pid, which the interrupt leaves running as real codex can; a .fake-helper
# file makes thread/start first start a `sleep 60` helper, pid in .fake-helper-pid, like an MCP server); "approve" first asks for an approval;
# "approve2" asks for `cargo test --lib` and then `cargo test --doc` (in the folder), reading an answer after each;
# one starting "usage" first reports rate limits (5h at 47%, weekly at 83%); one starting "hook" first starts a
# sessionStart hook ("Reporting status to Orca") and holds it until the folder has a .fake-hook-done file.
# With a fake-mcp-status file in CODEX_HOME, thread/start/resume first reports its lines as MCP startup statuses
# (`name status [error...]`, like `docs failed no such command`).
# model/list lists one model (gpt-fake, low/medium/high); turn/steer is accepted.
# `login status` says "Logged in using ChatGPT" (exit 0) while CODEX_HOME has a fake-login file, else "Not logged in"
# (exit 1), on stderr like codex; `login` writes that file. `mcp login -- <name>` writes the name and its folder to
# CODEX_HOME/fake-mcp-login and exits 0.
[ "$1 $2 $3" = "mcp login --" ] && { echo "fake codex mcp login $4"; printf '%s\n%s\n' "$4" "$(pwd -P)" > "$CODEX_HOME/fake-mcp-login"; exit 0; }
if [ "$1 $2" = "login status" ]; then
  if [ -f "$CODEX_HOME/fake-login" ]; then echo "Logged in using ChatGPT" >&2; exit 0; fi
  echo "Not logged in" >&2
  exit 1
fi
if [ "$1" = login ] && [ $# -eq 1 ]; then
  mkdir -p "$CODEX_HOME" && touch "$CODEX_HOME/fake-login"
  echo "fake codex login for $CODEX_HOME"
  exit 0
fi
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
    *'"method":"account/rateLimits/read"'*)
      # The usage poll: CODEX_HOME's fake-usage.json is the result, else Codex's signed-out refusal.
      if [ -f "$CODEX_HOME/fake-usage.json" ]; then
        printf '{"id":%s,"result":%s}\n' "$id" "$(tr -d '\n' < "$CODEX_HOME/fake-usage.json")"
      else
        printf '{"id":%s,"error":{"code":-32600,"message":"codex account authentication required to read rate limits"}}\n' "$id"
      fi ;;
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
      if [ -f .fake-helper ]; then sleep 60 >/dev/null 2>&1 & echo $! > .fake-helper-pid; fi
      if [ -f "$CODEX_HOME/fake-mcp-status" ]; then
        while read -r name status error; do
          printf '{"method":"mcpServer/startupStatus/updated","params":{"name":"%s","status":"%s","error":%s}}\n' \
            "$name" "$status" "$([ -n "$error" ] && printf '"%s"' "$error" || printf null)"
        done < "$CODEX_HOME/fake-mcp-status"
      fi
      printf '{"method":"fake/env","params":{"codex_home":"%s","claude_config_dir":"%s","openai_api_key":"%s","openai_base_url":"%s","openai_org_id":"%s","codex_api_key":"%s","codex_access_token":"%s","cwd":"%s","args":"%s"}}\n' \
        "$(v CODEX_HOME)" "$(v CLAUDE_CONFIG_DIR)" "$(v OPENAI_API_KEY)" "$(v OPENAI_BASE_URL)" "$(v OPENAI_ORG_ID)" \
        "$(v CODEX_API_KEY)" "$(v CODEX_ACCESS_TOKEN)" "$(pwd -P)" "$(printf '%s' "$*" | sed 's/\\/\\\\/g; s/"/\\"/g')"
      printf '{"id":%s,"result":{"thread":{"id":"%s"},"model":"gpt-fake","cwd":"%s"}}\n' "$id" "$thread" "$(pwd -P)" ;;
    *'"method":"turn/start"'*)
      case "$line" in
        *hang*)
          printf '{"id":%s,"result":{"turn":{"id":"turn_hang","status":"inProgress","items":[]}}}\n' "$id"
          printf '{"method":"turn/started","params":{"threadId":"thr_fake","turn":{"id":"turn_hang","status":"inProgress","items":[]}}}\n'
          case "$line" in *longcmd*)
            sleep 1.2 # a later second than anything started with the thread
            printf '{"method":"item/started","params":{"threadId":"thr_fake","turnId":"turn_hang","item":{"type":"commandExecution","id":"item_cmd","command":"sleep 40","cwd":"%s","status":"inProgress"}}}\n' "$(pwd -P)"
            sleep 40 >/dev/null 2>&1 &
            echo $! > .fake-cmd-pid ;;
          esac ;;
        *)
          printf '{"id":%s,"result":{"turn":{"id":"turn_fake","status":"inProgress","items":[]}}}\n' "$id"
          case "$line" in
            *approve2*)
              for n in 1 2; do
                cmd=$([ $n = 1 ] && echo 'cargo test --lib' || echo 'cargo test --doc')
                printf '{"id":%s,"method":"item/commandExecution/requestApproval","params":{"threadId":"thr_fake","turnId":"turn_fake","itemId":"item_%s","command":"%s","cwd":"%s"}}\n' "$((98 + n))" "$n" "$cmd" "$(pwd -P)"
                IFS= read -r reply
                printf '{"method":"fake/request","params":%s}\n' "$reply"
              done ;;
            *approve*)
              printf '{"id":99,"method":"item/commandExecution/requestApproval","params":{"threadId":"thr_fake","turnId":"turn_fake","itemId":"item_9","command":"make deploy"}}\n'
              IFS= read -r reply
              printf '{"method":"fake/request","params":%s}\n' "$reply" ;;
            *'"text":"hook'*)
              printf '{"method":"hook/started","params":{"threadId":"thr_fake","turnId":"turn_fake","run":{"id":"session-start:0:fake-hooks.json","eventName":"sessionStart","handlerType":"command","executionMode":"sync","scope":"thread","source":"user","status":"running","statusMessage":"Reporting status to Orca","completedAt":null}}}\n'
              n=0
              while [ ! -f .fake-hook-done ] && [ $n -lt 1200 ]; do sleep 0.1; n=$((n + 1)); done
              printf '{"method":"hook/completed","params":{"threadId":"thr_fake","turnId":"turn_fake","run":{"id":"session-start:0:fake-hooks.json","eventName":"sessionStart","handlerType":"command","executionMode":"sync","scope":"thread","source":"user","status":"completed","statusMessage":"Reporting status to Orca","completedAt":1}}}\n' ;;
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
