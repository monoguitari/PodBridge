// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  AppSettings,
  BackupManifest,
  BatchCommitSummary,
  DevicesPayload,
  IpodGeneration,
  LibrarySnapshot,
  TrackDto,
  TransferProgressSnapshot,
} from "../types";
import { isTauriRuntime } from "../utils/format";

// Lightweight browser preview state used ONLY when developing outside Tauri (`npm run dev` in browser)
let demoSettings: AppSettings = {
  theme: "dark",
  transfer: {
    lossless_target: "Alac",
    aac_bitrate_kbps: 256,
    duplicate_policy: "Skip",
  },
};

const DEMO_CATALOG: Array<{
  artist: string;
  album: string;
  genre: string;
  year: number;
  codec: string;
}> = [
  { artist: "Bad Bunny", album: "DeBÍ TiRAR MáS FOToS", genre: "Urbano", year: 2025, codec: "ALAC" },
  { artist: "Daft Punk", album: "Discovery", genre: "Electronic", year: 2001, codec: "ALAC" },
  { artist: "Daft Punk", album: "Random Access Memories", genre: "Electronic", year: 2013, codec: "ALAC" },
  { artist: "Radiohead", album: "In Rainbows", genre: "Alternative", year: 2007, codec: "ALAC" },
  { artist: "Radiohead", album: "OK Computer", genre: "Alternative", year: 1997, codec: "AAC" },
  { artist: "Kendrick Lamar", album: "good kid, m.A.A.d city", genre: "Hip-Hop", year: 2012, codec: "ALAC" },
  { artist: "Frank Ocean", album: "Blonde", genre: "R&B", year: 2016, codec: "AAC" },
  { artist: "Sade", album: "Love Deluxe", genre: "Soul", year: 1992, codec: "ALAC" },
  { artist: "Tame Impala", album: "Currents", genre: "Indie", year: 2015, codec: "ALAC" },
  { artist: "Justice", album: "Cross", genre: "Electronic", year: 2007, codec: "AAC" },
  { artist: "Aphex Twin", album: "Selected Ambient Works 85-92", genre: "Ambient", year: 1992, codec: "ALAC" },
  { artist: "Boards of Canada", album: "Music Has the Right to Children", genre: "IDM", year: 1998, codec: "ALAC" },
  { artist: "Rosalía", album: "MOTOMAMI", genre: "Pop", year: 2022, codec: "AAC" },
  { artist: "The Strokes", album: "Is This It", genre: "Rock", year: 2001, codec: "MP3" },
  { artist: "Portishead", album: "Dummy", genre: "Trip-Hop", year: 1994, codec: "ALAC" },
  { artist: "Massive Attack", album: "Mezzanine", genre: "Trip-Hop", year: 1998, codec: "ALAC" },
  { artist: "Nujabes", album: "Modal Soul", genre: "Jazz Hop", year: 2005, codec: "MP3" },
  { artist: "J Dilla", album: "Donuts", genre: "Hip-Hop", year: 2006, codec: "ALAC" },
  { artist: "MF DOOM", album: "MM..FOOD", genre: "Hip-Hop", year: 2004, codec: "AAC" },
  { artist: "Björk", album: "Homogenic", genre: "Art Pop", year: 1997, codec: "ALAC" },
  { artist: "Gustavo Cerati", album: "Bocanada", genre: "Rock", year: 1999, codec: "ALAC" },
  { artist: "Soda Stereo", album: "Dynamo", genre: "Shoegaze", year: 1992, codec: "ALAC" },
  { artist: "Charli xcx", album: "BRAT", genre: "Club", year: 2024, codec: "AAC" },
  { artist: "Khruangbin", album: "Con Todo El Mundo", genre: "Funk", year: 2018, codec: "ALAC" },
];

