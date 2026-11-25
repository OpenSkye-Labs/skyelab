# Contributing to SkyeLab

Thanks for helping. SkyeLab is early, so expect things to move. Small, focused pull requests get merged fastest.

Before a large change, open an issue or a discussion first so we can agree on the approach.

## Read first

- [docs/architecture.md](docs/architecture.md): what we're building, the crate layout and the roadmap.
- [docs/gpui.md](docs/gpui.md) and [docs/gpui/](docs/gpui): how GPUI code is written here. These are rules.
- [docs/security.md](docs/security.md): the security model your change must keep.
- [docs/debugging.md](docs/debugging.md): the debug overlay, traces and profilers.

## Set up

Install the latest stable Rust with [rustup](https://rustup.rs). `rust-toolchain.toml` picks the channel.

- **macOS 14 or later:** full Xcode plus the Metal Toolchain (`xcodebuild -downloadComponent MetalToolchain`).
  Without it the build fails with "metal shader compilation failed".
- **Linux (Debian/Ubuntu names):**
  `sudo apt-get install libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libvulkan1 libfontconfig-dev clang cmake libwebkit2gtk-4.1-dev libgtk-3-dev`
- **Windows:** the MSVC toolchain, nothing else.

To run agents you also need [Claude Code](https://docs.claude.com/en/docs/claude-code) (`claude`) and, for Codex
accounts, the [Codex CLI](https://github.com/openai/codex) (`codex`). You don't need either to build or run the tests.

## Build, run, test

```sh
cargo run -p skyelab-app      # the desktop app (binary: skyelab)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

A debug build keeps its own config and data dirs (`skyelab-dev`), so it never touches an installed SkyeLab's data.

### The gate

A change is done when all three pass. CI runs the same on macOS, Linux and Windows.

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### Windows compile check from macOS

After touching `cfg(windows)` or `cfg(unix)` code, run `scripts/check-windows.sh`. It runs clippy for
`x86_64-` and `aarch64-pc-windows-msvc` (tests can't run). One-time setup:

```sh
brew install llvm
cargo install cargo-xwin --locked
rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc
```

The first run downloads the MSVC CRT and Windows SDK (about 1.1 GB).

### The mobile workspace

`mobile/` (the iOS and Android client) is a separate Cargo workspace, outside the gate above and checked by
`.github/workflows/mobile.yml`. In `mobile/`, run `scripts/fetch-gpui-mobile.sh` before any cargo command, then the
same clippy and test commands. `scripts/run-ios.sh` and `scripts/run-android.sh` run it. See [docs/mobile.md](docs/mobile.md).

## Check UI changes in the running app

Compiling isn't enough for a UI change. Run the app and look at it, in light and dark.

**Never use your real accounts for this.** Start the app with a throwaway `HOME`, so your real `accounts.toml`,
logins and thread history are never touched, and with fakes instead of the real CLIs:

```sh
export HOME=$(mktemp -d)
SKYELAB_CLAUDE_BIN=$PWD/crates/skyelab-app/tests/fixtures/fake-claude.sh \
SKYELAB_GH_BIN=$PWD/crates/skyelab-git/tests/fixtures/fake-gh.sh \
SKYELAB_VERIFY_THEME=dark \
  cargo run -p skyelab-app
```

Useful switches (debug builds only):

| Variable | What it does |
|---|---|
| `SKYELAB_CLAUDE_BIN`, `SKYELAB_CODEX_BIN` | A fake agent. Its prompt words trigger cases (`permit`, `question`, `plan`, `table`, `stream`, `crash`, ...). |
| `SKYELAB_GH_BIN` | A fake `gh` for Push, Open PR and the PR and issue lists. |
| `SKYELAB_TAILSCALE_BIN`, `SKYELAB_ADB_BIN`, `SKYELAB_XCRUN_BIN` | Fake tailnet and devices, so no real machine, simulator or emulator shows up. |
| `SKYELAB_VERIFY_THEME=dark` or `light` | Forces a theme without changing the system appearance. |
| `SKYELAB_VERIFY_ACTIONS=...` | Runs actions, keystrokes and clicks two seconds apart, for scripted checks. |
| `SKYELAB_DEBUG=1`, `SKYELAB_DEBUG_RENDERS=1` | The debug overlay; renders per second per view on stderr. |

Push, Pull and Fetch are tested against a local fake https remote:
`cargo run -p skyelab-git --example fake_https_remote -- <folder>`. Never point SkyeLab at a real remote, the real
`gh`, or a real simulator in a check. The full list of fixtures and their cases is in the "How to work" section of
[CLAUDE.md](CLAUDE.md).

A hand-written binding in the throwaway `accounts.toml` needs `resolved = "<same path>"`, or it loads as stale.

## Rules a change must keep

- **The accounts invariant.** A folder resolves to its bound account or to nothing. Never fall back to a default
  account. Every agent starts with the env from `AccountsConfig::launch_env(dir)`, and every child process (agent,
  terminal, git, gh) starts from `env_clear()` plus the allowlist in `skyelab_core::env`, never from a hand-written
  `env_remove` list.
- **Domain logic lives in `skyelab-core`** and never imports GPUI, so it can be tested without a window.
- **No `unsafe`.** It is denied workspace-wide. If FFI ever needs it, it goes in one `-sys`-style crate with a
  `// SAFETY:` comment on every block.
- **Errors:** `anyhow` in app glue, `thiserror` only when a caller matches on variants, `.context(..)` at I/O
  boundaries. No `unwrap()`/`expect()` outside tests, except for an invariant with a message saying why it can't fail.
  Don't add lint allows to get past clippy.
- **UI uses SkyeLab's own components** in `crates/skyelab-app/src/ui/` ([docs/design/components.md](docs/design/components.md),
  and the UI Gallery in debug builds). Clippy bans gpui-component's button, switch, checkbox and tooltip in the app.
- **GPUI lessons go in the docs.** When you hit a GPUI bug, redraw or performance trap, or find a pattern that
  works, add a short rule with the why to [docs/gpui.md](docs/gpui.md) or a file in [docs/gpui/](docs/gpui), in the
  same pull request.
- **Never log prompts, env values or tokens.** Debug logging goes through `skyelab_core::debug::record`, which redacts.
- **Dependencies:** add them to `[workspace.dependencies]` first, then `dep.workspace = true` in the crate. Use the
  newest release. `gpui-pre` stays at the exact version `gpui-component` pins. Don't add a crate for a few lines.
- **User-facing changes update the docs:** [README.md](README.md), [docs/user-guide.md](docs/user-guide.md) and its
  pages, and [CHANGELOG.md](CHANGELOG.md) under "Unreleased".

## Commits and pull requests

- Work on a branch and keep commits small. Messages follow a loose conventional style:
  `feat(core): …`, `fix(app): …`, `docs: …`, `refactor(git): …`.
- Open the pull request against `main` and fill in the template. CI must be green.
- Say how you checked the change, and add a screenshot for UI changes (light and dark).
- Never commit secrets, real account folders, real machine names or personal paths.

## Translated READMEs

`README.md` (English) is the source of truth. The translations sit next to it as `README.<lang>.md` (`zh-CN`,
`zh-TW`, `ja`, `ko`, `es`, `pt-BR`, `de`, `fr`, `ru`).

- When you change `README.md`, update the translations you can, or say in the pull request which ones are behind.
  An outdated translation is fine for a while; a wrong one is not.
- Translate faithfully. Keep code, commands, paths, file names, key names and product names (SkyeLab, Claude Code,
  Codex, GPUI, Tailscale) as they are, and keep the same images and links.
- Each translation starts with the language switcher and a note naming the date it was last synced with the
  English README. Update that date when you sync it.
- To add a language, copy the closest translation, translate it, and add the language to the switcher line at the
  top of every README (including this list).
- Keep each README under the repo's 5,000-token file limit (about 20 KB); the pre-commit hook checks it.

## Questions

Ask in [GitHub Discussions](https://github.com/OpenSkye-Labs/skyelab/discussions) or open an issue. Report security
problems privately, as described in [SECURITY.md](SECURITY.md), never in a public issue.

By contributing you agree that your contribution is licensed under the [MIT License](LICENSE), and to follow the
[Code of Conduct](CODE_OF_CONDUCT.md).
