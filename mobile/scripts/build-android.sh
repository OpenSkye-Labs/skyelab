#!/usr/bin/env bash
# Build the APK: the cdylib with cargo-ndk, then our Gradle host (android-host/). Prints the APK's path.
# Usage: scripts/build-android.sh [debug|release]. Needs cargo-ndk, the Android SDK + NDK and JDK 17+.
# Release is unsigned unless all four SKYELAB_ANDROID_KEYSTORE{,_PASSWORD} / SKYELAB_ANDROID_KEY_{ALIAS,PASSWORD}
# are set (android-host/app/build.gradle.kts). Never commit a keystore or its passwords.
# SKYELAB_MOBILE_FEATURES=demo builds with canned data; it is refused for a signed build.
set -euo pipefail
cd "$(dirname "$0")/.."
profile=${1:-debug}
case $profile in
debug) cargo_profile=() ;;
release) cargo_profile=(--release) ;;
*) echo "usage: scripts/build-android.sh [debug|release]" >&2; exit 2 ;;
esac
if [[ ${SKYELAB_MOBILE_FEATURES:-} == *demo* && -n ${SKYELAB_ANDROID_KEYSTORE:-} ]]; then
    echo "build-android: refusing to sign a demo build" >&2; exit 2
fi
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
scripts/fetch-gpui-mobile.sh
# -P 26 must match minSdk in android-host/app/build.gradle.kts (scripts/check-hosts.sh checks both).
cargo ndk -t arm64-v8a -P 26 -o "target/jniLibs/$profile" build --locked ${cargo_profile[@]+"${cargo_profile[@]}"} -p skyelab-mobile-android \
    ${SKYELAB_MOBILE_FEATURES:+--features "$SKYELAB_MOBILE_FEATURES"} >&2
task=assemble$(tr '[:lower:]' '[:upper:]' <<<"${profile:0:1}")${profile:1}
(cd android-host && ./gradlew -q "$task" >&2)
# Signed release: app-release.apk; unsigned: app-release-unsigned.apk. An older one of the other kind may remain.
apk=$(ls -t target/android-host/app/outputs/apk/"$profile"/app-"$profile"*.apk | head -1)
echo "$PWD/$apk"
