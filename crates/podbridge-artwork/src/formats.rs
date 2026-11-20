// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// iPod artwork generation family determining which `.ithmb` correlation IDs and dimensions the
/// stock firmware expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArtworkFamily {
    /// iPod 5G / 5.5G Video (`1028`, `1029`).
    Video5G,
    /// iPod Classic 6G (`MB147LL`), 6.5G, 7G, and iPod Nano 3G (`1055`, `1060`, `1061`).
    Classic6G,
    /// iPod Nano 4G (`1055`, `1071`, `1074`, `1078`).
    Nano4G,
    /// iPod Nano 5G (`1056`, `1073`, `1074`, `1078`).
    Nano5G,
}

/// Exact specification for a single `.ithmb` RGB565 little-endian thumbnail format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IthmbFormatSpec {
    pub format_id: u32,
    pub width: u16,
    pub height: u16,
    /// Byte length of each scanline in the `.ithmb` slot (includes trailing row alignment padding,
    /// e.g. `112` bytes/row for `55px` format `1061` -> `6,160` bytes total).
    pub row_stride_bytes: usize,
}

impl IthmbFormatSpec {
    #[must_use]
    pub const fn new(format_id: u32, width: u16, height: u16, row_stride_bytes: usize) -> Self {
        Self {
            format_id,
            width,
            height,
            row_stride_bytes,
        }
    }

    /// Total byte size of one packed thumbnail slot inside `F{format_id}_N.ithmb`.
    #[must_use]
    pub const fn image_byte_size(&self) -> usize {
        self.row_stride_bytes * (self.height as usize)
    }
}

pub const CLASSIC_6G_SPECS: &[IthmbFormatSpec] = &[
    IthmbFormatSpec::new(1055, 128, 128, 256), // 32,768 bytes
    IthmbFormatSpec::new(1060, 320, 320, 640), // 204,800 bytes
    IthmbFormatSpec::new(1061, 55, 55, 112),   // 6,160 bytes (16B row-aligned: 56*2 * 55)
];

pub const VIDEO_5G_SPECS: &[IthmbFormatSpec] = &[
    IthmbFormatSpec::new(1028, 100, 100, 200), // 20,000 bytes
    IthmbFormatSpec::new(1029, 200, 200, 400), // 80,000 bytes
];

pub const NANO_4G_SPECS: &[IthmbFormatSpec] = &[
    IthmbFormatSpec::new(1055, 128, 128, 256), // 32,768 bytes
    IthmbFormatSpec::new(1071, 240, 240, 480), // 115,200 bytes
    IthmbFormatSpec::new(1074, 50, 50, 100),   // 5,000 bytes
    IthmbFormatSpec::new(1078, 80, 80, 160),   // 12,800 bytes
];

pub const NANO_5G_SPECS: &[IthmbFormatSpec] = &[
    IthmbFormatSpec::new(1056, 128, 128, 256), // 32,768 bytes
    IthmbFormatSpec::new(1073, 240, 240, 480), // 115,200 bytes
    IthmbFormatSpec::new(1074, 50, 50, 100),   // 5,000 bytes
    IthmbFormatSpec::new(1078, 80, 80, 160),   // 12,800 bytes
];

impl ArtworkFamily {
    /// Returns the default `.ithmb` format specifications for this hardware family.
    #[must_use]
    pub const fn default_specs(self) -> &'static [IthmbFormatSpec] {
        match self {
            Self::Video5G => VIDEO_5G_SPECS,
            Self::Classic6G => CLASSIC_6G_SPECS,
            Self::Nano4G => NANO_4G_SPECS,
            Self::Nano5G => NANO_5G_SPECS,
        }
    }
}

/// Known dimensions lookup for common iPod `.ithmb` correlation IDs when reconciling against an
/// on-disk `ArtworkDB` `mhif` list.
#[must_use]
pub fn known_dimensions_for_format_id(format_id: u32) -> Option<(u16, u16)> {
    match format_id {
        1028 => Some((100, 100)),
        1029 => Some((200, 200)),
        1055 | 1056 => Some((128, 128)),
        1060 => Some((320, 320)),
        1061 => Some((55, 55)),
        1066 => Some((64, 64)),
        1071 | 1073 => Some((240, 240)),
        1074 => Some((50, 50)),
        1078 => Some((80, 80)),
        _ => None,
    }
}

/// Reconciles default `IthmbFormatSpec` entries against `(format_id, mhif_img_size)` pairs read
/// from an existing `ArtworkDB` so PodBridge always matches the exact stride already on disk.
#[must_use]
pub fn reconcile_specs_with_mhif(
    defaults: &[IthmbFormatSpec],
    mhif_entries: &[(u32, u32)],
) -> Vec<IthmbFormatSpec> {
    if mhif_entries.is_empty() {
        return defaults.to_vec();
    }

    let mut out = Vec::with_capacity(mhif_entries.len());
    for &(fid, img_size) in mhif_entries {
        if let Some(def) = defaults.iter().find(|d| d.format_id == fid) {
            let h = usize::from(def.height.max(1));
            let stride = if (img_size as usize).is_multiple_of(h)
                && img_size as usize >= usize::from(def.width) * 2 * h
            {
                (img_size as usize) / h
            } else {
                def.row_stride_bytes
            };
            out.push(IthmbFormatSpec::new(fid, def.width, def.height, stride));
        } else if let Some((w, h)) = known_dimensions_for_format_id(fid) {
            let h_usize = usize::from(h.max(1));
            let stride = if (img_size as usize).is_multiple_of(h_usize) {
                (img_size as usize) / h_usize
            } else {
                usize::from(w) * 2
            };
            out.push(IthmbFormatSpec::new(fid, w, h, stride));
        }
    }

    if out.is_empty() {
        defaults.to_vec()
    } else {
        out
    }
}
