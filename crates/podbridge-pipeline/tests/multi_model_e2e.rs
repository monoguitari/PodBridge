// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;

use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use lofty::config::WriteOptions;
use lofty::tag::{Accessor, Tag, TagExt, TagType};
use podbridge_artwork::decode_source_artwork;
use podbridge_crypto::{verify_hash58, verify_hash72, FirewireGuid};
use podbridge_device::{create_mock_ipod, open_ipod_at_path, IpodGeneration};
use podbridge_pipeline::{
    commit_database_atomic, execute_transfer_batch, resolve_ffmpeg_binary, TransferSettings,
};
use tokio_util::sync::CancellationToken;

fn write_pcm_wav(path: &Path, title: &str, artist: &str, album: &str, track_no: u32) {
    let sr = 44_100u32;
    let samples = sr / 4; // 250ms
    let data_bytes = samples * 2;
    let mut buf = Vec::with_capacity(44 + data_bytes as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    buf.extend_from_slice(b"WAVEfmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&sr.to_le_bytes());
    buf.extend_from_slice(&(sr * 2).to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&16u16.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_bytes.to_le_bytes());
    for i in 0..samples {
        let s = ((i as f32 * 0.09).sin() * 10_000.0) as i16;
        buf.extend_from_slice(&s.to_le_bytes());
    }
    fs::write(path, &buf).unwrap();

    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.to_owned());
    tag.set_artist(artist.to_owned());
    tag.set_album(album.to_owned());
    tag.set_track(track_no);
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

fn write_rectangular_cover_png(path: &Path) {
    // Non-square 180x140 image exercises center-crop + multi-resolution RGB565 resize
    let mut img = RgbImage::new(180, 140);
    for (x, y, px) in img.enumerate_pixels_mut() {
        *px = Rgb([(x % 255) as u8, 140, (y % 255) as u8]);
    }
    let mut raw = Vec::new();
    DynamicImage::ImageRgb8(img)
        .write_to(&mut Cursor::new(&mut raw), ImageFormat::Png)
        .unwrap();
    fs::write(path, raw).unwrap();
}

#[tokio::test]
async fn transfers_audio_artwork_and_playlists_across_all_5_ipod_generations() {
    let base_dir = std::env::temp_dir().join(format!("podbridge_5gen_e2e_{}", std::process::id()));
    let _ = fs::remove_dir_all(&base_dir);
    let media_dir = base_dir.join("source_media");
    let backup_dir = base_dir.join("backups");
    fs::create_dir_all(&media_dir).unwrap();

    let wav_path = media_dir.join("01 NativeWave.wav");
    write_pcm_wav(
        &wav_path,
        "Native Wave",
        "PodBridge Synth",
        "MultiGen Suite",
        1,
    );
    write_rectangular_cover_png(&media_dir.join("folder.png"));

    let mut playlist_lines = String::from("#EXTM3U\n#PLAYLIST: MultiGen Mix\n01 NativeWave.wav\n");
    let mut expected_track_count = 1usize;

    if let Some(ff) = resolve_ffmpeg_binary(None) {
        let enc_out = Command::new(&ff).arg("-encoders").output().unwrap();
        let enc_txt = String::from_utf8_lossy(&enc_out.stdout);
        if enc_txt.contains(" flac ") && enc_txt.contains(" alac ") {
            let flac_path = media_dir.join("02 LosslessFlac.flac");
            let ogg_path = media_dir.join("03 LossyOgg.ogg");
            assert!(Command::new(&ff)
                .args(["-y", "-v", "error", "-i"])
                .arg(&wav_path)
                .args([
                    "-c:a",
                    "flac",
                    "-metadata",
                    "title=Lossless Flac",
                    "-metadata",
                    "artist=PodBridge Synth",
                    "-metadata",
                    "album=MultiGen Suite",
                ])
                .arg(&flac_path)
                .status()
                .unwrap()
                .success());
            assert!(Command::new(&ff)
                .args(["-y", "-v", "error", "-i"])
                .arg(&wav_path)
                .args([
                    "-metadata",
                    "title=Lossy Ogg",
                    "-metadata",
                    "artist=PodBridge Synth",
                    "-metadata",
                    "album=MultiGen Suite",
                ])
                .arg(&ogg_path)
                .status()
                .unwrap()
                .success());
            playlist_lines.push_str("02 LosslessFlac.flac\n03 LossyOgg.ogg\n");
            expected_track_count = 3;
        }
    }

    fs::write(media_dir.join("MultiGen.m3u8"), playlist_lines).unwrap();

    let guid = FirewireGuid::parse_hex("000A27001358762E").unwrap();
    let profiles = [
        (IpodGeneration::Video5G, &[20_000u32, 80_000][..]),
        (IpodGeneration::Classic6G, &[32_768, 204_800, 6_160][..]),
        (IpodGeneration::Nano3G, &[32_768, 204_800, 6_160][..]),
        (
            IpodGeneration::Nano4G,
            &[32_768, 115_200, 5_000, 12_800][..],
        ),
        (
            IpodGeneration::Nano5G,
            &[32_768, 115_200, 5_000, 12_800][..],
        ),
    ];

    for (idx, (generation, expected_ithmb_sizes)) in profiles.into_iter().enumerate() {
        let ipod_root = base_dir.join(format!("ipod_{idx}_{generation:?}"));
        let mut ipod = create_mock_ipod(&ipod_root, generation.display_name(), generation, guid)
            .expect("mock ipod created");

        let summary = execute_transfer_batch(
            &mut ipod,
            std::slice::from_ref(&media_dir),
            &TransferSettings::default(),
            &backup_dir,
            None,
            CancellationToken::new(),
            |_| {},
        )
        .await
        .expect("transfer batch succeeded");

        assert_eq!(summary.added_tracks, expected_track_count);
        assert_eq!(summary.created_playlists, 1);

        // Exercise custom playlist mutation + atomic commit
        let custom_pid = ipod.itdb.create_playlist("Workout", 1000);
        let first_tid = ipod.itdb.tracks[0].track_id;
        ipod.itdb
            .set_playlist_tracks(custom_pid, &[first_tid], 1005)
            .unwrap();
        commit_database_atomic(&mut ipod, &backup_dir).unwrap();

        // Reopen cold from disk and verify cryptographic signatures + .ithmb decoding
        let reopened = open_ipod_at_path(&ipod_root, None, true, None).unwrap();
        assert_eq!(reopened.itdb.tracks.len(), expected_track_count);
        assert_eq!(reopened.summary.playlist_count, 2); // MultiGen Mix + Workout

        let raw_db = fs::read(reopened.itunes_db_path()).unwrap();
        match generation {
            IpodGeneration::Video5G => {}
            IpodGeneration::Nano5G => {
                assert!(verify_hash72(&raw_db, None).unwrap());
            }
            _ => {
                assert!(verify_hash58(&raw_db, guid).unwrap());
            }
        }

        let actual_sizes: Vec<u32> = reopened
            .artwork_db
            .format_files
            .iter()
            .map(|f| f.img_size)
            .collect();
        assert_eq!(actual_sizes, expected_ithmb_sizes);

        // Decode live PNG preview from the generated .ithmb file
        let png = reopened
            .artwork_db
            .read_thumbnail_png(&reopened.artwork_dir(), 100, 100)
            .unwrap()
            .expect("thumbnail decoded");
        let decoded = decode_source_artwork(&png).unwrap();
        assert!(decoded.width() >= 100);
    }

    let _ = fs::remove_dir_all(&base_dir);
}
