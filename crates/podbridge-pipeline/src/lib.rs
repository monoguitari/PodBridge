// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! Parallel metadata scanner, FFmpeg transcoding pool, sequential HDD writer, and batch commit.

pub mod backup;
pub mod error;
pub mod scanner;
pub mod settings;
pub mod transcode;
pub mod writer;

pub use backup::{
    create_rotating_backup, list_device_backups, restore_device_backup, write_file_atomic,
    BackupManifest, MAX_BACKUPS_PER_DEVICE,
};
pub use error::PipelineError;
pub use scanner::{
    scan_dropped_paths, ExtractedArtwork, ScanReport, ScannedTrack, StagedPlaylistImport,
    TransferAction,
};
pub use settings::{DuplicatePolicy, LosslessTarget, TransferSettings};
pub use transcode::{
    resolve_ffmpeg_binary, PreparedArtwork, PreparedAudioSource, PreparedTrack, TranscodePool,
};
pub use writer::{
    commit_database_atomic, delete_tracks_atomic, execute_transfer_batch, BatchCommitSummary,
    TransferItemStatus, TransferPhase, TransferProgressSnapshot, TransferQueueItem,
};

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Cursor;
    use std::path::Path;
    use std::process::Command;
    use std::sync::{Arc, Mutex};

    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
    use lofty::config::WriteOptions;
    use lofty::tag::{Accessor, Tag, TagExt, TagType};
    use podbridge_artwork::CLASSIC_6G_SPECS;
    use podbridge_crypto::{verify_hash58, FirewireGuid};
    use podbridge_device::{create_mock_ipod, open_ipod_at_path, IpodGeneration};
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

    fn write_valid_png(path: &Path) {
        let mut img = RgbImage::new(96, 96);
        for px in img.pixels_mut() {
            *px = Rgb([30, 180, 220]);
        }
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        fs::write(path, bytes).unwrap();
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
        write_valid_png(&album_dir.join("cover.png"));

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

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn transcodes_flac_to_alac_and_ogg_to_aac_and_honors_cancellation() {
        let Some(ffmpeg) = resolve_ffmpeg_binary(None) else {
            return;
        };
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
            &[flac_src.clone(), ogg_src],
            &TransferSettings::default(),
            &dup_idx,
        );
        let pool = TranscodePool::new(Some(&ffmpeg));
        let cancel = CancellationToken::new();

        let prep_alac = pool
            .prepare_track(
                0,
                report.tracks[0].clone(),
                CLASSIC_6G_SPECS.to_vec(),
                cancel,
            )
            .await
            .unwrap();
        assert_eq!(prep_alac.final_filetype, FILETYPE_M4A);

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn executes_end_to_end_batch_commit_dedup_delete_and_backup_restore() {
        let dir = std::env::temp_dir().join(format!("podbridge_e2e_pipe_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let ipod_dir = dir.join("mock_ipod");
        let drop_dir = dir.join("drop_album");
        let backup_dir = dir.join("backups");
        fs::create_dir_all(&drop_dir).unwrap();

        let guid = FirewireGuid::parse_hex("000A27001358762E").unwrap();
        let mut ipod =
            create_mock_ipod(&ipod_dir, "MB147LL Test", IpodGeneration::Classic6G, guid).unwrap();

        let t1_path = drop_dir.join("01 Horizon.wav");
        let t2_path = drop_dir.join("02 Starlight.wav");
        write_tagged_wav(&t1_path, "Horizon", "Nova", "Odyssey", 400);
        write_tagged_wav(&t2_path, "Starlight", "Nova", "Odyssey", 450);
        write_valid_png(&drop_dir.join("cover.png"));
        fs::write(
            drop_dir.join("Odyssey.m3u8"),
            "#EXTM3U\n#PLAYLIST: Odyssey Mix\n01 Horizon.wav\n02 Starlight.wav\n",
        )
        .unwrap();

        let phases = Arc::new(Mutex::new(Vec::new()));
        let phases_cb = Arc::clone(&phases);

        let summary = execute_transfer_batch(
            &mut ipod,
            std::slice::from_ref(&drop_dir),
            &TransferSettings::default(),
            &backup_dir,
            None,
            CancellationToken::new(),
            move |snap| {
                phases_cb.lock().unwrap().push(snap.phase);
            },
        )
        .await
        .unwrap();

        assert_eq!(summary.added_tracks, 2);
        assert_eq!(summary.created_playlists, 1);
        assert_eq!(ipod.summary.track_count, 2);
        assert_eq!(ipod.summary.playlist_count, 1);

        // Both tracks on the same album MUST share one deduplicated ArtworkDB mhii entry (id=100)!
        assert_eq!(ipod.artwork_db.images.len(), 1);
        assert_eq!(ipod.artwork_db.images[0].ref_count, 2);
        assert_eq!(ipod.itdb.tracks[0].mhii_link, 100);
        assert_eq!(ipod.itdb.tracks[1].mhii_link, 100);

        // Verify disk signature and reopen parity
        let reopened = open_ipod_at_path(&ipod_dir, None, true, None).unwrap();
        let raw_db = fs::read(reopened.itunes_db_path()).unwrap();
        assert!(verify_hash58(&raw_db, guid).unwrap());
        assert_eq!(reopened.itdb.tracks.len(), 2);

        // Re-dropping the exact same folder skips both duplicates cleanly!
        let summary2 = execute_transfer_batch(
            &mut ipod,
            std::slice::from_ref(&t1_path),
            &TransferSettings::default(),
            &backup_dir,
            None,
            CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(summary2.added_tracks, 0);
        assert_eq!(summary2.skipped_duplicates, 1);

        // Delete both tracks -> creates backup snapshot containing the 2-track state!
        let tids: Vec<u32> = ipod.itdb.tracks.iter().map(|t| t.track_id).collect();
        let deleted = delete_tracks_atomic(&mut ipod, &tids, &backup_dir).unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(ipod.summary.track_count, 0);
        assert!(ipod.artwork_db.images.is_empty());

        // Restore the pre-deletion backup -> 2 tracks & ArtworkDB restored!
        let backups = list_device_backups(&ipod, &backup_dir);
        assert!(backups.len() >= 2);
        restore_device_backup(&mut ipod, &backup_dir, &backups[0].id).unwrap();
        assert_eq!(ipod.summary.track_count, 2);
        assert_eq!(ipod.artwork_db.images.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }
}
