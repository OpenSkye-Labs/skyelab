# SkyeLab

A native agentic development environment (ADE) in Rust + GPUI, inspired by T3 Code. Its reason to exist:
**every folder is bound to exactly one provider account, and that binding is enforced.** An agent can never start
in a folder under the wrong Claude account.

Read before working: [docs/architecture.md](docs/architecture.md) (what we're building, crate layout, roadmap; it links [remote.md](docs/remote.md), [orchestration.md](docs/orchestration.md) and [decisions.md](docs/decisions.md))
and [docs/gpui.md](docs/gpui.md) (how to write GPUI code here). Both are rules, not suggestions.
What users see is in [README.md](README.md) and [docs/user-guide.md](docs/user-guide.md); update them when a feature changes.

## How to work

- **Every feature, fix or refactor goes through alleo-loop**: `/loop` for the whole cycle, or its steps
  `/loop-plan` → `/loop-build` → `/verify` → `/loop-review`. Plans live in `.agent/plans/`.
- UI changes are checked in the running app with `/loop-verify-ui` (native window), not just by compiling.
  Run the app with a throwaway `HOME` (so your real `accounts.toml` is untouched) and `SKYELAB_VERIFY_TOPMOST=1`
  (debug builds: windows open above others, so nothing needs moving). `alleo-native-window` can't see those
  topmost windows. Find the window id (CGWindowList, layer > 0) and capture only that window with
  `screencapture -x -o -l <id> out.png`. Never capture the whole screen.
  Input can't reach a topmost window, so drive it with `SKYELAB_VERIFY_ACTIONS=skyelab::NewTerminal,skyelab::OpenSettings`
  (entries run two seconds apart, so pages load and tabs draw in between; `keys:h i cmd-enter` types into the focused element, `click:x;y` clicks at a window point,
  measured from below the title bar, so subtract its 32 pt from a window screenshot's y at 1x). Fake the agent with
  `SKYELAB_CLAUDE_BIN=crates/skyelab-app/tests/fixtures/fake-claude.sh` (absolute path; `fake-claude-delegate.sh` also
  delegates: the prompt `delegate <project id>` calls SkyeLab's `start_thread` tool, `browse <http url>` calls `browser_open`
  and `title` evaluates `document.title` in that tab), Codex with
  `SKYELAB_CODEX_BIN=crates/skyelab-agent/tests/fixtures/fake-codex.sh` (absolute path; a Codex account bound to the
  project), and `gh` (Push, Open PR, the
  GitHub login check) with `SKYELAB_GH_BIN=crates/skyelab-git/tests/fixtures/fake-gh.sh` (absolute path; it is logged in
  when the account's GitHub dir has a `fake-login` file). Never verify against the real `gh`. All of these are debug builds only.
- Done means `/verify` passes. That is `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace`.
- Commit small, on a branch, conventional-ish messages (`feat(core): …`). Don't push to `main` directly once CI exists.

## Rust rules

- Edition 2024, stable toolchain (`rust-toolchain.toml`). Lints live in `[workspace.lints]`, and each crate opts in with `[lints] workspace = true`.
- **Always the latest versions.** Rust: latest stable (`rustup update`, then raise `rust-version` in `Cargo.toml`).
  Crates: the newest release on crates.io (`cargo search <crate>`), including major bumps. The one exception is
  `gpui-pre`, which stays at whatever exact version the latest `gpui-component` pins. Older duplicates pulled in transitively are fine.
- `unsafe` is denied workspace-wide. If FFI ever needs it (e.g. libtailscale), it goes in one `-sys`-style
  crate with `#![allow(unsafe_code)]` and a `// SAFETY:` comment on every block.
- Errors: `anyhow` in binaries and app glue; `thiserror` only when a caller actually matches on variants.
  Add `.context(..)` at I/O boundaries. No `unwrap()`/`expect()` outside tests, except for invariants, with a message saying why it can't fail.
  In tests, `clippy.toml` allows unwrap/expect inside `#[test]` fns. Helper fns in test files use `.expect("why")`.
  Never add `#![allow(clippy::unwrap_used)]` or any other lint allow to get past clippy.
- Domain logic lives in `skyelab-core` and never imports GPUI. It must be unit-testable without a window.
- Add a dependency to `[workspace.dependencies]` first, then `dep.workspace = true` in the crate.
- Don't add a crate for a few lines. Don't add a new workspace crate until code actually needs the boundary.

## Accounts invariant (never break)

- `AccountsConfig::resolve(dir)` returns `None` for unbound folders. Callers must ask the user. **Never fall back to
  a default account.** Deepest binding wins, and path matching is component-wise.
- Every agent process is launched with the env from `AccountsConfig::launch_env(dir)` (`CLAUDE_CONFIG_DIR`,
  or `CODEX_HOME` for a Codex account). Never inherit the parent's `CLAUDE_CONFIG_DIR` / `CODEX_HOME`. Clear it, then set it.
- Config: `~/.config/skyelab/accounts.toml` (platform config dir). An unknown account id is a load error, not a warning.

## Platform setup

- macOS: **14 or later** (`MACOSX_DEPLOYMENT_TARGET` in `.cargo/config.toml`). Full Xcode plus the Metal Toolchain (`xcodebuild -downloadComponent MetalToolchain`), or `gpui-pre-apple` fails
  with "metal shader compilation failed".
- Linux: `libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libvulkan1 libfontconfig-dev clang cmake libwebkit2gtk-4.1-dev libgtk-3-dev`
  (webkit/gtk: the browser pane links them).
- Windows: MSVC toolchain, nothing else.

## Commands

```sh
cargo run -p skyelab-app          # the desktop app (binary: skyelab)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

`mobile/` (iOS/Android client) is its own workspace, outside this gate and checked by `.github/workflows/mobile.yml`.
In `mobile/`, run `scripts/fetch-gpui-mobile.sh` before any cargo command, then `cargo clippy --workspace --all-targets -- -D warnings`
and `cargo test --workspace`; run it with `scripts/run-ios.sh` / `scripts/run-android.sh`. See `mobile/UPSTREAM.md`.
Shipping builds: `scripts/build-ios.sh release [simulator|device]` / `scripts/build-android.sh release` (our own shells in
`mobile/ios/` and `mobile/android-host/`; signing only from env vars, docs/mobile.md step 6). `scripts/check-hosts.sh`
keeps Android at INTERNET only with no deep links, and the Gradle wrapper pinned by checksum.

## Release

Packaging uses [cargo-packager](https://github.com/crabnebula-dev/cargo-packager); its config is
`[package.metadata.packager]` in `crates/skyelab-app/Cargo.toml`. Install it with `cargo install cargo-packager --locked`.

- **Cut a release:** bump `version` in the root `Cargo.toml`, commit, then tag `v<version>` and push the tag (the workflow fails if the two differ).
  `.github/workflows/release.yml` builds a `.dmg` (macOS, arm64), an NSIS `-setup.exe` (Windows) and a `.deb` +
  `.AppImage` (Linux), and attaches them to a **draft** GitHub release. Check the draft, then publish it.
- **Locally (macOS):** `cargo packager --release -p skyelab-app -f app,dmg` writes `target/release/SkyeLab.app` and the `.dmg`.
  Prefix `CI=true` when the terminal has no Automation permission for Finder; that skips only the dmg window layout.
  `test/acceptance/packaging.sh all` checks the icons, release profile, Info.plist and workflow.
- **Icon:** a placeholder. Edit `crates/skyelab-app/icons/icon.svg`, then run `scripts/icons.sh` on macOS (needs `cargo install resvg`).
- **Re-running** a tag's workflow makes a second draft; delete the stale draft first.
- **Signing** is skipped unless these repository secrets exist. Never commit them. Each set is all-or-nothing.
  - macOS, sign + notarize: `APPLE_CERTIFICATE` (base64 Developer ID Application `.p12`), `APPLE_CERTIFICATE_PASSWORD`,
    `APPLE_SIGNING_IDENTITY` (e.g. `Developer ID Application: Name (TEAMID)`), `APPLE_ID`,
    `APPLE_PASSWORD` (an app-specific password), `APPLE_TEAM_ID`.
  - Windows, through [Azure Artifact Signing](https://learn.microsoft.com/azure/trusted-signing/) (public code-signing
    keys must live in an HSM, so a `.pfx` secret is not an option): `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`,
    `AZURE_TENANT_ID` (an app registration with the signer role), `AZURE_ARTIFACT_SIGNING_ENDPOINT`
    (e.g. `https://eus.codesigning.azure.net`), `AZURE_ARTIFACT_SIGNING_ACCOUNT`, `AZURE_ARTIFACT_SIGNING_CERTIFICATE_PROFILE`.
  - Linux packages are not signed.
