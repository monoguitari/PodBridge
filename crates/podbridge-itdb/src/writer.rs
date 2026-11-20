// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};

use podbridge_crypto::{sign_itunesdb, HashingScheme};
use rand::Rng;

use crate::chunks::{
    AlbumItem, ItdbDatabase, MhbdHeader, Playlist, PlaylistItem, RawMhod, TrackRecord,
    DEFAULT_MHBD_HEADER_LEN, DEFAULT_MHIA_HEADER_LEN, DEFAULT_MHIP_HEADER_LEN,
    DEFAULT_MHIT_HEADER_LEN, DEFAULT_MHYP_HEADER_LEN, FILETYPE_AIFF, FILETYPE_M4A, FILETYPE_MP3,
    FILETYPE_WAV,
};
use crate::error::ItdbError;

impl ItdbDatabase {
    /// Creates a brand-new empty `iTunesDB` with canonical `[4, 1, 3, 2]` sections and a
    /// Master Library playlist named `device_name`.
    #[must_use]
    pub fn new_empty(device_name: impl Into<String>, timestamp_mac: u32) -> Self {
        let mut rng = rand::thread_rng();
        let header = MhbdHeader {
            db_id: rng.gen::<u64>() | 1,
            library_id: rng.gen::<u64>() | 1,
            pid: rng.gen::<u64>() | 1,
            ..MhbdHeader::default()
        };

        let name = device_name.into();
        let master_pid = rng.gen::<u64>() | 1;
        let master = Playlist::new(name, true, master_pid, timestamp_mac);

        Self {
            header,
            albums: Vec::new(),
            tracks: Vec::new(),
            podcast_playlists: vec![master.clone()],
            playlists: vec![master],
            category_playlists: Vec::new(),
            artists: Vec::new(),
            section_order: vec![4, 1, 3, 2],
            extra_sections: Vec::new(),
            embedded_hash72: None,
            next_catalog_id: 1000,
            dirty_library_indices: true,
        }
    }

    /// Allocates the next unique 32-bit catalog ID (`track_id`, `album_id`, `artist_id`, `group_id`).
    pub fn alloc_catalog_id(&mut self) -> u32 {
        let id = self.next_catalog_id;
        self.next_catalog_id = self.next_catalog_id.saturating_add(1);
        id
    }

    /// Allocates a collision-free 64-bit persistent ID (`dbid`, `sql_id`, `persistent_id`).
    pub fn alloc_unique_u64(&self) -> u64 {
        let mut rng = rand::thread_rng();
        loop {
            let candidate = rng.gen::<u64>();
            if candidate == 0 {
                continue;
            }
            let collides_track = self.tracks.iter().any(|t| t.dbid == candidate);
            let collides_album = self.albums.iter().any(|a| a.sql_id == candidate);
            if !collides_track && !collides_album {
                return candidate;
            }
        }
    }

