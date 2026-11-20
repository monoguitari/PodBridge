// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, RgbImage};

use crate::error::ArtworkError;
use crate::formats::IthmbFormatSpec;

/// Encoded `.ithmb` thumbnail slot together with its rendered dimensions and letterbox padding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedThumbnail {
    pub format_id: u32,
    pub width: u16,
    pub height: u16,
    pub horizontal_padding: i16,
    pub vertical_padding: i16,
    pub bytes: Vec<u8>,
}

/// Decodes compressed cover art bytes (`JPEG`, `PNG`, `WebP`, `BMP`) once so multiple `.ithmb`
/// resolutions can be generated from the same decoded image.
pub fn decode_source_artwork(image_bytes: &[u8]) -> Result<DynamicImage, ArtworkError> {
    Ok(image::load_from_memory(image_bytes)?)
}

/// Center-crops `src` to a square (if non-square) and resizes to `spec`, packing pixels into
/// RGB565 little-endian (`R[15:11] G[10:5] B[4:0]`) with exact `spec.row_stride_bytes` alignment.
#[must_use]
pub fn encode_dynamic_image_to_slot(src: &DynamicImage, spec: IthmbFormatSpec) -> EncodedThumbnail {
    let (src_w, src_h) = (src.width(), src.height());
    let side = src_w.min(src_h);
    let cropped = if src_w != src_h && side > 0 {
        let x = (src_w - side) / 2;
        let y = (src_h - side) / 2;
        src.crop_imm(x, y, side, side)
    } else {
        src.clone()
    };

    let resized = cropped
        .resize_exact(
            u32::from(spec.width),
            u32::from(spec.height),
            FilterType::Lanczos3,
        )
        .to_rgb8();

    let mut out = vec![0u8; spec.image_byte_size()];
    for y in 0..usize::from(spec.height) {
        let row_start = y * spec.row_stride_bytes;
        for x in 0..usize::from(spec.width) {
            let px = resized.get_pixel(x as u32, y as u32);
            let rgb565 = pack_rgb565_le(px[0], px[1], px[2]);
            let off = row_start + x * 2;
            out[off..off + 2].copy_from_slice(&rgb565.to_le_bytes());
        }
    }

    EncodedThumbnail {
        format_id: spec.format_id,
        width: spec.width,
        height: spec.height,
        horizontal_padding: 0,
        vertical_padding: 0,
        bytes: out,
    }
}

/// Encodes `src` across all target `specs` for a given iPod model.
#[must_use]
pub fn encode_all_formats(src: &DynamicImage, specs: &[IthmbFormatSpec]) -> Vec<EncodedThumbnail> {
    specs
        .iter()
        .map(|&spec| encode_dynamic_image_to_slot(src, spec))
        .collect()
}

/// Decodes a raw RGB565 little-endian `.ithmb` slot back into PNG bytes for zero-lag UI rendering
/// via the `ipod-art://` custom protocol.
pub fn decode_slot_to_png(
    slot_bytes: &[u8],
    width: u16,
    height: u16,
    row_stride_bytes: usize,
) -> Result<Vec<u8>, ArtworkError> {
    let expected = row_stride_bytes * usize::from(height);
    if slot_bytes.len() < expected || row_stride_bytes < usize::from(width) * 2 {
        return Err(ArtworkError::InvalidSlotSize {
            actual: slot_bytes.len(),
            expected,
        });
    }

    let mut img = RgbImage::new(u32::from(width), u32::from(height));
    for y in 0..usize::from(height) {
        let row_start = y * row_stride_bytes;
        for x in 0..usize::from(width) {
            let off = row_start + x * 2;
            let px16 = u16::from_le_bytes([slot_bytes[off], slot_bytes[off + 1]]);
            let (r, g, b) = unpack_rgb565(px16);
            img.put_pixel(x as u32, y as u32, image::Rgb([r, g, b]));
        }
    }

    let mut png_buf = Vec::new();
    DynamicImage::ImageRgb8(img).write_to(&mut Cursor::new(&mut png_buf), ImageFormat::Png)?;
    Ok(png_buf)
}

#[inline]
const fn pack_rgb565_le(r: u8, g: u8, b: u8) -> u16 {
    let r5 = (r as u16) >> 3;
    let g6 = (g as u16) >> 2;
    let b5 = (b as u16) >> 3;
    (r5 << 11) | (g6 << 5) | b5
}

#[inline]
const fn unpack_rgb565(px: u16) -> (u8, u8, u8) {
    let r5 = ((px >> 11) & 0x1F) as u8;
    let g6 = ((px >> 5) & 0x3F) as u8;
    let b5 = (px & 0x1F) as u8;
    let r8 = (r5 << 3) | (r5 >> 2);
    let g8 = (g6 << 2) | (g6 >> 4);
    let b8 = (b5 << 3) | (b5 >> 2);
    (r8, g8, b8)
}
