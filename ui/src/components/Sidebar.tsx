// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

import {
  Disc3,
  FolderPlus,
  HardDrive,
  ListMusic,
  Mic2,
  Moon,
  Music2,
  Plus,
  Power,
  RefreshCw,
  Settings,
  Sun,
} from "lucide-react";
import type {
  ActiveView,
  IpodDeviceSummary,
  PlaylistDto,
} from "../types";
import { formatBytes } from "../utils/format";

interface SidebarProps {
  devices: IpodDeviceSummary[];
  activeDevice: IpodDeviceSummary | null;
  playlists: PlaylistDto[];
  albumCount: number;
  artistCount: number;
  activeView: ActiveView;
  theme: "dark" | "light";
  busyEjecting: boolean;
  onSelectDevice: (deviceId: string) => void;
  onRefreshDevices: () => void;
  onEjectDevice: (deviceId: string) => void;
  onChangeView: (view: ActiveView) => void;
  onCreatePlaylistPrompt: () => void;
  onDropTracksOnPlaylist: (persistentIdHex: string) => void;
  hasDraggedSelection: boolean;
  onOpenMockModal: () => void;
  onToggleTheme: () => void;
  onOpenSettings: () => void;
}

export function Sidebar({
  devices,
  activeDevice,
  playlists,
  albumCount,
  artistCount,
  activeView,
  theme,
  busyEjecting,
  onSelectDevice,
  onRefreshDevices,
  onEjectDevice,
  onChangeView,
  onCreatePlaylistPrompt,
  onDropTracksOnPlaylist,
  hasDraggedSelection,
  onOpenMockModal,
  onToggleTheme,
  onOpenSettings,
}: SidebarProps) {
  const storage = activeDevice?.storage;
  const total = Math.max(storage?.total_bytes ?? 1, 1);
  const musicPct = Math.min(100, ((storage?.music_bytes ?? 0) / total) * 100);
  const artPct = Math.min(
    100 - musicPct,
    ((storage?.artwork_bytes ?? 0) / total) * 100,
  );
  const otherPct = Math.min(
    100 - musicPct - artPct,
    ((storage?.other_bytes ?? 0) / total) * 100,
  );

  return (
    <aside className="sidebar">
      <div className="brand-row">
        <div className="brand-mark">
          <span className="brand-wheel" aria-hidden="true" />
          <span>PodBridge</span>
        </div>
        <button
          type="button"
          className="btn-icon"
          title="Rescan USB Devices"
          onClick={onRefreshDevices}
        >
          <RefreshCw size={14} />
        </button>
      </div>

      {devices.length > 1 && (
        <select
          className="btn"
          style={{ width: "100%", justifyContent: "space-between" }}
          value={activeDevice?.id ?? ""}
          onChange={(e) => onSelectDevice(e.target.value)}
          aria-label="Select connected iPod"
        >
          {devices.map((d) => (
            <option key={d.id} value={d.id}>
              {d.name} ({d.generation_label})
            </option>
          ))}
        </select>
      )}

      {activeDevice ? (
        <div className="device-card">
          <div className="device-header">
            <div style={{ minWidth: 0 }}>
              <div className="device-title" title={activeDevice.name}>
                {activeDevice.name}
              </div>
              <div className="device-sub">
                {activeDevice.generation_label} ·{" "}
                <span className="tabular">{activeDevice.root_path}</span>
              </div>
            </div>
            <span className="badge-pill">
              {activeDevice.is_mock ? "MOCK" : "USB"}
            </span>
          </div>

          <div>
            <div className="storage-bar" title="Storage usage breakdown">
              <div
                className="storage-seg-music"
                style={{ width: `${musicPct}%` }}
              />
              <div
                className="storage-seg-art"
                style={{ width: `${artPct}%` }}
              />
              <div
                className="storage-seg-other"
                style={{ width: `${otherPct}%` }}
              />
            </div>
            <div className="storage-caption tabular" style={{ marginTop: 6 }}>
              <span>{formatBytes(activeDevice.storage.music_bytes)} music</span>
              <span>{formatBytes(activeDevice.storage.free_bytes)} free</span>
            </div>
          </div>

          <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
            <button
              type="button"
              className="btn"
              style={{ flex: 1 }}
              disabled={busyEjecting}
              onClick={() => onEjectDevice(activeDevice.id)}
              title={
                activeDevice.is_mock
                  ? "Close Mock iPod session"
                  : "Flush buffers and safely eject iPod from Windows"
              }
            >
              <Power size={13} />
              <span>{activeDevice.is_mock ? "Close Mock" : "Safe Eject"}</span>
            </button>
          </div>
        </div>
      ) : (
        <div className="device-card">
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <HardDrive size={16} color="var(--text-muted)" />
            <div className="device-title">No iPod Connected</div>
          </div>
          <div className="device-sub">
            Connect an iPod Classic or Nano over USB, or open a local Mock iPod
            folder to test transfers safely.
          </div>
          <button
            type="button"
            className="btn btn-primary"
            onClick={onOpenMockModal}
          >
            <FolderPlus size={13} />
            <span>Open / Create Mock iPod</span>
          </button>
        </div>
      )}

      <div className="nav-section">
        <div className="nav-section-header">
          <span>Library</span>
        </div>
        <button
          type="button"
          className={`nav-item ${activeView.kind === "songs" ? "active" : ""}`}
          onClick={() => onChangeView({ kind: "songs" })}
        >
          <span className="nav-item-left">
            <Music2 size={15} />
            <span className="nav-item-label">Songs</span>
          </span>
          <span className="nav-count tabular">
            {activeDevice?.track_count ?? 0}
          </span>
        </button>

        <button
          type="button"
          className={`nav-item ${activeView.kind === "albums" ? "active" : ""}`}
          onClick={() => onChangeView({ kind: "albums" })}
        >
          <span className="nav-item-left">
            <Disc3 size={15} />
            <span className="nav-item-label">Albums</span>
          </span>
          <span className="nav-count tabular">{albumCount}</span>
        </button>

        <button
          type="button"
          className={`nav-item ${activeView.kind === "artists" ? "active" : ""}`}
          onClick={() => onChangeView({ kind: "artists" })}
        >
          <span className="nav-item-left">
            <Mic2 size={15} />
            <span className="nav-item-label">Artists</span>
          </span>
          <span className="nav-count tabular">{artistCount}</span>
        </button>
      </div>

      <div className="nav-section" style={{ flex: 1 }}>
        <div className="nav-section-header">
          <span>Playlists</span>
          {activeDevice && (
            <button
              type="button"
              className="btn-icon"
              title="New Playlist"
              onClick={onCreatePlaylistPrompt}
            >
              <Plus size={14} />
            </button>
          )}
        </div>

        {playlists.length === 0 ? (
          <div
            style={{
              padding: "6px 10px",
              fontSize: 11.5,
              color: "var(--text-muted)",
            }}
          >
            No custom playlists yet
          </div>
        ) : (
          playlists.map((pl) => {
            const isActive =
              activeView.kind === "playlist" &&
              activeView.persistentIdHex === pl.persistent_id_hex;
            return (
              <button
                key={pl.persistent_id_hex}
                type="button"
                className={`nav-item ${isActive ? "active" : ""}`}
                onClick={() =>
                  onChangeView({
                    kind: "playlist",
                    persistentIdHex: pl.persistent_id_hex,
                  })
                }
                onDragOver={(e) => {
                  if (hasDraggedSelection) {
                    e.preventDefault();
                  }
                }}
                onDrop={(e) => {
                  if (hasDraggedSelection) {
                    e.preventDefault();
                    onDropTracksOnPlaylist(pl.persistent_id_hex);
                  }
                }}
              >
                <span className="nav-item-left">
                  <ListMusic size={15} />
                  <span className="nav-item-label">{pl.name}</span>
                </span>
                <span className="nav-count tabular">{pl.track_ids.length}</span>
              </button>
            );
          })
        )}
      </div>

      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          paddingTop: 8,
          borderTop: "1px solid var(--border-subtle)",
        }}
      >
        <button
          type="button"
          className="btn"
          style={{ padding: "5px 9px", fontSize: 11.5 }}
          onClick={onOpenMockModal}
          title="Open or create a local Mock iPod directory"
        >
          <FolderPlus size={13} />
          <span>Mock Mode</span>
        </button>

        <div style={{ display: "flex", gap: 4 }}>
          <button
            type="button"
            className="btn-icon"
            onClick={onToggleTheme}
            title={`Switch to ${theme === "dark" ? "Light" : "Dark"} Theme`}
          >
            {theme === "dark" ? <Sun size={15} /> : <Moon size={15} />}
          </button>
          <button
            type="button"
            className="btn-icon"
            onClick={onOpenSettings}
            title="Transfer Settings & Database Backups"
          >
            <Settings size={15} />
          </button>
        </div>
      </div>
    </aside>
  );
}