    /// Rebuilds derived Album (`mhsd 4`), Artist IDs, Master Playlists (`mhsd 2` & `3`), and
    /// Master Library sorted/jump-bar indices (`mhod 52` & `53`) so menus and Cover Flow stay
    /// 100% consistent with `self.tracks`.
    pub fn rebuild_derived_tables(&mut self) {
        let used_dbids: HashSet<u64> = self.tracks.iter().map(|t| t.dbid).collect();
        let mut rng = rand::thread_rng();

        // 1. Index existing albums by (album_lower, album_artist_lower) to preserve IDs
        let mut existing_albums: HashMap<(String, String), AlbumItem> = HashMap::new();
        for alb in self.albums.drain(..) {
            let key = (
                alb.album.trim().to_lowercase(),
                alb.album_artist.trim().to_lowercase(),
            );
            existing_albums.entry(key).or_insert(alb);
        }

        // 2. Index existing artist_ids by lowercase effective artist
        let mut artist_ids: HashMap<String, u32> = HashMap::new();
        for t in &self.tracks {
            let art_key = t.effective_album_artist().trim().to_lowercase();
            if t.artist_id != 0 {
                artist_ids.entry(art_key).or_insert(t.artist_id);
            }
        }

        // 3. Rebuild AlbumItem list and assign album_id & artist_id on every track
        let mut album_order: Vec<(String, String)> = Vec::new();
        let mut new_albums_map: HashMap<(String, String), AlbumItem> = HashMap::new();

        for i in 0..self.tracks.len() {
            let album_str = self.tracks[i].album.trim().to_owned();
            let artist_str = self.tracks[i].effective_album_artist().trim().to_owned();
            let album_group_artist = if album_str.is_empty()
                || (self.tracks[i].compilation && self.tracks[i].album_artist.trim().is_empty())
            {
                String::new()
            } else {
                artist_str.clone()
            };
            let sort_artist_str = if album_str.is_empty() {
                String::new()
            } else if !self.tracks[i].sort_album_artist.trim().is_empty() {
                self.tracks[i].sort_album_artist.trim().to_owned()
            } else if !self.tracks[i].sort_artist.trim().is_empty() {
                self.tracks[i].sort_artist.trim().to_owned()
            } else {
                album_group_artist.clone()
            };

            let art_key = artist_str.to_lowercase();
            let artist_id = if let Some(&id) = artist_ids.get(&art_key) {
                id
            } else {
                let id = self.alloc_catalog_id();
                artist_ids.insert(art_key, id);
                id
            };
            self.tracks[i].artist_id = artist_id;

            let alb_key = (album_str.to_lowercase(), album_group_artist.to_lowercase());
            if !new_albums_map.contains_key(&alb_key) {
                album_order.push(alb_key.clone());
                let item = if let Some(mut prev) = existing_albums.remove(&alb_key) {
                    prev.artwork_track_dbid = 0; // refreshed below from live tracks
                    prev
                } else {
                    let album_id = self.alloc_catalog_id();
                    let mut sql_id = rng.gen::<u64>() | 1;
                    while used_dbids.contains(&sql_id) {
                        sql_id = rng.gen::<u64>() | 1;
                    }
                    let mut raw = vec![0u8; DEFAULT_MHIA_HEADER_LEN];
                    raw[0x00..0x04].copy_from_slice(b"mhia");
                    raw[0x04..0x08]
                        .copy_from_slice(&(DEFAULT_MHIA_HEADER_LEN as u32).to_le_bytes());
                    AlbumItem {
                        album_id,
                        sql_id,
                        unk_0x1c: 2,
                        artwork_track_dbid: 0,
                        album: album_str,
                        album_artist: album_group_artist,
                        sort_album_artist: sort_artist_str,
                        raw_header: raw,
                        extra_mhods: Vec::new(),
                    }
                };
                new_albums_map.insert(alb_key.clone(), item);
            }

            let entry = new_albums_map.get_mut(&alb_key).expect("just inserted");
            if entry.artwork_track_dbid == 0 && self.tracks[i].has_artwork {
                entry.artwork_track_dbid = self.tracks[i].dbid;
            }
            self.tracks[i].album_id = entry.album_id;
        }

        self.albums = album_order
            .into_iter()
            .filter_map(|k| new_albums_map.remove(&k))
            .collect();

        // 4. Prune deleted tracks from user playlists & renumber positions 1..=N
        let valid_track_dbids: HashMap<u32, u64> =
            self.tracks.iter().map(|t| (t.track_id, t.dbid)).collect();

        for pl in &mut self.playlists {
            if !pl.is_master {
                pl.items.retain(|it| {
                    it.podcast_group_flag != 0 || valid_track_dbids.contains_key(&it.track_id)
                });
                for (idx, it) in pl.items.iter_mut().enumerate() {
                    let pos_1 = (idx + 1) as u32;
                    it.position = pos_1;
                    it.order_index = pos_1;
                    if let Some(&dbid) = valid_track_dbids.get(&it.track_id) {
                        it.track_dbid = dbid;
                    }
                }
            }
        }

        // 5. Synchronize Master Playlist items + rebuild mhod 52/53 indices
        if self.playlists.first().is_none_or(|p| !p.is_master) {
            let pid = rng.gen::<u64>() | 1;
            self.playlists
                .insert(0, Playlist::new("iPod", true, pid, 0));
        }

        let existing_master_items: HashMap<u32, PlaylistItem> = self.playlists[0]
            .items
            .drain(..)
            .map(|it| (it.track_id, it))
            .collect();

        let mut master_items = Vec::with_capacity(self.tracks.len());
        let mut next_id = self.next_catalog_id;
        for (idx, t) in self.tracks.iter().enumerate() {
            let pos_1 = (idx + 1) as u32;
            let item = if let Some(mut prev) = existing_master_items.get(&t.track_id).cloned() {
                prev.position = pos_1;
                prev.order_index = pos_1;
                prev.track_dbid = t.dbid;
                prev
            } else {
                let gid = next_id;
                next_id = next_id.saturating_add(1);
                PlaylistItem::new(t.track_id, t.dbid, gid, pos_1, rng.gen::<u64>() | 1)
            };
            master_items.push(item);
        }
        self.next_catalog_id = next_id;

        let index_mhods = build_master_library_index_mhods(&self.tracks);
        let master = &mut self.playlists[0];
        master.items = master_items;
        master
            .non_title_mhods
            .retain(|m| m.mhod_type != 52 && m.mhod_type != 53);
        master.non_title_mhods.extend(index_mhods);

        let master_clone = master.clone();
        if self.podcast_playlists.first().is_some_and(|p| p.is_master) {
            self.podcast_playlists[0] = master_clone;
        } else {
            self.podcast_playlists.insert(0, master_clone);
        }

        if !self.section_order.contains(&4) {
            self.section_order.insert(0, 4);
        }
        if !self.section_order.contains(&1) {
            self.section_order.push(1);
        }
        if !self.section_order.contains(&3) {
            self.section_order.push(3);
        }
        if !self.section_order.contains(&2) {
            self.section_order.push(2);
        }

        self.dirty_library_indices = false;
    }
}

