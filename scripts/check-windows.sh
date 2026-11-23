#!/usr/bin/env bash
# Cross-check the Windows build from macOS: clippy for the whole workspace on the MSVC targets.
# Setup (once): brew install llvm; cargo install cargo-xwin --locked;
#               rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc
# cargo-xwin downloads the MSVC CRT and Windows SDK (~1 GB) into its cache on first run.
# Usage: scripts/check-windows.sh [target...]   (default: x86_64 and aarch64 MSVC)
set -euo pipefail
cd "$(dirname "$0")/.."

llvm_bin="$(brew --prefix llvm 2>/dev/null || true)/bin"
# llvm is keg-only; gpui's build script needs llvm-rc and the C deps need clang-cl.
[[ -x "$llvm_bin/llvm-rc" ]] || { echo "llvm-rc not found: brew install llvm" >&2; exit 1; }
command -v cargo-xwin >/dev/null || { echo "cargo-xwin not found: cargo install cargo-xwin --locked" >&2; exit 1; }
export PATH="$llvm_bin:$PATH"
# A separate target dir, so the host build's cache is not thrashed.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/windows}"

targets=("$@")
[[ ${#targets[@]} -gt 0 ]] || targets=(x86_64-pc-windows-msvc aarch64-pc-windows-msvc)
for t in "${targets[@]}"; do
  echo "== clippy $t"
  # clang, not clang-cl: ring builds aarch64 with clang, which rejects clang-cl flags (/imsvc).
  cargo xwin clippy --cross-compiler clang --workspace --all-targets --target "$t" -- -D warnings
done
