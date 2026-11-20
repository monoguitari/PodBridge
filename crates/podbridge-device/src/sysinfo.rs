// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use podbridge_crypto::{FirewireGuid, Hash72Info, HashingScheme};

use crate::error::DeviceError;
use crate::model::IpodGeneration;

/// Parsed metadata extracted from `iPod_Control/Device/SysInfo` and `SysInfoExtended`.
#[derive(Debug, Default, Clone)]
pub struct SysInfoMetadata {
    pub firewire_guid: Option<FirewireGuid>,
    pub model_num_str: Option<String>,
    pub serial_number: Option<String>,
}

/// Reads `iPod_Control/Device/SysInfo` and optional `SysInfoExtended` XML plist under `ipod_root`.
#[must_use]
pub fn read_sysinfo_metadata(ipod_root: &Path) -> SysInfoMetadata {
    let mut meta = SysInfoMetadata::default();
    let device_dir = ipod_root.join("iPod_Control").join("Device");

    // 1. Parse plain-text key: value lines in SysInfo
    let sysinfo_path = device_dir.join("SysInfo");
    if let Ok(text) = fs::read_to_string(&sysinfo_path) {
        for line in text.lines() {
            let Some((raw_k, raw_v)) = line.split_once(':') else {
                continue;
            };
            let k = raw_k.trim();
            let v = raw_v.trim();
            if k.eq_ignore_ascii_case("FirewireGuid") {
                if let Ok(g) = FirewireGuid::parse_hex(v) {
                    meta.firewire_guid = Some(g);
                }
            } else if k.eq_ignore_ascii_case("ModelNumStr") && !v.is_empty() {
                meta.model_num_str = Some(v.to_owned());
            } else if k.eq_ignore_ascii_case("pszSerialNumber") && !v.is_empty() {
                meta.serial_number = Some(v.to_owned());
            }
        }
    }

    // 2. Parse XML plist SysInfoExtended if present
    let ext_path = device_dir.join("SysInfoExtended");
    if let Ok(plist::Value::Dictionary(dict)) = plist::Value::from_file(&ext_path) {
        if meta.firewire_guid.is_none() {
            if let Some(s) = dict.get("FireWireGUID").and_then(plist::Value::as_string) {
                if let Ok(g) = FirewireGuid::parse_hex(s) {
                    meta.firewire_guid = Some(g);
                }
            }
        }
        if meta.serial_number.is_none() {
            if let Some(s) = dict.get("SerialNumber").and_then(plist::Value::as_string) {
                meta.serial_number = Some(s.to_owned());
            }
        }
    }

    meta
}

/// Resolves the [`HashingScheme`] required to sign `iTunesDB` on `ipod_root`.
pub fn resolve_hashing_scheme(
    ipod_root: &Path,
    generation: IpodGeneration,
    guid: Option<FirewireGuid>,
    existing_itunesdb_bytes: Option<&[u8]>,
) -> Result<HashingScheme, DeviceError> {
    match generation {
        IpodGeneration::Video5G => Ok(HashingScheme::None),
        IpodGeneration::Classic6G
        | IpodGeneration::Classic6_5G
        | IpodGeneration::Classic7G
        | IpodGeneration::Nano3G
        | IpodGeneration::Nano4G => {
            let fw_guid = guid.ok_or(DeviceError::MissingFirewireGuid {
                model: generation.display_name(),
            })?;
            let companion_hash72 =
                load_hash72_seed(ipod_root, Some(fw_guid), existing_itunesdb_bytes);
            Ok(HashingScheme::Hash58 {
                guid: fw_guid,
                companion_hash72,
            })
        }
        IpodGeneration::Nano5G => {
            let info = load_hash72_seed(ipod_root, guid, existing_itunesdb_bytes)
                .ok_or(DeviceError::MissingNano5GHashSeed)?;
            Ok(HashingScheme::Hash72 {
                info,
                companion_guid: guid,
            })
        }
    }
}

fn load_hash72_seed(
    ipod_root: &Path,
    guid: Option<FirewireGuid>,
    existing_itunesdb_bytes: Option<&[u8]>,
) -> Option<Hash72Info> {
    let candidates = [
        ipod_root
            .join("iPod_Control")
            .join("Device")
            .join("HashInfo"),
        ipod_root
            .join("iPod_Control")
            .join("iTunes")
            .join("HashInfo"),
    ];
    for p in &candidates {
        if let Ok(raw) = fs::read(p) {
            if let Ok(info) = Hash72Info::from_hash_info_bytes(&raw) {
                return Some(info);
            }
        }
    }
    if let Some(db_bytes) = existing_itunesdb_bytes {
        if let Ok(info) = Hash72Info::extract_from_itunesdb(db_bytes, guid) {
            return Some(info);
        }
    }
    None
}
