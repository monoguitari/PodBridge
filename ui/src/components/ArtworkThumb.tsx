// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { Disc3 } from "lucide-react";
import { artworkUrlFor } from "../utils/format";

interface ArtworkThumbProps {
  deviceId: string;
  mhiiLink: number;
  seedText: string;
  sizePx: number;
  borderRadius?: number;
}

function accentGradientFor(seed: string): string {
  let hash = 2166136261;
  for (let i = 0; i < seed.length; i += 1) {
    hash ^= seed.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  const hue = Math.abs(hash) % 360;
  const hue2 = (hue + 42) % 360;
  return `linear-gradient(135deg, hsl(${hue} 42% 24%), hsl(${hue2} 48% 14%))`;
}

export function ArtworkThumb({
  deviceId,
  mhiiLink,
  seedText,
  sizePx,
  borderRadius = 6,
}: ArtworkThumbProps) {
  const [failed, setFailed] = useState(false);
  const url = failed
    ? null
    : artworkUrlFor(deviceId, mhiiLink, sizePx > 140 ? 320 : 128);

  if (url) {
    return (
      <img
        src={url}
        alt=""
        loading="lazy"
        decoding="async"
        onError={() => setFailed(true)}
        style={{
          width: sizePx,
          height: sizePx,
          borderRadius,
          objectFit: "cover",
          flexShrink: 0,
          border: "1px solid var(--border-subtle)",
          background: "var(--bg-elevated)",
        }}
      />
    );
  }

  return (
    <div
      aria-hidden="true"
      style={{
        width: sizePx,
        height: sizePx,
        borderRadius,
        flexShrink: 0,
        background: accentGradientFor(seedText),
        border: "1px solid var(--border-subtle)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        color: "rgba(255, 255, 255, 0.55)",
      }}
    >
      <Disc3 size={Math.max(13, Math.round(sizePx * 0.42))} />
    </div>
  );
}