/// Serializes `db` into a complete, signed `iTunesDB` byte vector.
///
/// If `db.dirty_library_indices` is set, derived album/artist/master-index tables are rebuilt
/// automatically prior to serialization. If `scheme` is `Hash58` without an explicit
/// `companion_hash72` and `db.embedded_hash72` was captured on load, dual-signing (`hash72`
/// followed by `hash58`) is applied automatically to maintain 100% desktop iTunes parity.
pub fn serialize_itunesdb(
    db: &mut ItdbDatabase,
    scheme: &HashingScheme,
) -> Result<Vec<u8>, ItdbError> {
    if db.dirty_library_indices {
        db.rebuild_derived_tables();
    }

    let hdr_len = db.header.raw_header.len().max(DEFAULT_MHBD_HEADER_LEN);
    let mut mhbd = db.header.raw_header.clone();
    mhbd.resize(hdr_len, 0);
    mhbd[0x00..0x04].copy_from_slice(b"mhbd");
    mhbd[0x04..0x08].copy_from_slice(&(hdr_len as u32).to_le_bytes());
    mhbd[0x10..0x14].copy_from_slice(&db.header.version.to_le_bytes());
    mhbd[0x18..0x20].copy_from_slice(&db.header.db_id.to_le_bytes());
    mhbd[0x20..0x22].copy_from_slice(&db.header.platform.to_le_bytes());
    mhbd[0x22..0x24].copy_from_slice(&db.header.unk_0x22.to_le_bytes());
    mhbd[0x24..0x2C].copy_from_slice(&db.header.library_id.to_le_bytes());
    mhbd[0x30..0x32].copy_from_slice(&db.header.hashing_scheme.to_le_bytes());
    mhbd[0x46..0x48].copy_from_slice(&db.header.db_language);
    mhbd[0x48..0x50].copy_from_slice(&db.header.pid.to_le_bytes());
    mhbd[0x6C..0x70].copy_from_slice(&db.header.tz_offset.to_le_bytes());
    mhbd[0x70..0x72].copy_from_slice(&db.header.hash72_type.to_le_bytes());
    mhbd[0xA0..0xA2].copy_from_slice(&db.header.audio_language.to_le_bytes());
    mhbd[0xA2..0xA4].copy_from_slice(&db.header.subtitle_language.to_le_bytes());

    let mut sections_bytes = Vec::new();
    let mut emitted_sections = 0u32;
    let mut extra_idx_by_type: HashMap<u32, usize> = HashMap::new();

    let order = if db.section_order.is_empty() {
        vec![4, 1, 3, 2]
    } else {
        db.section_order.clone()
    };

    for sec_type in order {
        match sec_type {
            1 => {
                write_mhsd_wrapped(&mut sections_bytes, 1, |out| {
                    write_mhlt(out, &db.tracks, db.header.library_id);
                });
                emitted_sections += 1;
            }
            2 => {
                write_mhsd_wrapped(&mut sections_bytes, 2, |out| {
                    write_mhlp(out, &db.playlists);
                });
                emitted_sections += 1;
            }
            3 => {
                write_mhsd_wrapped(&mut sections_bytes, 3, |out| {
                    write_mhlp(out, &db.podcast_playlists);
                });
                emitted_sections += 1;
            }
            4 => {
                write_mhsd_wrapped(&mut sections_bytes, 4, |out| {
                    write_mhla(out, &db.albums);
                });
                emitted_sections += 1;
            }
            5 if !db.category_playlists.is_empty() => {
                write_mhsd_wrapped(&mut sections_bytes, 5, |out| {
                    write_mhlp(out, &db.category_playlists);
                });
                emitted_sections += 1;
            }
            8 if !db.artists.is_empty() => {
                write_mhsd_wrapped(&mut sections_bytes, 8, |out| {
                    write_mhli_artists(out, &db.artists);
                });
                emitted_sections += 1;
            }
            other => {
                let seen = extra_idx_by_type.entry(other).or_insert(0);
                if let Some(raw_sec) = db
                    .extra_sections
                    .iter()
                    .filter(|s| s.section_type == other)
                    .nth(*seen)
                {
                    sections_bytes.extend_from_slice(&raw_sec.raw_bytes);
                    *seen += 1;
                    emitted_sections += 1;
                }
            }
        }
    }

    let total_len = (mhbd.len() + sections_bytes.len()) as u32;
    mhbd[0x08..0x0C].copy_from_slice(&total_len.to_le_bytes());
    mhbd[0x14..0x18].copy_from_slice(&emitted_sections.to_le_bytes());

    let mut out = Vec::with_capacity(total_len as usize);
    out.extend_from_slice(&mhbd);
    out.extend_from_slice(&sections_bytes);

    let effective_scheme = match scheme {
        HashingScheme::Hash58 {
            guid,
            companion_hash72: None,
        } if db.embedded_hash72.is_some() => HashingScheme::Hash58 {
            guid: *guid,
            companion_hash72: db.embedded_hash72,
        },
        other => *other,
    };

    sign_itunesdb(&mut out, &effective_scheme)?;
    Ok(out)
}

