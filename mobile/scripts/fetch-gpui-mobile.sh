#!/usr/bin/env bash
# Clone longbridge/gpui-mobile at a pinned commit into target/gpui-mobile and port it to gpui-pre 0.3.14, and fetch
# Android's emoji font into target/emoji.
# Run before any cargo command in mobile/: cargo resolves this path dependency on every target. See UPSTREAM.md.
set -euo pipefail
cd "$(dirname "$0")/.."

# Android's emoji font, which android-host bundles as assets/fonts/NotoColorEmoji.ttf (docs/mobile.md, "Emoji").
# Twemoji Mozilla is COLR v0, which swash renders; Android 13+'s own emoji font is COLR v1, which it can't.
# gpui-pre-wgpu draws a glyph in colour only for the PostScript name NotoColorEmoji, so we swap the name (same length,
# both name records; the name table's checksum goes stale, which nothing reads).
emoji=target/emoji
emoji_url=https://github.com/mozilla/twemoji-colr/releases/download/v0.7.0/Twemoji.Mozilla.ttf
emoji_sha256=6d90152ee0d29e82fe2a87793af5aa4b7ad13e6538360889e141e81ed299ee8e
renamed_sha256=d0c56b50bdf311472592e45d4f200c1f67305629fd0c0b9efe64cf65e7812da0
license_url=https://raw.githubusercontent.com/mozilla/twemoji-colr/v0.7.0/LICENSE.md
license_sha256=64419edc28e9163204c3be73f835a8dfc34cd6c9b8f7d067bb685f119f839a99
sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
if [[ ! -f $emoji/twemoji-mozilla-LICENSE.md || ! -f $emoji/NotoColorEmoji.ttf || $(sha "$emoji/NotoColorEmoji.ttf") != "$renamed_sha256" ]]; then
    rm -rf "$emoji" && mkdir -p "$emoji"
    curl -fsSL --proto =https --proto-redir =https --retry 3 -o "$emoji/twemoji.ttf" "$emoji_url"
    curl -fsSL --proto =https --proto-redir =https --retry 3 -o "$emoji/LICENSE.md" "$license_url"
    [[ $(sha "$emoji/twemoji.ttf") == "$emoji_sha256" ]] || { echo "fetch: $emoji_url checksum mismatch" >&2; exit 1; }
    [[ $(sha "$emoji/LICENSE.md") == "$license_sha256" ]] || { echo "fetch: $license_url checksum mismatch" >&2; exit 1; }
    LC_ALL=C perl -0777 -pe 's/TwemojiMozilla/NotoColorEmoji/g; s/T\0w\0e\0m\0o\0j\0i\0M\0o\0z\0i\0l\0l\0a/N\0o\0t\0o\0C\0o\0l\0o\0r\0E\0m\0o\0j\0i/g' \
        "$emoji/twemoji.ttf" > "$emoji/renamed.ttf"
    # Both name records (Mac ASCII, Windows UTF-16) must be renamed; check before trusting a re-pinned sum.
    LC_ALL=C perl -0777 -ne 'exit 1 if /Twemoji\0?M|T\0w\0e\0m\0o\0j\0i\0M/' "$emoji/renamed.ttf" || { echo "fetch: emoji rename missed a name record" >&2; exit 1; }
    [[ $(sha "$emoji/renamed.ttf") == "$renamed_sha256" ]] || { echo "fetch: renamed emoji font checksum mismatch" >&2; exit 1; }
    # CC BY 4.0 asks us to say what we changed.
    printf '%s\n\n%s\n' "Twemoji Mozilla 0.7.0 ($emoji_url), changed by SkyeLab: PostScript name TwemojiMozilla renamed to NotoColorEmoji." \
        "$(cat "$emoji/LICENSE.md")" > "$emoji/twemoji-mozilla-LICENSE.md"
    mv "$emoji/renamed.ttf" "$emoji/NotoColorEmoji.ttf" && rm "$emoji/twemoji.ttf" "$emoji/LICENSE.md"
fi
rev=7548ebc4b24f00494af9976af8852c7163ded9ce
patches=(upstream/0001-port-to-gpui-pre-0.3.14.patch upstream/0002-refresh-vendored-gpui-pre-wgpu-0.3.14.patch upstream/0003-android-report-ime-insets.patch upstream/0004-android-skip-system-flags-font.patch upstream/local-no-offscreen.patch)
dest=target/gpui-mobile
# Re-fetch when the rev or any patch changes.
stamp="$rev $(cat "${patches[@]}" | git hash-object --stdin)"
[[ "$(cat "$dest/.ported" 2>/dev/null)" == "$stamp" ]] && exit 0
rm -rf "$dest" && mkdir -p target
git init -q "$dest"
git -C "$dest" fetch -q --depth 1 https://github.com/longbridge/gpui-mobile "$rev"
git -C "$dest" checkout -q FETCH_HEAD
for p in "${patches[@]}"; do git -C "$dest" apply "$PWD/$p"; done
echo "$stamp" > "$dest/.ported"
