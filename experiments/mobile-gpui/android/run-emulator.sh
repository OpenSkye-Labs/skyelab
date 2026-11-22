#!/usr/bin/env bash
# Build the cdylib, package it into an APK with gpui-mobile's Java host (GpuiActivity), install and launch.
# Usage: android/run-emulator.sh [adb-serial]
set -euo pipefail
cd "$(dirname "$0")/.."
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
android/fetch-gpui-mobile.sh
gradle=target/android-gradle
jni=$gradle/app/src/main/jniLibs
if [[ ! -d $gradle ]]; then
    cp -R target/gpui-mobile/example/android/gradle "$gradle"
    sed -i '' 's/"gpui_mobile_example"/"skyelab_mobile"/; s/libgpui_mobile_example/libskyelab_mobile/g; s/dev.gpui.mobile.example"/dev.skyelab.mobile_spike"/' "$gradle/app/build.gradle.kts"
    rm -rf "$jni"
fi
cargo ndk -t arm64-v8a -P 26 -o "$jni" build --lib
(cd "$gradle" && ./gradlew -q assembleDebug)
adb ${1:+-s "$1"} install -r "$gradle/app/build/outputs/apk/debug/app-debug.apk"
adb ${1:+-s "$1"} shell am start -n dev.skyelab.mobile_spike/dev.gpui.mobile.GpuiActivity
