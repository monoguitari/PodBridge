// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::chunks::{
    ItdbDatabase, Playlist, PlaylistItem, TrackRecord, FILETYPE_MP3, FILETYPE_WAV,
};
use crate::error::ItdbError;

/// Default duration tolerance (`2,500` ms) when matching duplicate songs across lossless/lossy
/// containers.
pub const DEFAULT_DUPLICATE_TOLERANCE_MS: u32 = 2_500;

/// Ergonomic metadata & audio descriptor for adding a new track to an [`ItdbDatabase`].
#[derive(Debug, Clone)]
pub struct NewTrackInput {
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub genre: String,
    pub composer: String,
    pub comment: String,
    /// Colon-delimited iPod path, e.g. `:iPod_Control:Music:F03:KLMN.m4a`.
    pub location: String,
    pub filetype: u32,
    pub filetype_desc: String,
    pub size: u32,
    pub length_ms: u32,
    pub bitrate: u32,
    pub sample_rate_hz: u32,
    pub sample_count: u64,
    pub track_nr: u32,
    pub tracks_total: u32,
    pub disc_nr: u32,
    pub discs_total: u32,
    pub year: u32,
    pub compilation: bool,
    pub has_artwork: bool,
    pub artwork_size: u32,
    pub mhii_link: u32,
    pub timestamp_mac: u32,
}

/// Summary returned for each deleted track so the storage/artwork layers can reclaim disk & `.ithmb`
/// slots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovedTrackInfo {
    pub track_id: u32,
    pub dbid: u64,
    pub location: String,
    pub mhii_link: u32,
}

/// Parsed `.m3u` / `.m3u8` playlist specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M3uPlaylistSpec {
    pub name: String,
    pub track_paths: Vec<PathBuf>,
}

/// Candidate metadata checked against [`DuplicateIndex`].
#[derive(Debug, Clone, Copy)]
pub struct DuplicateCandidate<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
    pub length_ms: u32,
}

/// O(1) normalized metadata + duration-tolerance lookup index for detecting duplicate tracks
/// already on the iPod or staged in the same transfer batch.
#[derive(Debug, Default, Clone)]
pub struct DuplicateIndex {
    entries: HashMap<(String, String, String), Vec<(u32, u32)>>,
}

impl DuplicateIndex {
    /// Builds a [`DuplicateIndex`] from all tracks currently in `db`.
    #[must_use]
    pub fn from_database(db: &ItdbDatabase) -> Self {
        let mut idx = Self::default();
        for t in &db.tracks {
            idx.insert(&t.title, &t.artist, &t.album, t.length_ms, t.track_id);
        }
        idx
    }

    /// Registers a track (either already on the iPod or newly staged in the current batch).
    pub fn insert(&mut self, title: &str, artist: &str, album: &str, length_ms: u32, id: u32) {
        let key = (
            normalize_tag_key(artist),
            normalize_tag_key(album),
            normalize_tag_key(title),
        );
        if key.2.is_empty() {
            return;
        }
        self.entries.entry(key).or_default().push((id, length_ms));
    }

    /// Checks if `candidate` matches an indexed track within `tolerance_ms` (returns matching
    /// `track_id`).
    #[must_use]
    pub fn find_duplicate(
        &self,
        candidate: DuplicateCandidate<'_>,
        tolerance_ms: u32,
    ) -> Option<u32> {
        let key = (
            normalize_tag_key(candidate.artist),
            normalize_tag_key(candidate.album),
            normalize_tag_key(candidate.title),
        );
        if key.2.is_empty() {
            return None;
        }
        let bucket = self.entries.get(&key)?;
        for &(track_id, existing_len) in bucket {
            if existing_len == 0
                || candidate.length_ms == 0
                || existing_len.abs_diff(candidate.length_ms) <= tolerance_ms
            {
                return Some(track_id);
            }
        }
        None
    }
}

