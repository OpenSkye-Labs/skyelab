#!/usr/bin/env bash
# Build, install and launch on an iOS Simulator. Usage: scripts/run-ios.sh [simulator-udid] (default: booted).
# SKYELAB_MOBILE_FEATURES=demo builds with canned data instead of a real environment.
# SKYELAB_IOS_PROFILE=release runs a release build.
set -euo pipefail
cd "$(dirname "$0")/.."
[[ "${1:-}" != -* ]] || { echo "usage: scripts/run-ios.sh [simulator-udid]" >&2; exit 2; }
app=$(scripts/build-ios.sh "${SKYELAB_IOS_PROFILE:-debug}" simulator)
xcrun simctl install "${1:-booted}" "$app"
xcrun simctl launch "${1:-booted}" dev.openskye.skyelab
