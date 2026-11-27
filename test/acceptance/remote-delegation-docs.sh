#!/bin/bash
# Criterion 18: the delegation-across-machines docs exist and the old "Not yet" line is gone.
set -u
cd "$(dirname "$0")/../.."
fail=0
check() { if ! "$@"; then echo "FAIL: $*"; fail=1; fi; }
check test -f docs/remote-delegation.md
check bash -c '! grep -q "Not yet:\*\* delegation to other machines" docs/orchestration.md'
check grep -q "remote-delegation.md" docs/orchestration.md
check grep -q '`delegate`' docs/remote.md
check grep -q "Let my agents delegate here" docs/user-guide-remote.md
check grep -q "user-guide-remote.md" docs/user-guide.md
check grep -qi "delegat" README.md
for f in docs/remote-delegation.md docs/orchestration.md docs/remote.md docs/user-guide.md docs/user-guide-remote.md; do
  [ "$(wc -c < "$f")" -le 20000 ] || { echo "FAIL: $f is over 20000 bytes"; fail=1; }
done
[ $fail -eq 0 ] && echo "remote-delegation-docs: ok"
exit $fail