const demoTracks: TrackDto[] = DEMO_CATALOG.flatMap((entry, albumIdx) =>
  Array.from({ length: 18 }, (_, idx) => {
    const trackNr = idx + 1;
    const trackId = 3400 + albumIdx * 18 + trackNr;
    return {
      track_id: trackId,
      dbid_hex: trackId.toString(16).toUpperCase().padStart(16, "0"),
      title: `${entry.album} — Track ${String(trackNr).padStart(2, "0")}`,
      artist: entry.artist,
      album_artist: entry.artist,
      album: entry.album,
      genre: entry.genre,
      format_badge: entry.codec,
      size_bytes: entry.codec === "ALAC" ? 26_400_000 + idx * 910_000 : 8_900_000 + idx * 320_000,
      length_ms: 175_000 + ((albumIdx * 19 + idx * 29) % 160) * 1000,
      bitrate_kbps: entry.codec === "ALAC" ? 960 : 256,
      sample_rate_hz: 44100,
      track_nr: trackNr,
      disc_nr: 1,
      year: entry.year,
      play_count: (albumIdx * 7 + trackNr * 3) % 48,
      rating: 80,
      mhii_link: 100 + albumIdx,
    };
  }),
);

let demoSnapshot: LibrarySnapshot = buildDemoSnapshot(
  demoTracks,
  Array.from({ length: 14 }, (_, i) => ({
    persistent_id_hex: (0x13b5dbb7775862a0 + i).toString(16).toUpperCase(),
    name:
      i === 0
        ? "Late Night Classic"
        : i === 1
          ? "Roadtrip Lossless"
          : `Studio Mix Vol. ${i + 1}`,
    is_master: false,
    track_ids: demoTracks.slice(i * 4, i * 4 + 36).map((t) => t.track_id),
  })),
);

function buildDemoSnapshot(
  tracks: TrackDto[],
  playlists: LibrarySnapshot["playlists"],
): LibrarySnapshot {
  const musicBytes =
    tracks.reduce((acc, t) => acc + t.size_bytes, 0) + 29_400_000_000;
  const totalBytes = 137_169_977_344;
  const artBytes = 17_360_000;
  const otherBytes = 138_000_000;
  const freeBytes = Math.max(0, totalBytes - musicBytes - artBytes - otherBytes);

  const albumMap = new Map<string, LibrarySnapshot["albums"][number]>();
  const artistAlbums = new Map<string, Set<string>>();
  const artistCounts = new Map<string, { tracks: number; mhii: number }>();

  for (const t of tracks) {
    const key = `${t.album_artist.toLowerCase()}::${t.album.toLowerCase()}`;
    const existing = albumMap.get(key);
    if (existing) {
      existing.track_count += 1;
      existing.total_length_ms += t.length_ms;
      existing.track_ids.push(t.track_id);
    } else {
      albumMap.set(key, {
        key,
        title: t.album,
        artist: t.album_artist,
        year: t.year,
        track_count: 1,
        total_length_ms: t.length_ms,
        mhii_link: t.mhii_link,
        track_ids: [t.track_id],
      });
    }

    let aSet = artistAlbums.get(t.artist);
    if (!aSet) {
      aSet = new Set();
      artistAlbums.set(t.artist, aSet);
    }
    aSet.add(t.album.toLowerCase());

    const ac = artistCounts.get(t.artist);
    if (ac) {
      ac.tracks += 1;
    } else {
      artistCounts.set(t.artist, { tracks: 1, mhii: t.mhii_link });
    }
  }

  const albums = Array.from(albumMap.values()).sort((a, b) =>
    a.artist.localeCompare(b.artist) || a.title.localeCompare(b.title),
  );
  const artists = Array.from(artistCounts.entries())
    .map(([name, info]) => ({
      name,
      album_count: artistAlbums.get(name)?.size ?? 1,
      track_count: info.tracks,
      mhii_link: info.mhii,
    }))
    .sort((a, b) => a.name.localeCompare(b.name));

  return {
    device: {
      id: "demo:D:\\",
      name: "Jesus’s iPod (Preview)",
      root_path: "D:\\",
      generation: "Classic6G",
      generation_label: "iPod Classic (6G)",
      firewire_guid: "0x000A27001358762E",
      is_mock: false,
      can_eject: true,
      track_count: tracks.length,
      playlist_count: playlists.length,
      storage: {
        total_bytes: totalBytes,
        music_bytes: musicBytes,
        artwork_bytes: artBytes,
        other_bytes: otherBytes,
        free_bytes: freeBytes,
      },
    },
    tracks,
    albums,
    artists,
    playlists,
  };
}

