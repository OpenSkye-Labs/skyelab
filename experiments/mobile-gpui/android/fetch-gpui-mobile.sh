#!/usr/bin/env bash
# Clone longbridge/gpui-mobile at a pinned rev into target/gpui-mobile and port it to gpui-pre 0.3.12.
set -euo pipefail
cd "$(dirname "$0")/.."
rev=7548ebc
dest=target/gpui-mobile
[[ -f $dest/.ported ]] && exit 0
rm -rf "$dest" && mkdir -p target
git clone -q https://github.com/longbridge/gpui-mobile "$dest"
git -C "$dest" checkout -q "$rev"
git -C "$dest" apply "$PWD/android/gpui-pre-mobile-0.3.12.patch"
touch "$dest/.ported"
