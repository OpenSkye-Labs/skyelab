#!/usr/bin/env bash
# Clone longbridge/gpui-mobile at a pinned rev into target/gpui-mobile and port it to gpui-pre 0.3.12.
set -euo pipefail
cd "$(dirname "$0")/.."
rev=7548ebc4b24f00494af9976af8852c7163ded9ce
dest=target/gpui-mobile
[[ "$(cat "$dest/.ported" 2>/dev/null)" == "$rev" ]] && exit 0
rm -rf "$dest" && mkdir -p target
git clone -q https://github.com/longbridge/gpui-mobile "$dest"
git -C "$dest" checkout -q "$rev"
git -C "$dest" apply "$PWD/android/gpui-pre-mobile-0.3.12.patch"
echo "$rev" > "$dest/.ported"
