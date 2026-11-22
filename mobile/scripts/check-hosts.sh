#!/usr/bin/env bash
# Checks the app shells are fit to distribute: Android asks for INTERNET only, exports only the launcher, has no deep
# links, and pins its Gradle wrapper and dependencies by checksum; iOS commits no team id or entitlements.
# Usage: scripts/check-hosts.sh [built.apk | built.app]...  With no arguments only the sources are checked.
# A built APK is checked with aapt2 (the merged manifest, which includes anything a library adds).
set -euo pipefail
args=()
for f in "$@"; do [[ $f == /* ]] && args+=("$f") || args+=("$PWD/$f"); done
cd "$(dirname "$0")/.."
fail=0
bad() { echo "check-hosts: $*" >&2; fail=1; }

# Bump these with the wrapper; the jar sum is Gradle's published one (gradle-<version>-wrapper.jar.sha256).
gradle_version=9.9.2
wrapper_jar_sha256=621008afc4f5d59357fcf06ffa644dd5aade5e091bdc363742710445f73cdaf8
# The latest stable Android API level; target and compile SDK follow it.
latest_sdk=37
min_sdk=26
app_id=dev.openskye.skyelab
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)

host=android-host
manifest=$host/app/src/main/AndroidManifest.xml
props=$host/gradle/wrapper/gradle-wrapper.properties
gradle=$host/app/build.gradle.kts

perms=$(grep -o 'android.permission\.[A-Z_]*' "$manifest" | sort -u | tr '\n' ' ' || true)
[[ $perms == "android.permission.INTERNET " ]] || bad "$manifest asks for $perms(INTERNET only)"
n=$(grep -c 'android:exported="true"' "$manifest" || true)
[[ $n == 1 ]] || bad "$manifest exports $n components (only the launcher)"
! grep -qE 'intent\.action\.VIEW|<data |<service|<receiver|<provider' "$manifest" || bad "$manifest has a deep link or an extra component"
grep -q "^distributionUrl=https\\\\://services.gradle.org/distributions/gradle-$gradle_version-bin.zip$" "$props" ||
    bad "$props: wrapper is not Gradle $gradle_version (bump the pins in $0 with it)"
grep -qE '^distributionSha256Sum=[0-9a-f]{64}$' "$props" || bad "$props has no distributionSha256Sum"
sha=$(shasum -a 256 "$host/gradle/wrapper/gradle-wrapper.jar" | cut -d' ' -f1)
[[ $sha == "$wrapper_jar_sha256" ]] || bad "gradle-wrapper.jar sha256 $sha is not Gradle $gradle_version's"
# Gradle checks every artifact it resolves against these sums; a mismatch fails the build (docs/mobile.md step 6).
meta=$host/gradle/verification-metadata.xml
grep -q '<verify-metadata>true</verify-metadata>' "$meta" 2>/dev/null && grep -q '<sha256 value="[0-9a-f]\{64\}"' "$meta" ||
    bad "$meta missing or has no sha256 checksums"
# Anything that exempts artifacts or switches checking off: trust rules, comments (a commented-out true), lenient mode.
! grep -qE '<trust|<ignored-key|<!--|>false</verify-metadata>' "$meta" 2>/dev/null || bad "$meta exempts artifacts or turns checking off"
! grep -rqE 'dependency[.-]verification' $host/gradle.properties scripts ../.github/workflows/mobile.yml --exclude=check-hosts.sh ||
    bad "something sets Gradle's dependency-verification mode (it must stay strict)"
# aapt2 is one jar per OS; --write-verification-metadata records only the one it ran on, and CI is Linux.
aapt2=$(grep -oE '<component group="com.android.tools.build" name="aapt2" version="[^"]+"' "$meta" | cut -d'"' -f6 || true)
[[ -n $aapt2 ]] || bad "$meta has no aapt2 entry"
for v in $aapt2; do
    for os in osx linux windows; do
        grep -q "<artifact name=\"aapt2-$v-$os.jar\">" "$meta" || bad "$meta has no aapt2-$v-$os.jar (docs/mobile.md step 6)"
    done
done
for k in applicationId namespace; do
    grep -q "^ *$k = \"$app_id\"$" "$gradle" || bad "$gradle: $k is not $app_id"
done
for k in compileSdk targetSdk; do
    grep -q "^ *$k = $latest_sdk$" "$gradle" || bad "$gradle: $k is not $latest_sdk"
done
grep -q "^ *minSdk = $min_sdk$" "$gradle" || bad "$gradle: minSdk is not $min_sdk"
# The cdylib must link against the same API level (cargo ndk -P).
levels=$(grep -ohE -- '-P [0-9]+' scripts/build-android.sh ../.github/workflows/mobile.yml | sort -u | tr '\n' ' ' || true)
[[ $levels == "-P $min_sdk " ]] || bad "cargo ndk API levels ($levels) must all be $min_sdk"
grep -q "^ *versionName = \"$version\"$" "$gradle" || bad "$gradle: versionName is not $version (Cargo.toml)"
grep -q 'android:name=".SkyeLabActivity"' "$manifest" || bad "$manifest: launcher is not .SkyeLabActivity"
grep -q '<string name="app_name">SkyeLab</string>' $host/app/src/main/res/values/strings.xml || bad "app label is not SkyeLab"
for d in mdpi hdpi xhdpi xxhdpi xxxhdpi; do
    [[ -s $host/app/src/main/res/mipmap-$d/ic_launcher_background.png ]] || bad "no $d launcher icon (scripts/icons.sh)"
done

pbx=ios/SkyeLab.xcodeproj/project.pbxproj
ids=$(grep -o 'PRODUCT_BUNDLE_IDENTIFIER = [^;]*' "$pbx" | sort -u || true)
[[ $ids == "PRODUCT_BUNDLE_IDENTIFIER = $app_id" ]] || bad "$pbx bundle ids: $ids"
! grep -qE 'DEVELOPMENT_TEAM = [^";]|DEVELOPMENT_TEAM = "[^"]' "$pbx" || bad "$pbx commits a team id (use SKYELAB_IOS_TEAM_ID)"
! grep -q CODE_SIGN_ENTITLEMENTS "$pbx" || bad "$pbx sets entitlements"
vers=$(grep -o 'MARKETING_VERSION = [^;]*' "$pbx" | sort -u || true)
[[ $vers == "MARKETING_VERSION = $version" ]] || bad "$pbx: $vers, not $version (Cargo.toml)"
[[ -z $(find ios -name '*.entitlements') ]] || bad "ios/ has an .entitlements file"
icon=ios/Assets.xcassets/AppIcon.appiconset/icon-1024.png
# PNG colour type (IHDR byte 25): 2 is RGB. App Store Connect rejects an icon with alpha (6).
[[ -s $icon && $(od -An -tu1 -j25 -N1 "$icon" | tr -d ' ') == 2 ]] || bad "$icon missing or has alpha (scripts/icons.sh)"

aapt2() {
    local sdk=${ANDROID_HOME:-$HOME/Library/Android/sdk}
    "$(find "$sdk/build-tools" -maxdepth 2 -name aapt2 | sort -V | tail -1)" "$@"
}
for f in ${args[@]+"${args[@]}"}; do
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
        badging=$(aapt2 dump badging "$f")
        grep -q "launchable-activity: name='$app_id.SkyeLabActivity'" <<<"$badging" || bad "$f launcher activity"
        grep -q "^application-label:'SkyeLab'$" <<<"$badging" || bad "$f label"
        grep -q "targetSdkVersion:'$latest_sdk'" <<<"$badging" || bad "$f targetSdk"
        # Apache-2.0 asks for the notice of the gpui-mobile code we ship.
        files=$(unzip -l "$f")
        grep -q 'assets/licenses/gpui-mobile-LICENSE-APACHE.txt' <<<"$files" || bad "$f lacks gpui-mobile's license"
        # gpui-mobile loads this path when the system emoji font is COLR v1 (Android 13+); CC BY 4.0 asks for the notice.
        grep -q 'assets/fonts/NotoColorEmoji.ttf' <<<"$files" || bad "$f lacks the emoji font"
        grep -q 'assets/licenses/twemoji-mozilla-LICENSE.md' <<<"$files" || bad "$f lacks the emoji font's license"
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
