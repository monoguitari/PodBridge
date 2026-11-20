// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

export type IpodGeneration =
  | "Video5G"
  | "Classic6G"
  | "Classic6_5G"
  | "Classic7G"
  | "Nano3G"
  | "Nano4G"
  | "Nano5G";

export interface StorageBreakdown {
  total_bytes: number;
  music_bytes: number;
  artwork_bytes: number;
  other_bytes: number;
  free_bytes: number;
}

export interface IpodDeviceSummary {
  id: string;
  name: string;
  root_path: string;
  generation: IpodGeneration;
  generation_label: string;
  firewire_guid: string | null;
  is_mock: boolean;
  can_eject: boolean;
  track_count: number;
  playlist_count: number;
  storage: StorageBreakdown;
}

export interface DevicesPayload {
  devices: IpodDeviceSummary[];
  selected_device_id: string | null;
}

export interface TrackDto {
  track_id: number;
  dbid_hex: string;
  title: string;
  artist: string;
  album_artist: string;
  album: string;
  genre: string;
  format_badge: string;
  size_bytes: number;
  length_ms: number;
  bitrate_kbps: number;
  sample_rate_hz: number;
  track_nr: number;
  disc_nr: number;
  year: number;
  play_count: number;
  rating: number;
  mhii_link: number;
}

export interface AlbumDto {
  key: string;
  title: string;
  artist: string;
  year: number;
  track_count: number;
  total_length_ms: number;
  mhii_link: number;
  track_ids: number[];
}

export interface ArtistDto {
  name: string;
  album_count: number;
  track_count: number;
  mhii_link: number;
}

export interface PlaylistDto {
  persistent_id_hex: string;
  name: string;
  is_master: boolean;
  track_ids: number[];
}

export interface LibrarySnapshot {
  device: IpodDeviceSummary;
  tracks: TrackDto[];
  albums: AlbumDto[];
  artists: ArtistDto[];
  playlists: PlaylistDto[];
}

export type LosslessTarget = "Alac" | "Aac";
export type DuplicatePolicy = "Skip" | "KeepBoth";

export interface TransferSettings {
  lossless_target: LosslessTarget;
  aac_bitrate_kbps: number;
  duplicate_policy: DuplicatePolicy;
}

export interface AppSettings {
  theme: "dark" | "light";
  transfer: TransferSettings;
}

export type TransferPhase =
  | "Scanning"
  | "Transferring"
  | "CommittingDatabase"
  | "Completed"
  | "Cancelled"
  | "Failed";

export type TransferItemStatus =
  | "Queued"
  | "TranscodingAlac"
  | "TranscodingAac"
  | "Copying"
  | "SkippedDuplicate"
  | "Done"
  | "Cancelled"
  | { Error: string };

export interface TransferQueueItem {
  index: number;
  title: string;
  artist: string;
  source_filename: string;
  status: TransferItemStatus;
}

export interface TransferProgressSnapshot {
  batch_id: number;
  phase: TransferPhase;
  completed_files: number;
  total_files: number;
  skipped_duplicates: number;
  bytes_written: number;
  total_bytes_estimated: number;
  throughput_mb_per_sec: number;
  eta_seconds: number | null;
  items: TransferQueueItem[];
}

export interface BatchCommitSummary {
  added_tracks: number;
  skipped_duplicates: number;
  created_playlists: number;
  bytes_written: number;
  elapsed_ms: number;
}

export interface BackupManifest {
  id: string;
  device_name: string;
  created_unix_ms: number;
  track_count: number;
  playlist_count: number;
  itunesdb_bytes: number;
}

export type SortColumn =
  | "title"
  | "artist"
  | "album"
  | "length_ms"
  | "format_badge"
  | "size_bytes"
  | "play_count";

export type SortDirection = "asc" | "desc";

export type ActiveView =
  | { kind: "songs" }
  | { kind: "albums"; selectedAlbumKey?: string }
  | { kind: "artists"; selectedArtist?: string }
  | { kind: "playlist"; persistentIdHex: string };
