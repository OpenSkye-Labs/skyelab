## What and why

<!-- What this changes and why. Link the issue: "Fixes #123". -->

## How I checked it

<!-- Tests added, what you ran, what you looked at in the app. Add light and dark screenshots for UI changes. -->

## Checklist

- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass
- [ ] UI changes checked in the running app, in light and dark, with a throwaway `HOME` and the fakes (no real accounts)
- [ ] `scripts/check-windows.sh` passes, if I touched `cfg(windows)` or `cfg(unix)` code
- [ ] Docs updated (README, user guide, `docs/gpui` for GPUI lessons) and a line under "Unreleased" in CHANGELOG.md
- [ ] The accounts invariant holds: no default account, child processes start from `env_clear()` plus the allowlist
- [ ] No secrets, tokens, real account folders, machine names or personal paths in code, fixtures or screenshots
