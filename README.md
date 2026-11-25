<p align="center">
  <img src="docs/images/banner.png" alt="SkyeLab" width="720">
</p>

<p align="center">
  <b>English</b> ·
  <a href="README.zh-CN.md">简体中文</a> ·
  <a href="README.zh-TW.md">繁體中文</a> ·
  <a href="README.ja.md">日本語</a> ·
  <a href="README.ko.md">한국어</a> ·
  <a href="README.es.md">Español</a> ·
  <a href="README.pt-BR.md">Português (Brasil)</a> ·
  <a href="README.de.md">Deutsch</a> ·
  <a href="README.fr.md">Français</a> ·
  <a href="README.ru.md">Русский</a>
</p>

<p align="center">
  <b>A native desktop app for running coding agents, where every folder is locked to one account.</b>
</p>

<p align="center">
  <a href="https://github.com/OpenSkye-Labs/skyelab/actions/workflows/ci.yml"><img src="https://github.com/OpenSkye-Labs/skyelab/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/OpenSkye-Labs/skyelab/releases"><img src="https://img.shields.io/github/v/release/OpenSkye-Labs/skyelab?include_prereleases&sort=semver" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
</p>

SkyeLab is an agentic development environment (ADE) written in Rust with [GPUI](https://www.gpui.rs/). You run
Claude Code and Codex agents in threads over your projects, review what they change, and keep a terminal, a browser
and your simulators next to them. It takes its overall shape from [T3 Code](https://github.com/pingdotgg/t3code).

> **Status: early.** Expect rough edges and breaking changes. macOS is the main platform.

## Why it exists

If you have a work account and a personal account, sooner or later an agent runs in the wrong one. SkyeLab makes
that impossible: **every folder is bound to exactly one account (Claude or Codex), and the binding is enforced.**

- The agent in your work repo always runs on the work account; the one in your side project on your personal one.
- The deepest binding wins, so `~/work` can be one account and `~/work/oss-fork` another.
- A folder with no binding never falls back to a default. SkyeLab asks you to bind it first.
- If a bound path later points somewhere else (a swapped symlink, a moved folder), the binding turns *stale* and no
  agent starts there until you confirm it again.
- GitHub works the same way: each account can have its own `gh` login, used for commits, pushes and pull requests.

## Features

- **Threads** for Claude Code and Codex, with a rich timeline: streamed replies and thinking, tool cards, inline
  diffs, shell output, todo lists, subagents, questions, plans and tool approvals you answer in place.
- **Composer** with file, thread and pull request mentions as inline chips, images and files, `/` commands, a model
  and effort picker, a permission mode per thread, a queue while the agent works, and dictation on macOS.
- **Review and ship**: a Changes panel with line comments, per-turn checkpoints and rewind, worktrees per thread,
  branches, commit, push and pull requests with each account's own GitHub login.
- **Command palette** (<kbd>⌘K</kbd>) to run any action, switch project or open a thread (also found by its
  messages); <kbd>⌘P</kbd> finds a file. Every shortcut can be rebound.
- **Side tools**: an embedded terminal with splits, a browser pane agents can drive, and a device hub for iOS
  Simulators and Android emulators.
- **Many threads at once**: one list across projects, scheduled tasks, side chats, sending one prompt to several
  models, and agents that start and drive other threads (on the same account only).
- **Your machines**: share a machine over your Tailscale tailnet and run threads on it from another one. A mobile
  client is in progress.
- **Onboarding, themes and updates**: a first-run guide that imports your existing agent folders, five themes with
  light and dark sides, and in-app updates checked against the release's SHA-256 list.

The [user guide](docs/user-guide.md) covers each of these in full.

## Screenshots

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/thread-dark.png">
  <img src="docs/images/thread-light.png" alt="A thread with a rich timeline and its Changes panel">
</picture>

| | |
|---|---|
| <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/composer-dark.png"><img src="docs/images/composer-light.png" alt="The composer with mention chips and the model picker"></picture> | <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/palette-dark.png"><img src="docs/images/palette-light.png" alt="The command palette"></picture> |
| The composer, with mention chips and the model picker | The command palette |
| <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/onboarding-dark.png"><img src="docs/images/onboarding-light.png" alt="First-run onboarding"></picture> | <picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/themes-dark.png"><img src="docs/images/themes-light.png" alt="Settings, Appearance"></picture> |
| First-run onboarding | Settings → Appearance |

## Install

Download the latest build from the [releases page](https://github.com/OpenSkye-Labs/skyelab/releases):

| Platform | File |
|---|---|
| macOS 14+ (Apple silicon) | `SkyeLab_0.1.1_aarch64.dmg`: open it and drag SkyeLab to Applications. |
| Windows (x64) | `skyelab_0.1.1_x64-setup.exe` |
| Linux (x86_64) | `skyelab_0.1.1_amd64.deb` (`sudo apt install ./skyelab_0.1.1_amd64.deb`) or `skyelab_0.1.1_x86_64.AppImage` (`chmod +x`, then run it) |

**First open on macOS.** Release builds are not notarized yet, so Gatekeeper blocks the first open. Open SkyeLab
once, then go to System Settings → Privacy & Security and click **Open Anyway**. Windows SmartScreen may ask too
(**More info** → **Run anyway**). After that, **Check for Updates** in the app keeps it current.

SkyeLab does not install the agents. Install [Claude Code](https://docs.claude.com/en/docs/claude-code) (`claude`),
the [Codex CLI](https://github.com/openai/codex) (`codex`) if you use Codex, and the
[GitHub CLI](https://cli.github.com) (`gh`) for pushes and pull requests.

On first start, onboarding finds your existing `~/.claude*` and `~/.codex*` folders, adds them as accounts, and
imports the projects their agents worked in, each on the account you confirm. [Getting started](docs/getting-started.md)
walks through the rest, and helps if you are coming from T3 Code.

## Build from source

You need the latest stable Rust ([rustup](https://rustup.rs)); `rust-toolchain.toml` picks it.

- **macOS 14+:** full Xcode plus the Metal toolchain (`xcodebuild -downloadComponent MetalToolchain`). Without it
  the build fails with "metal shader compilation failed".
- **Linux:** `libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libvulkan1 libfontconfig-dev clang cmake libwebkit2gtk-4.1-dev libgtk-3-dev`
- **Windows:** the MSVC toolchain.

```sh
cargo run -p skyelab-app                              # run a debug build
cargo install cargo-packager --locked                 # or build the macOS app and disk image:
cargo packager --release -p skyelab-app -f app,dmg    # writes target/release/SkyeLab.app and a .dmg
```

## Documentation

- [Getting started](docs/getting-started.md): install, accounts, bindings, importing sessions, the everyday loop.
- [User guide](docs/user-guide.md): every feature, page by page.
- [Security model](docs/security.md): what SkyeLab guarantees and how.
- [Architecture](docs/architecture.md): crates, the in-process daemon, decisions and roadmap.
- [Mobile](docs/mobile.md): the iOS and Android client.
- [Changelog](CHANGELOG.md)

| Platform | Status |
|---|---|
| macOS 14+ (Apple silicon) | Primary. Used and tested daily. |
| Windows, Linux | Built and tested in CI. Less tested by hand. |
| iOS, Android | Remote client in progress under [`mobile/`](mobile). |

## Contributing

Contributions are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) covers setup, the checks every change must pass, how
to test UI changes without your real accounts, and [how to update the translated READMEs](CONTRIBUTING.md#translated-readmes). Please follow the
[Code of Conduct](CODE_OF_CONDUCT.md). Questions go to
[Discussions](https://github.com/OpenSkye-Labs/skyelab/discussions).

## Security

Found a security problem? Report it privately, as [SECURITY.md](SECURITY.md) describes. Don't open a public issue.

## License

[MIT](LICENSE). Credit to [T3 Code](https://github.com/pingdotgg/t3code) (MIT) for the design this follows.
