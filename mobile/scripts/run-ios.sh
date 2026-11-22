#!/usr/bin/env bash
# Build, bundle and launch on an iOS Simulator. Usage: scripts/run-ios.sh [simulator-udid] (default: booted).
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/fetch-gpui-mobile.sh # cargo resolves the Android-only path dependency on every target
IPHONEOS_DEPLOYMENT_TARGET=15.0 cargo build -p skyelab-mobile --bin skyelab_mobile --target aarch64-apple-ios-sim
app=target/SkyeLab.app
mkdir -p "$app"
cp target/aarch64-apple-ios-sim/debug/skyelab_mobile ios/Info.plist "$app/"
codesign --force --sign - "$app"
xcrun simctl install "${1:-booted}" "$app"
xcrun simctl launch "${1:-booted}" dev.skyelab.mobile