fn write_mhsd_wrapped<F: FnOnce(&mut Vec<u8>)>(out: &mut Vec<u8>, section_type: u32, writer: F) {
    let start = out.len();
    out.extend_from_slice(b"mhsd");
    out.extend_from_slice(&96u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // patched below
    out.extend_from_slice(&section_type.to_le_bytes());
    out.resize(start + 96, 0);

    writer(out);

    let total = (out.len() - start) as u32;
    out[start + 0x08..start + 0x0C].copy_from_slice(&total.to_le_bytes());
}

fn write_string_mhod(out: &mut Vec<u8>, mhod_type: u32, text: &str) {
    let utf16_bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let total_len = (40 + utf16_bytes.len()) as u32;

    out.extend_from_slice(b"mhod");
    out.extend_from_slice(&24u32.to_le_bytes());
    out.extend_from_slice(&total_len.to_le_bytes());
    out.extend_from_slice(&mhod_type.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes()); // position_or_enc = 1 (UTF-16LE)
    out.extend_from_slice(&(utf16_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes()); // unk3 = 1
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&utf16_bytes);
}

fn write_opt_string_mhod(out: &mut Vec<u8>, num_mhods: &mut u32, mhod_type: u32, text: &str) {
    if !text.is_empty() {
        write_string_mhod(out, mhod_type, text);
        *num_mhods += 1;
    }
}

fn default_filetype_desc(filetype: u32) -> &'static str {
    match filetype {
        FILETYPE_MP3 => "MPEG audio file",
        FILETYPE_WAV => "WAV audio file",
        FILETYPE_AIFF => "AIFF audio file",
        FILETYPE_M4A => "AAC audio file",
        _ => "Audio file",
    }
}