export const podbridgeApi = {
  async listDevices(): Promise<DevicesPayload> {
    if (!isTauriRuntime()) {
      return {
        devices: [demoSnapshot.device],
        selected_device_id: demoSnapshot.device.id,
      };
    }
    return invoke<DevicesPayload>("list_devices");
  },

  async selectDevice(deviceId: string): Promise<LibrarySnapshot> {
    if (!isTauriRuntime()) return demoSnapshot;
    return invoke<LibrarySnapshot>("select_device", { deviceId });
  },

  async getLibrarySnapshot(deviceId?: string): Promise<LibrarySnapshot | null> {
    if (!isTauriRuntime()) return demoSnapshot;
    return invoke<LibrarySnapshot | null>("get_library_snapshot", {
      deviceId: deviceId ?? null,
    });
  },

  async createMockDevice(
    path: string,
    name: string,
    generation: IpodGeneration,
  ): Promise<LibrarySnapshot> {
    return invoke<LibrarySnapshot>("create_mock_device", {
      path,
      name,
      generation,
    });
  },

  async openMockDevice(path: string): Promise<LibrarySnapshot> {
    return invoke<LibrarySnapshot>("open_mock_device", { path });
  },

  async enqueueTransfer(
    deviceId: string,
    paths: string[],
  ): Promise<BatchCommitSummary> {
    if (!isTauriRuntime()) {
      return {
        added_tracks: paths.length,
        skipped_duplicates: 0,
        created_playlists: 0,
        bytes_written: paths.length * 12_000_000,
        elapsed_ms: 420,
      };
    }
    return invoke<BatchCommitSummary>("enqueue_transfer", { deviceId, paths });
  },

  async cancelActiveTransfer(): Promise<void> {
    if (!isTauriRuntime()) return;
    return invoke<void>("cancel_active_transfer");
  },

  async deleteTracks(
    deviceId: string,
    trackIds: number[],
  ): Promise<LibrarySnapshot> {
    if (!isTauriRuntime()) {
      const idSet = new Set(trackIds);
      const remaining = demoSnapshot.tracks.filter((t) => !idSet.has(t.track_id));
      demoSnapshot = buildDemoSnapshot(remaining, demoSnapshot.playlists);
      return demoSnapshot;
    }
    return invoke<LibrarySnapshot>("delete_tracks", { deviceId, trackIds });
  },

  async createPlaylist(deviceId: string, name: string): Promise<LibrarySnapshot> {
    if (!isTauriRuntime()) {
      const hex = Math.floor(Math.random() * 0xffffffff)
        .toString(16)
        .toUpperCase()
        .padStart(16, "0");
      demoSnapshot = buildDemoSnapshot(demoSnapshot.tracks, [
        ...demoSnapshot.playlists,
        { persistent_id_hex: hex, name, is_master: false, track_ids: [] },
      ]);
      return demoSnapshot;
    }
    return invoke<LibrarySnapshot>("create_playlist", { deviceId, name });
  },

  async renamePlaylist(
    deviceId: string,
    persistentIdHex: string,
    newName: string,
  ): Promise<LibrarySnapshot> {
    if (!isTauriRuntime()) {
      const updated = demoSnapshot.playlists.map((p) =>
        p.persistent_id_hex === persistentIdHex ? { ...p, name: newName } : p,
      );
      demoSnapshot = buildDemoSnapshot(demoSnapshot.tracks, updated);
      return demoSnapshot;
    }
    return invoke<LibrarySnapshot>("rename_playlist", {
      deviceId,
      persistentIdHex,
      newName,
    });
  },

  async setPlaylistTracks(
    deviceId: string,
    persistentIdHex: string,
    trackIds: number[],
  ): Promise<LibrarySnapshot> {
    if (!isTauriRuntime()) {
      const updated = demoSnapshot.playlists.map((p) =>
        p.persistent_id_hex === persistentIdHex
          ? { ...p, track_ids: trackIds }
          : p,
      );
      demoSnapshot = buildDemoSnapshot(demoSnapshot.tracks, updated);
      return demoSnapshot;
    }
    return invoke<LibrarySnapshot>("set_playlist_tracks", {
      deviceId,
      persistentIdHex,
      trackIds,
    });
  },

  async appendToPlaylist(
    deviceId: string,
    persistentIdHex: string,
    trackIds: number[],
  ): Promise<LibrarySnapshot> {
    if (!isTauriRuntime()) {
      const updated = demoSnapshot.playlists.map((p) =>
        p.persistent_id_hex === persistentIdHex
          ? { ...p, track_ids: [...p.track_ids, ...trackIds] }
          : p,
      );
      demoSnapshot = buildDemoSnapshot(demoSnapshot.tracks, updated);
      return demoSnapshot;
    }
    return invoke<LibrarySnapshot>("append_to_playlist", {
      deviceId,
      persistentIdHex,
      trackIds,
    });
  },

  async deletePlaylist(
    deviceId: string,
    persistentIdHex: string,
  ): Promise<LibrarySnapshot> {
    if (!isTauriRuntime()) {
      const updated = demoSnapshot.playlists.filter(
        (p) => p.persistent_id_hex !== persistentIdHex,
      );
      demoSnapshot = buildDemoSnapshot(demoSnapshot.tracks, updated);
      return demoSnapshot;
    }
    return invoke<LibrarySnapshot>("delete_playlist", {
      deviceId,
      persistentIdHex,
    });
  },

  async safeEjectDevice(deviceId: string): Promise<void> {
    if (!isTauriRuntime()) return;
    return invoke<void>("safe_eject_device", { deviceId });
  },

  async getAppSettings(): Promise<AppSettings> {
    if (!isTauriRuntime()) return demoSettings;
    return invoke<AppSettings>("get_app_settings");
  },

  async updateAppSettings(settings: AppSettings): Promise<AppSettings> {
    if (!isTauriRuntime()) {
      demoSettings = settings;
      return demoSettings;
    }
    return invoke<AppSettings>("update_app_settings", { settings });
  },

  async listBackups(deviceId: string): Promise<BackupManifest[]> {
    if (!isTauriRuntime()) {
      return [
        {
          id: "1732000000000",
          device_name: "Jesus’s iPod",
          created_unix_ms: Date.now() - 3600_000,
          track_count: 994,
          playlist_count: 1,
          itunesdb_bytes: 1_599_130,
        },
      ];
    }
    return invoke<BackupManifest[]>("list_backups", { deviceId });
  },

  async restoreBackup(
    deviceId: string,
    backupId: string,
  ): Promise<LibrarySnapshot> {
    if (!isTauriRuntime()) return demoSnapshot;
    return invoke<LibrarySnapshot>("restore_backup", { deviceId, backupId });
  },

  async pickAudioOrM3uFiles(multiple = true): Promise<string[]> {
    if (!isTauriRuntime()) return [];
    const selected = await open({
      multiple,
      directory: false,
      filters: [
        {
          name: "Audio & Playlists",
          extensions: [
            "mp3",
            "m4a",
            "aac",
            "wav",
            "aif",
            "aiff",
            "flac",
            "ogg",
            "opus",
            "wma",
            "m3u",
            "m3u8",
          ],
        },
      ],
    });
    if (!selected) return [];
    return Array.isArray(selected) ? selected : [selected];
  },

  async pickFolderDialog(title = "Select Music Folder"): Promise<string | null> {
    if (!isTauriRuntime()) return null;
    const selected = await open({
      multiple: false,
      directory: true,
      title,
    });
    if (!selected || Array.isArray(selected)) return null;
    return selected;
  },

  async onTransferProgress(
    handler: (snap: TransferProgressSnapshot) => void,
  ): Promise<UnlistenFn> {
    if (!isTauriRuntime()) return () => {};
    return listen<TransferProgressSnapshot>("transfer://progress", (ev) => {
      handler(ev.payload);
    });
  },

  async onDeviceHotplug(handler: () => void): Promise<UnlistenFn> {
    if (!isTauriRuntime()) return () => {};
    return listen("device://hotplug", () => handler());
  },

  async onWindowDragDrop(handlers: {
    onHover: (paths: string[]) => void;
    onLeave: () => void;
    onDrop: (paths: string[]) => void;
  }): Promise<UnlistenFn> {
    if (!isTauriRuntime()) return () => {};
    return getCurrentWebview().onDragDropEvent((event) => {
      const p = event.payload;
      if (p.type === "enter" || p.type === "over") {
        handlers.onHover("paths" in p ? p.paths : []);
      } else if (p.type === "leave") {
        handlers.onLeave();
      } else if (p.type === "drop") {
        handlers.onDrop(p.paths);
      }
    });
  },
};
