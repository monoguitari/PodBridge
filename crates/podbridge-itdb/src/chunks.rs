// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use podbridge_crypto::Hash72Info;
use serde::{Deserialize, Serialize};

/// Seconds between Classic Mac HFS+ epoch (`1904-01-01T00:00:00Z`) and Unix epoch (`1970-01-01T00:00:00Z`).
pub const MAC_EPOCH_OFFSET_SECS: u64 = 2_082_844_800;

/// Converts a Unix timestamp (seconds since 1970) into a 32-bit Mac HFS+ timestamp.
#[must_use]
pub fn unix_to_mac_timestamp(unix_secs: u64) -> u32 {
    if unix_secs == 0 {
        0
    } else {
        unix_secs.saturating_add(MAC_EPOCH_OFFSET_SECS) as u32
    }
}

/// Converts a 32-bit Mac HFS+ timestamp into Unix seconds (returns `0` if `mac_secs == 0`).
#[must_use]
pub fn mac_to_unix_timestamp(mac_secs: u32) -> u64 {
    if mac_secs == 0 {
        0
    } else {
        u64::from(mac_secs).saturating_sub(MAC_EPOCH_OFFSET_SECS)
    }
}

/// Canonical 4-character LE `filetype` codes stored at `mhit + 0x18`.
pub const FILETYPE_MP3: u32 = 0x4D50_3320; // 'MP3 '
pub const FILETYPE_M4A: u32 = 0x4D34_4120; // 'M4A '
pub const FILETYPE_WAV: u32 = 0x5741_5620; // 'WAV '
pub const FILETYPE_AIFF: u32 = 0x4149_4646; // 'AIFF'

/// Standard `mhbd` header size for modern `iTunesDB` (`version = 0x73`).
pub const DEFAULT_MHBD_HEADER_LEN: usize = 244;
/// Standard `mhit` header size for modern `iTunesDB` (`version = 0x73`).
pub const DEFAULT_MHIT_HEADER_LEN: usize = 624;
/// Standard `mhyp` header size (`184` bytes).
pub const DEFAULT_MHYP_HEADER_LEN: usize = 184;
/// Standard `mhip` header size (`76` bytes).
pub const DEFAULT_MHIP_HEADER_LEN: usize = 76;
/// Standard `mhia` header size (`88` bytes).
pub const DEFAULT_MHIA_HEADER_LEN: usize = 88;

/// Opaque `mhod` chunk preserved verbatim across parse -> write cycles (e.g., Smart Playlist rules
/// `50`/`51`, UI column configs `100`/`102`, Podcast URLs `15`/`16`, or Chapter atoms `17`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawMhod {
    pub mhod_type: u32,
    pub raw_bytes: Vec<u8>,
}

/// Parsed `mhbd` database root header with full raw-byte preservation for unknown fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MhbdHeader {
    pub version: u32,
    pub db_id: u64,
    pub platform: u16,
    pub unk_0x22: u16,
    /// Library ID at `mhbd + 0x24` — MUST be mirrored to every `mhit + 0x124`.
    pub library_id: u64,
    pub hashing_scheme: u16,
    pub db_language: [u8; 2],
    pub pid: u64,
    pub tz_offset: i32,
    pub hash72_type: u16,
    pub audio_language: i16,
    pub subtitle_language: i16,
    /// Full original header bytes (`>= 244` bytes when serialized).
    pub raw_header: Vec<u8>,
}