fn write_mhlt(out: &mut Vec<u8>, tracks: &[TrackRecord], library_id: u64) {
    let mhlt_start = out.len();
    out.extend_from_slice(b"mhlt");
    out.extend_from_slice(&92u32.to_le_bytes());
    out.extend_from_slice(&(tracks.len() as u32).to_le_bytes());
    out.resize(mhlt_start + 92, 0);

    for t in tracks {
        let mhit_start = out.len();
        let hdr_len = t.raw_header.len().max(DEFAULT_MHIT_HEADER_LEN);
        out.extend_from_slice(&t.raw_header);
        out.resize(mhit_start + hdr_len, 0);

        let hdr = &mut out[mhit_start..mhit_start + hdr_len];
        hdr[0x00..0x04].copy_from_slice(b"mhit");
        hdr[0x04..0x08].copy_from_slice(&(hdr_len as u32).to_le_bytes());
        hdr[0x10..0x14].copy_from_slice(&t.track_id.to_le_bytes());
        hdr[0x14..0x18].copy_from_slice(&t.visible.to_le_bytes());
        hdr[0x18..0x1C].copy_from_slice(&t.filetype.to_le_bytes());
        hdr[0x1C] = t.type1;
        hdr[0x1D] = t.type2;
        hdr[0x1E] = u8::from(t.compilation);
        hdr[0x1F] = t.rating;
        hdr[0x20..0x24].copy_from_slice(&t.time_modified.to_le_bytes());
        hdr[0x24..0x28].copy_from_slice(&t.size.to_le_bytes());
        hdr[0x28..0x2C].copy_from_slice(&t.length_ms.to_le_bytes());
        hdr[0x2C..0x30].copy_from_slice(&t.track_nr.to_le_bytes());
        hdr[0x30..0x34].copy_from_slice(&t.tracks_total.to_le_bytes());
        hdr[0x34..0x38].copy_from_slice(&t.year.to_le_bytes());
        hdr[0x38..0x3C].copy_from_slice(&t.bitrate.to_le_bytes());
        hdr[0x3C..0x40].copy_from_slice(&(t.sample_rate_hz << 16).to_le_bytes());
        hdr[0x40..0x44].copy_from_slice(&t.volume.to_le_bytes());
        hdr[0x44..0x48].copy_from_slice(&t.start_time.to_le_bytes());
        hdr[0x48..0x4C].copy_from_slice(&t.stop_time.to_le_bytes());
        hdr[0x4C..0x50].copy_from_slice(&t.soundcheck.to_le_bytes());
        hdr[0x50..0x54].copy_from_slice(&t.play_count.to_le_bytes());
        hdr[0x54..0x58].copy_from_slice(&t.play_count2.to_le_bytes());
        hdr[0x58..0x5C].copy_from_slice(&t.last_played.to_le_bytes());
        hdr[0x5C..0x60].copy_from_slice(&t.disc_nr.to_le_bytes());
        hdr[0x60..0x64].copy_from_slice(&t.discs_total.to_le_bytes());
        hdr[0x68..0x6C].copy_from_slice(&t.time_added.to_le_bytes());
        hdr[0x6C..0x70].copy_from_slice(&t.bookmark_time.to_le_bytes());
        hdr[0x70..0x78].copy_from_slice(&t.dbid.to_le_bytes());
        hdr[0x78] = t.checked;
        hdr[0x7A..0x7C].copy_from_slice(&t.bpm.to_le_bytes());
        hdr[0x7C..0x7E].copy_from_slice(&t.artwork_count.to_le_bytes());
        hdr[0x80..0x84].copy_from_slice(&t.artwork_size.to_le_bytes());
        let sr_float = (t.sample_rate_hz.max(1) as f32).to_le_bytes();
        hdr[0x88..0x8C].copy_from_slice(&sr_float);
        hdr[0x8C..0x90].copy_from_slice(&t.time_released.to_le_bytes());
        hdr[0x92..0x94].copy_from_slice(&t.explicit_flag.to_le_bytes());
        hdr[0x98..0x9C].copy_from_slice(&t.skip_count.to_le_bytes());
        hdr[0x9C..0xA0].copy_from_slice(&t.recent_skip_count.to_le_bytes());
        hdr[0xA0..0xA4].copy_from_slice(&t.last_skipped.to_le_bytes());
        hdr[0xA4] = if t.has_artwork { 1 } else { 2 };
        hdr[0xA5] = u8::from(t.skip_when_shuffling);
        hdr[0xA6] = u8::from(t.remember_playback_position);
        hdr[0xA7] = t.podcast_flag;
        hdr[0xA8..0xB0].copy_from_slice(&t.dbid.to_le_bytes());
        hdr[0xB0] = u8::from(t.lyrics_flag);
        hdr[0xB1] = u8::from(t.movie_flag);
        hdr[0xB2] = if t.play_count > 0 { 1 } else { 2 };
        hdr[0xB8..0xBC].copy_from_slice(&t.pregap.to_le_bytes());
        hdr[0xBC..0xC4].copy_from_slice(&t.sample_count.to_le_bytes());
        hdr[0xC8..0xCC].copy_from_slice(&t.postgap.to_le_bytes());
        hdr[0xD0..0xD4].copy_from_slice(&t.mediatype.to_le_bytes());
        hdr[0xF8..0xFC].copy_from_slice(&t.gapless_data.to_le_bytes());
        hdr[0x100..0x102].copy_from_slice(&t.gapless_track_flag.to_le_bytes());
        hdr[0x102..0x104].copy_from_slice(&t.gapless_album_flag.to_le_bytes());
        hdr[0x120..0x124].copy_from_slice(&t.album_id.to_le_bytes());
        hdr[0x124..0x12C].copy_from_slice(&library_id.to_le_bytes());
        hdr[0x12C..0x130].copy_from_slice(&t.size.to_le_bytes());
        hdr[0x160..0x164].copy_from_slice(&t.mhii_link.to_le_bytes());
        hdr[0x195] = (t.explicit_flag >> 8) as u8;
        hdr[0x1E0..0x1E4].copy_from_slice(&t.artist_id.to_le_bytes());
        hdr[0x1F4..0x1F8].copy_from_slice(&t.composer_id.to_le_bytes());

        let mut num_mhods = 0u32;

        // Canonical Apple iTunes ordering: [1, 4, 22, 3, 5, 6, 2, ...]
        write_opt_string_mhod(out, &mut num_mhods, 1, &t.title);
        write_opt_string_mhod(out, &mut num_mhods, 4, &t.artist);
        write_opt_string_mhod(out, &mut num_mhods, 22, &t.album_artist);
        write_opt_string_mhod(out, &mut num_mhods, 3, &t.album);
        write_opt_string_mhod(out, &mut num_mhods, 5, &t.genre);

        let desc = if t.filetype_desc.is_empty() {
            default_filetype_desc(t.filetype)
        } else {
            &t.filetype_desc
        };
        write_string_mhod(out, 6, desc);
        num_mhods += 1;

        write_string_mhod(out, 2, &t.location);
        num_mhods += 1;

        write_opt_string_mhod(out, &mut num_mhods, 8, &t.comment);
        write_opt_string_mhod(out, &mut num_mhods, 12, &t.composer);
        write_opt_string_mhod(out, &mut num_mhods, 13, &t.grouping);
        write_opt_string_mhod(out, &mut num_mhods, 14, &t.description);
        write_opt_string_mhod(out, &mut num_mhods, 23, &t.sort_artist);
        write_opt_string_mhod(out, &mut num_mhods, 27, &t.sort_title);
        write_opt_string_mhod(out, &mut num_mhods, 28, &t.sort_album);
        write_opt_string_mhod(out, &mut num_mhods, 29, &t.sort_album_artist);
        write_opt_string_mhod(out, &mut num_mhods, 30, &t.sort_composer);
        write_opt_string_mhod(out, &mut num_mhods, 39, &t.copyright);

        for extra in &t.extra_mhods {
            out.extend_from_slice(&extra.raw_bytes);
            num_mhods += 1;
        }

        let mhit_tot = (out.len() - mhit_start) as u32;
        out[mhit_start + 0x08..mhit_start + 0x0C].copy_from_slice(&mhit_tot.to_le_bytes());
        out[mhit_start + 0x0C..mhit_start + 0x10].copy_from_slice(&num_mhods.to_le_bytes());
    }
}

