// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey};
use podbridge_itdb::{
    parse_m3u_file, DuplicateCandidate, DuplicateIndex, DEFAULT_DUPLICATE_TOLERANCE_MS,
    FILETYPE_AIFF, FILETYPE_M4A, FILETYPE_MP3, FILETYPE_WAV,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::settings::{DuplicatePolicy, LosslessTarget, TransferSettings};

/// Action required to transfer an audio file onto a stock-firmware iPod.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferAction {
    CopyNative {
        ipod_ext: &'static str,
        filetype: u32,
        filetype_desc: &'static str,
    },
    TranscodeAlac,
    TranscodeAac {
        bitrate_kbps: u32,
    },
}

/// Extracted cover-art payload with pre-computed SHA-256 digest for O(1) album deduplication.
#[derive(Debug, Clone)]
pub struct ExtractedArtwork {
    pub sha256: [u8; 32],
    pub bytes: Vec<u8>,
}

/// Scanned audio track ready for duplicate resolution, optional transcoding, and USB transfer.
#[derive(Debug, Clone)]
pub struct ScannedTrack {
    pub source_path: PathBuf,
    pub source_size: u64,
    pub action: TransferAction,
    pub duplicate_of_track_id: Option<u32>,
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub genre: String,
    pub composer: String,
    pub comment: String,
    pub length_ms: u32,
    pub bitrate_kbps: u32,
    pub sample_rate_hz: u32,
    pub track_nr: u32,
    pub tracks_total: u32,
    pub disc_nr: u32,
    pub discs_total: u32,
    pub year: u32,
    pub compilation: bool,
    pub artwork: Option<ExtractedArtwork>,
}

/// Playlist discovered from a dropped `.m3u` / `.m3u8` file referencing local source paths.
#[derive(Debug, Clone)]
pub struct StagedPlaylistImport {
    pub name: String,
    pub source_paths: Vec<PathBuf>,
}

/// Complete output of scanning dropped files, directories, and `.m3u`/`.m3u8` playlists.
#[derive(Debug, Default, Clone)]
pub struct ScanReport {
    pub tracks: Vec<ScannedTrack>,
    pub playlists: Vec<StagedPlaylistImport>,
    pub unsupported_files: Vec<PathBuf>,
    pub skipped_duplicates: usize,
}

const COVER_FILENAMES: &[&str] = &[
    "cover.jpg",
    "cover.jpeg",
    "cover.png",
    "folder.jpg",
    "folder.jpeg",
    "folder.png",
    "front.jpg",
    "front.jpeg",
    "front.png",
    "album.jpg",
    "album.png",
];

fn classify_extension(ext_lower: &str, settings: &TransferSettings) -> Option<TransferAction> {
    match ext_lower {
        "mp3" => Some(TransferAction::CopyNative {
            ipod_ext: "mp3",
            filetype: FILETYPE_MP3,
            filetype_desc: "MPEG audio file",
        }),
        "m4a" | "aac" | "m4b" => Some(TransferAction::CopyNative {
            ipod_ext: "m4a",
            filetype: FILETYPE_M4A,
            filetype_desc: "AAC audio file",
        }),
        "wav" => Some(TransferAction::CopyNative {
            ipod_ext: "wav",
            filetype: FILETYPE_WAV,
            filetype_desc: "WAV audio file",
        }),
        "aif" | "aiff" => Some(TransferAction::CopyNative {
            ipod_ext: "aif",
            filetype: FILETYPE_AIFF,
            filetype_desc: "AIFF audio file",
        }),
        "flac" | "ape" | "wv" => Some(match settings.lossless_target {
            LosslessTarget::Alac => TransferAction::TranscodeAlac,
            LosslessTarget::Aac => TransferAction::TranscodeAac {
                bitrate_kbps: settings.aac_bitrate_kbps,
            },
        }),
        "ogg" | "oga" | "opus" | "wma" | "mpc" => Some(TransferAction::TranscodeAac {
            bitrate_kbps: settings.aac_bitrate_kbps,
        }),
        _ => None,
    }
}

fn is_playlist_ext(ext_lower: &str) -> bool {
    matches!(ext_lower, "m3u" | "m3u8")
}

fn is_ignorable_sidecar_ext(ext_lower: &str) -> bool {
    matches!(
        ext_lower,
        "jpg"
            | "jpeg"
            | "png"
            | "webp"
            | "bmp"
            | "gif"
            | "cue"
            | "log"
            | "txt"
            | "nfo"
            | "ini"
            | "db"
            | "ds_store"
    )
}

