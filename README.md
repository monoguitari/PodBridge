# PodBridge

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

**PodBridge** is an open-source, low-latency Windows desktop application (**Rust + Tauri v2 + React/TypeScript**) for transferring music, cover artwork, and playlists from a PC to stock-firmware iPod Classic and iPod Nano devices—with first-class support for the **iPod Classic 6th Gen (`MB147LL`)**.

## Supported Devices (Stock Apple Firmware)

| Family | Generations | Database Signing | Cover Art Formats (`.ithmb` RGB565 LE) |
| --- | --- | --- | --- |
| **iPod Video** | 5G / 5.5G | Standard `iTunesDB` | `1028` (`100×100`), `1029` (`200×200`) |
| **iPod Classic** | 6G (`MB147LL`), 6.5G, 7G | `hash58` (FireWire GUID) | `1055` (`128×128`), `1060` (`320×320`), `1061` (`55×55`, 4B row-aligned) |
| **iPod Nano** | 3G | `hash58` (FireWire GUID) | `1055` (`128×128`), `1060` (`320×320`), `1061` (`55×55`, 4B row-aligned) |
| **iPod Nano** | 4G | `hash58` (FireWire GUID) | `1055`, `1071` (`240×240`), `1074` (`50×50`), `1078` (`80×80`) |
| **iPod Nano** | 5G | `hash72` (`HashInfo` / DB seed) | `1056`, `1073` (`240×240`), `1074` (`50×50`), `1078` (`80×80`) |

## Workspace Architecture

- `crates/podbridge-crypto` — Clean-room `hash58` (HMAC-SHA1 / FireWire GUID) and `hash72` (AES-128-CBC / `HashInfo`) signing & verification.
- `crates/podbridge-itdb` — Lossless binary `iTunesDB` parser & writer (`mhbd`, `mhsd` `[4, 1, 3, 2, 5]`), playlist/M3U8 support, and O(1) duplicate detection.
- `crates/podbridge-artwork` — `ArtworkDB` parser/writer and 4-byte stride-aligned RGB565 little-endian `.ithmb` thumbnail encoder with SHA-256 album deduplication.
- `crates/podbridge-device` — Native Win32 `SetupAPI` / `CfgMgr32` USB PID & FireWire GUID discovery, one-click Safe Eject, and cross-platform Mock iPod Folder mode.
- `crates/podbridge-pipeline` — Multi-core `lofty` metadata scanner + cancellable FFmpeg transcode pool (`FLAC -> ALAC`, `OGG/Opus -> AAC`) feeding a single 2 MB buffered sequential USB writer with rotating `iTunesDB` backups and atomic batch commits.
- `src-tauri` & `ui` — Tauri v2 Windows shell paired with a 60 fps virtualized React + TypeScript Dark-First Studio UI.

## Development & Building

```bash
# Bootstrap Rust + Windows cross-compile toolchain on WSL / Ubuntu
./scripts/setup-wsl-toolchain.sh

# Run workspace unit & integration tests
cargo test --workspace

# Cross-check or build Windows x86_64 MSVC target from WSL/Linux
cargo xwin check --workspace --target x86_64-pc-windows-msvc
```

## License

Licensed under the [Apache License, Version 2.0](LICENSE). See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for dependency and external FFmpeg subprocess compliance details.
