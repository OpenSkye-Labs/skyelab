#!/usr/bin/env bash
# Regenerates every app icon size from crates/skyelab-app/icons/icon.svg.
# Needs resvg (`cargo install resvg`) and python3.
set -euo pipefail
dir="$(cd "$(dirname "$0")/../crates/skyelab-app/icons" && pwd)"
command -v resvg >/dev/null || { echo "resvg not found: cargo install resvg" >&2; exit 1; }

render() { resvg -w "$2" -h "$2" "$dir/icon.svg" "$dir/$1.png"; }
render 32x32 32
render 128x128 128
render 128x128@2x 256
render 256x256 256
render 512x512 512
render 512x512@2x 1024

# Windows .ico: PNG-compressed entries (supported since Vista), 16 to 256 px.
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
for s in 16 24 32 48 64 128 256; do resvg -w $s -h $s "$dir/icon.svg" "$tmp/$s.png"; done
python3 - "$dir/icon.ico" "$tmp" <<'PY'
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
echo "icons written to $dir"
