// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! Parallel metadata scanner, FFmpeg transcoding pool, sequential HDD writer, and batch commit.

pub mod error;
pub mod scanner;
pub mod settings;

pub use error::PipelineError;
pub use scanner::{
    scan_dropped_paths, ExtractedArtwork, ScanReport, ScannedTrack, StagedPlaylistImport,
    TransferAction,
};
pub use settings::{DuplicatePolicy, LosslessTarget, TransferSettings};

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use lofty::config::WriteOptions;
    use lofty::tag::{Accessor, Tag, TagExt, TagType};
    use podbridge_itdb::{DuplicateIndex, ItdbDatabase};

    use super::*;

    fn write_tagged_wav(path: &Path, title: &str, artist: &str, album: &str, duration_ms: u32) {
        let sample_rate = 44_100u32;
        let num_samples = (sample_rate * duration_ms) / 1_000;
        let data_size = num_samples * 2;
        let mut wav = Vec::with_capacity(44 + data_size as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_size).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_size.to_le_bytes());
        wav.resize(44 + data_size as usize, 0);
        fs::write(path, &wav).unwrap();

        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(title.to_owned());
        tag.set_artist(artist.to_owned());
        tag.set_album(album.to_owned());
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }

    #[test]
    fn scans_audio_folders_folder_artwork_m3u8_and_intra_batch_duplicates() {
        let dir = std::env::temp_dir().join(format!("podbridge_scan_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let album_dir = dir.join("AlbumA");
        fs::create_dir_all(&album_dir).unwrap();

        let wav_path = album_dir.join("01 Intro.wav");
        let dup_wav_path = dir.join("01 Intro Copy.wav");
        let flac_stub = album_dir.join("02 LosslessTrack.flac");
        let ogg_stub = album_dir.join("03 LossyTrack.ogg");

        write_tagged_wav(&wav_path, "Intro", "Test Artist", "Test Album", 500);
        fs::copy(&wav_path, &dup_wav_path).unwrap();
        // Minimal files fallback cleanly to filename stem when untagged
        fs::write(&flac_stub, b"fLaC\x00\x00\x00\x22").unwrap();
        fs::write(&ogg_stub, b"OggS\x00\x02").unwrap();

        // Write sibling cover.png
        fs::write(album_dir.join("cover.png"), b"\x89PNG\r\n\x1a\nmock").unwrap();

        // Write M3U8 playlist
        let m3u_path = dir.join("Favorites.m3u8");
        fs::write(
            &m3u_path,
            "#EXTM3U\n#PLAYLIST: My Favorites\nAlbumA/01 Intro.wav\nAlbumA/02 LosslessTrack.flac\n",
        )
        .unwrap();

        let db = ItdbDatabase::new_empty("iPod", 0);
        let dup_idx = DuplicateIndex::from_database(&db);
        let settings = TransferSettings::default();

        let report = scan_dropped_paths(std::slice::from_ref(&dir), &settings, &dup_idx);
        assert_eq!(report.tracks.len(), 4);
        assert_eq!(report.skipped_duplicates, 1);
        assert_eq!(report.playlists.len(), 1);
        assert_eq!(report.playlists[0].name, "My Favorites");

        let intro = report
            .tracks
            .iter()
            .find(|t| t.source_path.ends_with("01 Intro.wav"))
            .unwrap();
        assert_eq!(intro.title, "Intro");
        assert_eq!(intro.artist, "Test Artist");
        assert!(intro.artwork.is_some());

        let flac_item = report
            .tracks
            .iter()
            .find(|t| t.source_path.ends_with("02 LosslessTrack.flac"))
            .unwrap();
        assert_eq!(flac_item.action, TransferAction::TranscodeAlac);

        let ogg_item = report
            .tracks
            .iter()
            .find(|t| t.source_path.ends_with("03 LossyTrack.ogg"))
            .unwrap();
        assert_eq!(
            ogg_item.action,
            TransferAction::TranscodeAac { bitrate_kbps: 256 }
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
