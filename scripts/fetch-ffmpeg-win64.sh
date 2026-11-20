#!/usr/bin/env bash
# Copyright 2026 PodBridge Contributors
# SPDX-License-Identifier: Apache-2.0
#
# Downloads the official LGPL-2.1 static Windows x64 FFmpeg binary from BtbN/FFmpeg-Builds
# and stages it as a Tauri v2 sidecar (`src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe`).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$REPO_ROOT/src-tauri/binaries"
TARGET_BIN="$BIN_DIR/ffmpeg-x86_64-pc-windows-msvc.exe"
TARGET_LIC="$BIN_DIR/FFMPEG-LGPL-2.1.txt"

mkdir -p "$BIN_DIR"

if [[ -f "$TARGET_BIN" && -f "$TARGET_LIC" ]]; then
  echo "==> Windows LGPL FFmpeg sidecar already present: $TARGET_BIN"
  exit 0
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

URL="https://github.com/BtbN/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-win64-lgpl.zip"
echo "==> Downloading LGPL-2.1 Windows x64 FFmpeg from $URL ..."
curl -sSL "$URL" -o "$TMP_DIR/ffmpeg-lgpl.zip"

unzip -q "$TMP_DIR/ffmpeg-lgpl.zip" -d "$TMP_DIR/unpacked"
EXTRACTED_EXE="$(find "$TMP_DIR/unpacked" -type f -name "ffmpeg.exe" | head -n 1)"
EXTRACTED_LIC="$(find "$TMP_DIR/unpacked" -type f -iname "LICENSE*" | head -n 1)"

if [[ -z "$EXTRACTED_EXE" ]]; then
  echo "ERROR: ffmpeg.exe not found inside downloaded archive" >&2
  exit 1
fi

cp "$EXTRACTED_EXE" "$TARGET_BIN"
chmod +x "$TARGET_BIN"

if [[ -n "$EXTRACTED_LIC" ]]; then
  cp "$EXTRACTED_LIC" "$TARGET_LIC"
fi

echo "==> Staged LGPL FFmpeg sidecar ($(du -h "$TARGET_BIN" | cut -f1))"
