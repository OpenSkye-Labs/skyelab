#!/usr/bin/env bash
# Build, bundle and launch the spike on the booted iOS Simulator. Usage: ios/run-simulator.sh [simulator-udid]
set -euo pipefail
cd "$(dirname "$0")/.."
android/fetch-gpui-mobile.sh # cargo resolves the Android-only path dependency on every target
IPHONEOS_DEPLOYMENT_TARGET=15.0 cargo build --bin skyelab_mobile --target aarch64-apple-ios-sim
app=target/SkyeLabSpike.app
mkdir -p "$app"
cp target/aarch64-apple-ios-sim/debug/skyelab_mobile "$app/"
cp ios/Info.plist "$app/"
codesign --force --sign - "$app"
xcrun simctl install "${1:-booted}" "$app"
xcrun simctl launch "${1:-booted}" dev.skyelab.mobile-spike
