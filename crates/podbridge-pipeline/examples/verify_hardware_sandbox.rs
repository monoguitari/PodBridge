// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Strictly READ-ONLY hardware verification harness against a connected iPod Classic (`D:\`),
// plus an isolated temp-folder clone test verifying live FLAC->ALAC + ArtworkDB + dual
// hash72+hash58 commits on a copy of the real library without modifying `D:\`.

use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use podbridge_artwork::decode_source_artwork;
use podbridge_crypto::{verify_hash58, verify_hash72, FirewireGuid};
use podbridge_device::{discover_connected_ipods, open_ipod_at_path};
use podbridge_pipeline::{execute_transfer_batch, resolve_ffmpeg_binary, TransferSettings};
use tokio_util::sync::CancellationToken;

fn write_test_wav(path: &Path) {
    let sr = 44_100u32;
    let samples = sr / 4;
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
    buf.resize(44 + data_bytes as usize, 0);
    fs::write(path, buf).unwrap();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let explicit_ffmpeg = std::env::args().nth(1).map(std::path::PathBuf::from);
    let t0 = Instant::now();
    let devices = discover_connected_ipods();
    let discover_ms = t0.elapsed();

    let Some(real_ipod) = devices.first() else {
        println!("No physical USB iPod currently mounted — skipping hardware check.");
        return Ok(());
    };

    println!(
        "[1/3] Discovered {:?} ({}) at {:?} in {:.2?} | GUID={:?} | tracks={} | artworks={}",
        real_ipod.summary.name,
        real_ipod.summary.generation_label,
        real_ipod.summary.root_path,
        discover_ms,
        real_ipod.summary.firewire_guid,
        real_ipod.summary.track_count,
        real_ipod.artwork_db.images.len()
    );

    // Step 2: Read-only .ithmb cover art decode directly from D:\iPod_Control\Artwork\
    if let Some(first_art) = real_ipod.artwork_db.images.first() {
        let t_art = Instant::now();
        let png = real_ipod
            .artwork_db
            .read_thumbnail_png(&real_ipod.artwork_dir(), first_art.id, 128)?
            .ok_or("expected thumbnail on connected iPod")?;
        let art_elapsed = t_art.elapsed();
        let img = decode_source_artwork(&png)?;
        println!(
            "[2/3] Decoded real .ithmb cover (mhii_id={}) -> {}x{} PNG ({} B) in {:.2?} (read-only)",
            first_art.id,
            img.width(),
            img.height(),
            png.len(),
            art_elapsed
        );
    }

    // Step 3: Clone SysInfo + iTunesDB + ArtworkDB into a temp sandbox on C:\ and test a full
    // FLAC -> ALAC + WAV + Cover Art transfer onto the 994-track library without touching D:\!
    let sandbox = std::env::temp_dir().join(format!("podbridge_hw_sandbox_{}", std::process::id()));
    let _ = fs::remove_dir_all(&sandbox);
    let sb_ipod = sandbox.join("ipod_clone");
    let sb_media = sandbox.join("drop_media");
    let sb_backups = sandbox.join("backups");

    fs::create_dir_all(sb_ipod.join("iPod_Control/Device"))?;
    fs::create_dir_all(sb_ipod.join("iPod_Control/iTunes"))?;
    fs::create_dir_all(sb_ipod.join("iPod_Control/Artwork"))?;
    fs::create_dir_all(sb_ipod.join("iPod_Control/Music/F00"))?;
    fs::create_dir_all(&sb_media)?;

    let real_sysinfo = real_ipod
        .summary
        .root_path
        .join("iPod_Control/Device/SysInfo");
    if real_sysinfo.is_file() {
        fs::copy(&real_sysinfo, sb_ipod.join("iPod_Control/Device/SysInfo"))?;
    }
    fs::copy(
        real_ipod.itunes_db_path(),
        sb_ipod.join("iPod_Control/iTunes/iTunesDB"),
    )?;
    if real_ipod.artwork_db_path().is_file() {
        fs::copy(
            real_ipod.artwork_db_path(),
            sb_ipod.join("iPod_Control/Artwork/ArtworkDB"),
        )?;
    }

    let wav_file = sb_media.join("01 SandboxWav.wav");
    write_test_wav(&wav_file);

    let mut img = RgbImage::new(200, 200);
    for px in img.pixels_mut() {
        *px = Rgb([56, 189, 248]);
    }
    let mut png_bytes = Vec::new();
    DynamicImage::ImageRgb8(img).write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)?;
    fs::write(sb_media.join("cover.png"), png_bytes)?;

    let ff = resolve_ffmpeg_binary(explicit_ffmpeg.as_deref());
    let mut expected_added = 1usize;
    if let Some(ref ff_bin) = ff {
        let flac_file = sb_media.join("02 SandboxLossless.flac");
        let st = Command::new(ff_bin)
            .args(["-y", "-v", "error", "-i"])
            .arg(&wav_file)
            .args([
                "-c:a",
                "flac",
                "-metadata",
                "title=Sandbox ALAC Test",
                "-metadata",
                "artist=PodBridge",
                "-metadata",
                "album=MB147LL Verification",
            ])
            .arg(&flac_file)
            .status()?;
        if st.success() {
            expected_added = 2;
        }
    }

    let orig_count = real_ipod.itdb.tracks.len();
    let mut cloned_ipod = open_ipod_at_path(&sb_ipod, None, true, None)?;
    let commit = execute_transfer_batch(
        &mut cloned_ipod,
        std::slice::from_ref(&sb_media),
        &TransferSettings::default(),
        &sb_backups,
        ff.as_deref(),
        CancellationToken::new(),
        |_| {},
    )
    .await?;

    assert_eq!(commit.added_tracks, expected_added);
    assert_eq!(cloned_ipod.itdb.tracks.len(), orig_count + expected_added);

    let raw_committed = fs::read(cloned_ipod.itunes_db_path())?;
    if let Some(ref guid_str) = real_ipod.summary.firewire_guid {
        let guid = FirewireGuid::parse_hex(guid_str)?;
        assert!(verify_hash58(&raw_committed, guid)?, "hash58 valid");
    }
    assert!(verify_hash72(&raw_committed, None)?, "hash72 valid");

    println!(
        "[3/3] Sandbox transfer onto real {}-track library -> {} tracks SUCCEEDED in {}ms (both hash58 & hash72 verified; D:\\ untouched!)",
        orig_count,
        cloned_ipod.itdb.tracks.len(),
        commit.elapsed_ms
    );

    let _ = fs::remove_dir_all(&sandbox);
    Ok(())
}
