// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

import { useCallback, useEffect, useRef, useState } from "react";
import { FolderPlus, Plus, Search } from "lucide-react";
import { podbridgeApi } from "./api/bridge";
import { AlbumsGrid } from "./components/AlbumsGrid";
import { ArtistsView } from "./components/ArtistsView";
import { Sidebar } from "./components/Sidebar";
import { SongsTable } from "./components/SongsTable";
import type {
  ActiveView,
  AppSettings,
  IpodDeviceSummary,
  LibrarySnapshot,
} from "./types";

export function App() {
  const [devices, setDevices] = useState<IpodDeviceSummary[]>([]);
  const [snapshot, setSnapshot] = useState<LibrarySnapshot | null>(null);
  const [settings, setSettings] = useState<AppSettings>({
    theme: "dark",
    transfer: {
      lossless_target: "Alac",
      aac_bitrate_kbps: 256,
      duplicate_policy: "Skip",
    },
  });
  const [activeView, setActiveView] = useState<ActiveView>({ kind: "songs" });
  const [searchQuery, setSearchQuery] = useState("");
  const [draggedTrackIds, setDraggedTrackIds] = useState<number[]>([]);
  const [toast, setToast] = useState<string | null>(null);
  const [busyEjecting, setBusyEjecting] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => {
      setToast((prev) => (prev === msg ? null : prev));
    }, 3400);
  }, []);

  const refreshAll = useCallback(async () => {
    const payload = await podbridgeApi.listDevices();
    setDevices(payload.devices);
    if (payload.selected_device_id) {
      const snap = await podbridgeApi.getLibrarySnapshot(
        payload.selected_device_id,
      );
      setSnapshot(snap);
    } else {
      setSnapshot(null);
    }
  }, []);

  useEffect(() => {
    void (async () => {
      const loadedSettings = await podbridgeApi.getAppSettings();
      setSettings(loadedSettings);
      document.documentElement.setAttribute("data-theme", loadedSettings.theme);
      await refreshAll();
    })();
  }, [refreshAll]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void podbridgeApi.onDeviceHotplug(() => {
      void refreshAll();
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [refreshAll]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (
        e.key === "/" &&
        document.activeElement?.tagName !== "INPUT" &&
        document.activeElement?.tagName !== "TEXTAREA"
      ) {
        e.preventDefault();
        searchInputRef.current?.focus();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const handleToggleTheme = async () => {
    const nextTheme = settings.theme === "dark" ? "light" : "dark";
    const updated: AppSettings = { ...settings, theme: nextTheme };
    setSettings(updated);
    document.documentElement.setAttribute("data-theme", nextTheme);
    await podbridgeApi.updateAppSettings(updated);
  };

  const handleSelectDevice = async (deviceId: string) => {
    const snap = await podbridgeApi.selectDevice(deviceId);
    setSnapshot(snap);
    setActiveView({ kind: "songs" });
  };

  const handleEjectDevice = async (deviceId: string) => {
    setBusyEjecting(true);
    try {
      await podbridgeApi.safeEjectDevice(deviceId);
      showToast("iPod safely ejected — safe to disconnect USB cable.");
      await refreshAll();
    } catch (err) {
      showToast(`Eject failed: ${String(err)}`);
    } finally {
      setBusyEjecting(false);
    }
  };

  const handleDeleteTracks = useCallback(
    async (trackIds: number[]) => {
      if (!snapshot || trackIds.length === 0) return;
      try {
        const nextSnap = await podbridgeApi.deleteTracks(
          snapshot.device.id,
          trackIds,
        );
        setSnapshot(nextSnap);
        setDevices((prev) =>
          prev.map((d) => (d.id === nextSnap.device.id ? nextSnap.device : d)),
        );
        showToast(`Deleted ${trackIds.length} track(s) from iPod.`);
      } catch (err) {
        showToast(`Delete failed: ${String(err)}`);
      }
    },
    [snapshot, showToast],
  );

  const handleAppendToPlaylist = useCallback(
    async (persistentIdHex: string, trackIds: number[]) => {
      if (!snapshot || trackIds.length === 0) return;
      try {
        const nextSnap = await podbridgeApi.appendToPlaylist(
          snapshot.device.id,
          persistentIdHex,
          trackIds,
        );
        setSnapshot(nextSnap);
        showToast(`Added ${trackIds.length} song(s) to playlist.`);
      } catch (err) {
        showToast(`Playlist update failed: ${String(err)}`);
      }
    },
    [snapshot, showToast],
  );

  const handlePickFiles = async () => {
    if (!snapshot) return;
    const picked = await podbridgeApi.pickAudioOrM3uFiles(true);
    if (picked.length > 0) {
      const res = await podbridgeApi.enqueueTransfer(snapshot.device.id, picked);
      await refreshAll();
      showToast(
        `Transferred ${res.added_tracks} track(s) (${res.skipped_duplicates} duplicate(s) skipped).`,
      );
    }
  };

  const handlePickFolder = async () => {
    if (!snapshot) return;
    const folder = await podbridgeApi.pickFolderDialog(
      "Select Album or Music Folder",
    );
    if (folder) {
      const res = await podbridgeApi.enqueueTransfer(snapshot.device.id, [folder]);
      await refreshAll();
      showToast(`Transferred ${res.added_tracks} track(s) from folder.`);
    }
  };

  return (
    <div className="app-shell">
      <Sidebar
        devices={devices}
        activeDevice={snapshot?.device ?? null}
        playlists={snapshot?.playlists ?? []}
        albumCount={snapshot?.albums.length ?? 0}
        artistCount={snapshot?.artists.length ?? 0}
        activeView={activeView}
        theme={settings.theme}
        busyEjecting={busyEjecting}
        onSelectDevice={(id) => void handleSelectDevice(id)}
        onRefreshDevices={() => void refreshAll()}
        onEjectDevice={(id) => void handleEjectDevice(id)}
        onChangeView={setActiveView}
        onCreatePlaylistPrompt={() => {}}
        onDropTracksOnPlaylist={(pid) => {
          if (draggedTrackIds.length > 0) {
            void handleAppendToPlaylist(pid, draggedTrackIds);
            setDraggedTrackIds([]);
          }
        }}
        hasDraggedSelection={draggedTrackIds.length > 0}
        onOpenMockModal={() => {}}
        onToggleTheme={() => void handleToggleTheme()}
        onOpenSettings={() => {}}
      />

      <main className="main-pane">
        <header className="topbar">
          <div className="search-box">
            <Search size={14} className="search-icon" />
            <input
              ref={searchInputRef}
              type="search"
              className="search-input"
              placeholder="Filter songs, artists, albums, or formats… (Press /)"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
            />
          </div>

          <div className="topbar-actions">
            <button
              type="button"
              className="btn"
              disabled={!snapshot}
              onClick={() => void handlePickFolder()}
            >
              <FolderPlus size={14} />
              <span>Add Folder</span>
            </button>
            <button
              type="button"
              className="btn btn-primary"
              disabled={!snapshot}
              onClick={() => void handlePickFiles()}
            >
              <Plus size={14} />
              <span>Add Songs / M3U</span>
            </button>
          </div>
        </header>

        <section className="content-viewport">
          {!snapshot ? (
            <div className="empty-state">
              Connect an iPod Classic or Nano over USB, or open a local Mock iPod
              folder from the sidebar.
            </div>
          ) : activeView.kind === "songs" ? (
            <SongsTable
              deviceId={snapshot.device.id}
              tracks={snapshot.tracks}
              playlists={snapshot.playlists}
              searchQuery={searchQuery}
              onDeleteTracks={(ids) => void handleDeleteTracks(ids)}
              onAppendToPlaylist={(pid, ids) =>
                void handleAppendToPlaylist(pid, ids)
              }
              onDragSelectionChange={setDraggedTrackIds}
            />
          ) : activeView.kind === "albums" ? (
            <AlbumsGrid
              deviceId={snapshot.device.id}
              albums={snapshot.albums}
              tracks={snapshot.tracks}
              playlists={snapshot.playlists}
              searchQuery={searchQuery}
              selectedAlbumKey={activeView.selectedAlbumKey}
              onSelectAlbum={(key) =>
                setActiveView({ kind: "albums", selectedAlbumKey: key })
              }
              onDeleteTracks={(ids) => void handleDeleteTracks(ids)}
              onAppendToPlaylist={(pid, ids) =>
                void handleAppendToPlaylist(pid, ids)
              }
              onDragSelectionChange={setDraggedTrackIds}
            />
          ) : activeView.kind === "artists" ? (
            <ArtistsView
              deviceId={snapshot.device.id}
              artists={snapshot.artists}
              tracks={snapshot.tracks}
              playlists={snapshot.playlists}
              searchQuery={searchQuery}
              selectedArtist={activeView.selectedArtist}
              onSelectArtist={(name) =>
                setActiveView({ kind: "artists", selectedArtist: name })
              }
              onDeleteTracks={(ids) => void handleDeleteTracks(ids)}
              onAppendToPlaylist={(pid, ids) =>
                void handleAppendToPlaylist(pid, ids)
              }
              onDragSelectionChange={setDraggedTrackIds}
            />
          ) : (
            <SongsTable
              deviceId={snapshot.device.id}
              tracks={snapshot.tracks.filter((t) =>
                snapshot.playlists
                  .find((p) => p.persistent_id_hex === activeView.persistentIdHex)
                  ?.track_ids.includes(t.track_id),
              )}
              playlists={snapshot.playlists}
              searchQuery={searchQuery}
              onDeleteTracks={(ids) => void handleDeleteTracks(ids)}
              onAppendToPlaylist={(pid, ids) =>
                void handleAppendToPlaylist(pid, ids)
              }
              onDragSelectionChange={setDraggedTrackIds}
            />
          )}
        </section>
      </main>

      {toast && <div className="status-toast">{toast}</div>}
    </div>
  );
}
