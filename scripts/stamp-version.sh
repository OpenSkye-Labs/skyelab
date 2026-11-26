#!/usr/bin/env bash
# A pre-release build's version, stamped by package.yml and never committed (docs/ci.md "Releases and nightlies").
#   stamp-version.sh stamp <version>       Cargo.toml, Cargo.lock and the macOS Info.plist, from the repo root
#   stamp-version.sh deb <version> <deb>…  rewrites each .deb's control Version, in place
set -euo pipefail
fail() { echo "stamp-version: $*" >&2; exit 1; }
[[ "${2:-}" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || fail "usage: $0 stamp|deb <X.Y.Z[-pre]> [deb…]"
export VERSION=$2

case $1 in
  stamp)
    # The workspace version (every crate inherits it) and its entries in Cargo.lock, so a --locked build still matches.
    # A Windows checkout has CRLF lines; cargo reads LF as well, and the patterns below expect it.
    perl -pi -e 's/\r$//' Cargo.toml Cargo.lock
    old=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
    perl -pi -e 'if (!$done && s/^version = ".*"/version = "$ENV{VERSION}"/) { $done = 1 }' Cargo.toml
    # Workspace crates are the lock entries with no `source` line.
    OLD="$old" perl -0pi -e 's/(\nname = "[^"]+"\nversion = )"\Q$ENV{OLD}\E"\n(?!source = )/$1"$ENV{VERSION}"\n/g' Cargo.lock
    grep -qxF "version = \"$VERSION\"" Cargo.toml || fail "Cargo.toml not stamped"
    # Apple wants CFBundleShortVersionString as X.Y.Z: a pre-release shows its release's numbers there (cargo-packager
    # merges this plist over its own keys). CFBundleVersion stays cargo-packager's UTC YYYYMMDD.HHMMSS, numeric and
    # rising for stable and nightly alike. The full version is the binary's own (About, the updater).
    if [[ $VERSION == *-* ]]; then
      plist=crates/skyelab-app/macos/Info.plist
      SHORT=${VERSION%%-*} perl -0pi -e 's{</dict>}{\t<key>CFBundleShortVersionString</key>\n\t<string>$ENV{SHORT}</string>\n</dict>}' "$plist"
      grep -qF "<string>${VERSION%%-*}</string>" "$plist" || fail "$plist not stamped"
    fi
    ;;
  deb)
    # Debian reads `-…` as a revision, so 0.1.8-nightly.N would sort above 0.1.8 and apt would call the stable a
    # downgrade. `~` sorts before the release: 0.1.7 < 0.1.8~nightly.N < 0.1.8. The file name keeps the `-`.
    deb_version=$(sed "s/-/~/" <<<"$VERSION")
    shift 2
    for deb in "$@"; do
      dir=$(mktemp -d)
      dpkg-deb -R "$deb" "$dir/root"
      V="$deb_version" perl -pi -e 's/^Version: .*/Version: $ENV{V}/' "$dir/root/DEBIAN/control"
      dpkg-deb --root-owner-group -b "$dir/root" "$deb" >/dev/null
      rm -rf "$dir"
      [ "$(dpkg-deb -f "$deb" Version)" = "$deb_version" ] || fail "$deb not stamped"
    done
    ;;
  *) fail "unknown command $1" ;;
esac
