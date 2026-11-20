# Third-Party Notices & Open-Source Compliance

PodBridge is licensed under the **Apache License, Version 2.0** (see [LICENSE](LICENSE)).

## Dependency Licensing Policy

1. **Rust Workspace & Frontend Dependencies**:
   All direct and transitive Rust (`Cargo.toml`) and TypeScript (`ui/package.json`) libraries used by PodBridge are licensed under permissive open-source licenses (`Apache-2.0`, `MIT`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`, `Unicode-3.0`). No GPL or AGPL libraries are linked into PodBridge.

2. **Clean-Room Binary Protocol Implementation**:
   `podbridge-crypto`, `podbridge-itdb`, and `podbridge-artwork` are original, clean-room Rust implementations authored from binary format specifications and empirical hardware interoperability testing.

3. **External Audio Transcoding (`ffmpeg`)**:
   - PodBridge invokes `ffmpeg` strictly as a separate, standalone OS subprocess (`Command::new("ffmpeg")`) for optional audio transcoding (e.g., `FLAC` -> `ALAC`, `OGG`/`Opus` -> `AAC`). PodBridge does **not** link against `libavcodec`, `libavformat`, or any FFmpeg libraries.
   - When redistributing Windows installer/portable packages that include a prebuilt `ffmpeg.exe` sidecar, use an **LGPL-2.1+** build of FFmpeg (configured without `--enable-gpl` / `--enable-nonfree`, such as BtbN `win64-lgpl` releases) and include the accompanying FFmpeg LGPL-2.1 license text and source offer/pointer in the distribution archive (`licenses/FFMPEG-LGPL-2.1.txt`).

4. **Trademarks**:
   Apple, iPod, iPod Classic, iPod Nano, Cover Flow, and iTunes are trademarks of Apple Inc., registered in the U.S. and other countries. PodBridge is an independent open-source project and is not affiliated with or endorsed by Apple Inc.
