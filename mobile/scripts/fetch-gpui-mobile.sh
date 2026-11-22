#!/usr/bin/env bash
# Clone longbridge/gpui-mobile at a pinned commit into target/gpui-mobile and port it to gpui-pre 0.3.12.
# Run before any cargo command in mobile/: cargo resolves this path dependency on every target. See UPSTREAM.md.
set -euo pipefail
cd "$(dirname "$0")/.."
rev=7548ebc4b24f00494af9976af8852c7163ded9ce
patches=(upstream/0001-port-to-gpui-pre-0.3.12.patch upstream/0002-refresh-vendored-gpui-pre-wgpu-0.3.12.patch upstream/local-no-offscreen.patch)
dest=target/gpui-mobile
# Re-fetch when the rev or either patch changes.
stamp="$rev $(cat "${patches[@]}" | git hash-object --stdin)"
[[ "$(cat "$dest/.ported" 2>/dev/null)" == "$stamp" ]] && exit 0
rm -rf "$dest" && mkdir -p target
git init -q "$dest"
git -C "$dest" fetch -q --depth 1 https://github.com/longbridge/gpui-mobile "$rev"
git -C "$dest" checkout -q FETCH_HEAD
for p in "${patches[@]}"; do git -C "$dest" apply "$PWD/$p"; done
echo "$stamp" > "$dest/.ported"
