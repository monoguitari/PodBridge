// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! iPod `ArtworkDB` parser/writer and RGB565 little-endian `.ithmb` thumbnail encoder.

pub mod artwork_db;
pub mod error;
pub mod formats;
pub mod rgb565;

pub use artwork_db::{
    flush_pending_ithmb_writes, parse_artworkdb, serialize_artworkdb, ArtworkDatabase,
    ArtworkFormatFileEntry, ArtworkItem, ArtworkThumbnailRef, PendingIthmbWrite, RawArtworkMhod,
    DEFAULT_MHFD_HEADER_LEN, DEFAULT_MHIF_HEADER_LEN, DEFAULT_MHII_ART_HEADER_LEN,
    DEFAULT_MHNI_HEADER_LEN, MAX_ITHMB_FILE_BYTES,
};
pub use error::ArtworkError;
pub use formats::{
    known_dimensions_for_format_id, reconcile_specs_with_mhif, ArtworkFamily, IthmbFormatSpec,
    CLASSIC_6G_SPECS, NANO_4G_SPECS, NANO_5G_SPECS, VIDEO_5G_SPECS,
};
pub use rgb565::{
    decode_slot_to_png, decode_source_artwork, encode_all_formats, encode_dynamic_image_to_slot,
    EncodedThumbnail,
};

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

    use super::*;

    fn make_png(color: [u8; 3]) -> Vec<u8> {
        let mut img = RgbImage::new(64, 64);
        for px in img.pixels_mut() {
            *px = Rgb(color);
        }
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn classic_6g_specs_match_mb147ll_exact_slot_sizes_and_1061_row_padding() {
        let specs = ArtworkFamily::Classic6G.default_specs();
        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].format_id, 1055);
        assert_eq!(specs[0].image_byte_size(), 32_768);
        assert_eq!(specs[1].format_id, 1060);
        assert_eq!(specs[1].image_byte_size(), 204_800);
        assert_eq!(specs[2].format_id, 1061);
        assert_eq!(specs[2].image_byte_size(), 6_160);

        let mut img = RgbImage::new(200, 160);
        for px in img.pixels_mut() {
            *px = Rgb([248, 128, 32]);
        }
        let dyn_img = DynamicImage::ImageRgb8(img);
        let encoded = encode_all_formats(&dyn_img, specs);

        assert_eq!(encoded[0].bytes.len(), 32_768);
        assert_eq!(encoded[1].bytes.len(), 204_800);
        assert_eq!(encoded[2].bytes.len(), 6_160);

        for row in 0..55 {
            let pad_off = row * 112 + 110;
            assert_eq!(&encoded[2].bytes[pad_off..pad_off + 2], &[0, 0]);
        }

        let png = decode_slot_to_png(&encoded[0].bytes, 128, 128, 256).unwrap();
        let decoded = decode_source_artwork(&png).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (128, 128));
    }

    #[test]
    fn deduplicates_by_sha256_reuses_freed_slots_and_round_trips_artworkdb() {
        let specs = ArtworkFamily::Classic6G.default_specs();
        let mut adb = ArtworkDatabase::new_empty(specs);

        let cover_red = make_png([240, 20, 20]);
        let cover_blue = make_png([20, 40, 240]);

        // Track 1 registers cover_red -> mhii_id = 100 at offset 0
        let (id1, writes1) = adb
            .register_artwork_bytes(&cover_red, 0xAAAA_0001, specs)
            .unwrap();
        assert_eq!(id1, 100);
        assert_eq!(writes1.len(), 3);
        assert_eq!(writes1[0].offset, 0);

        // Track 2 on same album registers identical cover_red -> deduplicated! 0 new writes, ref_count=2
        let (id1_dup, writes_dup) = adb
            .register_artwork_bytes(&cover_red, 0xAAAA_0002, specs)
            .unwrap();
        assert_eq!(id1_dup, 100);
        assert!(writes_dup.is_empty());
        assert_eq!(adb.images[0].ref_count, 2);

        // Track 3 registers cover_blue -> mhii_id = 101 at offset 32768 / 204800 / 6160
        let (id2, writes2) = adb
            .register_artwork_bytes(&cover_blue, 0xBBBB_0001, specs)
            .unwrap();
        assert_eq!(id2, 101);
        assert_eq!(writes2[0].offset, 32_768);

        // Release both refs on id 100 -> slot 0 becomes free and reused by next cover!
        adb.release_artwork_ref(100);
        assert_eq!(adb.images.len(), 2); // ref_count dropped 2 -> 1
        adb.release_artwork_ref(100);
        assert_eq!(adb.images.len(), 1); // removed!

        let cover_green = make_png([20, 220, 40]);
        let (id3, writes3) = adb
            .register_artwork_bytes(&cover_green, 0xCCCC_0001, specs)
            .unwrap();
        assert_eq!(id3, 102);
        assert_eq!(writes3[0].offset, 0); // Slot 0 reused!

        // Round-trip binary ArtworkDB + flush .ithmb to temp dir + read_thumbnail_png
        let dir = std::env::temp_dir().join(format!("podbridge_art_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        flush_pending_ithmb_writes(&dir, &writes2).unwrap();
        flush_pending_ithmb_writes(&dir, &writes3).unwrap();

        let raw_db = serialize_artworkdb(&adb);
        let reparsed = parse_artworkdb(&raw_db).unwrap();
        assert_eq!(reparsed.version, 6);
        assert_eq!(reparsed.images.len(), 2);
        assert_eq!(reparsed.format_files.len(), 3);

        let preview_png = reparsed
            .read_thumbnail_png(&dir, id3, 100)
            .unwrap()
            .expect("thumbnail exists");
        let preview_img = decode_source_artwork(&preview_png).unwrap();
        assert_eq!(preview_img.width(), 128);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reconciles_mhif_stride_overrides_cleanly() {
        let reconciled = reconcile_specs_with_mhif(
            CLASSIC_6G_SPECS,
            &[(1055, 32_768), (1060, 204_800), (1061, 6_160)],
        );
        assert_eq!(reconciled[2].row_stride_bytes, 112);
        assert_eq!(reconciled[2].image_byte_size(), 6_160);
    }
}