impl Default for MhbdHeader {
    fn default() -> Self {
        let mut raw = vec![0u8; DEFAULT_MHBD_HEADER_LEN];
        raw[0x00..0x04].copy_from_slice(b"mhbd");
        raw[0x04..0x08].copy_from_slice(&(DEFAULT_MHBD_HEADER_LEN as u32).to_le_bytes());
        raw[0x0C..0x10].copy_from_slice(&1u32.to_le_bytes()); // compressed_flag = 1
        raw[0x10..0x14].copy_from_slice(&0x73u32.to_le_bytes()); // version = 0x73
        raw[0x20..0x22].copy_from_slice(&2u16.to_le_bytes()); // platform = Windows (2)
        raw[0x22..0x24].copy_from_slice(&0x0263u16.to_le_bytes());
        raw[0x46..0x48].copy_from_slice(b"en");
        raw[0x50..0x54].copy_from_slice(&1u32.to_le_bytes());
        raw[0x54..0x58].copy_from_slice(&0x0Fu32.to_le_bytes());
        raw[0xA0..0xA2].copy_from_slice(&(-1i16).to_le_bytes());
        raw[0xA2..0xA4].copy_from_slice(&(-1i16).to_le_bytes());
        raw[0xA4..0xA6].copy_from_slice(&0x0019u16.to_le_bytes());
        raw[0xA6..0xA8].copy_from_slice(&0x000Au16.to_le_bytes());

        Self {
            version: 0x73,
            db_id: 0x0102_0304_0506_0708,
            platform: 2,
            unk_0x22: 0x0263,
            library_id: 0x93BB_7784_4685_3A25,
            hashing_scheme: 1,
            db_language: *b"en",
            pid: 0x3785_E9DD_E6F6_A0FD,
            tz_offset: 0,
            hash72_type: 0,
            audio_language: -1,
            subtitle_language: -1,
            raw_header: raw,
        }
    }
}

/// Parsed `mhit` track record and its associated `mhod` metadata strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackRecord {
    pub track_id: u32,
    pub visible: u32,
    pub filetype: u32,
    pub type1: u8,
    pub type2: u8,
    pub compilation: bool,
    /// Rating `0..=100` (`stars * 20`).
    pub rating: u8,
    pub time_modified: u32,
    pub size: u32,
    pub length_ms: u32,
    pub track_nr: u32,
    pub tracks_total: u32,
    pub year: u32,
    pub bitrate: u32,
    pub sample_rate_hz: u32,
    pub volume: i32,
    pub start_time: u32,
    pub stop_time: u32,
    pub soundcheck: u32,
    pub play_count: u32,
    pub play_count2: u32,
    pub last_played: u32,
    pub disc_nr: u32,
    pub discs_total: u32,
    pub time_added: u32,
    pub bookmark_time: u32,
    pub dbid: u64,
    pub checked: u8,
    pub bpm: u16,
    pub artwork_count: u16,
    pub artwork_size: u32,
    pub time_released: u32,
    pub explicit_flag: u16,
    pub skip_count: u32,
    pub recent_skip_count: u32,
    pub last_skipped: u32,
    /// Maps to `0x01` when `true` and `0x02` when `false` at `mhit + 0xA4`.
    pub has_artwork: bool,
    pub skip_when_shuffling: bool,
    pub remember_playback_position: bool,
    pub podcast_flag: u8,
    pub lyrics_flag: bool,
    pub movie_flag: bool,
    pub pregap: u32,
    pub sample_count: u64,
    pub postgap: u32,
    pub mediatype: u32,
    pub gapless_data: u32,
    pub gapless_track_flag: u16,
    pub gapless_album_flag: u16,
    pub album_id: u32,
    pub mhii_link: u32,
    pub artist_id: u32,
    pub composer_id: u32,

    // Standard UTF-16LE string mhods
    pub title: String,
    /// Colon-delimited iPod path, e.g. `:iPod_Control:Music:F04:ABCD.m4a`.
    pub location: String,
    pub album: String,
    pub artist: String,
    pub genre: String,
    pub filetype_desc: String,
    pub comment: String,
    pub composer: String,
    pub grouping: String,
    pub description: String,
    pub album_artist: String,
    pub sort_artist: String,
    pub sort_title: String,
    pub sort_album: String,
    pub sort_album_artist: String,
    pub sort_composer: String,
    pub copyright: String,

    /// Original `mhit` header bytes for lossless preservation of unknown fields.
    pub raw_header: Vec<u8>,
    /// Unrecognized or non-string `mhod` children preserved verbatim.
    pub extra_mhods: Vec<RawMhod>,
}

