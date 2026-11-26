# CI

`.github/workflows/ci.yml` runs the gate from CLAUDE.md on Ubuntu, macOS and Windows: `cargo fmt --check` (Ubuntu),
then `clippy --workspace --all-targets -D warnings` and the tests as separate jobs per OS. Linux's tests run as three
jobs (`--partition slice:N/3`), each building from the shared cache. `mobile.yml` builds the iOS simulator app and
the Android release APK; `release.yml` packages tagged releases and `nightly.yml` nightly pre-releases (below).

## Tests run on cargo-nextest

`cargo nextest run --workspace` (locally: `brew install cargo-nextest` or `cargo install cargo-nextest --locked`;
CI: `taiki-e/install-action`, pinned by SHA), then `cargo test --workspace --doc`, since nextest skips doc tests.
`.verify.conf` sets that as `/verify`'s test step. Config: `.config/nextest.toml`.

- Every test runs in its own process and every test binary at once. `cargo test` ran the binaries one after another
  and the app's ~900 tests in one process, so the whole suite waited on the slowest binary's slowest tests.
  On one loaded M-series Mac (load average 80–280 from other sessions) the full suite went from 38 min to 2.5–3.5
  min. Most tests wait on real processes, so the run uses ~2 cores on average.
- Local runs default to `test-threads = 4` (`[profile.default]`) so heavy GPUI/git test binaries don't saturate the
  fast cores or starve macOS FSEvents; `[profile.ci]` overrides this to `test-threads = "num-cpus"` so GitHub Actions
  runners keep full parallelism. FSEvents-heavy tests (`binary_id(skyelab-git::watch)` and `test(/folder_watch/)`)
  run in the `fs-watch` test group (`max-threads = 2`) so they never swarm macOS FSEvents under load.
- Wall time is about the sum of test times over the thread count, plus whichever long test starts last. The long
  tests outside the app's binary have `priority` in `.config/nextest.toml`, so they start first; give a new one
  there when it shows up last in the run (`--profile ci`'s JUnit has each test's start and time).
- `fail-fast = false` is `--no-fail-fast`: one run lists every failing test, not just the first binary's.
- A test still running at 3 min (three 60 s slow periods) is killed and named `TIMEOUT`; a test's own waits are 60 s
  (`test_wait::DEADLINE`), so it is hung. `SLOW` lines name each test past 60 s.
- No retries. A test that must retry gets an override in `.config/nextest.toml` with the reason; none does yet.
- Process isolation shows tests that passed only because an earlier test in the same process set some state up (a
  `OnceLock`, a global): `the_scratch_index_keeps_the_index_mtime` passed only after `missing_git_binary_is_typed`
  had cached "old git", until `git init` ran on live config. Run one test alone (`cargo nextest run <name>`) before
  blaming the runner.
- `harness = false` test binaries (the WKWebView tests in `skyelab-browser`) answer nextest's `--list --format terse`
  themselves (`tests/support/mod.rs`): one test named after the binary.
- `/verify` can't name nextest's failures, so `.agent/expected-red` doesn't apply; the gate fails closed and
  nextest's `FAIL` lines name them. `--profile ci` also writes `target/nextest/ci/junit.xml`, with each test's time.

Only macOS is tested locally. Check the others like this before merging something platform-sensitive:

- **Windows:** `scripts/check-windows.sh` compiles clippy for every target (tests included), so `cfg` mistakes and
  dead code on Windows show up. Nothing here can run the tests.
- **Linux:** Docker, if installed: an `ubuntu:24.04` container with the Linux packages from CLAUDE.md, rustup, and
  `cargo nextest run --workspace`. Give the VM 8 GB and build with `-j 3` (linking the app's test binary
  in parallel runs out of memory).
- **Android:** run the Gradle build with an empty `GRADLE_USER_HOME` (see below).

## Releases and nightlies

