// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

import { useMemo } from "react";
import { ArrowLeft, Trash2 } from "lucide-react";
import type { AlbumDto, PlaylistDto, TrackDto } from "../types";
import { formatDurationMs } from "../utils/format";
import { ArtworkThumb } from "./ArtworkThumb";
import { SongsTable } from "./SongsTable";

interface AlbumsGridProps {
  deviceId: string;
  albums: AlbumDto[];
  tracks: TrackDto[];
  playlists: PlaylistDto[];
  searchQuery: string;
  selectedAlbumKey?: string;
  onSelectAlbum: (key: string | undefined) => void;
  onDeleteTracks: (trackIds: number[]) => void;
  onAppendToPlaylist: (persistentIdHex: string, trackIds: number[]) => void;
  onDragSelectionChange?: (trackIds: number[]) => void;
}

export function AlbumsGrid({
  deviceId,
  albums,
  tracks,
  playlists,
  searchQuery,
  selectedAlbumKey,
  onSelectAlbum,
  onDeleteTracks,
  onAppendToPlaylist,
  onDragSelectionChange,
}: AlbumsGridProps) {
  const filteredAlbums = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return albums;
    return albums.filter(
      (a) =>
        a.title.toLowerCase().includes(q) || a.artist.toLowerCase().includes(q),
    );
  }, [albums, searchQuery]);

  const activeAlbum = useMemo(
    () => albums.find((a) => a.key === selectedAlbumKey),
    [albums, selectedAlbumKey],
  );

  const activeAlbumTracks = useMemo(() => {
    if (!activeAlbum) return [];
    const idSet = new Set(activeAlbum.track_ids);
    return tracks
      .filter((t) => idSet.has(t.track_id))
      .sort((a, b) => a.disc_nr - b.disc_nr || a.track_nr - b.track_nr);
  }, [activeAlbum, tracks]);

  if (activeAlbum) {
    const banner = (
      <div className="album-hero">
        <button
          type="button"
          className="btn"
          onClick={() => onSelectAlbum(undefined)}
        >
          <ArrowLeft size={13} />
          <span>Albums</span>
        </button>

        <ArtworkThumb
          deviceId={deviceId}
          mhiiLink={activeAlbum.mhii_link}
          seedText={activeAlbum.key}
          sizePx={82}
          borderRadius={10}
        />

        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 18, fontWeight: 700 }} className="truncate-text">
            {activeAlbum.title}
          </div>
          <div style={{ color: "var(--text-secondary)", marginTop: 2 }}>
            {activeAlbum.artist}
            {activeAlbum.year > 0 ? ` · ${activeAlbum.year}` : ""} ·{" "}
            <span className="tabular">
              {activeAlbum.track_count} songs ·{" "}
              {formatDurationMs(activeAlbum.total_length_ms)}
            </span>
          </div>
        </div>

        <button
          type="button"
          className="btn btn-danger"
          onClick={() => {
            onDeleteTracks(activeAlbum.track_ids);
            onSelectAlbum(undefined);
          }}
        >
          <Trash2 size={13} />
          <span>Delete Album</span>
        </button>
      </div>
    );

    return (
      <SongsTable
        deviceId={deviceId}
        tracks={activeAlbumTracks}
        playlists={playlists}
        searchQuery=""
        headerBanner={banner}
        onDeleteTracks={onDeleteTracks}
        onAppendToPlaylist={onAppendToPlaylist}
        onDragSelectionChange={onDragSelectionChange}
      />
    );
  }

  return (
    <div className="albums-scroll">
      {filteredAlbums.length === 0 ? (
        <div className="empty-state">No albums found.</div>
      ) : (
        <div className="albums-grid">
          {filteredAlbums.map((alb) => (
            <button
              key={alb.key}
              type="button"
              className="album-card"
              onClick={() => onSelectAlbum(alb.key)}
            >
              <ArtworkThumb
                deviceId={deviceId}
                mhiiLink={alb.mhii_link}
                seedText={alb.key}
                sizePx={148}
                borderRadius={9}
              />
              <div className="album-card-title truncate-text" title={alb.title}>
                {alb.title}
              </div>
              <div
                className="album-card-artist truncate-text"
                title={alb.artist}
              >
                {alb.artist}
              </div>
              <div className="album-card-meta tabular">
                {alb.track_count} {alb.track_count === 1 ? "song" : "songs"}
                {alb.year > 0 ? ` · ${alb.year}` : ""}
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
