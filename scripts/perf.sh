#!/usr/bin/env bash
# Repeatable performance runs (docs/debugging.md, "Performance runs"). Builds the `profiling` profile with debug
# assertions (so the fake agent is honored), makes a fixture HOME with examples/perf_fixture.rs (40 projects,
# 600 threads, one thread of ~7,000 events left open), runs one scripted scenario and prints what it measured:
# launch milestones, frame times read back from the SKYELAB_TRACE trace, and the busiest views' renders. Debug mode
# is on through the fixture's Developer mode, so frames are timed but no overlay ticks.
#
#   scripts/perf.sh open        launch on the long thread, scroll up through it, quit
#   scripts/perf.sh stream      launch on the long thread, send "stream" (1,500 deltas), quit when it ends
#   scripts/perf.sh search      type a query into the sidebar search, a key at a time, then clear it
#   scripts/perf.sh startup     launch and quit twice on the same HOME: cold, then warm
#   scripts/perf.sh idle        launch, wait 10 s, quit (frames drawn while idle; the overlay is off)
#   scripts/perf.sh leaks       open 48 threads, 20 terminals, settings 20 times, the gallery 5 times, Dump Debug
#                               State after each round (live counts and memory_mb in home/.../skyelab-dev/debug/state-*.json)
#   PERF_SAMPLY=1 scripts/perf.sh open     also record a samply CPU profile of the run
#   PERF_FEATURES=dhat-heap scripts/perf.sh open   build with extra features (hotpath, dhat-heap, ...)
#
# Everything lands in target/perf/<scenario>-<time>/: home/, trace.json, stderr.txt, profile.json.gz.
set -euo pipefail
cd "$(dirname "$0")/.."
scenario="${1:?usage: scripts/perf.sh open|stream|search|startup|idle|leaks}"
here="$PWD"
fx="$here/crates/skyelab-app/tests/fixtures"

export CARGO_PROFILE_PROFILING_DEBUG_ASSERTIONS=true
features=()
[ -n "${PERF_FEATURES:-}" ] && features=(--features "$PERF_FEATURES")
cargo build -q --profile profiling -p skyelab-app ${features[@]+"${features[@]}"} --bin skyelab --example perf_fixture
bin="$here/target/profiling/skyelab"

out="$here/target/perf/$scenario-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out"
"$here/target/profiling/examples/perf_fixture" "$out/home" ${PERF_FIXTURE_ARGS:-}

# The window opens at x 80, y 60, 1400 x 900 (the fixture's settings.toml); points below are window points: the
# composer at 700,722, the sidebar search at 80,66. The thread opens at its first turn, so `open` scrolls down.
wait_s() { local w=""; for _ in $(seq 1 "$1"); do w+=",wait"; done; printf '%s' "${w#,}"; }
case "$scenario" in
  open) actions="$(wait_s 3)$(for _ in $(seq 1 15); do printf ',scroll:700;450;-60'; done),$(wait_s 1)" ;;
  stream) actions="$(wait_s 2),click:700;722,keys:s t r e a m cmd-enter,$(wait_s 25)" ;;
  search) actions="$(wait_s 2),click:80;66,$(for k in p a r s e r; do printf 'keys:%s,' "$k"; done)keys:backspace backspace backspace backspace backspace backspace,$(wait_s 1)" ;;
  idle) actions="$(wait_s 6)" ;;
  leaks)
    # Dump Debug State, then each kind of view opened and left again; a dump after each round. Sidebar rows sit
    # 87 points apart from y 134; a wheel scroll brings the next ones up.
    d=skyelab::DumpDebugState
    actions="$(wait_s 2),$d"
    for _ in $(seq 1 6); do
      for k in 0 1 2 3 4 5 6 7; do actions+=",click:100;$((134 + 87 * k))"; done
      actions+=",scroll:100;400;-9"
    done
    actions+=",$d"
    for _ in $(seq 1 20); do actions+=",skyelab::NewTerminal,keys:cmd-w"; done
    actions+=",$d"
    for _ in $(seq 1 20); do actions+=",skyelab::OpenSettings,skyelab::GoBack"; done
    actions+=",$d"
    for _ in $(seq 1 5); do actions+=",skyelab::OpenUiGallery,window:1,keys:cmd-w,window:0"; done
    actions+=",$d" ;;
  startup) actions="$(wait_s 2)" ;;
  *) echo "unknown scenario $scenario" >&2; exit 2 ;;
esac
actions+=",skyelab::Quit"

run() {
  local trace="$1"
  local cmd=("$bin")
  if [ -n "${PERF_SAMPLY:-}" ]; then
    cmd=(samply record --save-only --unstable-presymbolicate -o "$out/profile.json.gz" "$bin")
  fi
  HOME="$out/home" SKYELAB_VERIFY_TOPMOST=1 SKYELAB_VERIFY_ACTIONS="$actions" \
    SKYELAB_TRACE="$trace" SKYELAB_DEBUG_RENDERS=1 \
    SKYELAB_CLAUDE_BIN="$fx/fake-claude.sh" SKYELAB_GH_BIN="$here/crates/skyelab-git/tests/fixtures/fake-gh.sh" \
    SKYELAB_TAILSCALE_BIN="$fx/fake-tailscale.sh" \
    "${cmd[@]}" 2>>"$out/stderr.txt" || true
}

report() {
  local trace="$1"
  echo "== $scenario ($(basename "$trace"))"
  grep -E 'after launch|slow frame' "$out/stderr.txt" | sed 's/^/  /' | tail -n 20 || true
  jq -r '
    [.[] | select(.cat == "skyelab::frame") | .args.ms | tonumber] as $f
    | ($f | sort) as $s
    | if ($s | length) == 0 then "  no frames in the trace" else
      "  frames \($s | length), p50 \($s[($s | length) / 2 | floor] * 10 | round / 10) ms, p95 \($s[($s | length) * 0.95 | floor] * 10 | round / 10) ms, max \($s[-1] * 10 | round / 10) ms, over 16 ms: \([$s[] | select(. > 16)] | length)"
      end' "$trace"
  # How frame time grows with the reply: p50 per 10 s of the run (flat means a delta costs the same at any length).
  [ "$scenario" = stream ] && jq -r '
    [.[] | select(.cat == "skyelab::frame") | {t: .ts, ms: (.args.ms | tonumber)}] | sort_by(.t)
    | (.[0].t) as $t0 | group_by(((.t - $t0) / 10000000) | floor)
    | map((map(.ms) | sort) as $s | "\($s[($s | length) / 2 | floor] * 10 | round / 10)") | "  p50 per 10 s: " + join(" / ") + " ms"' "$trace"
  jq -r '[.[] | select(.ph == "B" and .name == "render") | .args.view] | group_by(.) | map({v: .[0], n: length}) | sort_by(-.n) | .[:8] | map("\(.v)=\(.n)") | "  renders: " + join(" ")' "$trace"
}

if [ "$scenario" = startup ]; then
  sed -i '' '/^view = /d' "$out/home/Library/Application Support/skyelab-dev/settings.toml"
  run "$out/trace-cold.json"; report "$out/trace-cold.json"
  : > "$out/stderr.txt"
  run "$out/trace-warm.json"; report "$out/trace-warm.json"
else
  run "$out/trace.json"; report "$out/trace.json"
fi
for f in "$out"/home/Library/Application\ Support/skyelab-dev/debug/state-*.json; do
  [ -f "$f" ] && jq -c '{memory_mb, live, children: (.child_processes | length)}' "$f"
done
echo "  output: $out"
