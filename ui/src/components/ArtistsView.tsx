// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

import { useMemo } from "react";
import type { ArtistDto, PlaylistDto, TrackDto } from "../types";
import { ArtworkThumb } from "./ArtworkThumb";
import { SongsTable } from "./SongsTable";

interface ArtistsViewProps {
  deviceId: string;
  artists: ArtistDto[];
  tracks: TrackDto[];
  playlists: PlaylistDto[];
  searchQuery: string;
  selectedArtist?: string;
  onSelectArtist: (name: string) => void;
  onDeleteTracks: (trackIds: number[]) => void;
  onAppendToPlaylist: (persistentIdHex: string, trackIds: number[]) => void;
  onDragSelectionChange?: (trackIds: number[]) => void;
}

export function ArtistsView({
  deviceId,
  artists,
  tracks,
  playlists,
  searchQuery,
  selectedArtist,
  onSelectArtist,
  onDeleteTracks,
  onAppendToPlaylist,
  onDragSelectionChange,
}: ArtistsViewProps) {
  const filteredArtists = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return artists;
    return artists.filter((a) => a.name.toLowerCase().includes(q));
  }, [artists, searchQuery]);

  const effectiveArtist =
    selectedArtist && filteredArtists.some((a) => a.name === selectedArtist)
      ? selectedArtist
      : filteredArtists[0]?.name;

  const artistTracks = useMemo(() => {
    if (!effectiveArtist) return [];
    return tracks.filter(
      (t) => t.artist.toLowerCase() === effectiveArtist.toLowerCase(),
    );
  }, [tracks, effectiveArtist]);

  if (artists.length === 0) {
    return <div className="empty-state">No artists on this iPod yet.</div>;
  }

  return (
    <div className="artists-split">
      <div className="artists-list">
        {filteredArtists.map((art) => {
          const active = art.name === effectiveArtist;
          return (
            <button
              key={art.name}
              type="button"
              className={`artist-row ${active ? "active" : ""}`}
              onClick={() => onSelectArtist(art.name)}
            >
              <ArtworkThumb
                deviceId={deviceId}
                mhiiLink={art.mhii_link}
                seedText={art.name}
                sizePx={34}
                borderRadius={999}
              />
              <div style={{ minWidth: 0, flex: 1 }}>
                <div className="truncate-text" style={{ fontWeight: 600 }}>
                  {art.name}
                </div>
                <div
                  className="tabular"
                  style={{ fontSize: 11, color: "var(--text-muted)" }}
                >
                  {art.album_count} {art.album_count === 1 ? "album" : "albums"} ·{" "}
                  {art.track_count} {art.track_count === 1 ? "song" : "songs"}
                </div>
              </div>
            </button>
          );
        })}
      </div>

      <div className="artists-detail">
        <SongsTable
          deviceId={deviceId}
          tracks={artistTracks}
          playlists={playlists}
          searchQuery=""
          onDeleteTracks={onDeleteTracks}
          onAppendToPlaylist={onAppendToPlaylist}
          onDragSelectionChange={onDragSelectionChange}
        />
      </div>
    </div>
  );
}