fn write_mhla(out: &mut Vec<u8>, albums: &[AlbumItem]) {
    let start = out.len();
    out.extend_from_slice(b"mhla");
    out.extend_from_slice(&92u32.to_le_bytes());
    out.extend_from_slice(&(albums.len() as u32).to_le_bytes());
    out.resize(start + 92, 0);

    for alb in albums {
        let istart = out.len();
        let hdr_len = alb.raw_header.len().max(DEFAULT_MHIA_HEADER_LEN);
        out.extend_from_slice(&alb.raw_header);
        out.resize(istart + hdr_len, 0);

        let hdr = &mut out[istart..istart + hdr_len];
        hdr[0x00..0x04].copy_from_slice(b"mhia");
        hdr[0x04..0x08].copy_from_slice(&(hdr_len as u32).to_le_bytes());
        hdr[0x10..0x14].copy_from_slice(&alb.album_id.to_le_bytes());
        hdr[0x14..0x1C].copy_from_slice(&alb.sql_id.to_le_bytes());
        hdr[0x1C..0x20].copy_from_slice(&alb.unk_0x1c.to_le_bytes());
        hdr[0x20..0x28].copy_from_slice(&alb.artwork_track_dbid.to_le_bytes());

        let mut num_mhods = 0u32;
        if !alb.album.is_empty() {
            write_string_mhod(out, 200, &alb.album);
            num_mhods += 1;
        }
        if !alb.album_artist.is_empty() {
            write_string_mhod(out, 201, &alb.album_artist);
            num_mhods += 1;
        }
        if !alb.sort_album_artist.is_empty() {
            write_string_mhod(out, 202, &alb.sort_album_artist);
            num_mhods += 1;
        }
        for extra in &alb.extra_mhods {
            out.extend_from_slice(&extra.raw_bytes);
            num_mhods += 1;
        }

        let itot = (out.len() - istart) as u32;
        out[istart + 0x08..istart + 0x0C].copy_from_slice(&itot.to_le_bytes());
        out[istart + 0x0C..istart + 0x10].copy_from_slice(&num_mhods.to_le_bytes());
    }
}

fn write_mhli_artists(out: &mut Vec<u8>, artists: &[crate::chunks::ArtistItem]) {
    let start = out.len();
    out.extend_from_slice(b"mhli");
    out.extend_from_slice(&92u32.to_le_bytes());
    out.extend_from_slice(&(artists.len() as u32).to_le_bytes());
    out.resize(start + 92, 0);

    for art in artists {
        let istart = out.len();
        let hdr_len = art.raw_header.len().max(80);
        out.extend_from_slice(&art.raw_header);
        out.resize(istart + hdr_len, 0);

        let hdr = &mut out[istart..istart + hdr_len];
        hdr[0x00..0x04].copy_from_slice(b"mhii");
        hdr[0x04..0x08].copy_from_slice(&(hdr_len as u32).to_le_bytes());
        hdr[0x10..0x14].copy_from_slice(&art.artist_id.to_le_bytes());
        hdr[0x14..0x1C].copy_from_slice(&art.sql_id.to_le_bytes());
        hdr[0x1C..0x20].copy_from_slice(&art.unk_0x1c.to_le_bytes());

        let mut num_mhods = 0u32;
        if !art.artist.is_empty() {
            write_string_mhod(out, 300, &art.artist);
            num_mhods += 1;
        }
        for extra in &art.extra_mhods {
            out.extend_from_slice(&extra.raw_bytes);
            num_mhods += 1;
        }

        let itot = (out.len() - istart) as u32;
        out[istart + 0x08..istart + 0x0C].copy_from_slice(&itot.to_le_bytes());
        out[istart + 0x0C..istart + 0x10].copy_from_slice(&num_mhods.to_le_bytes());
    }
}

