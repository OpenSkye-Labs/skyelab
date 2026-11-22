#!/usr/bin/env bash
# Build the cdylib, package it with gpui-mobile's Java host (GpuiActivity), install and launch.
# Usage: scripts/run-android.sh [adb-serial]. Needs cargo-ndk and the Android SDK + NDK.
set -euo pipefail
cd "$(dirname "$0")/.."
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
scripts/fetch-gpui-mobile.sh
# later (docs/mobile.md step 6): our own Gradle host cut down to GpuiActivity and text input, with a pinned
# distributionSha256Sum. Until then this borrows the example host, which asks for every plugin's permissions.
gradle=target/android-gradle
jni=$gradle/app/src/main/jniLibs
if [[ ! -d $gradle ]]; then
    cp -R target/gpui-mobile/example/android/gradle "$gradle"
    perl -pi -e 's/gpui_mobile_example/skyelab_mobile_android/g; s/"dev\.gpui\.mobile\.example"/"dev.skyelab.mobile"/g' \
        "$gradle/app/build.gradle.kts"
    rm -rf "$jni"
fi
cargo ndk -t arm64-v8a -P 26 -o "$jni" build -p skyelab-mobile-android
(cd "$gradle" && ./gradlew -q assembleDebug)
adb ${1:+-s "$1"} install -r "$gradle/app/build/outputs/apk/debug/app-debug.apk"
adb ${1:+-s "$1"} shell am start -n dev.skyelab.mobile/dev.gpui.mobile.GpuiActivity
