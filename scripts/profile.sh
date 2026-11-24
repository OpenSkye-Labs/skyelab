#!/usr/bin/env bash
# Records a CPU profile (flame graph) of SkyeLab. See docs/debugging.md.
#
# Builds the `profiling` cargo profile (release code with symbols), then runs the app under samply (preferred) or
# cargo-flamegraph in a throwaway HOME, so your accounts.toml and data are never touched. Use the app, quit it
# (cmd-q or the menu), and the script prints where the profile is. Neither tool is needed to build SkyeLab.
#
#   scripts/profile.sh                 fake agents, gh and tailscale (needs debug assertions on: see below)
#   scripts/profile.sh --real          plain release semantics, no fakes (no account, so no agent runs)
#   scripts/profile.sh --tool flamegraph
#   SKYELAB_VERIFY_ACTIONS='…,skyelab::Quit' scripts/profile.sh    a scripted, repeatable run that quits itself
#
# The fakes are honored only in builds with debug assertions, so the default run turns them on for this profile
# (debug_assert! checks then cost a little). Use --real when that matters.
set -euo pipefail
cd "$(dirname "$0")/.."

tool=""
fakes=1
while [ $# -gt 0 ]; do
  case "$1" in
    --real) fakes=0 ;;
    --tool) tool="${2:?--tool needs samply or flamegraph}"; shift ;;
    -h|--help) sed -n 2,15p "$0"; exit 0 ;;
    *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

if [ -z "$tool" ]; then
  if command -v samply >/dev/null 2>&1; then
    tool=samply
  elif command -v flamegraph >/dev/null 2>&1; then
    tool=flamegraph
  else
    echo "No profiler found. Install one (user-level, into ~/.cargo/bin):" >&2
    echo "  cargo install samply --locked        # macOS and Linux, opens in the Firefox Profiler" >&2
    echo "  cargo install flamegraph --locked    # writes an SVG; uses dtrace (sudo) on macOS, perf on Linux" >&2
    exit 1
  fi
fi

if [ "$fakes" = 1 ]; then
  export CARGO_PROFILE_PROFILING_DEBUG_ASSERTIONS=true
fi
cargo build --profile profiling -p skyelab-app
bin="$PWD/target/profiling/skyelab"

out="${SKYELAB_PROFILE_DIR:-$PWD/target/profiles}/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out/home"
export HOME="$out/home"
export SKYELAB_DEBUG=1
if [ "$fakes" = 1 ]; then
  export SKYELAB_CLAUDE_BIN="$PWD/crates/skyelab-app/tests/fixtures/fake-claude.sh"
  export SKYELAB_CODEX_BIN="$PWD/crates/skyelab-agent/tests/fixtures/fake-codex.sh"
  export SKYELAB_GH_BIN="$PWD/crates/skyelab-git/tests/fixtures/fake-gh.sh"
  export SKYELAB_TAILSCALE_BIN="$PWD/crates/skyelab-app/tests/fixtures/fake-tailscale.sh"
fi

echo "Profiling $bin with $tool (HOME=$HOME). Use the app, then quit it to finish."
case "$tool" in
  samply)
    samply record --save-only -o "$out/profile.json.gz" "$bin"
    echo "Profile: $out/profile.json.gz"
    echo "Open it: samply load $out/profile.json.gz"
    ;;
  flamegraph)
    flamegraph -o "$out/flamegraph.svg" -- "$bin"
    echo "Flame graph: $out/flamegraph.svg (open it in a browser)"
    ;;
  *) echo "unknown tool: $tool" >&2; exit 2 ;;
esac
