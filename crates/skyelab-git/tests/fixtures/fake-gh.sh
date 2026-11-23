#!/bin/sh
# Stand-in for `gh` (tests and SKYELAB_GH_BIN). Never touches the network. Records each call in the
# GH_CONFIG_DIR it was given, so a missing or wrong dir shows up as a missing record:
#   fake-gh.args  one arg per line     fake-gh.env  its GH_/GITHUB_/GIT_ env     fake-gh.body  stdin of `pr create`
#   fake-gh.fullenv  its whole env, from the last call
#   fake-gh.prview  one line per `pr view` call (its args)
# Logged in when "$GH_CONFIG_DIR/fake-login" exists (its content is the login). `pr view` prints
# "$GH_CONFIG_DIR/fake-pr.json" when it exists, else an open PR whose one check passed.
dir="${GH_CONFIG_DIR:?fake gh: no GH_CONFIG_DIR}"
env > "$dir/fake-gh.fullenv"
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
  if [ ! -f "$dir/fake-login" ]; then
    echo "To get started with GitHub CLI, please run:  gh auth login" >&2
    exit 4
  fi
  if [ -f "$dir/fake-pr.json" ]; then
    cat "$dir/fake-pr.json"
  else
    echo '{"state":"OPEN","isDraft":false,"mergedAt":null,"reviewDecision":"","statusCheckRollup":[{"__typename":"CheckRun","name":"build","status":"COMPLETED","conclusion":"SUCCESS","detailsUrl":"https://github.com/acme/demo/actions/runs/1"}]}'
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
