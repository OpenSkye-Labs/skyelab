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
- Git can't take a `\\?\C:\…` path as a folder to create: `git worktree add \\?\C:\…` fails with "could not
  create leading directories of '//?/C:/…': Invalid argument" (`-C \\?\…` works). Paths handed to git as
  arguments go through `skyelab_core::files::without_verbatim` (`create_worktree`, `remove_worktree`, and test
  setup that runs `git worktree add` itself).
- Git lists paths with `/`; a path built with `join` has `\`. Compare them by components (`Path` equality does),
  never as strings or as string keys (the rewind's `Removed` set lowercases and joins parts with `/`).
- A server that closes a socket with the client's bytes still unread sends a reset, and Windows then discards
  what it had already sent before the client reads it (os error 10053). A fake server that "answers then hangs
  up" shuts down its write side and drains the client first.
- Git for Windows checks text out with CRLF (`core.autocrlf=true`). CI turns it off before checkout so fixtures
  are the committed bytes; test repos made in a test set `core.autocrlf=false` themselves.
- Fakes that are shell scripts (`fake-claude.sh`, `fake-gh.sh`, …), `mkfifo`, symlinks and Unix permissions:
  `#[cfg(unix)]` on the test (or just that step), never on a test of a feature that also ships on Windows.

**Linux**

- `cmd-` in a test keystroke is the super key: a `secondary-` binding is ctrl there. Write `secondary-…`.
- `std::fs::copy` stamps the copy with the current time (macOS and Windows keep the source's mtime). A copied
  git index must keep the original's mtime: git trusts an entry's stat only when the file is older than the
  index, and ext4's timestamps are coarse, so a same-size edit right after the index was written looked
  unchanged in the copy (`snapshot_tree`'s scratch index; the Commit snapshot failed only on Linux).
- A spawn returns once the child execs, but the kernel closes the child's close-on-exec fds a moment later. For that
  moment a child holds every fd this process had open, listeners included. Holding the spawn lock while binding
  can't close that gap. A port the daemon just closed can still be in use, so `Daemon::start` retries an
  "Address already in use" bind for up to 0.5 s (`rebind_while_spawning`, `rebind_retry`).
- A test that forks in a loop must keep a bound on its live children and threads. Reaping each child on a thread
  of its own let `rebind_while_spawning` reach the kernel's thread limit (63k) in Docker. Thread creation then
  failed, and tokio's `Builder::build` hung while it dropped its half-made pool, with no error. Wait on each child
  in the loop that forked it.

**macOS runners**

- macos-latest runners can be short: a job GitHub never hands to a runner ends as **cancelled** after about
  15 min with the annotation "The job was not acquired by Runner of type hosted even after multiple attempts".
  That is not a timeout or a hang in the tests; re-run it.
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

**Timing and cache**

- A cold build of the test targets takes ~15 min on Linux and Windows, and the app's unit tests ~13 min on
  Linux (one binary, ~800 tests that wait on real processes) against ~1 min on Windows (the Unix-only fake-agent
  tests don't run there). Jobs time out at 70 min (test; its test step at 55) and 30 min (clippy).
- `Swatinem/rust-cache` saves only after a successful job by default. The test job had never passed, so it
  rebuilt from nothing every run; `cache-on-failure: true` keeps the build either way.

**Reading a failed run**

- `gh run view --job <id> --log` can return only part of a long job log. In run 45459381411 it stopped halfway
  through the Linux tests, so the run looked hung. The full log comes from the API:
  `curl -sSL -H "Authorization: Bearer $(gh auth token)" https://api.github.com/repos/OpenSkye-Labs/skyelab/actions/jobs/<id>/logs`.
  Search it for `FAILED`, `panicked` and `error: test failed`. The failure annotation's line number tells you how
  long the full log is.
- A real hang: the test step stops at 55 min, 15 before the job's 70, so the cache still saves. Every test that runs
  past 60 s is named in the log ("has been running for over 60 seconds"). The hung test is a named one that never
  prints `ok`.

**Any loaded runner**

- Tests that wait on real processes must hold at high load: see the rules under Testing in [gpui.md](gpui.md).
  A wait that times out at 60 s is usually a wait for a state that was skipped, not a slow machine.