fn write_mhlp(out: &mut Vec<u8>, playlists: &[Playlist]) {
    let start = out.len();
    out.extend_from_slice(b"mhlp");
    out.extend_from_slice(&92u32.to_le_bytes());
    out.extend_from_slice(&(playlists.len() as u32).to_le_bytes());
    out.resize(start + 92, 0);

    for pl in playlists {
        let pstart = out.len();
        let hdr_len = pl.raw_header.len().max(DEFAULT_MHYP_HEADER_LEN);
        out.extend_from_slice(&pl.raw_header);
        out.resize(pstart + hdr_len, 0);

        let hdr = &mut out[pstart..pstart + hdr_len];
        hdr[0x00..0x04].copy_from_slice(b"mhyp");
        hdr[0x04..0x08].copy_from_slice(&(hdr_len as u32).to_le_bytes());
        hdr[0x10..0x14].copy_from_slice(&(pl.items.len() as u32).to_le_bytes());
        hdr[0x14] = u8::from(pl.is_master);
        hdr[0x18..0x1C].copy_from_slice(&pl.timestamp.to_le_bytes());
        hdr[0x1C..0x24].copy_from_slice(&pl.persistent_id.to_le_bytes());
        hdr[0x28..0x2A].copy_from_slice(&1u16.to_le_bytes()); // string_mhod_count = 1
        hdr[0x2A] = pl.podcast_flag;
        hdr[0x2B] = pl.group_flag;
        hdr[0x2C..0x30].copy_from_slice(&pl.sort_order.to_le_bytes());
        hdr[0x50..0x52].copy_from_slice(&pl.distinguished_kind.to_le_bytes());
        hdr[0x52..0x54].copy_from_slice(&pl.distinguished_kind.to_le_bytes());
        hdr[0x58..0x5C].copy_from_slice(&pl.timestamp_modified.to_le_bytes());

        let mut mhod_count = 0u32;
        write_string_mhod(out, 1, &pl.name);
        mhod_count += 1;

        for m in &pl.non_title_mhods {
            out.extend_from_slice(&m.raw_bytes);
            mhod_count += 1;
        }

        for it in &pl.items {
            let istart = out.len();
            let ihdr_len = it.raw_header.len().max(DEFAULT_MHIP_HEADER_LEN);
            out.extend_from_slice(&it.raw_header);
            out.resize(istart + ihdr_len, 0);

            let ihdr = &mut out[istart..istart + ihdr_len];
            ihdr[0x00..0x04].copy_from_slice(b"mhip");
            ihdr[0x04..0x08].copy_from_slice(&(ihdr_len as u32).to_le_bytes());
            ihdr[0x10..0x12].copy_from_slice(&it.podcast_group_flag.to_le_bytes());
            ihdr[0x14..0x18].copy_from_slice(&it.group_id.to_le_bytes());
            ihdr[0x18..0x1C].copy_from_slice(&it.track_id.to_le_bytes());
            ihdr[0x1C..0x20].copy_from_slice(&it.timestamp.to_le_bytes());
            ihdr[0x20..0x24].copy_from_slice(&it.podcast_group_ref.to_le_bytes());
            ihdr[0x24..0x28].copy_from_slice(&it.order_index.to_le_bytes());
            ihdr[0x2C..0x34].copy_from_slice(&it.track_dbid.to_le_bytes());
            ihdr[0x3C..0x44].copy_from_slice(&it.item_pid.to_le_bytes());

            let mut imhod_count = 0u32;
            if let Some(ref gtitle) = it.group_title {
                write_string_mhod(out, 1, gtitle);
                imhod_count += 1;
            }

            // Write 44-byte mhod type=100 playlist position indicator
            let m100_start = out.len();
            out.extend_from_slice(b"mhod");
            out.extend_from_slice(&24u32.to_le_bytes());
            out.extend_from_slice(&44u32.to_le_bytes());
            out.extend_from_slice(&100u32.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&it.position.to_le_bytes());
            out.resize(m100_start + 44, 0);
            imhod_count += 1;

            for extra in &it.extra_mhods {
                out.extend_from_slice(&extra.raw_bytes);
                imhod_count += 1;
            }

            let itot = (out.len() - istart) as u32;
            out[istart + 0x08..istart + 0x0C].copy_from_slice(&itot.to_le_bytes());
            out[istart + 0x0C..istart + 0x10].copy_from_slice(&imhod_count.to_le_bytes());
        }

        let ptot = (out.len() - pstart) as u32;
        out[pstart + 0x08..pstart + 0x0C].copy_from_slice(&ptot.to_le_bytes());
        out[pstart + 0x0C..pstart + 0x10].copy_from_slice(&mhod_count.to_le_bytes());
    }
}

