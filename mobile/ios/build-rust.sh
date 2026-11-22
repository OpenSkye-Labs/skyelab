#!/usr/bin/env bash
# Xcode's "Build Rust" phase: builds skyelab_mobile with cargo and puts it in the bundle as the executable.
# Runs cargo in a clean environment: Xcode's SDKROOT (the iOS SDK) would break host build scripts.
set -euo pipefail
case $PLATFORM_NAME in
iphonesimulator) triple=aarch64-apple-ios-sim ;;
iphoneos) triple=aarch64-apple-ios ;;
*) echo "error: unsupported platform $PLATFORM_NAME" >&2; exit 1 ;;
esac
profile=debug flags=()
[[ $CONFIGURATION == Release ]] && profile=release flags=(--release)
mobile="$SRCROOT/.."
env -i HOME="$HOME" PATH="$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin" DEVELOPER_DIR="$DEVELOPER_DIR" \
    IPHONEOS_DEPLOYMENT_TARGET="$IPHONEOS_DEPLOYMENT_TARGET" \
    cargo build --locked --manifest-path "$mobile/Cargo.toml" -p skyelab-mobile --bin skyelab_mobile --target "$triple" \
    ${flags[@]+"${flags[@]}"} ${SKYELAB_MOBILE_FEATURES:+--features "$SKYELAB_MOBILE_FEATURES"}
mkdir -p "$TARGET_BUILD_DIR/$CONTENTS_FOLDER_PATH"
cp "$mobile/target/$triple/$profile/skyelab_mobile" "$TARGET_BUILD_DIR/$EXECUTABLE_PATH"
