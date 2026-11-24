#!/usr/bin/env bash
# Xcode's "Build Rust" phase. cargo runs in a clean environment: Xcode's SDKROOT (the iOS SDK) would break host
# build scripts.
set -euo pipefail
case $PLATFORM_NAME in
iphonesimulator) triple=aarch64-apple-ios-sim ;;
iphoneos)
    triple=aarch64-apple-ios
    [[ ${SKYELAB_MOBILE_FEATURES:-} != *demo* ]] || { echo "error: refusing a device build with demo data" >&2; exit 1; }
    ;;
*) echo "error: unsupported platform $PLATFORM_NAME" >&2; exit 1 ;;
esac
profile=debug flags=()
[[ $CONFIGURATION == Release ]] && profile=release flags=(--release)
mobile="$SRCROOT/.."
# cargo wherever it is installed (~/.cargo/bin with rustup's installer; Homebrew's rustup on GitHub's macOS runners).
cargo=$(PATH="$PATH:${CARGO_HOME:-$HOME/.cargo}/bin" command -v cargo) || { echo "error: cargo not found on PATH" >&2; exit 1; }
homes=()
[[ -z ${CARGO_HOME:-} ]] || homes+=(CARGO_HOME="$CARGO_HOME")
[[ -z ${RUSTUP_HOME:-} ]] || homes+=(RUSTUP_HOME="$RUSTUP_HOME")
env -i HOME="$HOME" PATH="$(dirname "$cargo"):/usr/bin:/bin:/usr/sbin:/sbin" DEVELOPER_DIR="$DEVELOPER_DIR" \
    ${homes[@]+"${homes[@]}"} \
    IPHONEOS_DEPLOYMENT_TARGET="$IPHONEOS_DEPLOYMENT_TARGET" \
    cargo build --locked --manifest-path "$mobile/Cargo.toml" -p skyelab-mobile --bin skyelab_mobile --target "$triple" \
    ${flags[@]+"${flags[@]}"} ${SKYELAB_MOBILE_FEATURES:+--features "$SKYELAB_MOBILE_FEATURES"}
mkdir -p "$TARGET_BUILD_DIR/$CONTENTS_FOLDER_PATH"
cp "$mobile/target/$triple/$profile/skyelab_mobile" "$TARGET_BUILD_DIR/$EXECUTABLE_PATH"