/// Builds `mhod type=52` (Sorted Library Index) and `mhod type=53` (Alphabetical Jump Table)
/// chunks for the Master Library playlist so iPod Classic menus scroll and jump in O(1).
fn build_master_library_index_mhods(tracks: &[TrackRecord]) -> Vec<RawMhod> {
    const SPECS: &[(u32, bool)] = &[
        (0x03, true),  // Title
        (0x05, true),  // Artist (Album -> Disc -> Track)
        (0x04, true),  // Album (Disc -> Track -> Title)
        (0x07, true),  // Genre
        (0x12, true),  // Composer
        (0x23, false), // Album Artist
        (0x24, false), // Artist (ignoring sort_artist)
    ];

    let mut result = Vec::with_capacity(12);
    let count = tracks.len();

    for &(sort_type, emit_jump_table) in SPECS {
        let mut indices: Vec<u32> = (0..count as u32).collect();
        indices
            .sort_by(|&a, &b| compare_tracks_for_sort(tracks, a as usize, b as usize, sort_type));

        let m52_tot = 72 + 4 * count;
        let mut m52 = Vec::with_capacity(m52_tot);
        m52.extend_from_slice(b"mhod");
        m52.extend_from_slice(&24u32.to_le_bytes());
        m52.extend_from_slice(&(m52_tot as u32).to_le_bytes());
        m52.extend_from_slice(&52u32.to_le_bytes());
        m52.extend_from_slice(&0u32.to_le_bytes());
        m52.extend_from_slice(&0u32.to_le_bytes());
        m52.extend_from_slice(&sort_type.to_le_bytes());
        m52.extend_from_slice(&(count as u32).to_le_bytes());
        m52.resize(72, 0);
        for &idx in &indices {
            m52.extend_from_slice(&idx.to_le_bytes());
        }
        result.push(RawMhod {
            mhod_type: 52,
            raw_bytes: m52,
        });

        if emit_jump_table {
            let mut entries: Vec<(u32, u32, u32)> = Vec::new();
            for (pos, &track_idx) in indices.iter().enumerate() {
                let key = primary_sort_str(&tracks[track_idx as usize], sort_type);
                let letter = jump_letter_code(key);
                if let Some(last) = entries.last_mut() {
                    if last.0 == letter {
                        last.2 += 1;
                        continue;
                    }
                }
                entries.push((letter, pos as u32, 1));
            }

            let m53_tot = 40 + 12 * entries.len();
            let mut m53 = Vec::with_capacity(m53_tot);
            m53.extend_from_slice(b"mhod");
            m53.extend_from_slice(&24u32.to_le_bytes());
            m53.extend_from_slice(&(m53_tot as u32).to_le_bytes());
            m53.extend_from_slice(&53u32.to_le_bytes());
            m53.extend_from_slice(&0u32.to_le_bytes());
            m53.extend_from_slice(&0u32.to_le_bytes());
            m53.extend_from_slice(&sort_type.to_le_bytes());
            m53.extend_from_slice(&(entries.len() as u32).to_le_bytes());
            m53.resize(40, 0);
            for (letter, start, run_len) in entries {
                m53.extend_from_slice(&letter.to_le_bytes());
                m53.extend_from_slice(&start.to_le_bytes());
                m53.extend_from_slice(&run_len.to_le_bytes());
            }
            result.push(RawMhod {
                mhod_type: 53,
                raw_bytes: m53,
            });
        }
    }

    result
}

fn primary_sort_str(t: &TrackRecord, sort_type: u32) -> &str {
    match sort_type {
        0x03 => {
            if t.sort_title.is_empty() {
                &t.title
            } else {
                &t.sort_title
            }
        }
        0x04 => {
            if t.sort_album.is_empty() {
                &t.album
            } else {
                &t.sort_album
            }
        }
        0x05 => {
            if !t.sort_artist.is_empty() {
                &t.sort_artist
            } else {
                &t.artist
            }
        }
        0x07 => &t.genre,
        0x12 => {
            if t.sort_composer.is_empty() {
                &t.composer
            } else {
                &t.sort_composer
            }
        }
        0x23 => {
            if !t.sort_album_artist.is_empty() {
                &t.sort_album_artist
            } else {
                t.effective_album_artist()
            }
        }
        0x24 => &t.artist,
        _ => &t.title,
    }
}

fn ascii_fold_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut ia = a.trim().chars().map(|c| c.to_ascii_uppercase());
    let mut ib = b.trim().chars().map(|c| c.to_ascii_uppercase());
    loop {
        match (ia.next(), ib.next()) {
            (Some(ca), Some(cb)) => match ca.cmp(&cb) {
                std::cmp::Ordering::Equal => continue,
                non_eq => return non_eq,
            },
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (None, None) => return std::cmp::Ordering::Equal,
        }
    }
}

fn compare_tracks_for_sort(
    tracks: &[TrackRecord],
    a: usize,
    b: usize,
    sort_type: u32,
) -> std::cmp::Ordering {
    let ta = &tracks[a];
    let tb = &tracks[b];
    let primary = ascii_fold_cmp(
        primary_sort_str(ta, sort_type),
        primary_sort_str(tb, sort_type),
    );
    if primary != std::cmp::Ordering::Equal {
        return primary;
    }
    ascii_fold_cmp(&ta.album, &tb.album)
        .then_with(|| ta.disc_nr.cmp(&tb.disc_nr))
        .then_with(|| ta.track_nr.cmp(&tb.track_nr))
        .then_with(|| ascii_fold_cmp(&ta.title, &tb.title))
}

fn jump_letter_code(s: &str) -> u32 {
    match s.trim().chars().next() {
        None => 0,
        Some(ch) => {
            let up = ch.to_ascii_uppercase();
            if up.is_ascii_alphabetic() {
                u32::from(up as u8)
            } else {
                u32::from(b'0')
            }
        }
    }
}
