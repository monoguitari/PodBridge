// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! iPod `ArtworkDB` parser/writer and RGB565 little-endian `.ithmb` thumbnail encoder.

pub mod error;
pub mod formats;
pub mod rgb565;

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
    use image::{DynamicImage, Rgb, RgbImage};

    use super::*;

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

        // Verify column 55 padding bytes (bytes 110..112 of each 112-byte row) in format 1061 are 0
        for row in 0..55 {
            let pad_off = row * 112 + 110;
            assert_eq!(&encoded[2].bytes[pad_off..pad_off + 2], &[0, 0]);
        }

        // Round-trip slot decode to PNG for ipod-art:// UI preview
        let png = decode_slot_to_png(&encoded[0].bytes, 128, 128, 256).unwrap();
        let decoded = decode_source_artwork(&png).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (128, 128));
        let sample = decoded.get_pixel(64, 64);
        assert!(sample[0] > 240 && sample[1] > 120 && sample[2] < 45);
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
