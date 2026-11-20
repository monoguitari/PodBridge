// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use podbridge_device::{ConnectedIpod, IpodDeviceSummary};
use podbridge_itdb::{FILETYPE_AIFF, FILETYPE_M4A, FILETYPE_MP3, FILETYPE_WAV};
use podbridge_pipeline::TransferSettings;
use serde::{Deserialize, Serialize};

/// Persisted user preferences (`<AppData>/PodBridge/settings.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// `"dark"` (default Studio Dark) or `"light"`.
    pub theme: String,
    pub transfer: TransferSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".to_owned(),
            transfer: TransferSettings::default(),
        }
    }
}

/// Device list response returned to the React sidebar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevicesPayload {
    pub devices: Vec<IpodDeviceSummary>,
    pub selected_device_id: Option<String>,
}

/// Lean UI DTO for virtualized 60 fps track rendering.
///
/// Note: 64-bit `dbid` is serialized as a 16-char hex string so JavaScript `Number` never loses
/// precision above `2^53 - 1`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackDto {
    pub track_id: u32,
    pub dbid_hex: String,
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub genre: String,
    pub format_badge: String,
    pub size_bytes: u32,
    pub length_ms: u32,
    pub bitrate_kbps: u32,
    pub sample_rate_hz: u32,
    pub track_nr: u32,
    pub disc_nr: u32,
    pub year: u32,
    pub play_count: u32,
    pub rating: u8,
    pub mhii_link: u32,
}

/// Aggregated album card DTO for the Albums Cover Flow grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumDto {
    pub key: String,
    pub title: String,
    pub artist: String,
    pub year: u32,
    pub track_count: usize,
    pub total_length_ms: u64,
    pub mhii_link: u32,
    pub track_ids: Vec<u32>,
}

/// Aggregated artist summary DTO for the Artists master-detail view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistDto {
    pub name: String,
    pub album_count: usize,
    pub track_count: usize,
    pub mhii_link: u32,
}

/// Playlist DTO for sidebar navigation and playlist editor.
///
/// `persistent_id_hex` is formatted as 16-digit uppercase hex to avoid JS 53-bit truncation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistDto {
    pub persistent_id_hex: String,
    pub name: String,
    pub is_master: bool,
    pub track_ids: Vec<u32>,
}

/// Complete library snapshot returned whenever a device is selected or mutated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySnapshot {
    pub device: IpodDeviceSummary,
    pub tracks: Vec<TrackDto>,
    pub albums: Vec<AlbumDto>,
    pub artists: Vec<ArtistDto>,
    pub playlists: Vec<PlaylistDto>,
}

fn format_badge_for(filetype: u32, filetype_desc: &str) -> &'static str {
    let lower = filetype_desc.to_ascii_lowercase();
    if lower.contains("lossless") {
        return "ALAC";
    }
    match filetype {
        FILETYPE_MP3 => "MP3",
        FILETYPE_M4A => "AAC",
        FILETYPE_WAV => "WAV",
        FILETYPE_AIFF => "AIFF",
        _ => "AUDIO",
    }
}

/// Projects a [`ConnectedIpod`] into a compact [`LibrarySnapshot`] in a single O(N) pass.
#[must_use]
pub fn build_library_snapshot(ipod: &ConnectedIpod) -> LibrarySnapshot {
    let mut tracks = Vec::with_capacity(ipod.itdb.tracks.len());
    let mut album_map: HashMap<(String, String), usize> = HashMap::new();
    let mut albums: Vec<AlbumDto> = Vec::new();
    let mut artist_map: HashMap<String, (usize, u32, std::collections::HashSet<String>)> =
        HashMap::new();

    for t in &ipod.itdb.tracks {
        let badge = format_badge_for(t.filetype, &t.filetype_desc).to_owned();
        let disp_title = if t.title.trim().is_empty() {
            "Untitled Track".to_owned()
        } else {
            t.title.clone()
        };
        let disp_artist = if t.artist.trim().is_empty() {
            "Unknown Artist".to_owned()
        } else {
            t.artist.clone()
        };
        let disp_album = if t.album.trim().is_empty() {
            "Unknown Album".to_owned()
        } else {
            t.album.clone()
        };
        let eff_alb_artist = if !t.album_artist.trim().is_empty() {
            t.album_artist.clone()
        } else {
            disp_artist.clone()
        };

        let mhii = if t.has_artwork { t.mhii_link } else { 0 };

        let alb_key = (
            disp_album.to_lowercase(),
            if t.compilation && t.album_artist.trim().is_empty() {
                "various artists".to_owned()
            } else {
                eff_alb_artist.to_lowercase()
            },
        );
        if let Some(&idx) = album_map.get(&alb_key) {
            let entry = &mut albums[idx];
            entry.track_count += 1;
            entry.total_length_ms += u64::from(t.length_ms);
            if entry.mhii_link == 0 && mhii != 0 {
                entry.mhii_link = mhii;
            }
            if entry.year == 0 && t.year != 0 {
                entry.year = t.year;
            }
            entry.track_ids.push(t.track_id);
        } else {
            let idx = albums.len();
            album_map.insert(alb_key.clone(), idx);
            albums.push(AlbumDto {
                key: format!("{}::{}", alb_key.1, alb_key.0),
                title: disp_album.clone(),
                artist: if t.compilation && t.album_artist.trim().is_empty() {
                    "Various Artists".to_owned()
                } else {
                    eff_alb_artist.clone()
                },
                year: t.year,
                track_count: 1,
                total_length_ms: u64::from(t.length_ms),
                mhii_link: mhii,
                track_ids: vec![t.track_id],
            });
        }

        let art_entry = artist_map
            .entry(disp_artist.clone())
            .or_insert_with(|| (0, mhii, std::collections::HashSet::new()));
        art_entry.0 += 1;
        if art_entry.1 == 0 && mhii != 0 {
            art_entry.1 = mhii;
        }
        art_entry.2.insert(disp_album.to_lowercase());

        tracks.push(TrackDto {
            track_id: t.track_id,
            dbid_hex: format!("{:016X}", t.dbid),
            title: disp_title,
            artist: disp_artist,
            album_artist: eff_alb_artist,
            album: disp_album,
            genre: t.genre.clone(),
            format_badge: badge,
            size_bytes: t.size,
            length_ms: t.length_ms,
            bitrate_kbps: t.bitrate,
            sample_rate_hz: t.sample_rate_hz,
            track_nr: t.track_nr,
            disc_nr: t.disc_nr,
            year: t.year,
            play_count: t.play_count,
            rating: t.rating,
            mhii_link: mhii,
        });
    }

    albums.sort_by(|a, b| {
        a.artist
            .to_lowercase()
            .cmp(&b.artist.to_lowercase())
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });

    let mut artists: Vec<ArtistDto> = artist_map
        .into_iter()
        .map(|(name, (tcount, mhii_link, albs))| ArtistDto {
            name,
            album_count: albs.len(),
            track_count: tcount,
            mhii_link,
        })
        .collect();
    artists.sort_by_key(|a| a.name.to_lowercase());

    let playlists: Vec<PlaylistDto> = ipod
        .itdb
        .playlists
        .iter()
        .filter(|p| !p.is_master)
        .map(|p| PlaylistDto {
            persistent_id_hex: format!("{:016X}", p.persistent_id),
            name: p.name.clone(),
            is_master: false,
            track_ids: p.items.iter().map(|it| it.track_id).collect(),
        })
        .collect();

    LibrarySnapshot {
        device: ipod.summary.clone(),
        tracks,
        albums,
        artists,
        playlists,
    }
}
