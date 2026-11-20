// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use podbridge_artwork::ArtworkFamily;
use serde::{Deserialize, Serialize};

/// Supported stock-firmware iPod hardware generations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IpodGeneration {
    Video5G,
    Classic6G,
    Classic6_5G,
    Classic7G,
    Nano3G,
    Nano4G,
    Nano5G,
}

impl IpodGeneration {
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Video5G => "iPod Video (5G)",
            Self::Classic6G => "iPod Classic (6G)",
            Self::Classic6_5G => "iPod Classic (6.5G)",
            Self::Classic7G => "iPod Classic (7G)",
            Self::Nano3G => "iPod Nano (3G)",
            Self::Nano4G => "iPod Nano (4G)",
            Self::Nano5G => "iPod Nano (5G)",
        }
    }

    #[must_use]
    pub const fn artwork_family(self) -> ArtworkFamily {
        match self {
            Self::Video5G => ArtworkFamily::Video5G,
            Self::Classic6G | Self::Classic6_5G | Self::Classic7G | Self::Nano3G => {
                ArtworkFamily::Classic6G
            }
            Self::Nano4G => ArtworkFamily::Nano4G,
            Self::Nano5G => ArtworkFamily::Nano5G,
        }
    }

    /// Number of `F00..F{N-1}` subdirectories under `iPod_Control/Music/`.
    #[must_use]
    pub const fn music_dir_count(self) -> u32 {
        match self {
            Self::Video5G | Self::Classic6G | Self::Classic6_5G | Self::Classic7G => 50,
            Self::Nano3G | Self::Nano4G | Self::Nano5G => 20,
        }
    }

    #[must_use]
    pub const fn default_usb_pid(self) -> u16 {
        match self {
            Self::Video5G => 0x1209,
            Self::Classic6G | Self::Classic6_5G | Self::Classic7G => 0x1261,
            Self::Nano3G => 0x1262,
            Self::Nano4G => 0x1263,
            Self::Nano5G => 0x1265,
        }
    }

    #[must_use]
    pub const fn model_code_hint(self) -> &'static str {
        match self {
            Self::Video5G => "MA002",
            Self::Classic6G => "MB147",
            Self::Classic6_5G => "MB565",
            Self::Classic7G => "MC293",
            Self::Nano3G => "MB261",
            Self::Nano4G => "MB903",
            Self::Nano5G => "MC060",
        }
    }

    /// Infers the iPod generation from its Apple USB Product ID (`VID_05AC`) and optional
    /// `ModelNumStr` from `SysInfo` / `SysInfoExtended`.
    #[must_use]
    pub fn detect(usb_pid: Option<u16>, model_num: Option<&str>) -> Self {
        if let Some(code) = model_num {
            let up = code.trim().to_ascii_uppercase();
            // Strip leading 'x'/'M' revision char when matching Apple 4-digit part stems
            if up.contains("147") || up.contains("150") || up.contains("029") || up.contains("145")
            {
                return Self::Classic6G;
            }
            if up.contains("562") || up.contains("565") {
                return Self::Classic6_5G;
            }
            if up.contains("293") || up.contains("297") {
                return Self::Classic7G;
            }
            if up.contains("002") || up.contains("003") || up.contains("444") || up.contains("446")
            {
                return Self::Video5G;
            }
            if up.contains("249") || up.contains("261") {
                return Self::Nano3G;
            }
            if up.contains("598") || up.contains("903") || up.contains("754") {
                return Self::Nano4G;
            }
            if up.contains("027") || up.contains("060") || up.contains("031") {
                return Self::Nano5G;
            }
        }

        match usb_pid {
            Some(0x1209) => Self::Video5G,
            Some(0x1262) => Self::Nano3G,
            Some(0x1263) => Self::Nano4G,
            Some(0x1265) => Self::Nano5G,
            _ => Self::Classic6G,
        }
    }
}
