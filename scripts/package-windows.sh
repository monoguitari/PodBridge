#!/usr/bin/env bash
# Copyright 2026 PodBridge Contributors
# SPDX-License-Identifier: Apache-2.0
#
# Builds both the self-contained Windows NSIS installer (-setup.exe) and standalone Portable ZIP
# for PodBridge from WSL / Linux using cargo-xwin + makensis.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"

"$REPO_ROOT/scripts/fetch-ffmpeg-win64.sh"

echo "==> Building React + TypeScript production UI..."
npm --prefix "$REPO_ROOT/ui" run build

echo "==> Cross-compiling PodBridge Windows x64 release & NSIS installer..."
(
  cd "$REPO_ROOT/src-tauri"
  npx --prefix "$REPO_ROOT/ui" tauri build \
    --runner cargo-xwin \
    --target x86_64-pc-windows-msvc \
    --bundles nsis \
    --config '{"build":{"beforeBuildCommand":""}}'
)

REL_DIR="$REPO_ROOT/src-tauri/target/x86_64-pc-windows-msvc/release"
NSIS_EXE="$(find "$REL_DIR/bundle/nsis" -maxdepth 1 -type f -name "*-setup.exe" | head -n 1)"
OUT_DIR="$REPO_ROOT/dist-packages"
PORTABLE_STAGE="$OUT_DIR/PodBridge-Portable"

rm -rf "$OUT_DIR"
mkdir -p "$PORTABLE_STAGE"

if [[ -n "$NSIS_EXE" ]]; then
  cp "$NSIS_EXE" "$OUT_DIR/"
fi

cp "$REL_DIR/podbridge.exe" "$PORTABLE_STAGE/PodBridge.exe"
cp "$REPO_ROOT/src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe" "$PORTABLE_STAGE/ffmpeg.exe"
cp "$REPO_ROOT/LICENSE" "$PORTABLE_STAGE/LICENSE.txt"
cp "$REPO_ROOT/THIRD_PARTY_NOTICES.md" "$PORTABLE_STAGE/THIRD_PARTY_NOTICES.md"
if [[ -f "$REPO_ROOT/src-tauri/binaries/FFMPEG-LGPL-2.1.txt" ]]; then
  cp "$REPO_ROOT/src-tauri/binaries/FFMPEG-LGPL-2.1.txt" "$PORTABLE_STAGE/FFMPEG-LGPL-2.1.txt"
fi

python3 - "$PORTABLE_STAGE" "$OUT_DIR/PodBridge_0.1.0_x64-portable.zip" << 'PY'
import os, sys, zipfile
src_dir, zip_path = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as zf:
    for root, _, files in os.walk(src_dir):
        for fname in sorted(files):
            full = os.path.join(root, fname)
            rel = os.path.relpath(full, os.path.dirname(src_dir))
            zf.write(full, rel)
PY

rm -rf "$PORTABLE_STAGE"
echo "==> Windows distribution packages ready in $OUT_DIR:"
ls -lh "$OUT_DIR"
