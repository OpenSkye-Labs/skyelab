#!/usr/bin/env bash
# Regenerates every app icon (desktop and mobile) from crates/skyelab-app/icons/icon.svg. Needs resvg, python3, and macOS for icon.icns.
set -euo pipefail
dir="$(cd "$(dirname "$0")/../crates/skyelab-app/icons" && pwd)"
command -v resvg >/dev/null || { echo "resvg not found: cargo install resvg" >&2; exit 1; }
command -v iconutil >/dev/null || { echo "iconutil not found: icon.icns is built on macOS" >&2; exit 1; }
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
render() { resvg -w "$2" -h "$2" "$dir/icon.svg" "$1"; }

# Linux and the .deb/AppImage take PNGs; linuxdeploy rejects icons over 512 px, so 1024 lives only in the .icns.
render "$dir/32x32.png" 32
render "$dir/128x128.png" 128
render "$dir/128x128@2x.png" 256
render "$dir/256x256.png" 256
render "$dir/512x512.png" 512

mkdir "$tmp/icon.iconset"
for s in 16 32 128 256 512; do
  render "$tmp/icon.iconset/icon_${s}x${s}.png" $s
  render "$tmp/icon.iconset/icon_${s}x${s}@2x.png" $((s * 2))
done
iconutil -c icns "$tmp/icon.iconset" -o "$dir/icon.icns"

# Windows .ico: PNG-compressed entries (supported since Vista), 16 to 256 px.
for s in 16 24 32 48 64 128 256; do render "$tmp/$s.png" $s; done
python3 -I - "$dir/icon.ico" "$tmp" <<'PY'
import struct, sys, pathlib
out, tmp = sys.argv[1], pathlib.Path(sys.argv[2])
sizes = [16, 24, 32, 48, 64, 128, 256]
blobs = [(tmp / f"{s}.png").read_bytes() for s in sizes]
offset = 6 + 16 * len(sizes)
header = struct.pack("<HHH", 0, 1, len(sizes))
entries = b""
for s, b in zip(sizes, blobs):
    entries += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(b), offset)  # 0 means 256
    offset += len(b)
pathlib.Path(out).write_bytes(header + entries + b"".join(blobs))
PY
# Mobile launchers apply their own masks. Render the tile bounds full bleed and remove the desktop corner radius.
mobile="$(cd "$dir/../../../mobile" && pwd)"
sed -e 's|viewBox="0 0 1024 1024"|viewBox="100 100 824 824"|' -e 's| rx="185"||' -e '/stroke="url(#rim)"/d' "$dir/icon.svg" > "$tmp/full.svg"
grep -q 'viewBox="100 100 824 824"' "$tmp/full.svg" || { echo "icon.svg changed: update the tile bounds in $0" >&2; exit 1; }
# Android (minSdk 26, so adaptive icons only): the art is the 108 dp background layer at each density.
for d in mdpi:108 hdpi:162 xhdpi:216 xxhdpi:324 xxxhdpi:432; do
  res="$mobile/android-host/app/src/main/res/mipmap-${d%%:*}"
  mkdir -p "$res"
  resvg -w "${d##*:}" -h "${d##*:}" "$tmp/full.svg" "$res/ic_launcher_background.png"
done
# iOS: one 1024 px image; Xcode makes the other sizes. App Store Connect rejects an icon with an alpha channel, and
# resvg always writes one, so flatten it through a full-quality JPEG.
resvg -w 1024 -h 1024 "$tmp/full.svg" "$tmp/ios.png"
sips -s format jpeg -s formatOptions 100 "$tmp/ios.png" --out "$tmp/ios.jpg" >/dev/null
sips -s format png "$tmp/ios.jpg" --out "$mobile/ios/Assets.xcassets/AppIcon.appiconset/icon-1024.png" >/dev/null

echo "icons written to $dir, $mobile/android-host and $mobile/ios"
