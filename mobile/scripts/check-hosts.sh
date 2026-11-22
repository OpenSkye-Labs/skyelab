#!/usr/bin/env bash
# Checks the app shells are fit to distribute: Android asks for INTERNET only, exports only the launcher, has no deep
# links, and pins its Gradle wrapper by checksum; iOS commits no team id or entitlements.
# Usage: scripts/check-hosts.sh [built.apk | built.app]...  With no arguments only the sources are checked.
# A built APK is checked with aapt2 (the merged manifest), a built .app with plutil.
set -euo pipefail
cd "$(dirname "$0")/.."
fail=0
bad() { echo "check-hosts: $*" >&2; fail=1; }

# Gradle 9.9.2's published checksums (services.gradle.org/versions/current). Bump with the wrapper.
gradle_version=9.9.2
wrapper_jar_sha256=621008afc4f5d59357fcf06ffa644dd5aade5e091bdc363742710445f73cdaf8
app_id=dev.openskye.skyelab

host=android-host
manifest=$host/app/src/main/AndroidManifest.xml
props=$host/gradle/wrapper/gradle-wrapper.properties
gradle=$host/app/build.gradle.kts

# Android sources
perms=$(grep -o 'android.permission\.[A-Z_]*' "$manifest" | sort -u | tr '\n' ' ')
[[ $perms == "android.permission.INTERNET " ]] || bad "$manifest asks for $perms(INTERNET only)"
n=$(grep -c 'android:exported="true"' "$manifest" || true)
[[ $n == 1 ]] || bad "$manifest exports $n components (only the launcher)"
! grep -qE 'intent\.action\.VIEW|<data |<service|<receiver|<provider' "$manifest" || bad "$manifest has a deep link or an extra component"
grep -q "^distributionUrl=https\\\\://services.gradle.org/distributions/gradle-$gradle_version-bin.zip$" "$props" ||
    bad "$props: wrapper is not Gradle $gradle_version (bump the pins in $0 with it)"
grep -qE '^distributionSha256Sum=[0-9a-f]{64}$' "$props" || bad "$props has no distributionSha256Sum"
sha=$(shasum -a 256 "$host/gradle/wrapper/gradle-wrapper.jar" | cut -d' ' -f1)
[[ $sha == "$wrapper_jar_sha256" ]] || bad "gradle-wrapper.jar sha256 $sha is not Gradle $gradle_version's"
for k in applicationId namespace; do
    grep -q "^ *$k = \"$app_id\"$" "$gradle" || bad "$gradle: $k is not $app_id"
done
compile=$(sed -n 's/^ *compileSdk = \([0-9]*\)$/\1/p' "$gradle")
target=$(sed -n 's/^ *targetSdk = \([0-9]*\)$/\1/p' "$gradle")
[[ -n $target && $target == "$compile" ]] || bad "$gradle: targetSdk ($target) must equal compileSdk ($compile)"
grep -q '^ *minSdk = 26$' "$gradle" && grep -q -- '-P 26' scripts/build-android.sh ||
    bad "minSdk in $gradle and cargo ndk -P in scripts/build-android.sh must both be 26"
for d in mdpi hdpi xhdpi xxhdpi xxxhdpi; do
    [[ -s $host/app/src/main/res/mipmap-$d/ic_launcher_background.png ]] || bad "no $d launcher icon (scripts/icons.sh)"
done

# iOS sources
pbx=ios/SkyeLab.xcodeproj/project.pbxproj
ids=$(grep -o 'PRODUCT_BUNDLE_IDENTIFIER = [^;]*' "$pbx" | sort -u)
[[ $ids == "PRODUCT_BUNDLE_IDENTIFIER = $app_id" ]] || bad "$pbx bundle ids: $ids"
! grep -qE 'DEVELOPMENT_TEAM = [^";]|DEVELOPMENT_TEAM = "[^"]' "$pbx" || bad "$pbx commits a team id (use SKYELAB_IOS_TEAM_ID)"
! grep -q CODE_SIGN_ENTITLEMENTS "$pbx" || bad "$pbx sets entitlements"
[[ -z $(find ios -name '*.entitlements') ]] || bad "ios/ has an .entitlements file"
[[ -s ios/Assets.xcassets/AppIcon.appiconset/icon-1024.png ]] || bad "no iOS app icon (scripts/icons.sh)"

# Built artifacts
aapt2() {
    local sdk=${ANDROID_HOME:-$HOME/Library/Android/sdk}
    "$(find "$sdk/build-tools" -name aapt2 -maxdepth 2 | sort -V | tail -1)" "$@"
}
for f in "$@"; do
    case $f in
    *.apk)
        got=$(aapt2 dump permissions "$f" | sort | tr '\n' ' ')
        want="package: $app_id uses-permission: name='android.permission.INTERNET' "
        [[ $got == "$want" ]] || bad "$f permissions: $got"
        xml=$(aapt2 dump xmltree --file AndroidManifest.xml "$f")
        n=$(grep -cE 'android:exported\(0x[0-9a-f]+\)=true' <<<"$xml" || true)
        [[ $n == 1 ]] || bad "$f exports $n components"
        n=$(grep -cE 'E: (activity|activity-alias|service|receiver|provider) ' <<<"$xml" || true)
        [[ $n == 1 ]] || bad "$f declares $n components (only the launcher)"
        ! grep -qE 'android.intent.action.VIEW|E: data ' <<<"$xml" || bad "$f has a deep link"
        ;;
    *.app)
        id=$(plutil -extract CFBundleIdentifier raw "$f/Info.plist")
        [[ $id == "$app_id" ]] || bad "$f bundle id $id"
        [[ $(plutil -extract CFBundleDisplayName raw "$f/Info.plist") == SkyeLab ]] || bad "$f display name"
        ;;
    *) bad "don't know how to check $f" ;;
    esac
done
[[ $fail == 0 ]] && echo "check-hosts: ok"
exit $fail
