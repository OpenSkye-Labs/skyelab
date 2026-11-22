#!/usr/bin/env bash
# Build with our Gradle host, install and launch. Usage: scripts/run-android.sh [adb-serial].
# SKYELAB_MOBILE_FEATURES=demo builds with canned data instead of a real environment.
# Debug by default; SKYELAB_ANDROID_PROFILE=release runs a release build (it must be signed to install).
set -euo pipefail
cd "$(dirname "$0")/.."
[[ "${1:-}" != -* ]] || { echo "usage: scripts/run-android.sh [adb-serial]" >&2; exit 2; }
apk=$(scripts/build-android.sh "${SKYELAB_ANDROID_PROFILE:-debug}")
adb ${1:+-s "$1"} install -r "$apk"
adb ${1:+-s "$1"} shell am start -n dev.openskye.skyelab/.SkyeLabActivity
