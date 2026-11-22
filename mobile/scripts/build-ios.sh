#!/usr/bin/env bash
# Build SkyeLab.app with ios/SkyeLab.xcodeproj (cargo runs inside it). Prints the .app's path.
# Usage: scripts/build-ios.sh [debug|release] [simulator|device]   (default: debug simulator)
# Device builds are signed with the team in SKYELAB_IOS_TEAM_ID (automatic signing); never commit a team id.
# SKYELAB_MOBILE_FEATURES=demo builds with canned data; it is refused for a device build.
set -euo pipefail
cd "$(dirname "$0")/.."
case ${1:-debug} in debug) config=Debug ;; release) config=Release ;; *) echo "usage: $0 [debug|release] [simulator|device]" >&2; exit 2 ;; esac
case ${2:-simulator} in
simulator)
    sdk=iphonesimulator
    signing=(CODE_SIGN_IDENTITY=-) # ad hoc, enough for the simulator
    ;;
device)
    sdk=iphoneos
    [[ -n ${SKYELAB_IOS_TEAM_ID:-} ]] || { echo "build-ios: set SKYELAB_IOS_TEAM_ID to sign a device build" >&2; exit 2; }
    [[ ${SKYELAB_MOBILE_FEATURES:-} != *demo* ]] || { echo "build-ios: refusing to sign a demo build" >&2; exit 2; }
    signing=(DEVELOPMENT_TEAM="$SKYELAB_IOS_TEAM_ID" -allowProvisioningUpdates)
    ;;
*) echo "usage: $0 [debug|release] [simulator|device]" >&2; exit 2 ;;
esac
scripts/fetch-gpui-mobile.sh # cargo resolves the Android-only path dependency on every target
xcodebuild -quiet -project ios/SkyeLab.xcodeproj -scheme SkyeLab -configuration "$config" -sdk "$sdk" \
    -destination "generic/platform=$([[ $sdk == iphoneos ]] && echo iOS || echo 'iOS Simulator')" \
    -derivedDataPath target/ios-build "${signing[@]}" build >&2
echo "$PWD/target/ios-build/Build/Products/$config-$sdk/SkyeLab.app"
