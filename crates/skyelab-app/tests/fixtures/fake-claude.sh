#!/bin/sh
# Stand-in for `claude` (tests and SKYELAB_CLAUDE_BIN): replays a recorded session, reports the
# account dir and args it got, then echoes every stdin line so tests can see what was sent.
if [ -f .fake-agent-commits ]; then
  # An agent that commits its work, for tests of the Changes diff base.
  printf 'by the agent\n' > agent-commit.txt
  git add agent-commit.txt >/dev/null 2>&1
  git -c user.name=fake -c user.email=fake@example.com -c commit.gpgsign=false commit -q -m "agent commit" >/dev/null 2>&1
fi
cat "$(dirname "$0")/session.jsonl"
printf '{"type":"fake_env","config_dir":"%s","cwd":"%s","args":"%s"}\n' "$CLAUDE_CONFIG_DIR" "$(pwd -P)" "$*"
while IFS= read -r line; do printf '%s\n' "$line"; done