`package.yml` is the packaging matrix (macOS .dmg, Windows NSIS, Linux .deb + AppImage, signing when the secrets
are set), a reusable workflow that uploads each OS's files as artifacts of the run that called it. Two workflows call
it and attach the files plus `SHA256SUMS` (the in-app updater refuses anything it doesn't list):

- **Stable:** `release.yml`, on a pushed `v<version>` tag. Bump `version` in the root `Cargo.toml`, commit, tag, push
  the tag. It checks the tag matches the version, packages, and makes a **draft** release; check it, then publish.
- **Nightly:** `nightly.yml`, only when run by hand (Actions → Nightly → Run workflow, on `main`; no schedule). It
  refuses a ref other than main and a commit without a successful CI run, then builds that commit as the next
  version's nightly: `vX.Y.Z-nightly.YYYYMMDD`, `.2`, `.3`… for a second run that day. Next is the `Cargo.toml`
  version without its `-dev` when it has one, else its patch + 1 (main normally carries the last stable, so after
  0.1.7 the nightlies are `0.1.8-nightly.*`). The version is stamped into `Cargo.toml` and `Cargo.lock` inside the
  job and never committed, by `scripts/stamp-version.sh` (package.yml runs it). The same script makes the
  platform packages sort right: the macOS bundle's `CFBundleShortVersionString` is the numeric `0.1.8` (Apple's
  format; `CFBundleVersion` stays cargo-packager's UTC `YYYYMMDD.HHMMSS` for every build), and the `.deb`'s control
  `Version` is `0.1.8~nightly.…`, since Debian reads `-nightly…` as a revision and would rank it above `0.1.8`. The
  tag, the binary's version and the asset file names keep `0.1.8-nightly.…`; the updater compares the tag with the
  binary's version, never the plist or the package. It publishes a **pre-release** at once (not a draft, never "latest", so GitHub's latest
  release and the Stable channel still see only stable releases), then keeps the newest 14 nightlies and deletes the
  older ones with their tags (`gh release delete --cleanup-tag`, only tags matching `v*-nightly.*`).
- Semver sorts `0.1.8-nightly.*` below `0.1.8`, so the stable release supersedes the nightlies: the updater's
  Nightly channel moves to it. `release.yml` ignores `v*-nightly*` tags.
- `SKYELAB_UPDATE_FAKE=<tag>[,<tag>…]` fakes the release list in a debug build (each channel takes its newest):
  `SKYELAB_UPDATE_FAKE=v0.1.7,v0.1.8-nightly.20261128` shows the nightly offer on Nightly and nothing on Stable.
- `test/acceptance/packaging.sh workflow` lints the three workflows (actionlint) and checks these rules;
  `packaging.sh versions` stamps a temp copy and checks the plist and the Debian order (`brew install dpkg`).

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
- WebView2's `NavigateToString` load is a navigation of its own (not reported as `about:blank` the way WKWebView's
  `loadHTMLString` is). A navigation handler that allows only `about:blank` cancels the page's own load, and the
  page measures as an empty viewport (`wkwebview_page`: "height 80"). `page::Page` allows the first navigation,
  the string load, then only `about:blank`.
