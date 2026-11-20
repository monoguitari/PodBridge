// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

import { useCallback, useEffect, useRef, useState } from "react";
import { FolderPlus, Plus, Search } from "lucide-react";
import { podbridgeApi } from "./api/bridge";
import { AlbumsGrid } from "./components/AlbumsGrid";
import { ArtistsView } from "./components/ArtistsView";
import { DropOverlay } from "./components/DropOverlay";
import { MockDeviceModal } from "./components/MockDeviceModal";
import { NewPlaylistModal } from "./components/NewPlaylistModal";
import { PlaylistView } from "./components/PlaylistView";
import { SettingsModal } from "./components/SettingsModal";
import { Sidebar } from "./components/Sidebar";
import { SongsTable } from "./components/SongsTable";
import { TransferDrawer } from "./components/TransferDrawer";
import type {
  ActiveView,
  AppSettings,
  IpodDeviceSummary,
  IpodGeneration,
  LibrarySnapshot,
  TransferProgressSnapshot,
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
  const [dropHoverPaths, setDropHoverPaths] = useState<string[] | null>(null);
  const [transferProgress, setTransferProgress] =
    useState<TransferProgressSnapshot | null>(null);
  const [newPlaylistOpen, setNewPlaylistOpen] = useState(false);
  const [mockModalOpen, setMockModalOpen] = useState(false);
  const [settingsModalOpen, setSettingsModalOpen] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [busyEjecting, setBusyEjecting] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => {
      setToast((prev) => (prev === msg ? null : prev));
    }, 3600);
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

  const startTransferPaths = useCallback(
    async (paths: string[]) => {
      if (!snapshot || paths.length === 0) {
        if (!snapshot) {
          showToast("Connect an iPod or open Mock Mode before dropping files.");
        }
        return;
      }
      try {
        const res = await podbridgeApi.enqueueTransfer(
          snapshot.device.id,
          paths,
        );
        await refreshAll();
        showToast(
          `Added ${res.added_tracks} song(s)${
            res.skipped_duplicates > 0
              ? ` · ${res.skipped_duplicates} duplicate(s) skipped`
              : ""
          }${
            res.created_playlists > 0
              ? ` · ${res.created_playlists} playlist(s) imported`
              : ""
          }`,
        );
      } catch (err) {
        const msg = String(err);
        if (!msg.toLowerCase().includes("cancelled")) {
          showToast(`Transfer error: ${msg}`);
        }
      }
    },
    [snapshot, refreshAll, showToast],
  );

  useEffect(() => {
    void (async () => {
      const loadedSettings = await podbridgeApi.getAppSettings();
      setSettings(loadedSettings);
      document.documentElement.setAttribute("data-theme", loadedSettings.theme);
      await refreshAll();
    })();
  }, [refreshAll]);

  useEffect(() => {
    let unlistenHotplug: (() => void) | undefined;
    let unlistenProgress: (() => void) | undefined;

    void podbridgeApi.onDeviceHotplug(() => {
      void refreshAll();
    }).then((fn) => {
      unlistenHotplug = fn;
    });

    void podbridgeApi.onTransferProgress((snap) => {
      setTransferProgress(snap);
    }).then((fn) => {
      unlistenProgress = fn;
    });

    return () => {
      unlistenHotplug?.();
      unlistenProgress?.();
    };
  }, [refreshAll]);

  useEffect(() => {
    let unlistenDrop: (() => void) | undefined;
    void podbridgeApi
      .onWindowDragDrop({
        onHover: (paths) => setDropHoverPaths(paths),
        onLeave: () => setDropHoverPaths(null),
        onDrop: (paths) => {
          setDropHoverPaths(null);
          void startTransferPaths(paths);
        },
      })
      .then((fn) => {
        unlistenDrop = fn;
      });
    return () => {
      unlistenDrop?.();
    };
  }, [startTransferPaths]);

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

  const handleSaveSettings = async (next: AppSettings) => {
    setSettings(next);
    document.documentElement.setAttribute("data-theme", next.theme);
    await podbridgeApi.updateAppSettings(next);
    showToast("Preferences saved.");
  };

  const handleToggleTheme = async () => {
    const nextTheme = settings.theme === "dark" ? "light" : "dark";
    await handleSaveSettings({ ...settings, theme: nextTheme });
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
      showToast("Device safely closed / ejected.");
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

  const handleCreatePlaylist = async (name: string) => {
    if (!snapshot) return;
    try {
      const nextSnap = await podbridgeApi.createPlaylist(
        snapshot.device.id,
        name,
      );
      setSnapshot(nextSnap);
      const newest = nextSnap.playlists[nextSnap.playlists.length - 1];
      if (newest) {
        setActiveView({
          kind: "playlist",
          persistentIdHex: newest.persistent_id_hex,
        });
      }
      showToast(`Created playlist "${name}".`);
    } catch (err) {
      showToast(`Playlist creation failed: ${String(err)}`);
    }
  };

  const handleRenamePlaylist = async (
    persistentIdHex: string,
    newName: string,
  ) => {
    if (!snapshot) return;
    try {
      const nextSnap = await podbridgeApi.renamePlaylist(
        snapshot.device.id,
        persistentIdHex,
        newName,
      );
      setSnapshot(nextSnap);
    } catch (err) {
      showToast(`Rename failed: ${String(err)}`);
    }
  };

  const handleSetPlaylistTracks = async (
    persistentIdHex: string,
    trackIds: number[],
  ) => {
    if (!snapshot) return;
    try {
      const nextSnap = await podbridgeApi.setPlaylistTracks(
        snapshot.device.id,
        persistentIdHex,
        trackIds,
      );
      setSnapshot(nextSnap);
    } catch (err) {
      showToast(`Playlist update failed: ${String(err)}`);
    }
  };

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

  const handleDeletePlaylist = async (persistentIdHex: string) => {
    if (!snapshot) return;
    try {
      const nextSnap = await podbridgeApi.deletePlaylist(
        snapshot.device.id,
        persistentIdHex,
      );
      setSnapshot(nextSnap);
      setActiveView({ kind: "songs" });
      showToast("Playlist deleted.");
    } catch (err) {
      showToast(`Delete failed: ${String(err)}`);
    }
  };

  const handleCreateMock = async (
    path: string,
    name: string,
    gen: IpodGeneration,
  ) => {
    try {
      const snap = await podbridgeApi.createMockDevice(path, name, gen);
      setSnapshot(snap);
      await refreshAll();
      setActiveView({ kind: "songs" });
      showToast(`Created Mock ${snap.device.generation_label}.`);
    } catch (err) {
      showToast(`Mock creation failed: ${String(err)}`);
    }
  };

  const handleOpenMock = async (path: string) => {
    try {
      const snap = await podbridgeApi.openMockDevice(path);
      setSnapshot(snap);
      await refreshAll();
      setActiveView({ kind: "songs" });
      showToast(`Opened Mock ${snap.device.name}.`);
    } catch (err) {
      showToast(`Open Mock failed: ${String(err)}`);
    }
  };

  const handleRestoreBackup = async (backupId: string) => {
    if (!snapshot) return;
    try {
      const snap = await podbridgeApi.restoreBackup(
        snapshot.device.id,
        backupId,
      );
      setSnapshot(snap);
      await refreshAll();
      showToast("Database backup restored successfully.");
    } catch (err) {
      showToast(`Restore failed: ${String(err)}`);
    }
  };

  const handlePickFiles = async () => {
    if (!snapshot) return;
    const picked = await podbridgeApi.pickAudioOrM3uFiles(true);
    if (picked.length > 0) {
      await startTransferPaths(picked);
    }
  };

  const handlePickFolder = async () => {
    if (!snapshot) return;
    const folder = await podbridgeApi.pickFolderDialog(
      "Select Album or Music Folder",
    );
    if (folder) {
      await startTransferPaths([folder]);
    }
  };

  const activePlaylist =
    activeView.kind === "playlist"
      ? snapshot?.playlists.find(
          (p) => p.persistent_id_hex === activeView.persistentIdHex,
        )
      : undefined;

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
        onCreatePlaylistPrompt={() => setNewPlaylistOpen(true)}
        onDropTracksOnPlaylist={(pid) => {
          if (draggedTrackIds.length > 0) {
            void handleAppendToPlaylist(pid, draggedTrackIds);
            setDraggedTrackIds([]);
          }
        }}
        hasDraggedSelection={draggedTrackIds.length > 0}
        onOpenMockModal={() => setMockModalOpen(true)}
        onToggleTheme={() => void handleToggleTheme()}
        onOpenSettings={() => setSettingsModalOpen(true)}
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
          ) : activePlaylist ? (
            <PlaylistView
              deviceId={snapshot.device.id}
              playlist={activePlaylist}
              allTracks={snapshot.tracks}
              searchQuery={searchQuery}
              onRenamePlaylist={(pid, name) =>
                void handleRenamePlaylist(pid, name)
              }
              onReorderOrUpdateTracks={(pid, ids) =>
                void handleSetPlaylistTracks(pid, ids)
              }
              onDeletePlaylist={(pid) => void handleDeletePlaylist(pid)}
            />
          ) : null}
        </section>

        <TransferDrawer
          progress={transferProgress}
          onCancel={() => void podbridgeApi.cancelActiveTransfer()}
          onDismiss={() => setTransferProgress(null)}
        />
      </main>

      <DropOverlay
        visible={dropHoverPaths !== null}
        hoveredPaths={dropHoverPaths ?? []}
        deviceName={snapshot?.device.name ?? null}
        losslessTargetLabel={
          settings.transfer.lossless_target === "Alac" ? "ALAC" : "AAC"
        }
      />

      <NewPlaylistModal
        open={newPlaylistOpen}
        onClose={() => setNewPlaylistOpen(false)}
        onCreate={(name) => void handleCreatePlaylist(name)}
      />

      <MockDeviceModal
        open={mockModalOpen}
        onClose={() => setMockModalOpen(false)}
        onCreateMock={(p, n, g) => void handleCreateMock(p, n, g)}
        onOpenExistingMock={(p) => void handleOpenMock(p)}
      />

      <SettingsModal
        open={settingsModalOpen}
        settings={settings}
        activeDeviceId={snapshot?.device.id ?? null}
        onClose={() => setSettingsModalOpen(false)}
        onSaveSettings={(s) => void handleSaveSettings(s)}
        onRestoreBackup={(id) => void handleRestoreBackup(id)}
      />

      {toast && <div className="status-toast">{toast}</div>}
    </div>
  );
}
