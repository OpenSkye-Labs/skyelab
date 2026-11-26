#!/usr/bin/env bash
# Acceptance checks for desktop packaging (macOS host). Usage: packaging.sh [icons|plist|profile|start [bin]|workflow|versions|all]
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
  local w=.github/workflows/package.yml r=.github/workflows/release.yml n=.github/workflows/nightly.yml
  actionlint "$w" "$r" "$n" || fail "actionlint"
  grep -q 'tags: \["v\*", "!v\*-nightly\*"\]' "$r" || fail "no v* tag trigger (nightly tags excluded)"
  for os in macos windows ubuntu; do grep -q "os: $os-" "$w" || fail "no $os runner"; done
  grep -q 'draft: true' "$r" || fail "release not draft"
  for f in "$r" "$n"; do
    grep -q 'uses: ./.github/workflows/package.yml' "$f" || fail "$f doesn't package through package.yml"
    grep -q "sha256sum -- \* > ../SHA256SUMS" "$f" || fail "$f: no SHA256SUMS for the in-app updater"
  done
  grep -q 'prerelease: true' "$n" && grep -q 'make_latest: "false"' "$n" || fail "nightly not a pre-release"
  grep -q 'workflow_dispatch:' "$n" && ! grep -q 'schedule:' "$n" || fail "nightly runs only by hand"
  grep -q 's/^signing-identity = ' "$w" || fail "the Developer ID step must replace the ad-hoc identity, not add one"
  # Only the detect step and signing/package steps may hold secrets: none at job level.
  ! sed -n '/^    runs-on:/,/^    steps:/p' "$w" | grep -q 'secrets\.' || fail "secrets in job-level env"
  local gate; gate=$(sed -n '/id: signing/,/- name: Enable macOS/p' "$w" | grep -v 'secrets\.')
  for s in $(grep -o 'secrets\.[A-Z_]*' "$w" | cut -d. -f2 | sort -u); do
    grep -q "$s" CLAUDE.md || fail "$s not documented"
    grep -qw "$s" <<<"$gate" || fail "$s gates no step"
  done
  grep -q 'stamp-version.sh stamp "$VERSION"' "$w" && grep -q 'stamp-version.sh deb "$VERSION"' "$w" \
    || fail "package.yml must stamp the version through scripts/stamp-version.sh (Cargo, Info.plist, .deb)"
  grep -q '^## Release' CLAUDE.md || fail "no Release section"
  echo "workflow ok"
}

versions() {
  command -v dpkg-deb >/dev/null || fail "versions needs dpkg (brew install dpkg)"
  local t v=0.1.8-nightly.20261127.2; t=$(mktemp -d)
  mkdir -p "$t/crates/skyelab-app/macos" "$t/deb/DEBIAN"
  cp Cargo.toml Cargo.lock "$t/"; cp crates/skyelab-app/macos/Info.plist "$t/crates/skyelab-app/macos/"
  local stamp=$PWD/scripts/stamp-version.sh p=$t/crates/skyelab-app/macos/Info.plist
  (cd "$t" && "$stamp" stamp 0.1.9)
  cmp -s crates/skyelab-app/macos/Info.plist "$p" || fail "a release build changed Info.plist"
  (cd "$t" && "$stamp" stamp "$v")
  grep -qxF "version = \"$v\"" "$t/Cargo.toml" || fail "Cargo.toml not stamped"
  grep -qxF "version = \"$v\"" "$t/Cargo.lock" || fail "Cargo.lock not stamped"
  plutil -lint "$p" >/dev/null || fail "stamped Info.plist doesn't parse"
  [ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$p")" = 0.1.8 ] || fail "CFBundleShortVersionString"
  printf 'Package: skyelab\nVersion: %s\nArchitecture: amd64\nMaintainer: x\nDescription: x\n' "$v" > "$t/deb/DEBIAN/control"
  dpkg-deb --root-owner-group -b "$t/deb" "$t/SkyeLab_${v}_amd64.deb" >/dev/null
  "$stamp" deb "$v" "$t/SkyeLab_${v}_amd64.deb"
  local dv; dv=$(dpkg-deb -f "$t/SkyeLab_${v}_amd64.deb" Version)
  [ "$dv" = 0.1.8~nightly.20261127.2 ] || fail "deb Version $dv"
  dpkg --compare-versions "$dv" lt 0.1.8 || fail "$dv not below 0.1.8"
  dpkg --compare-versions 0.1.7 lt "$dv" || fail "$dv not above 0.1.7"
  dpkg --compare-versions 0.1.8~nightly.20261127 lt "$dv" || fail "$dv not above the day's first nightly"
  rm -rf "$t"
  echo "versions ok"
}

case "${1:-all}" in
  icons) icons ;; plist) plist ;; profile) profile ;; start) start "${2:-}" ;; workflow) workflow ;; versions) versions ;;
  all) icons; profile; plist; workflow; versions ;;
  *) fail "unknown check $1" ;;
esac
