#!/usr/bin/env bash
# Acceptance checks for desktop packaging (macOS host). Usage: packaging.sh [icons|plist|profile|start [bin]|workflow|all]
set -euo pipefail
cd "$(dirname "$0")/../.."
fail() { echo "FAIL: $*" >&2; exit 1; }

icons() {
  local d=crates/skyelab-app/icons
  local stamp; stamp=$(mktemp); sleep 1
  scripts/icons.sh >/dev/null
  for pair in 32x32.png:32 128x128.png:128 128x128@2x.png:256 256x256.png:256 512x512.png:512 icon.icns:1024; do
    local f="$d/${pair%%:*}" px="${pair##*:}"
    [ "$f" -nt "$stamp" ] || fail "$f not regenerated"
    [ "$(sips -g pixelWidth "$f" | awk '/pixelWidth/ {print $2}')" = "$px" ] || fail "$f is not ${px}px"
  done
  [ "$d/icon.ico" -nt "$stamp" ] || fail "icon.ico not regenerated"
  rm -f "$stamp"
  # ICO header: reserved 0, type 1.
  [ "$(head -c 4 "$d/icon.ico" | od -An -tx1 | tr -d ' \n')" = "00000100" ] || fail "icon.ico is not an ICO"
  echo "icons ok"
}

plist() {
  local app=target/release/SkyeLab.app sig
  local p=$app/Contents/Info.plist
  [ -f "$p" ] || fail "no $p (run cargo packager --release -p skyelab-app -f app,dmg)"
  read_key() { /usr/libexec/PlistBuddy -c "Print :$1" "$p"; }
  [ "$(read_key CFBundleIdentifier)" = dev.openskye.skyelab ] || fail "bundle id"
  [ "$(read_key LSMinimumSystemVersion)" = 14.0 ] || fail "min macOS"
  [ "$(read_key NSHighResolutionCapable)" = true ] || fail "NSHighResolutionCapable"
  read_key LSApplicationCategoryType | grep -q '^public.app-category\.' || fail "category"
  read_key NSMicrophoneUsageDescription | grep -q . || fail "NSMicrophoneUsageDescription"
  read_key NSSpeechRecognitionUsageDescription | grep -q . || fail "NSSpeechRecognitionUsageDescription"
  local icon; icon=$(read_key CFBundleIconFile)
  [ -s "$app/Contents/Resources/$icon" ] || fail "icon file $icon"
  codesign --verify --deep --strict "$app" || fail "SkyeLab.app signature doesn't verify"
  sig=$(codesign -dv "$app" 2>&1)
  grep -qx 'Identifier=dev.openskye.skyelab' <<<"$sig" || fail "signature identifier"
  grep -q 'flags=.*runtime' <<<"$sig" || fail "no hardened runtime"
  codesign -d --entitlements - --xml "$app" 2>/dev/null | grep -q 'com.apple.security.device.audio-input' || fail "audio-input entitlement"
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

start() {
  local bin=${1:-target/release/skyelab} home
  [ -x "$bin" ] || fail "no $bin"
  home=$(mktemp -d)
  env -u CLAUDE_CONFIG_DIR -u XDG_CONFIG_HOME HOME="$home" "$bin" & pid=$!
  trap 'kill "$pid" 2>/dev/null' EXIT
  sleep 5
  kill -0 "$pid" 2>/dev/null || fail "$bin exited early"
  echo "start ok ($bin, HOME=$home)"
}

workflow() {
  local w=.github/workflows/release.yml
  actionlint "$w" || fail "actionlint"
  grep -q 'tags: \["v\*"\]' "$w" || fail "no v* tag trigger"
  for os in macos windows ubuntu; do grep -q "os: $os-" "$w" || fail "no $os runner"; done
  grep -q 'draft: true' "$w" || fail "release not draft"
  grep -q 's/^signing-identity = ' "$w" || fail "the Developer ID step must replace the ad-hoc identity, not add one"
  grep -q "sha256sum -- \* > ../SHA256SUMS" "$w" || fail "no SHA256SUMS for the in-app updater"
  # Only the detect step and signing/package steps may hold secrets: none at job level.
  ! sed -n '/^    runs-on:/,/^    steps:/p' "$w" | grep -q 'secrets\.' || fail "secrets in job-level env"
  local gate; gate=$(sed -n '/id: signing/,/- name: Enable macOS/p' "$w" | grep -v 'secrets\.')
  for s in $(grep -o 'secrets\.[A-Z_]*' "$w" | cut -d. -f2 | sort -u); do
    grep -q "$s" CLAUDE.md || fail "$s not documented"
    grep -qw "$s" <<<"$gate" || fail "$s gates no step"
  done
  grep -q '^## Release' CLAUDE.md || fail "no Release section"
  echo "workflow ok"
}

case "${1:-all}" in
  icons) icons ;; plist) plist ;; profile) profile ;; start) start "${2:-}" ;; workflow) workflow ;;
  all) icons; profile; plist; workflow ;;
  *) fail "unknown check $1" ;;
esac