- A WebView2 page locked by `WebResourceRequested` still opens bare TCP connections (no bytes sent): Chromium
  preconnects outside that hook, and `<link rel=preconnect>` is outside CSP (`wkwebview_page`: "reached the local
  server 2 times: (no request), (no request)"). The page's environment gets `--host-resolver-rules` mapping local
  names and addresses to `^NOTFOUND` (`page::PAGE_BROWSER_ARGS`); `~NOTFOUND` is not the sentinel, it resolves as a
  name. `--dns-prefetch-disable` no longer exists. Browser args are per environment, so the page has its own data folder.
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
- The per-user process limit (`kern.maxprocperuid`) is far lower than on a big dev Mac (5,333 here), and zombies
  count. A child that is started and never waited stays a zombie until the test binary exits; enough of them and
  every spawn fails with "Resource temporarily unavailable (os error 35)", so fake agents never start and app tests
  time out "waiting for the question". `frozen::freeze` started two git children and dropped the second unwaited
  whenever the first failed (every call outside a repo). Reap every `Captured` child, even on an error path
  (`skyelab-git/tests/no_zombie_children.rs`). Every handle from `skyelab_core::process::spawn` and `Captured` is a
  `process::Owned`: dropped unreaped, it kills and reaps its child (off the caller's thread when the child is slow), and
  one meant to outlive it is `detach`ed (`skyelab-core/tests/no_zombie_children.rs`). To check locally, count the test binary's `Z` children with
  `ps -A -o ppid=,stat=` while the suite runs.
- Real file events (FSEvents) arrive late and in batches on a loaded runner: assert "a few", not an exact count,
  when the count is about coalescing (`skyelab-git/tests/watch.rs`).
  Before counting, wait until a whole second passes with no report (`settle` there), not a fixed sleep: setup's
  events arrived after the old 800 ms and counted as the next step's.
- On GitHub's macos-26 image cargo was not in `~/.cargo/bin` when Xcode ran the "Build Rust" phase ("env: cargo:
  No such file or directory"), so the phase takes cargo from the PATH it inherits, `~/.cargo/bin` only as a
  fallback (`mobile/ios/build-rust.sh`).

**Android (Gradle dependency verification)**

- `verification-metadata.xml` made from a warm Gradle cache misses files a clean machine fetches: a BOM's `.module`
  where the cache had its `.pom`, a newer parent POM. Regenerate with an empty `GRADLE_USER_HOME`, then run the
  build again with another empty one in verify mode. Gradle's write mode does not record some of these (buildscript
  classpath BOMs); add them by hand with the sha256 of the file from Maven Central, checked against Central's
  `.sha1`.

**All platforms**

- Tests never run the developer's shell setup. A terminal with no command runs `$SHELL -l` for users, which sources
  ~/.zshrc and friends (on one Mac that started `op daemon` on every test run). `skyelab-terminal`'s `test-support`
  feature, on only through dev-dependencies (its own and `skyelab-app`'s), swaps in `/bin/sh` without `-l` (reads
  only `$ENV`, which the env allowlist drops) or `powershell.exe -NoProfile`. A crate whose tests open a default
  terminal turns the feature on in its dev-dependencies; `tests/no_user_rc.rs` proves no rc is read.

- A test that unwatches and then expects silence makes a call first (`settled` in skyelab-client's tests): `unwatch`
  is a queued command, and an event pushed right after it could reach the connection first.
- A test that answers the agent and then waits for both the agent's reply and `PermissionResolved` must accept them
  in either order: the session sends the resolved event after the answer is queued, so a fast agent's reply can come
  first (`ask_user_question_answers_reach_the_agent` failed this way on Linux).

**Timing and cache**

- A cold build of the test targets takes ~15 min on Linux and Windows (a warm cache: 2–5 min). Under `cargo test`
  the app's unit tests took ~10 min on Linux (one binary, ~900 tests that wait on real processes) against ~1 min on
  Windows (the Unix-only fake-agent tests don't run there); hence Linux's three shards. Jobs time out at 70 min
  (test; its test step at 55) and 30 min (clippy).
- `Swatinem/rust-cache` saves only after a successful job by default. The test job had never passed, so it
  rebuilt from nothing every run; `cache-on-failure: true` keeps the build either way.

**Reading a failed run**

- `gh run view --job <id> --log` can return only part of a long job log. In run 45459381411 it stopped halfway
  through the Linux tests, so the run looked hung. The full log comes from the API:
  `curl -sSL -H "Authorization: Bearer $(gh auth token)" https://api.github.com/repos/OpenSkye-Labs/skyelab/actions/jobs/<id>/logs`.
  Search it for `FAIL [`, `TIMEOUT`, `panicked` and `error: test run failed`. The failure annotation's line number tells you how
  long the full log is.
- A real hang: nextest kills the test at 3 min and names it `TIMEOUT`; the run goes on. `SLOW [> 60s]` lines name
  every test past 60 s. The step's own 55 min limit (15 before the job's 70, so the cache still saves) is the
  backstop.

**Any loaded runner**

- Tests that wait on real processes must hold at high load: see the rules under Testing in [gpui.md](gpui.md).
  A wait that times out at 60 s is usually a wait for a state that was skipped, not a slow machine.
- A signal sent by spawning `kill` can be lost: a loaded machine can refuse the new process (EAGAIN), and the error
  was ignored. `cursor_interrupt_ends_hanging_turn` then waited out its fake's 60 s turn at load 200. `interrupt_group` calls
  `rustix::process::kill_process_group`. (`kill_group` still spawns `kill`, which `hardening.rs` spies on.)
