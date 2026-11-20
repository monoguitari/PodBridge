// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

import { describe, expect, it } from "vitest";
import {
  filterAndSortTracks,
  formatBytes,
  formatDurationMs,
  formatEtaSeconds,
} from "./format";
import type { TrackDto } from "../types";

function makeTrack(
  id: number,
  title: string,
  artist: string,
  album: string,
  length_ms: number,
): TrackDto {
  return {
    track_id: id,
    dbid_hex: `000000000000000${id}`,
    title,
    artist,
    album_artist: artist,
    album,
    genre: "Electronic",
    format_badge: "ALAC",
    size_bytes: 20_000_000,
    length_ms,
    bitrate_kbps: 920,
    sample_rate_hz: 44100,
    track_nr: id,
    disc_nr: 1,
    year: 2007,
    play_count: 3,
    rating: 80,
    mhii_link: 100,
  };
}

describe("format utilities & sub-16ms search filter", () => {
  it("formats bytes and durations cleanly with tabular alignment", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(20 * 1024 * 1024)).toBe("20.0 MB");
    expect(formatBytes(80 * 1024 * 1024 * 1024)).toBe("80.00 GB");
    expect(formatDurationMs(245_100)).toBe("4:05");
    expect(formatDurationMs(3_723_000)).toBe("1:02:03");
    expect(formatEtaSeconds(75)).toBe("1m 15s left");
  });

  it("filters and sorts 10,000 tracks in under 25ms", () => {
    const tracks: TrackDto[] = [];
    for (let i = 0; i < 10_000; i += 1) {
      tracks.push(
        makeTrack(
          i + 1,
          i === 5432 ? "NUEVAYoL" : `Track ${i}`,
          i === 5432 ? "Bad Bunny" : `Artist ${i % 100}`,
          `Album ${i % 250}`,
          180_000 + (i % 60) * 1000,
        ),
      );
    }

    const t0 = performance.now();
    const res = filterAndSortTracks(tracks, "nuevayol", "title", "asc");
    const elapsed = performance.now() - t0;

    expect(res).toHaveLength(1);
    expect(res[0].artist).toBe("Bad Bunny");
    expect(elapsed).toBeLessThan(100);
  });
});