fn normalize_tag_key(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_space = true;
    for ch in input.trim().chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            prev_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Converts a colon-delimited iPod location (`:iPod_Control:Music:F00:ABCD.m4a`) into a relative
/// filesystem path (`iPod_Control/Music/F00/ABCD.m4a`).
#[must_use]
pub fn ipod_location_to_rel_path(location: &str) -> PathBuf {
    let trimmed = location.trim_matches(|c| c == ':' || c == '/' || c == '\\');
    let mut rel = PathBuf::new();
    for seg in trimmed.split([':', '/', '\\']) {
        if !seg.is_empty() && seg != "." && seg != ".." {
            rel.push(seg);
        }
    }
    rel
}

/// Parses `.m3u` / `.m3u8` text content, resolving relative track entries against `base_dir`.
#[must_use]
pub fn parse_m3u_content(content: &str, base_dir: &Path, fallback_name: &str) -> M3uPlaylistSpec {
    let clean = content.strip_prefix('\u{FEFF}').unwrap_or(content);
    let mut name = fallback_name.trim().to_owned();
    if name.is_empty() {
        name = "Imported Playlist".to_owned();
    }

    let mut track_paths = Vec::new();
    for raw_line in clean.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#PLAYLIST:") {
            let custom = rest.trim();
            if !custom.is_empty() {
                name = custom.to_owned();
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        // Skip network URLs (PodBridge transfers local audio files)
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            continue;
        }

        let normalized = if cfg!(windows) {
            line.replace('/', "\\")
        } else {
            line.replace('\\', "/")
        };
        let candidate = PathBuf::from(&normalized);
        let resolved = if candidate.is_absolute() {
            candidate
        } else {
            base_dir.join(candidate)
        };
        track_paths.push(resolved);
    }

    M3uPlaylistSpec { name, track_paths }
}

/// Reads and parses a `.m3u` or `.m3u8` playlist file from disk.
pub fn parse_m3u_file(path: &Path) -> Result<M3uPlaylistSpec, ItdbError> {
    let bytes = fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let fallback = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Imported Playlist");
    Ok(parse_m3u_content(&text, base_dir, fallback))
}

impl ItdbDatabase {
    /// Appends a new track to the database and marks library indices dirty. Returns `(track_id, dbid)`.
    pub fn add_track(&mut self, input: NewTrackInput) -> (u32, u64) {
        let track_id = self.alloc_catalog_id();
        let dbid = self.alloc_unique_u64();
        let mut rec = TrackRecord::new(track_id, dbid);

        if input.filetype == FILETYPE_WAV {
            rec.raw_header[0x7E..0x80].copy_from_slice(&0u16.to_le_bytes());
            rec.raw_header[0x90..0x92].copy_from_slice(&0u16.to_le_bytes());
        } else if input.filetype == FILETYPE_MP3 {
            rec.type2 = 1;
            rec.raw_header[0x90..0x92].copy_from_slice(&0x000Cu16.to_le_bytes());
        } else {
            rec.type2 = 0;
        }

        rec.filetype = input.filetype;
        rec.filetype_desc = input.filetype_desc;
        rec.title = input.title;
        rec.artist = input.artist;
        rec.album_artist = input.album_artist;
        rec.album = input.album;
        rec.genre = input.genre;
        rec.composer = input.composer;
        rec.comment = input.comment;
        rec.location = input.location;
        rec.size = input.size;
        rec.length_ms = input.length_ms;
        rec.bitrate = input.bitrate;
        rec.sample_rate_hz = input.sample_rate_hz.max(8_000);
        rec.sample_count = if input.sample_count > 0 {
            input.sample_count
        } else {
            (u64::from(input.length_ms) * u64::from(rec.sample_rate_hz)) / 1_000
        };
        rec.track_nr = input.track_nr;
        rec.tracks_total = input.tracks_total;
        rec.disc_nr = input.disc_nr;
        rec.discs_total = input.discs_total;
        rec.year = input.year;
        rec.compilation = input.compilation;
        rec.time_added = input.timestamp_mac;
        rec.time_modified = input.timestamp_mac;
        rec.has_artwork = input.has_artwork;
        rec.artwork_count = u16::from(input.has_artwork);
        rec.artwork_size = if input.has_artwork {
            input.artwork_size
        } else {
            0
        };
        rec.mhii_link = if input.has_artwork {
            input.mhii_link
        } else {
            0
        };

        self.tracks.push(rec);
        self.dirty_library_indices = true;
        (track_id, dbid)
    }

    /// Removes all tracks whose `track_id` is in `track_ids`, scrubbing them from every playlist
    /// and marking library indices dirty.
    pub fn remove_tracks(&mut self, track_ids: &[u32]) -> Vec<RemovedTrackInfo> {
        if track_ids.is_empty() {
            return Vec::new();
        }
        let target: HashSet<u32> = track_ids.iter().copied().collect();
        let mut removed = Vec::new();

        self.tracks.retain(|t| {
            if target.contains(&t.track_id) {
                removed.push(RemovedTrackInfo {
                    track_id: t.track_id,
                    dbid: t.dbid,
                    location: t.location.clone(),
                    mhii_link: t.mhii_link,
                });
                false
            } else {
                true
            }
        });

        if !removed.is_empty() {
            for pl in &mut self.playlists {
                pl.items.retain(|it| !target.contains(&it.track_id));
            }
            for pl in &mut self.podcast_playlists {
                pl.items.retain(|it| !target.contains(&it.track_id));
            }
            self.dirty_library_indices = true;
        }
        removed
    }

