#!/usr/bin/env bash
# Fails when mobile/ pins a different GPUI or gpui-component than the desktop (two GPUIs can't share views).
set -euo pipefail
cd "$(dirname "$0")/../.."
pins() { grep -oE '^(gpui|gpui_platform) = \{ package = "[^"]+", version = "=[^"]+"|^gpui-component = "=[^"]+"' "$1" | sort; }
if ! diff <(pins Cargo.toml) <(pins mobile/Cargo.toml); then
    echo "mobile/Cargo.toml GPUI pins differ from the root Cargo.toml (< root, > mobile)" >&2
    exit 1
fi
[[ $(pins Cargo.toml | wc -l) -eq 3 ]] || { echo "expected 3 GPUI pins in the root Cargo.toml" >&2; exit 1; }
# gpui-pre-wgpu (the Android text system, tested on the host) ships in lockstep with gpui-pre.
gpui=$(grep -oE '^gpui = \{ package = "gpui-pre", version = "=[^"]+"' mobile/Cargo.toml | grep -oE '=[0-9][^"]*')
wgpu=$(grep -oE '^gpui_wgpu = \{ package = "gpui-pre-wgpu", version = "=[^"]+"' mobile/Cargo.toml | grep -oE '=[0-9][^"]*')
[[ -n $gpui && $gpui == "$wgpu" ]] || { echo "mobile/Cargo.toml: gpui-pre-wgpu ($wgpu) must match gpui-pre ($gpui)" >&2; exit 1; }
ios=$(grep -oE '^gpui_ios = \{ package = "gpui-pre-ios", version = "=[^"]+"' mobile/Cargo.toml | grep -oE '=[0-9][^"]*')
[[ $gpui == "$ios" ]] || { echo "mobile/Cargo.toml: gpui-pre-ios ($ios) must match gpui-pre ($gpui)" >&2; exit 1; }
