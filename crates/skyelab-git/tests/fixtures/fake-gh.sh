#!/bin/sh
# Stand-in for `gh` (tests and SKYELAB_GH_BIN). Never touches the network. Records each call in the
# GH_CONFIG_DIR it was given, so a missing or wrong dir shows up as a missing record:
#   fake-gh.args  one arg per line     fake-gh.env  its GH_/GITHUB_/GIT_ env     fake-gh.body  stdin of `pr create`
#   fake-gh.fullenv  its whole env, from the last call
#   fake-gh.prview  one line per `pr view` call (its args)
#   fake-gh.list  one line per `pr list` / `issue list` call: its args, cwd, GH_CONFIG_DIR and whether GH_TOKEN was set
# Logged in when "$GH_CONFIG_DIR/fake-login" exists (its content is the login). `pr view` prints
# "$GH_CONFIG_DIR/fake-pr.json" when it exists, else an open PR whose one check passed. `pr list` / `issue list`
# print "$GH_CONFIG_DIR/fake-prs.json" / "fake-issues.json" when they exist, else a short default list; a
# "$GH_CONFIG_DIR/fake-hold" file makes them hang for 30 s.
#   fake-gh.actions  one line per `pr merge` / `pr review` call (its args)     fake-gh.review-body  stdin of `pr review`
#   fake-gh.repoview  one line per `repo view` call (its args)
# A "$GH_CONFIG_DIR/fake-action-fail" file makes `pr merge` and `pr review` fail (exit 1) with its text on stderr. A
# merge that succeeds leaves "fake-merged", and `pr view` then answers MERGED (over fake-pr.json too). `repo view` prints
# "$GH_CONFIG_DIR/fake-repo.json" when it exists, else a repo allowing every merge method.
# The PR's head commit is the content of "$GH_CONFIG_DIR/fake-head", else 40 1s; the default `pr view` and `pr list`
# answers report it as headRefOid, and `pr merge` fails unless its --match-head-commit is that sha.
# `--version` needs no config dir (onboarding's agent list asks it with none).
if [ "$1" = "--version" ]; then
  echo "gh version 2.99.0 (fake)"
  exit 0
fi
dir="${GH_CONFIG_DIR:?fake gh: no GH_CONFIG_DIR}"
env > "$dir/fake-gh.fullenv"
head_sha=$(cat "$dir/fake-head" 2>/dev/null || echo 1111111111111111111111111111111111111111)
case "$1 $2" in
"auth status")
  if [ -f "$dir/fake-login" ]; then
    printf '{"hosts":{"github.com":[{"active":true,"host":"github.com","login":"%s","state":"success"}]}}\n' "$(cat "$dir/fake-login")"
  else
    echo '{"hosts":{}}'
  fi
  ;;
"auth git-credential")
  printf '%s\n' "$@" > "$dir/fake-gh.credential"
  cat > /dev/null
  printf 'username=x-access-token\npassword=fake-token\n'
  ;;
"pr view")
  echo "$*" >> "$dir/fake-gh.prview"
  # A slow read: "fake-view-delay" holds the seconds to wait before answering.
  [ -f "$dir/fake-view-delay" ] && sleep "$(cat "$dir/fake-view-delay")"
  if [ ! -f "$dir/fake-login" ]; then
    echo "To get started with GitHub CLI, please run:  gh auth login" >&2
    exit 4
  fi
  if [ -f "$dir/fake-merged" ]; then
    echo '{"state":"MERGED","isDraft":false,"mergedAt":"2026-11-24T09:00:00Z","reviewDecision":"","statusCheckRollup":[]}'
  elif [ -f "$dir/fake-pr.json" ]; then
    cat "$dir/fake-pr.json"
  else
    echo '{"state":"OPEN","isDraft":false,"mergedAt":null,"reviewDecision":"","headRefOid":"'"$head_sha"'","statusCheckRollup":[{"__typename":"CheckRun","name":"build","status":"COMPLETED","conclusion":"SUCCESS","detailsUrl":"https://github.com/acme/demo/actions/runs/1"}]}'
  fi
  ;;
"pr list" | "issue list")
  echo "$* cwd=$(pwd) cfg=$GH_CONFIG_DIR token=${GH_TOKEN:+set}" >> "$dir/fake-gh.list"
  if [ ! -f "$dir/fake-login" ]; then
    echo "To get started with GitHub CLI, please run:  gh auth login" >&2
    exit 4
  fi
  # A hung gh, for timeout tests.
  [ -f "$dir/fake-hold" ] && sleep 30
  if [ "$1" = pr ]; then fixture="$dir/fake-prs.json"; else fixture="$dir/fake-issues.json"; fi
  if [ -f "$fixture" ]; then
    cat "$fixture"
  elif [ "$1" = pr ]; then
    echo '[{"number":12,"title":"Add the release workflow","url":"https://github.com/acme/demo/pull/12","author":{"login":"ada"},"isDraft":false,"reviewDecision":"APPROVED","headRefOid":"'"$head_sha"'","statusCheckRollup":[{"__typename":"CheckRun","name":"build","status":"COMPLETED","conclusion":"SUCCESS"}],"updatedAt":"2026-11-23T09:00:00Z"},{"number":15,"title":"Draft: rework the sidebar","url":"https://github.com/acme/demo/pull/15","author":{"login":"bob"},"isDraft":true,"reviewDecision":"","statusCheckRollup":[],"updatedAt":"2026-11-22T09:00:00Z"}]'
  else
    echo '[{"number":31,"title":"Crash when the folder is renamed","url":"https://github.com/acme/demo/issues/31","author":{"login":"bob"},"labels":[{"name":"bug"}],"assignees":[{"login":"ada"}],"updatedAt":"2026-11-23T08:00:00Z","body":"Steps: rename the folder while a thread runs."}]'
  fi
  ;;
"pr merge" | "pr review")
  echo "$*" >> "$dir/fake-gh.actions"
  [ "$2" = review ] && cat > "$dir/fake-gh.review-body"
  if [ ! -f "$dir/fake-login" ]; then
    echo "To get started with GitHub CLI, please run:  gh auth login" >&2
    exit 4
  fi
  if [ -f "$dir/fake-action-fail" ]; then
    cat "$dir/fake-action-fail" >&2
    exit 1
  fi
  if [ "$2" = merge ]; then
    pinned=""
    prev=""
    for a in "$@"; do
      [ "$prev" = "--match-head-commit" ] && pinned="$a"
      prev="$a"
    done
    if [ "$pinned" != "$head_sha" ]; then
      echo "Head branch was modified. Review and try the merge again." >&2
      exit 1
    fi
    : > "$dir/fake-merged"
  fi
  exit 0
  ;;
"repo view")
  echo "$*" >> "$dir/fake-gh.repoview"
  if [ -f "$dir/fake-repo.json" ]; then
    cat "$dir/fake-repo.json"
  else
    echo '{"mergeCommitAllowed":true,"squashMergeAllowed":true,"rebaseMergeAllowed":true}'
  fi
  ;;
"pr create")
  printf '%s\n' "$@" > "$dir/fake-gh.args"
  env | grep -E '^(GH_|GITHUB_|GIT_)' > "$dir/fake-gh.env"
  cat > "$dir/fake-gh.body"
  head=""
  prev=""
  for a in "$@"; do
    [ "$prev" = "--head" ] && head="$a"
    prev="$a"
  done
  echo "Creating pull request for $head into main in acme/demo"
  echo
  echo "https://github.com/acme/demo/pull/1#$head"
  ;;
*)
  echo "fake gh: unexpected $*" >&2
  exit 2
  ;;
esac