    /// Creates a new user playlist in `mhsd type = 2` and returns its `persistent_id`.
    pub fn create_playlist(&mut self, name: impl Into<String>, timestamp_mac: u32) -> u64 {
        let mut rng = rand::thread_rng();
        let mut pid = rng.gen::<u64>() | 1;
        while self.playlists.iter().any(|p| p.persistent_id == pid) {
            pid = rng.gen::<u64>() | 1;
        }
        let pl = Playlist::new(name, false, pid, timestamp_mac);
        self.playlists.push(pl);
        pid
    }

    /// Renames a user playlist identified by `persistent_id`.
    pub fn rename_playlist(
        &mut self,
        persistent_id: u64,
        new_name: impl Into<String>,
        timestamp_mac: u32,
    ) -> Result<(), ItdbError> {
        let pl = self
            .playlists
            .iter_mut()
            .find(|p| p.persistent_id == persistent_id)
            .ok_or(ItdbError::PlaylistNotFound(persistent_id))?;
        if pl.is_master {
            return Err(ItdbError::CannotDeleteMasterPlaylist);
        }
        pl.name = new_name.into();
        pl.timestamp_modified = timestamp_mac;
        Ok(())
    }

    /// Replaces the ordered track membership of a user playlist.
    pub fn set_playlist_tracks(
        &mut self,
        persistent_id: u64,
        ordered_track_ids: &[u32],
        timestamp_mac: u32,
    ) -> Result<(), ItdbError> {
        let dbid_by_tid: HashMap<u32, u64> =
            self.tracks.iter().map(|t| (t.track_id, t.dbid)).collect();

        let pl_idx = self
            .playlists
            .iter()
            .position(|p| p.persistent_id == persistent_id)
            .ok_or(ItdbError::PlaylistNotFound(persistent_id))?;

        if self.playlists[pl_idx].is_master {
            return Err(ItdbError::CannotDeleteMasterPlaylist);
        }

        let mut rng = rand::thread_rng();
        let mut items = Vec::with_capacity(ordered_track_ids.len());
        let mut next_gid = self.next_catalog_id;

        for &tid in ordered_track_ids {
            if let Some(&dbid) = dbid_by_tid.get(&tid) {
                let pos_1 = (items.len() + 1) as u32;
                let gid = next_gid;
                next_gid = next_gid.saturating_add(1);
                let mut item = PlaylistItem::new(tid, dbid, gid, pos_1, rng.gen::<u64>() | 1);
                item.timestamp = timestamp_mac;
                items.push(item);
            }
        }

        self.next_catalog_id = next_gid;
        let pl = &mut self.playlists[pl_idx];
        pl.items = items;
        pl.timestamp_modified = timestamp_mac;
        Ok(())
    }

    /// Appends `track_ids` to an existing user playlist (skipping invalid track IDs).
    pub fn append_tracks_to_playlist(
        &mut self,
        persistent_id: u64,
        track_ids: &[u32],
        timestamp_mac: u32,
    ) -> Result<usize, ItdbError> {
        let dbid_by_tid: HashMap<u32, u64> =
            self.tracks.iter().map(|t| (t.track_id, t.dbid)).collect();

        let pl_idx = self
            .playlists
            .iter()
            .position(|p| p.persistent_id == persistent_id)
            .ok_or(ItdbError::PlaylistNotFound(persistent_id))?;

        if self.playlists[pl_idx].is_master {
            return Err(ItdbError::CannotDeleteMasterPlaylist);
        }

        let mut rng = rand::thread_rng();
        let mut next_gid = self.next_catalog_id;
        let mut added = 0usize;
        let base_len = self.playlists[pl_idx].items.len();

        for &tid in track_ids {
            if let Some(&dbid) = dbid_by_tid.get(&tid) {
                let pos_1 = (base_len + added + 1) as u32;
                let gid = next_gid;
                next_gid = next_gid.saturating_add(1);
                let mut item = PlaylistItem::new(tid, dbid, gid, pos_1, rng.gen::<u64>() | 1);
                item.timestamp = timestamp_mac;
                self.playlists[pl_idx].items.push(item);
                added += 1;
            }
        }

        self.next_catalog_id = next_gid;
        if added > 0 {
            self.playlists[pl_idx].timestamp_modified = timestamp_mac;
        }
        Ok(added)
    }

    /// Deletes a user playlist by `persistent_id` (refuses to delete the Master Library playlist).
    pub fn delete_playlist(&mut self, persistent_id: u64) -> Result<(), ItdbError> {
        let idx = self
            .playlists
            .iter()
            .position(|p| p.persistent_id == persistent_id)
            .ok_or(ItdbError::PlaylistNotFound(persistent_id))?;
        if self.playlists[idx].is_master {
            return Err(ItdbError::CannotDeleteMasterPlaylist);
        }
        self.playlists.remove(idx);
        Ok(())
    }
}