/// Scans dropped files, nested directories, and `.m3u`/`.m3u8` playlists in parallel across CPU
/// cores using `rayon` + `lofty`, then marks duplicates against `existing_dup_index`.
#[must_use]
pub fn scan_dropped_paths(
    dropped_paths: &[PathBuf],
    settings: &TransferSettings,
    existing_dup_index: &DuplicateIndex,
) -> ScanReport {
    let mut candidate_audio_paths: Vec<PathBuf> = Vec::new();
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    let mut playlists = Vec::new();
    let mut unsupported_files = Vec::new();

    let mut push_audio = |path: PathBuf, unsupported: &mut Vec<PathBuf>| {
        let canonical = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if !seen_paths.insert(canonical.clone()) {
            return;
        }
        let ext = canonical
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if classify_extension(&ext, settings).is_some() {
            candidate_audio_paths.push(canonical);
        } else if !is_ignorable_sidecar_ext(&ext) {
            unsupported.push(canonical);
        }
    };

    for root in dropped_paths {
        if root.is_dir() {
            let mut dir_entries: Vec<PathBuf> = WalkDir::new(root)
                .follow_links(false)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file())
                .map(|e| e.into_path())
                .collect();
            dir_entries.sort();

            for entry in dir_entries {
                let ext = entry
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase)
                    .unwrap_or_default();
                if is_playlist_ext(&ext) {
                    if let Ok(spec) = parse_m3u_file(&entry) {
                        let mut canonical_members = Vec::with_capacity(spec.track_paths.len());
                        for member in spec.track_paths {
                            if member.is_file() {
                                let can = fs::canonicalize(&member).unwrap_or(member);
                                canonical_members.push(can.clone());
                                push_audio(can, &mut unsupported_files);
                            }
                        }
                        playlists.push(StagedPlaylistImport {
                            name: spec.name,
                            source_paths: canonical_members,
                        });
                    }
                } else {
                    push_audio(entry, &mut unsupported_files);
                }
            }
        } else if root.is_file() {
            let ext = root
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            if is_playlist_ext(&ext) {
                if let Ok(spec) = parse_m3u_file(root) {
                    let mut canonical_members = Vec::with_capacity(spec.track_paths.len());
                    for member in spec.track_paths {
                        if member.is_file() {
                            let can = fs::canonicalize(&member).unwrap_or(member);
                            canonical_members.push(can.clone());
                            push_audio(can, &mut unsupported_files);
                        }
                    }
                    playlists.push(StagedPlaylistImport {
                        name: spec.name,
                        source_paths: canonical_members,
                    });
                }
            } else {
                push_audio(root.clone(), &mut unsupported_files);
            }
        }
    }

    // Parallel metadata + embedded/folder artwork extraction across CPU cores
    let scanned_results: Vec<Option<ScannedTrack>> = candidate_audio_paths
        .par_iter()
        .map(|path| probe_single_audio_file(path, settings))
        .collect();

    let mut working_dup = existing_dup_index.clone();
    let mut tracks = Vec::with_capacity(scanned_results.len());
    let mut skipped_duplicates = 0usize;
    let mut synthetic_staged_id = 0x8000_0000u32;

    for (idx, maybe_track) in scanned_results.into_iter().enumerate() {
        let Some(mut track) = maybe_track else {
            unsupported_files.push(candidate_audio_paths[idx].clone());
            continue;
        };

        if settings.duplicate_policy == DuplicatePolicy::Skip {
            let dup = working_dup.find_duplicate(
                DuplicateCandidate {
                    title: &track.title,
                    artist: &track.artist,
                    album: &track.album,
                    length_ms: track.length_ms,
                },
                DEFAULT_DUPLICATE_TOLERANCE_MS,
            );
            if let Some(existing_id) = dup {
                track.duplicate_of_track_id = Some(existing_id);
                skipped_duplicates += 1;
            } else {
                working_dup.insert(
                    &track.title,
                    &track.artist,
                    &track.album,
                    track.length_ms,
                    synthetic_staged_id,
                );
                synthetic_staged_id = synthetic_staged_id.wrapping_add(1);
            }
        }

        tracks.push(track);
    }

    ScanReport {
        tracks,
        playlists,
        unsupported_files,
        skipped_duplicates,
    }
}

