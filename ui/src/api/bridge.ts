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

const demoTracks: TrackDto[] = [
  {
    track_id: 3415,
    dbid_hex: "0000000000000D57",
    title: "NUEVAYoL",
    artist: "Bad Bunny",
    album_artist: "Bad Bunny",
    album: "DeBÍ TiRAR MáS FOToS",
    genre: "Urbano",
    format_badge: "ALAC",
    size_bytes: 24_812_544,
    length_ms: 183_600,
    bitrate_kbps: 1081,
    sample_rate_hz: 48000,
    track_nr: 1,
    disc_nr: 1,
    year: 2025,
    play_count: 14,
    rating: 100,
    mhii_link: 100,
  },
  {
    track_id: 3416,
    dbid_hex: "0000000000000D58",
    title: "Digital Love",
    artist: "Daft Punk",
    album_artist: "Daft Punk",
    album: "Discovery",
    genre: "Electronic",
    format_badge: "AAC",
    size_bytes: 9_842_110,
    length_ms: 301_200,
    bitrate_kbps: 256,
    sample_rate_hz: 44100,
    track_nr: 3,
    disc_nr: 1,
    year: 2001,
    play_count: 42,
    rating: 100,
    mhii_link: 101,
  },
  {
    track_id: 3417,
    dbid_hex: "0000000000000D59",
    title: "Veridis Quo",
    artist: "Daft Punk",
    album_artist: "Daft Punk",
    album: "Discovery",
    genre: "Electronic",
    format_badge: "ALAC",
    size_bytes: 31_420_900,
    length_ms: 345_100,
    bitrate_kbps: 728,
    sample_rate_hz: 44100,
    track_nr: 11,
    disc_nr: 1,
    year: 2001,
    play_count: 19,
    rating: 80,
    mhii_link: 101,
  },
];

let demoSnapshot: LibrarySnapshot = buildDemoSnapshot(demoTracks, [
  {
    persistent_id_hex: "13B5DBB7775862A2",
    name: "Late Night Classic",
    is_master: false,
    track_ids: [3416, 3417],
  },
]);

function buildDemoSnapshot(
  tracks: TrackDto[],
  playlists: LibrarySnapshot["playlists"],
): LibrarySnapshot {
  const musicBytes = tracks.reduce((acc, t) => acc + t.size_bytes, 0) + 34_800_000_000;
  const totalBytes = 137_169_977_344;
  const artBytes = 17_360_000;
  const otherBytes = 138_000_000;
  const freeBytes = Math.max(0, totalBytes - musicBytes - artBytes - otherBytes);

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
    albums: [
      {
        key: "bad bunny::debí tirar más fotos",
        title: "DeBÍ TiRAR MáS FOToS",
        artist: "Bad Bunny",
        year: 2025,
        track_count: 1,
        total_length_ms: 183_600,
        mhii_link: 100,
        track_ids: [3415],
      },
      {
        key: "daft punk::discovery",
        title: "Discovery",
        artist: "Daft Punk",
        year: 2001,
        track_count: 2,
        total_length_ms: 646_300,
        mhii_link: 101,
        track_ids: [3416, 3417],
      },
    ],
    artists: [
      { name: "Bad Bunny", album_count: 1, track_count: 1, mhii_link: 100 },
      { name: "Daft Punk", album_count: 1, track_count: 2, mhii_link: 101 },
    ],
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
