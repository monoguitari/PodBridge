// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// Target codec when dropping lossless non-native audio files (such as `FLAC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LosslessTarget {
    /// Apple Lossless (`.m4a` ALAC) — bit-perfect lossless, very fast CPU encode.
    Alac,
    /// Lossy AAC (`.m4a`) at `aac_bitrate_kbps` — saves storage on 80GB/120GB drives.
    Aac,
}

/// How PodBridge handles tracks that match an existing track in `DuplicateIndex`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DuplicatePolicy {
    /// Skip copying duplicate tracks (still linking existing `track_id` into imported playlists).
    Skip,
    /// Transfer a second copy alongside the existing track.
    KeepBoth,
}

/// User-configurable transfer & transcoding preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferSettings {
    pub lossless_target: LosslessTarget,
    pub aac_bitrate_kbps: u32,
    pub duplicate_policy: DuplicatePolicy,
}

impl Default for TransferSettings {
    fn default() -> Self {
        Self {
            lossless_target: LosslessTarget::Alac,
            aac_bitrate_kbps: 256,
            duplicate_policy: DuplicatePolicy::Skip,
        }
    }
}
