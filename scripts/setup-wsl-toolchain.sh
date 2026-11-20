#!/usr/bin/env bash
# Copyright 2026 PodBridge Contributors
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail

export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
mkdir -p "$HOME/.cargo/bin" "$HOME/.local/bin"

echo "==> Checking Rust toolchain..."
if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain stable
fi

rustup target add x86_64-pc-windows-msvc
rustup component add rustfmt clippy llvm-tools-preview

LLVM_BIN="$(dirname "$(find "$HOME/.rustup/toolchains" -name rust-lld | head -n 1)")"
if [[ -n "$LLVM_BIN" ]]; then
  ln -sf "$LLVM_BIN/rust-lld" "$HOME/.local/bin/lld-link"
  ln -sf "$LLVM_BIN/llvm-ar" "$HOME/.local/bin/llvm-lib"
fi

echo "==> Checking cargo-xwin..."
if ! command -v cargo-xwin >/dev/null 2>&1; then
  XWIN_VER="v0.23.1"
  curl -sSL "https://github.com/rust-cross/cargo-xwin/releases/download/${XWIN_VER}/cargo-xwin-${XWIN_VER}.x86_64-unknown-linux-musl.tar.gz" \
    | tar -xz -C "$HOME/.cargo/bin"
fi

echo "==> Toolchain ready:"
rustc --version
cargo --version
cargo-xwin --version
