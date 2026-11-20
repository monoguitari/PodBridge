// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

import { useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp, ListPlus, Trash2, X } from "lucide-react";
import type {
  PlaylistDto,
  SortColumn,
  SortDirection,
  TrackDto,
} from "../types";
import {
  filterAndSortTracks,
  formatBytes,
  formatDurationMs,
} from "../utils/format";
import { ArtworkThumb } from "./ArtworkThumb";

interface SongsTableProps {
  deviceId: string;
  tracks: TrackDto[];
  playlists: PlaylistDto[];
  searchQuery: string;
  headerBanner?: React.ReactNode;
  onDeleteTracks: (trackIds: number[]) => void;
  onAppendToPlaylist: (persistentIdHex: string, trackIds: number[]) => void;
  onDragSelectionChange?: (trackIds: number[]) => void;
}

const ROW_HEIGHT = 38;

export function SongsTable({
  deviceId,
  tracks,
  playlists,
  searchQuery,
  headerBanner,
  onDeleteTracks,
  onAppendToPlaylist,
  onDragSelectionChange,
}: SongsTableProps) {
  const [sortCol, setSortCol] = useState<SortColumn>("artist");
  const [sortDir, setSortDir] = useState<SortDirection>("asc");
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const [lastClickedIdx, setLastClickedIdx] = useState<number | null>(null);
  const [playlistMenuOpen, setPlaylistMenuOpen] = useState(false);

  const parentRef = useRef<HTMLDivElement>(null);

  const visibleTracks = useMemo(
    () => filterAndSortTracks(tracks, searchQuery, sortCol, sortDir),
    [tracks, searchQuery, sortCol, sortDir],
  );

  // Prune stale selections if tracks are deleted
  useEffect(() => {
    const valid = new Set(tracks.map((t) => t.track_id));
    setSelectedIds((prev) => {
      let changed = false;
      const next = new Set<number>();
      for (const id of prev) {
        if (valid.has(id)) next.add(id);
        else changed = true;
      }
      return changed ? next : prev;
    });
  }, [tracks]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (
        document.activeElement?.tagName === "INPUT" ||
        document.activeElement?.tagName === "TEXTAREA"
      ) {
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") {
        e.preventDefault();
        setSelectedIds(new Set(visibleTracks.map((t) => t.track_id)));
      } else if (
        (e.key === "Delete" || e.key === "Backspace") &&
        selectedIds.size > 0
      ) {
        e.preventDefault();
        onDeleteTracks(Array.from(selectedIds));
      } else if (e.key === "Escape" && selectedIds.size > 0) {
        setSelectedIds(new Set());
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [visibleTracks, selectedIds, onDeleteTracks]);

  const rowVirtualizer = useVirtualizer({
    count: visibleTracks.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 16,
  });

  const handleSortClick = (col: SortColumn) => {
    if (sortCol === col) {
      setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    } else {
      setSortCol(col);
      setSortDir("asc");
    }
  };

  const handleRowClick = (
    e: React.MouseEvent,
    rowIndex: number,
    trackId: number,
  ) => {
    if (e.shiftKey && lastClickedIdx !== null) {
      const lo = Math.min(lastClickedIdx, rowIndex);
      const hi = Math.max(lastClickedIdx, rowIndex);
      const next = new Set(selectedIds);
      for (let i = lo; i <= hi; i += 1) {
        const item = visibleTracks[i];
        if (item) next.add(item.track_id);
      }
      setSelectedIds(next);
    } else if (e.ctrlKey || e.metaKey) {
      const next = new Set(selectedIds);
      if (next.has(trackId)) next.delete(trackId);
      else next.add(trackId);
      setSelectedIds(next);
      setLastClickedIdx(rowIndex);
    } else {
      setSelectedIds(new Set([trackId]));
      setLastClickedIdx(rowIndex);
    }
  };

  const renderSortIcon = (col: SortColumn) => {
    if (sortCol !== col) return null;
    return sortDir === "asc" ? <ArrowUp size={12} /> : <ArrowDown size={12} />;
  };

  const selectedArray = useMemo(() => Array.from(selectedIds), [selectedIds]);

  return (
    <div className="songs-view">
      {headerBanner}

      {selectedIds.size > 0 && (
        <div className="selection-bar">
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span className="badge-pill tabular">
              {selectedIds.size} selected
            </span>
            <button
              type="button"
              className="btn-icon"
              onClick={() => setSelectedIds(new Set())}
              title="Clear selection (Esc)"
            >
              <X size={14} />
            </button>
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: 8, position: "relative" }}>
            {playlists.length > 0 && (
              <div style={{ position: "relative" }}>
                <button
                  type="button"
                  className="btn"
                  onClick={() => setPlaylistMenuOpen((o) => !o)}
                >
                  <ListPlus size={13} />
                  <span>Add to Playlist</span>
                </button>
                {playlistMenuOpen && (
                  <div className="popover-menu">
                    {playlists.map((pl) => (
                      <button
                        key={pl.persistent_id_hex}
                        type="button"
                        className="popover-item"
                        onClick={() => {
                          onAppendToPlaylist(pl.persistent_id_hex, selectedArray);
                          setPlaylistMenuOpen(false);
                        }}
                      >
                        {pl.name}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            )}

            <button
              type="button"
              className="btn btn-danger"
              onClick={() => onDeleteTracks(selectedArray)}
            >
              <Trash2 size={13} />
              <span>Delete from iPod</span>
            </button>
          </div>
        </div>
      )}

      <div ref={parentRef} className="table-scroll">
        <div className="table-inner">
          <div className="table-header">
            <button
              type="button"
              className="th-cell th-title"
              onClick={() => handleSortClick("title")}
            >
              <span>Title</span>
              {renderSortIcon("title")}
            </button>
            <button
              type="button"
              className="th-cell th-artist"
              onClick={() => handleSortClick("artist")}
            >
              <span>Artist</span>
              {renderSortIcon("artist")}
            </button>
            <button
              type="button"
              className="th-cell th-album"
              onClick={() => handleSortClick("album")}
            >
              <span>Album</span>
              {renderSortIcon("album")}
            </button>
            <button
              type="button"
              className="th-cell th-num"
              onClick={() => handleSortClick("length_ms")}
            >
              <span>Time</span>
              {renderSortIcon("length_ms")}
            </button>
            <button
              type="button"
              className="th-cell th-fmt"
              onClick={() => handleSortClick("format_badge")}
            >
              <span>Codec</span>
              {renderSortIcon("format_badge")}
            </button>
            <button
              type="button"
              className="th-cell th-num"
              onClick={() => handleSortClick("size_bytes")}
            >
              <span>Size</span>
              {renderSortIcon("size_bytes")}
            </button>
            <button
              type="button"
              className="th-cell th-plays"
              onClick={() => handleSortClick("play_count")}
            >
              <span>Plays</span>
              {renderSortIcon("play_count")}
            </button>
          </div>

          {visibleTracks.length === 0 ? (
            <div className="empty-state">
              {tracks.length === 0
                ? "No songs on this iPod yet — drag audio files, folders, or .m3u8 playlists anywhere onto the window."
                : `No tracks matching "${searchQuery}".`}
            </div>
          ) : (
            <div
              style={{
                height: `${rowVirtualizer.getTotalSize()}px`,
                width: "100%",
                position: "relative",
              }}
            >
              {rowVirtualizer.getVirtualItems().map((virtualRow) => {
                const track = visibleTracks[virtualRow.index];
                const isSelected = selectedIds.has(track.track_id);
                return (
                  <div
                    key={track.track_id}
                    className={`tr-row ${isSelected ? "selected" : ""}`}
                    style={{
                      height: `${virtualRow.size}px`,
                      transform: `translateY(${virtualRow.start}px)`,
                    }}
                    draggable
                    onDragStart={(e) => {
                      const ids = isSelected ? selectedArray : [track.track_id];
                      e.dataTransfer.effectAllowed = "copy";
                      e.dataTransfer.setData("text/plain", ids.join(","));
                      onDragSelectionChange?.(ids);
                    }}
                    onDragEnd={() => onDragSelectionChange?.([])}
                    onClick={(e) =>
                      handleRowClick(e, virtualRow.index, track.track_id)
                    }
                  >
                    <div className="td-cell th-title">
                      <ArtworkThumb
                        deviceId={deviceId}
                        mhiiLink={track.mhii_link}
                        seedText={`${track.artist}-${track.album}`}
                        sizePx={26}
                        borderRadius={5}
                      />
                      <span className="truncate-text" title={track.title}>
                        {track.title}
                      </span>
                    </div>
                    <div
                      className="td-cell th-artist truncate-text"
                      title={track.artist}
                    >
                      {track.artist}
                    </div>
                    <div
                      className="td-cell th-album truncate-text"
                      title={track.album}
                    >
                      {track.album}
                    </div>
                    <div className="td-cell th-num tabular">
                      {formatDurationMs(track.length_ms)}
                    </div>
                    <div className="td-cell th-fmt">
                      <span className="codec-chip">{track.format_badge}</span>
                    </div>
                    <div className="td-cell th-num tabular">
                      {formatBytes(track.size_bytes)}
                    </div>
                    <div className="td-cell th-plays tabular">
                      {track.play_count}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
