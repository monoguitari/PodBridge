// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! Parallel metadata scanner, FFmpeg transcoding pool, sequential HDD writer, and batch commit.

pub mod error;
pub mod scanner;
pub mod settings;
pub mod transcode;

pub use error::PipelineError;
pub use scanner::{
    scan_dropped_paths, ExtractedArtwork, ScanReport, ScannedTrack, StagedPlaylistImport,
    TransferAction,
};
pub use settings::{DuplicatePolicy, LosslessTarget, TransferSettings};
pub use transcode::{
    resolve_ffmpeg_binary, PreparedArtwork, PreparedAudioSource, PreparedTrack, TranscodePool,
};

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use lofty::config::WriteOptions;
    use lofty::tag::{Accessor, Tag, TagExt, TagType};
    use podbridge_artwork::CLASSIC_6G_SPECS;
    use podbridge_itdb::{DuplicateIndex, ItdbDatabase, FILETYPE_M4A};
    use tokio_util::sync::CancellationToken;

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
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_size.to_le_bytes());
        for i in 0..num_samples {
            let sample = ((i as f32 * 0.06).sin() * 12_000.0) as i16;
            wav.extend_from_slice(&sample.to_le_bytes());
        }
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
        fs::write(&flac_stub, b"fLaC\x00\x00\x00\x22").unwrap();
        fs::write(&ogg_stub, b"OggS\x00\x02").unwrap();

        fs::write(album_dir.join("cover.png"), b"\x89PNG\r\n\x1a\nmock").unwrap();

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

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn transcodes_flac_to_alac_and_ogg_to_aac_and_honors_cancellation() {
        let Some(ffmpeg) = resolve_ffmpeg_binary(None) else {
            return;
        };
        // Verify this ffmpeg binary supports flac & alac (skip if minimal video-only stub)
        let enc_check = Command::new(&ffmpeg).arg("-encoders").output().unwrap();
        let enc_str = String::from_utf8_lossy(&enc_check.stdout);
        if !enc_str.contains(" alac ") || !enc_str.contains(" flac ") {
            return;
        }

        let dir = std::env::temp_dir().join(format!("podbridge_tx_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let wav_src = dir.join("src.wav");
        let flac_src = dir.join("lossless.flac");
        let ogg_src = dir.join("lossy.ogg");
        write_tagged_wav(&wav_src, "Synth", "Artist", "Album", 350);

        assert!(Command::new(&ffmpeg)
            .args(["-y", "-v", "error", "-i"])
            .arg(&wav_src)
            .args(["-c:a", "flac"])
            .arg(&flac_src)
            .status()
            .unwrap()
            .success());

        assert!(Command::new(&ffmpeg)
            .args(["-y", "-v", "error", "-i"])
            .arg(&wav_src)
            .arg(&ogg_src)
            .status()
            .unwrap()
            .success());

        let db = ItdbDatabase::new_empty("iPod", 0);
        let dup_idx = DuplicateIndex::from_database(&db);
        let report = scan_dropped_paths(
            &[flac_src.clone(), ogg_src.clone()],
            &TransferSettings::default(),
            &dup_idx,
        );
        assert_eq!(report.tracks.len(), 2);

        let pool = TranscodePool::new(Some(&ffmpeg));
        let cancel = CancellationToken::new();

        let prep_alac = pool
            .prepare_track(
                0,
                report.tracks[0].clone(),
                CLASSIC_6G_SPECS.to_vec(),
                cancel.clone(),
            )
            .await
            .unwrap();
        assert_eq!(prep_alac.final_filetype, FILETYPE_M4A);
        assert_eq!(prep_alac.final_filetype_desc, "Apple Lossless audio file");
        assert!(prep_alac.final_size > 1_000);
        assert!(prep_alac.audio_source.as_path().is_file());

        let prep_aac = pool
            .prepare_track(
                1,
                report.tracks[1].clone(),
                CLASSIC_6G_SPECS.to_vec(),
                cancel.clone(),
            )
            .await
            .unwrap();
        assert_eq!(prep_aac.final_filetype_desc, "AAC audio file");
        assert!(prep_aac.final_size > 500);

        // Pre-cancelled token aborts immediately
        let cancelled_token = CancellationToken::new();
        cancelled_token.cancel();
        let err = pool
            .prepare_track(
                2,
                report.tracks[0].clone(),
                CLASSIC_6G_SPECS.to_vec(),
                cancelled_token,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, PipelineError::Cancelled));

        let _ = fs::remove_dir_all(&dir);
    }
}