impl TrackRecord {
    /// Creates a new `TrackRecord` initialized with clean `version = 0x73` header defaults.
    #[must_use]
    pub fn new(track_id: u32, dbid: u64) -> Self {
        let mut raw = vec![0u8; DEFAULT_MHIT_HEADER_LEN];
        raw[0x00..0x04].copy_from_slice(b"mhit");
        raw[0x04..0x08].copy_from_slice(&(DEFAULT_MHIT_HEADER_LEN as u32).to_le_bytes());
        raw[0x7E..0x80].copy_from_slice(&0xFFFFu16.to_le_bytes());
        raw[0x90..0x92].copy_from_slice(&0x003Cu16.to_le_bytes());
        raw[0xB2] = 0x02; // unplayed mark
        raw[0xCC..0xD0].copy_from_slice(&1u32.to_le_bytes());
        raw[0x100..0x102].copy_from_slice(&1u16.to_le_bytes()); // gapless_track_flag = 1
        raw[0x134..0x13C].copy_from_slice(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00, 0x00]);
        raw[0x148..0x14C].copy_from_slice(&0x0200_0000u32.to_le_bytes());
        raw[0x168..0x16C].copy_from_slice(&0x0000_0020u32.to_le_bytes());

        Self {
            track_id,
            visible: 1,
            filetype: FILETYPE_M4A,
            type1: 0,
            type2: 0,
            compilation: false,
            rating: 0,
            time_modified: 0,
            size: 0,
            length_ms: 0,
            track_nr: 0,
            tracks_total: 0,
            year: 0,
            bitrate: 0,
            sample_rate_hz: 44_100,
            volume: 0,
            start_time: 0,
            stop_time: 0,
            soundcheck: 0,
            play_count: 0,
            play_count2: 0,
            last_played: 0,
            disc_nr: 0,
            discs_total: 0,
            time_added: 0,
            bookmark_time: 0,
            dbid,
            checked: 0,
            bpm: 0,
            artwork_count: 0,
            artwork_size: 0,
            time_released: 0,
            explicit_flag: 0,
            skip_count: 0,
            recent_skip_count: 0,
            last_skipped: 0,
            has_artwork: false,
            skip_when_shuffling: false,
            remember_playback_position: false,
            podcast_flag: 0,
            lyrics_flag: false,
            movie_flag: false,
            pregap: 0,
            sample_count: 0,
            postgap: 0,
            mediatype: 1,
            gapless_data: 0,
            gapless_track_flag: 1,
            gapless_album_flag: 0,
            album_id: 0,
            mhii_link: 0,
            artist_id: 0,
            composer_id: 0,
            title: String::new(),
            location: String::new(),
            album: String::new(),
            artist: String::new(),
            genre: String::new(),
            filetype_desc: String::new(),
            comment: String::new(),
            composer: String::new(),
            grouping: String::new(),
            description: String::new(),
            album_artist: String::new(),
            sort_artist: String::new(),
            sort_title: String::new(),
            sort_album: String::new(),
            sort_album_artist: String::new(),
            sort_composer: String::new(),
            copyright: String::new(),
            raw_header: raw,
            extra_mhods: Vec::new(),
        }
    }

    /// Effective album artist (falls back to `artist` when `album_artist` is empty).
    #[must_use]
    pub fn effective_album_artist(&self) -> &str {
        if self.album_artist.is_empty() {
            &self.artist
        } else {
            &self.album_artist
        }
    }
}

/// Parsed `mhia` album record inside `mhsd type = 4` (`mhla`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumItem {
    pub album_id: u32,
    pub sql_id: u64,
    pub unk_0x1c: u32,
    pub artwork_track_dbid: u64,
    pub album: String,
    pub album_artist: String,
    pub sort_album_artist: String,
    pub raw_header: Vec<u8>,
    pub extra_mhods: Vec<RawMhod>,
}

/// Parsed `mhii` artist record inside optional `mhsd type = 8` (`mhli`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistItem {
    pub artist_id: u32,
    pub sql_id: u64,
    pub unk_0x1c: u32,
    pub artist: String,
    pub raw_header: Vec<u8>,
    pub extra_mhods: Vec<RawMhod>,
}

/// Parsed `mhip` playlist item inside a `Playlist`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistItem {
    pub podcast_group_flag: u16,
    pub group_id: u32,
    pub track_id: u32,
    pub timestamp: u32,
    pub podcast_group_ref: u32,
    pub order_index: u32,
    pub track_dbid: u64,
    pub item_pid: u64,
    /// 1-based position from child `mhod type=100` (`1..=N`).
    pub position: u32,
    pub group_title: Option<String>,
    pub raw_header: Vec<u8>,
    pub extra_mhods: Vec<RawMhod>,
}

