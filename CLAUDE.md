# SkyeLab

A native agentic development environment (ADE) in Rust + GPUI, inspired by T3 Code. Its reason to exist:
**every folder is bound to exactly one provider account, and that binding is enforced.** An agent can never start
in a folder under the wrong Claude account.

Read before working: [docs/architecture.md](docs/architecture.md) (what we're building, crate layout, roadmap)
and [docs/gpui.md](docs/gpui.md) (how to write GPUI code here). Both are rules, not suggestions.

## How to work

- **Every feature, fix or refactor goes through alleo-loop**: `/loop` for the whole cycle, or its steps
  `/loop-plan` → `/loop-build` → `/verify` → `/loop-review`. Plans live in `.agent/plans/`.
- UI changes are checked in the running app with `/loop-verify-ui` (native window), not just by compiling.
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
- Domain logic lives in `skyelab-core` and never imports GPUI. It must be unit-testable without a window.
- Add a dependency to `[workspace.dependencies]` first, then `dep.workspace = true` in the crate.
- Don't add a crate for a few lines. Don't add a new workspace crate until code actually needs the boundary.

## Accounts invariant (never break)

- `AccountsConfig::resolve(dir)` returns `None` for unbound folders. Callers must ask the user. **Never fall back to
  a default account.** Deepest binding wins, and path matching is component-wise.
- Every agent process is launched with the env from `AccountsConfig::launch_env(dir)` (today: `CLAUDE_CONFIG_DIR`).
  Never inherit the parent's `CLAUDE_CONFIG_DIR`. Clear it, then set it.
- Config: `~/.config/skyelab/accounts.toml` (platform config dir). An unknown account id is a load error, not a warning.

## Platform setup

- macOS: full Xcode plus the Metal Toolchain (`xcodebuild -downloadComponent MetalToolchain`), or `gpui-pre-apple` fails
  with "metal shader compilation failed".
- Linux: `libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libvulkan1 libfontconfig-dev clang cmake`.
- Windows: MSVC toolchain, nothing else.

## Commands

```sh
cargo run -p skyelab-app          # the desktop app (binary: skyelab)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
