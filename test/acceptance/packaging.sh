#!/usr/bin/env bash
# Acceptance checks for desktop packaging (macOS host). Usage: packaging.sh [icons|plist|profile|start [bin]|workflow|all]
set -euo pipefail
cd "$(dirname "$0")/../.."
fail() { echo "FAIL: $*" >&2; exit 1; }

icons() {
  local d=crates/skyelab-app/icons
  grep -qi placeholder "$d/icon.svg" || fail "icon.svg not marked placeholder"
  grep -qi placeholder "$d/README.md" || fail "icons/README.md not marked placeholder"
  # Age every output, regenerate, then require each one rewritten at its real size.
  touch -t 200001010000 "$d"/*.png "$d/icon.ico" 2>/dev/null || true
  scripts/icons.sh >/dev/null
  for pair in 32x32:32 128x128:128 128x128@2x:256 256x256:256 512x512:512 512x512@2x:1024; do
    local f="$d/${pair%%:*}.png" px="${pair##*:}"
    [ "$f" -nt "$d/icon.svg" ] || fail "$f not regenerated"
    [ "$(sips -g pixelWidth "$f" | awk '/pixelWidth/ {print $2}')" = "$px" ] || fail "$f is not ${px}px"
  done
  [ "$d/icon.ico" -nt "$d/icon.svg" ] || fail "icon.ico not regenerated"
  # ICO header: reserved 0, type 1.
  [ "$(head -c 4 "$d/icon.ico" | od -An -tx1 | tr -d ' \n')" = "00000100" ] || fail "icon.ico is not an ICO"
  echo "icons ok"
}

plist() {
  local p=target/release/SkyeLab.app/Contents/Info.plist
  [ -f "$p" ] || fail "no $p (run cargo packager --release -p skyelab-app -f app,dmg)"
  read_key() { /usr/libexec/PlistBuddy -c "Print :$1" "$p"; }
  [ "$(read_key CFBundleIdentifier)" = dev.openskye.skyelab ] || fail "bundle id"
  [ "$(read_key LSMinimumSystemVersion)" = 14.0 ] || fail "min macOS"
  [ "$(read_key NSHighResolutionCapable)" = true ] || fail "NSHighResolutionCapable"
  read_key LSApplicationCategoryType | grep -q '^public.app-category\.' || fail "category"
  local icon; icon=$(read_key CFBundleIconFile)
  [ -s "target/release/SkyeLab.app/Contents/Resources/$icon" ] || fail "icon file $icon"
  ls target/release/SkyeLab_*.dmg >/dev/null 2>&1 || fail "no dmg"
  echo "plist ok"
}

profile() {
  local prof; prof=$(sed -n '/^\[profile.release\]/,/^\[/p' Cargo.toml)
  grep -qx 'lto = "thin"' <<<"$prof" || fail "lto"
  grep -qx 'codegen-units = 1' <<<"$prof" || fail "codegen-units"
  grep -qx 'strip = true' <<<"$prof" || fail "strip"
  ! grep -q '^panic' <<<"$prof" || fail "panic changed"
  echo "profile ok"
}

# The binary stays up for 5s with a throwaway HOME.
start() {
  local bin=${1:-target/release/skyelab} home
  [ -x "$bin" ] || fail "no $bin"
  home=$(mktemp -d)
  HOME="$home" "$bin" & local pid=$!
  sleep 5
  kill -0 "$pid" 2>/dev/null || fail "$bin exited early"
  kill "$pid"
  echo "start ok ($bin, HOME=$home)"
}

workflow() {
  local w=.github/workflows/release.yml
  actionlint "$w" || fail "actionlint"
  grep -q 'tags: \["v\*"\]' "$w" || fail "no v* tag trigger"
  for os in macos windows ubuntu; do grep -q "os: $os-" "$w" || fail "no $os runner"; done
  grep -q 'draft: true' "$w" || fail "release not draft"
  # Every secret is documented in CLAUDE.md and gates a step.
  for s in $(grep -o 'secrets\.[A-Z_]*' "$w" | cut -d. -f2 | sort -u); do
    grep -q "$s" CLAUDE.md || fail "$s not documented"
    grep -q "env.$s != ''" "$w" || fail "$s gates no step"
  done
  grep -q '^## Release' CLAUDE.md || fail "no Release section"
  echo "workflow ok"
}

case "${1:-all}" in
  icons) icons ;; plist) plist ;; profile) profile ;; start) start "${2:-}" ;; workflow) workflow ;;
  all) icons; profile; plist; workflow ;;
  *) fail "unknown check $1" ;;
esac