impl PlaylistItem {
    #[must_use]
    pub fn new(
        track_id: u32,
        track_dbid: u64,
        group_id: u32,
        position: u32,
        item_pid: u64,
    ) -> Self {
        let mut raw = vec![0u8; DEFAULT_MHIP_HEADER_LEN];
        raw[0x00..0x04].copy_from_slice(b"mhip");
        raw[0x04..0x08].copy_from_slice(&(DEFAULT_MHIP_HEADER_LEN as u32).to_le_bytes());

        Self {
            podcast_group_flag: 0,
            group_id,
            track_id,
            timestamp: 0,
            podcast_group_ref: 0,
            order_index: position,
            track_dbid,
            item_pid,
            position,
            group_title: None,
            raw_header: raw,
            extra_mhods: Vec::new(),
        }
    }
}

/// Parsed `mhyp` playlist (Master Library playlist, user playlist, or smart category playlist).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    pub name: String,
    pub is_master: bool,
    pub timestamp: u32,
    pub timestamp_modified: u32,
    pub persistent_id: u64,
    pub podcast_flag: u8,
    pub group_flag: u8,
    pub sort_order: u32,
    pub distinguished_kind: u16,
    pub items: Vec<PlaylistItem>,
    /// Non-title `mhod`s directly under `mhyp` (e.g. `50`, `51`, `52`, `53`, `100`, `102`).
    pub non_title_mhods: Vec<RawMhod>,
    pub raw_header: Vec<u8>,
}

impl Playlist {
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        is_master: bool,
        persistent_id: u64,
        timestamp: u32,
    ) -> Self {
        let mut raw = vec![0u8; DEFAULT_MHYP_HEADER_LEN];
        raw[0x00..0x04].copy_from_slice(b"mhyp");
        raw[0x04..0x08].copy_from_slice(&(DEFAULT_MHYP_HEADER_LEN as u32).to_le_bytes());
        raw[0x28..0x2A].copy_from_slice(&1u16.to_le_bytes()); // string_mhod_count = 1

        Self {
            name: name.into(),
            is_master,
            timestamp,
            timestamp_modified: timestamp,
            persistent_id,
            podcast_flag: 0,
            group_flag: 0,
            sort_order: if is_master { 5 } else { 1 },
            distinguished_kind: 0,
            items: Vec::new(),
            non_title_mhods: Vec::new(),
            raw_header: raw,
        }
    }
}

/// Unrecognized `mhsd` top-level section preserved verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawMhsdSection {
    pub section_type: u32,
    pub raw_bytes: Vec<u8>,
}

/// Complete in-memory `iTunesDB` representation supporting lossless round-tripping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItdbDatabase {
    pub header: MhbdHeader,
    /// `mhsd type = 4` (`mhla -> mhia`).
    pub albums: Vec<AlbumItem>,
    /// `mhsd type = 1` (`mhlt -> mhit`).
    pub tracks: Vec<TrackRecord>,
    /// `mhsd type = 3` (`mhlp -> mhyp`, Master + Podcast playlists).
    pub podcast_playlists: Vec<Playlist>,
    /// `mhsd type = 2` (`mhlp -> mhyp`, Master + User playlists).
    pub playlists: Vec<Playlist>,
    /// `mhsd type = 5` (`mhlp -> mhyp`, Built-in Category Smart Playlists when present).
    pub category_playlists: Vec<Playlist>,
    /// Optional `mhsd type = 8` (`mhli -> mhii`, non-Classic artist list if present).
    pub artists: Vec<ArtistItem>,
    /// Original top-level `mhsd` ordering (defaults to `[4, 1, 3, 2, 5]` or `[4, 1, 3, 2]`).
    pub section_order: Vec<u32>,
    /// Any additional `mhsd` sections (`6`, `9`, `10`, etc.) preserved verbatim.
    pub extra_sections: Vec<RawMhsdSection>,
    /// `Hash72Info` extracted automatically when opening an iTunes-signed database.
    pub embedded_hash72: Option<Hash72Info>,
    /// Next unique 32-bit catalog ID (`track_id`, `album_id`, `artist_id`, `group_id`).
    pub next_catalog_id: u32,
    /// Whether tracks changed since parse (triggers `mhod 52`/`53` & `mhla` rebuild on write).
    pub dirty_library_indices: bool,
}
