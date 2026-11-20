// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

import { UploadCloud } from "lucide-react";

interface DropOverlayProps {
  visible: boolean;
  hoveredPaths: string[];
  deviceName: string | null;
  losslessTargetLabel: string;
}

export function DropOverlay({
  visible,
  hoveredPaths,
  deviceName,
  losslessTargetLabel,
}: DropOverlayProps) {
  if (!visible) return null;

  const needsTranscode = hoveredPaths.some((p) => {
    const lower = p.toLowerCase();
    return (
      lower.endsWith(".flac") ||
      lower.endsWith(".ogg") ||
      lower.endsWith(".opus") ||
      lower.endsWith(".wma")
    );
  });

  return (
    <div className="drop-overlay" role="status" aria-live="polite">
      <div className="drop-overlay-box">
        <div className="drop-icon-ring">
          <UploadCloud size={30} />
        </div>
        <div style={{ fontSize: 17, fontWeight: 700, marginTop: 10 }}>
          {deviceName
            ? `Drop to transfer onto ${deviceName}`
            : "Connect an iPod or open Mock Mode first"}
        </div>
        <div style={{ color: "var(--text-secondary)", marginTop: 4 }}>
          {hoveredPaths.length > 0
            ? `${hoveredPaths.length} item${hoveredPaths.length === 1 ? "" : "s"} ready · Native MP3/M4A/WAV pass-through`
            : "Audio files, album folders, and .m3u / .m3u8 playlists supported"}
        </div>
        {needsTranscode && (
          <div style={{ marginTop: 10 }}>
            <span className="badge-pill">
              Auto-transcode active ({losslessTargetLabel} / 256k AAC)
            </span>
          </div>
        )}
      </div>
    </div>
  );
}
