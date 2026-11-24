# CI

`.github/workflows/ci.yml` runs the gate from CLAUDE.md on Ubuntu, macOS and Windows: `cargo fmt --check` (Ubuntu),
then `clippy --workspace --all-targets -D warnings` and `cargo test --workspace --no-fail-fast` as separate jobs per
OS. `--no-fail-fast` matters: without it cargo stops at the first failing test binary, and every binary after it
never runs on that OS, so one run hides the next failure. `mobile.yml` builds the iOS simulator app and the Android
release APK; `release.yml` packages tagged releases.

Only macOS is tested locally. Check the others like this before merging something platform-sensitive:

- **Windows:** `scripts/check-windows.sh` compiles clippy for every target (tests included), so `cfg` mistakes and
  dead code on Windows show up. Nothing here can run the tests.
- **Linux:** Docker, if installed: an `ubuntu:24.04` container with the Linux packages from CLAUDE.md, rustup, and
  `cargo test --workspace --no-fail-fast`. Give the VM 8 GB and build with `-j 3` (linking the app's test binary
  in parallel runs out of memory).
- **Android:** run the Gradle build with an empty `GRADLE_USER_HOME` (see below).

## Traps, by platform

**Windows**

- `/w` or `/home/me/x` is not absolute (no drive). Code that checks `is_absolute()` (session import, account config
  dirs, bindings) rejects it, so tests build paths from a temp dir, or `C:\w` under `cfg(windows)`.
- `canonicalize` returns `\\?\C:\…`, git prints `C:/…`. Canonicalize both sides before comparing.
- Git for Windows checks text out with CRLF (`core.autocrlf=true`). CI turns it off before checkout so fixtures
  are the committed bytes; test repos made in a test set `core.autocrlf=false` themselves.
- Fakes that are shell scripts (`fake-claude.sh`, `fake-gh.sh`, …), `mkfifo`, symlinks and Unix permissions:
  `#[cfg(unix)]` on the test (or just that step), never on a test of a feature that also ships on Windows.

**Linux**

- `cmd-` in a test keystroke is the super key: a `secondary-` binding is ctrl there. Write `secondary-…`.

**macOS runners**

- Real file events (FSEvents) arrive late and in batches on a loaded runner: assert "a few", not an exact count,
  when the count is about coalescing (`skyelab-git/tests/watch.rs`).
- On GitHub's macos-26 image cargo was not in `~/.cargo/bin` when Xcode ran the "Build Rust" phase ("env: cargo:
  No such file or directory"), so the phase takes cargo from the PATH it inherits, `~/.cargo/bin` only as a
  fallback (`mobile/ios/build-rust.sh`).

**Android (Gradle dependency verification)**

- `verification-metadata.xml` made from a warm Gradle cache misses files a clean machine fetches: a BOM's `.module`
  where the cache had its `.pom`, a newer parent POM. Regenerate with an empty `GRADLE_USER_HOME`, then run the
  build again with another empty one in verify mode. Gradle's write mode does not record some of these (buildscript
  classpath BOMs); add them by hand with the sha256 of the file from Maven Central, checked against Central's
  `.sha1`.

**Any loaded runner**

- Tests that wait on real processes must hold at high load: see the rules under Testing in [gpui.md](gpui.md).
  A wait that times out at 60 s is usually a wait for a state that was skipped, not a slow machine.