fn probe_single_audio_file(path: &Path, settings: &TransferSettings) -> Option<ScannedTrack> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)?;
    let mut action = classify_extension(&ext, settings)?;
    let source_size = fs::metadata(path).ok()?.len();

    let fallback_title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_owned();

    let tagged = Probe::open(path)
        .ok()
        .and_then(|p| p.options(ParseOptions::new()).read().ok());

    let mut title = fallback_title;
    let mut artist = String::new();
    let mut album_artist = String::new();
    let mut album = String::new();
    let mut genre = String::new();
    let mut composer = String::new();
    let mut comment = String::new();
    let mut length_ms = 0u32;
    let mut bitrate_kbps = 0u32;
    let mut sample_rate_hz = 44_100u32;
    let mut track_nr = 0u32;
    let mut tracks_total = 0u32;
    let mut disc_nr = 0u32;
    let mut discs_total = 0u32;
    let mut year = 0u32;
    let mut compilation = false;
    let mut embedded_art: Option<Vec<u8>> = None;

    if let Some(ref tf) = tagged {
        let props = tf.properties();
        length_ms = props.duration().as_millis() as u32;
        bitrate_kbps = props
            .audio_bitrate()
            .or_else(|| props.overall_bitrate())
            .unwrap_or(0);
        if let Some(sr) = props.sample_rate() {
            if sr > 0 {
                sample_rate_hz = sr;
            }
        }

        // Distinguish ALAC vs AAC description on native .m4a files
        if let TransferAction::CopyNative {
            filetype,
            ref mut filetype_desc,
            ..
        } = action
        {
            if filetype == FILETYPE_M4A && bitrate_kbps > 450 {
                *filetype_desc = "Apple Lossless audio file";
            }
        }

        if let Some(tag) = tf.primary_tag().or_else(|| tf.first_tag()) {
            if let Some(t) = tag.title().filter(|s| !s.trim().is_empty()) {
                title = t.into_owned();
            }
            if let Some(a) = tag.artist().filter(|s| !s.trim().is_empty()) {
                artist = a.into_owned();
            }
            if let Some(alb) = tag.album().filter(|s| !s.trim().is_empty()) {
                album = alb.into_owned();
            }
            if let Some(g) = tag.genre().filter(|s| !s.trim().is_empty()) {
                genre = g.into_owned();
            }
            if let Some(c) = tag.comment().filter(|s| !s.trim().is_empty()) {
                comment = c.into_owned();
            }
            if let Some(aa) = tag.get_string(&ItemKey::AlbumArtist) {
                if !aa.trim().is_empty() {
                    album_artist = aa.to_owned();
                }
            }
            if let Some(comp) = tag.get_string(&ItemKey::Composer) {
                if !comp.trim().is_empty() {
                    composer = comp.to_owned();
                }
            }
            if let Some(flag) = tag.get_string(&ItemKey::FlagCompilation) {
                compilation = flag.trim() == "1" || flag.eq_ignore_ascii_case("true");
            }
            track_nr = tag.track().unwrap_or(0);
            tracks_total = tag.track_total().unwrap_or(0);
            disc_nr = tag.disk().unwrap_or(0);
            discs_total = tag.disk_total().unwrap_or(0);
            year = tag.year().unwrap_or(0);

            let pics = tag.pictures();
            if let Some(pic) = pics
                .iter()
                .find(|p| p.pic_type() == PictureType::CoverFront)
                .or_else(|| pics.first())
            {
                let data = pic.data();
                if !data.is_empty() {
                    embedded_art = Some(data.to_vec());
                }
            }
        }
    }

    let art_bytes = embedded_art.or_else(|| find_folder_artwork(path.parent()?));
    let artwork = art_bytes.map(|bytes| {
        let sha256: [u8; 32] = Sha256::digest(&bytes).into();
        ExtractedArtwork { sha256, bytes }
    });

    Some(ScannedTrack {
        source_path: path.to_path_buf(),
        source_size,
        action,
        duplicate_of_track_id: None,
        title,
        artist,
        album_artist,
        album,
        genre,
        composer,
        comment,
        length_ms,
        bitrate_kbps,
        sample_rate_hz,
        track_nr,
        tracks_total,
        disc_nr,
        discs_total,
        year,
        compilation,
        artwork,
    })
}

fn find_folder_artwork(dir: &Path) -> Option<Vec<u8>> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.filter_map(Result::ok) {
        if !entry.file_type().ok()?.is_file() {
            continue;
        }
        let fname = entry.file_name();
        let lower = fname.to_string_lossy().to_ascii_lowercase();
        if COVER_FILENAMES.contains(&lower.as_str()) {
            if let Ok(bytes) = fs::read(entry.path()) {
                if !bytes.is_empty() {
                    return Some(bytes);
                }
            }
        }
    }
    None
}
