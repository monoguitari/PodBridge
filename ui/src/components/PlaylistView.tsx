// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

import { useMemo, useState } from "react";
import {
  ArrowDown,
  ArrowUp,
  Check,
  Edit3,
  Trash2,
  X,
} from "lucide-react";
import type { PlaylistDto, TrackDto } from "../types";
import { formatDurationMs } from "../utils/format";
import { ArtworkThumb } from "./ArtworkThumb";

interface PlaylistViewProps {
  deviceId: string;
  playlist: PlaylistDto;
  allTracks: TrackDto[];
  searchQuery: string;
  onRenamePlaylist: (persistentIdHex: string, newName: string) => void;
  onReorderOrUpdateTracks: (persistentIdHex: string, trackIds: number[]) => void;
  onDeletePlaylist: (persistentIdHex: string) => void;
}

export function PlaylistView({
  deviceId,
  playlist,
  allTracks,
  searchQuery,
  onRenamePlaylist,
  onReorderOrUpdateTracks,
  onDeletePlaylist,
}: PlaylistViewProps) {
  const [editingName, setEditingName] = useState(false);
  const [draftName, setDraftName] = useState(playlist.name);
  const [dragIdx, setDragIdx] = useState<number | null>(null);

  const trackById = useMemo(() => {
    const map = new Map<number, TrackDto>();
    for (const t of allTracks) {
      map.set(t.track_id, t);
    }
    return map;
  }, [allTracks]);

  const orderedTracks = useMemo(() => {
    const list: TrackDto[] = [];
    for (const tid of playlist.track_ids) {
      const found = trackById.get(tid);
      if (found) list.push(found);
    }
    return list;
  }, [playlist.track_ids, trackById]);

  const visibleTracks = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return orderedTracks.map((track, idx) => ({ track, idx }));
    return orderedTracks
      .map((track, idx) => ({ track, idx }))
      .filter(
        ({ track }) =>
          track.title.toLowerCase().includes(q) ||
          track.artist.toLowerCase().includes(q) ||
          track.album.toLowerCase().includes(q),
      );
  }, [orderedTracks, searchQuery]);

  const totalDurationMs = useMemo(
    () => orderedTracks.reduce((acc, t) => acc + t.length_ms, 0),
    [orderedTracks],
  );

  const moveIndex = (from: number, to: number) => {
    if (to < 0 || to >= orderedTracks.length || from === to) return;
    const ids = orderedTracks.map((t) => t.track_id);
    const [moved] = ids.splice(from, 1);
    ids.splice(to, 0, moved);
    onReorderOrUpdateTracks(playlist.persistent_id_hex, ids);
  };

  const removeAtIndex = (idx: number) => {
    const ids = orderedTracks.map((t) => t.track_id);
    ids.splice(idx, 1);
    onReorderOrUpdateTracks(playlist.persistent_id_hex, ids);
  };

  const commitRename = () => {
    const trimmed = draftName.trim();
    if (trimmed && trimmed !== playlist.name) {
      onRenamePlaylist(playlist.persistent_id_hex, trimmed);
    }
    setEditingName(false);
  };

  return (
    <div className="songs-view">
      <div className="album-hero">
        <div style={{ flex: 1, minWidth: 0 }}>
          {editingName ? (
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <input
                type="text"
                className="search-input"
                style={{ maxWidth: 280, fontSize: 15, fontWeight: 650 }}
                value={draftName}
                autoFocus
                onChange={(e) => setDraftName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") commitRename();
                  if (e.key === "Escape") setEditingName(false);
                }}
              />
              <button
                type="button"
                className="btn btn-primary"
                onClick={commitRename}
              >
                <Check size={13} />
              </button>
              <button
                type="button"
                className="btn"
                onClick={() => setEditingName(false)}
              >
                <X size={13} />
              </button>
            </div>
          ) : (
            <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
              <span style={{ fontSize: 18, fontWeight: 700 }}>
                {playlist.name}
              </span>
              <button
                type="button"
                className="btn-icon"
                title="Rename Playlist"
                onClick={() => {
                  setDraftName(playlist.name);
                  setEditingName(true);
                }}
              >
                <Edit3 size={14} />
              </button>
            </div>
          )}
          <div
            className="tabular"
            style={{ color: "var(--text-secondary)", marginTop: 3 }}
          >
            {orderedTracks.length}{" "}
            {orderedTracks.length === 1 ? "song" : "songs"} ·{" "}
            {formatDurationMs(totalDurationMs)} · Drag rows or use arrows to
            reorder
          </div>
        </div>

        <button
          type="button"
          className="btn btn-danger"
          onClick={() => onDeletePlaylist(playlist.persistent_id_hex)}
        >
          <Trash2 size={13} />
          <span>Delete Playlist</span>
        </button>
      </div>

      <div className="table-scroll" style={{ padding: "4px 0" }}>
        {visibleTracks.length === 0 ? (
          <div className="empty-state">
            {orderedTracks.length === 0
              ? "Playlist is empty — select songs in the Songs view and click 'Add to Playlist' or drag tracks onto this playlist in the sidebar."
              : "No playlist items match your filter."}
          </div>
        ) : (
          visibleTracks.map(({ track, idx }) => (
            <div
              key={`${track.track_id}-${idx}`}
              className="playlist-row"
              draggable={!searchQuery.trim()}
              onDragStart={() => setDragIdx(idx)}
              onDragOver={(e) => {
                if (dragIdx !== null && dragIdx !== idx) e.preventDefault();
              }}
              onDrop={(e) => {
                e.preventDefault();
                if (dragIdx !== null && dragIdx !== idx) {
                  moveIndex(dragIdx, idx);
                }
                setDragIdx(null);
              }}
            >
              <span
                className="tabular"
                style={{
                  width: 28,
                  textAlign: "right",
                  color: "var(--text-muted)",
                  fontSize: 12,
                }}
              >
                {idx + 1}
              </span>

              <ArtworkThumb
                deviceId={deviceId}
                mhiiLink={track.mhii_link}
                seedText={`${track.artist}-${track.album}`}
                sizePx={28}
                borderRadius={5}
              />

              <div style={{ flex: 1, minWidth: 0 }}>
                <div className="truncate-text" style={{ fontWeight: 550 }}>
                  {track.title}
                </div>
                <div
                  className="truncate-text"
                  style={{ fontSize: 11.5, color: "var(--text-secondary)" }}
                >
                  {track.artist} · {track.album}
                </div>
              </div>

              <span
                className="tabular"
                style={{ color: "var(--text-secondary)", fontSize: 12 }}
              >
                {formatDurationMs(track.length_ms)}
              </span>

              {!searchQuery.trim() && (
                <div style={{ display: "flex", gap: 2 }}>
                  <button
                    type="button"
                    className="btn-icon"
                    disabled={idx === 0}
                    onClick={() => moveIndex(idx, idx - 1)}
                    title="Move Up"
                  >
                    <ArrowUp size={13} />
                  </button>
                  <button
                    type="button"
                    className="btn-icon"
                    disabled={idx === orderedTracks.length - 1}
                    onClick={() => moveIndex(idx, idx + 1)}
                    title="Move Down"
                  >
                    <ArrowDown size={13} />
                  </button>
                </div>
              )}

              <button
                type="button"
                className="btn-icon"
                onClick={() => removeAtIndex(idx)}
                title="Remove from Playlist (keeps song on iPod)"
              >
                <X size={14} />
              </button>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
