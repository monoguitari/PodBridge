// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use podbridge_artwork::{parse_artworkdb, serialize_artworkdb, ArtworkDatabase};
use podbridge_crypto::{FirewireGuid, Hash72Info, HashingScheme};
use podbridge_itdb::{parse_itunesdb, serialize_itunesdb, unix_to_mac_timestamp, ItdbDatabase};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::error::DeviceError;
use crate::model::IpodGeneration;
use crate::sysinfo::{read_sysinfo_metadata, resolve_hashing_scheme};
use crate::windows_usb::query_volume_space;

/// Segmented storage bar breakdown in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageBreakdown {
    pub total_bytes: u64,
    pub music_bytes: u64,
    pub artwork_bytes: u64,
    pub other_bytes: u64,
    pub free_bytes: u64,
}

/// Summary descriptor for a connected physical USB iPod or opened Mock iPod directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpodDeviceSummary {
    pub id: String,
    pub name: String,
    pub root_path: PathBuf,
    pub generation: IpodGeneration,
    pub generation_label: String,
    pub firewire_guid: Option<String>,
    pub is_mock: bool,
    pub can_eject: bool,
    pub track_count: usize,
    pub playlist_count: usize,
    pub storage: StorageBreakdown,
}

/// Loaded iPod handle bundling device metadata, signing configuration, `ItdbDatabase`, and
/// `ArtworkDatabase`.
#[derive(Debug, Clone)]
pub struct ConnectedIpod {
    pub summary: IpodDeviceSummary,
    pub hashing_scheme: HashingScheme,
    pub itdb: ItdbDatabase,
    pub artwork_db: ArtworkDatabase,
}

impl ConnectedIpod {
    #[must_use]
    pub fn itunes_db_path(&self) -> PathBuf {
        self.summary
            .root_path
            .join("iPod_Control")
            .join("iTunes")
            .join("iTunesDB")
    }

    #[must_use]
    pub fn artwork_dir(&self) -> PathBuf {
        self.summary.root_path.join("iPod_Control").join("Artwork")
    }

    #[must_use]
    pub fn artwork_db_path(&self) -> PathBuf {
        self.artwork_dir().join("ArtworkDB")
    }

    #[must_use]
    pub fn music_dir(&self) -> PathBuf {
        self.summary.root_path.join("iPod_Control").join("Music")
    }

    /// Recomputes `StorageBreakdown` and track/playlist counts after an in-memory mutation.
    pub fn refresh_summary_metrics(&mut self) {
        let (total_bytes, free_bytes) = if self.summary.is_mock {
            let music_sum: u64 = self.itdb.tracks.iter().map(|t| u64::from(t.size)).sum();
            let art_bytes = compute_artwork_dir_bytes(&self.artwork_dir());
            let total = 80_000_000_000u64;
            let used = music_sum + art_bytes + 64_000_000;
            (total, total.saturating_sub(used))
        } else {
            query_volume_space(&self.summary.root_path)
        };

        let music_bytes: u64 = self.itdb.tracks.iter().map(|t| u64::from(t.size)).sum();
        let artwork_bytes = compute_artwork_dir_bytes(&self.artwork_dir());
        let used = total_bytes.saturating_sub(free_bytes);
        let other_bytes = used.saturating_sub(music_bytes.saturating_add(artwork_bytes));

        self.summary.track_count = self.itdb.tracks.len();
        self.summary.playlist_count = self.itdb.playlists.iter().filter(|p| !p.is_master).count();
        self.summary.storage = StorageBreakdown {
            total_bytes,
            music_bytes,
            artwork_bytes,
            other_bytes,
            free_bytes,
        };
    }
}

fn compute_artwork_dir_bytes(art_dir: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(art_dir) else {
        return 0;
    };
    rd.filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// Initializes a brand-new Mock iPod directory on disk with valid `SysInfo`, `F00..F{N-1}` music
/// folders, signed empty `iTunesDB`, and empty `ArtworkDB`.
pub fn create_mock_ipod(
    root: &Path,
    device_name: &str,
    generation: IpodGeneration,
    guid: FirewireGuid,
) -> Result<ConnectedIpod, DeviceError> {
    let control = root.join("iPod_Control");
    let dev_dir = control.join("Device");
    let itunes_dir = control.join("iTunes");
    let music_dir = control.join("Music");
    let art_dir = control.join("Artwork");

    fs::create_dir_all(&dev_dir)?;
    fs::create_dir_all(&itunes_dir)?;
    fs::create_dir_all(&art_dir)?;
    for idx in 0..generation.music_dir_count() {
        fs::create_dir_all(music_dir.join(format!("F{idx:02}")))?;
    }

    let sysinfo_content = format!(
        "ModelNumStr: {}\nFirewireGuid: 0x{}\n",
        generation.model_code_hint(),
        guid.to_hex_string()
    );
    fs::write(dev_dir.join("SysInfo"), sysinfo_content)?;

    // For Nano 5G mock devices, provision a valid 54-byte HashInfo file automatically
    if generation == IpodGeneration::Nano5G {
        let mut rng = rand::thread_rng();
        let mut uuid = [0u8; 20];
        uuid[0..8].copy_from_slice(guid.as_bytes());
        let info = Hash72Info {
            uuid,
            rndpart: rng.gen(),
            iv: rng.gen(),
        };
        fs::write(dev_dir.join("HashInfo"), info.to_hash_info_bytes())?;
    }

    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let now_mac = unix_to_mac_timestamp(now_unix);

    let scheme = resolve_hashing_scheme(root, generation, Some(guid), None)?;
    let mut itdb = ItdbDatabase::new_empty(device_name, now_mac);
    let itdb_bytes = serialize_itunesdb(&mut itdb, &scheme)?;
    fs::write(itunes_dir.join("iTunesDB"), &itdb_bytes)?;

    let specs = generation.artwork_family().default_specs();
    let adb = ArtworkDatabase::new_empty(specs);
    fs::write(art_dir.join("ArtworkDB"), serialize_artworkdb(&adb))?;

    open_ipod_at_path(
        root,
        Some((generation.default_usb_pid(), Some(guid))),
        true,
        None,
    )
}

/// Opens an existing physical or Mock iPod root path, parsing `iTunesDB` and `ArtworkDB`.
pub fn open_ipod_at_path(
    root: &Path,
    usb_hint: Option<(u16, Option<FirewireGuid>)>,
    is_mock: bool,
    volume_label_hint: Option<&str>,
) -> Result<ConnectedIpod, DeviceError> {
    let control = root.join("iPod_Control");
    if !control.is_dir() {
        return Err(DeviceError::NotAnIpodRoot(
            root.to_string_lossy().into_owned(),
        ));
    }

    let sysinfo = read_sysinfo_metadata(root);
    let usb_pid = usb_hint.map(|(pid, _)| pid);
    let usb_guid = usb_hint.and_then(|(_, g)| g);
    let effective_guid = usb_guid.or(sysinfo.firewire_guid);

    let generation = IpodGeneration::detect(usb_pid, sysinfo.model_num_str.as_deref());

    let itdb_path = control.join("iTunes").join("iTunesDB");
    let existing_itdb_bytes = fs::read(&itdb_path).ok();
    let hashing_scheme = resolve_hashing_scheme(
        root,
        generation,
        effective_guid,
        existing_itdb_bytes.as_deref(),
    )?;

    let itdb = if let Some(ref raw) = existing_itdb_bytes {
        parse_itunesdb(raw)?
    } else {
        let fallback_name = volume_label_hint.unwrap_or("iPod");
        ItdbDatabase::new_empty(fallback_name, 0)
    };

    let art_db_path = control.join("Artwork").join("ArtworkDB");
    let artwork_db = if art_db_path.is_file() {
        let raw_art = fs::read(&art_db_path)?;
        parse_artworkdb(&raw_art)?
    } else {
        ArtworkDatabase::new_empty(generation.artwork_family().default_specs())
    };

    let master_name = itdb
        .playlists
        .first()
        .filter(|p| p.is_master && !p.name.trim().is_empty())
        .map(|p| p.name.clone());
    let name = master_name
        .or_else(|| volume_label_hint.map(ToOwned::to_owned))
        .unwrap_or_else(|| generation.display_name().to_owned());

    let id = format!(
        "{}:{}",
        if is_mock { "mock" } else { "usb" },
        root.to_string_lossy()
    );

    let mut connected = ConnectedIpod {
        summary: IpodDeviceSummary {
            id,
            name,
            root_path: root.to_path_buf(),
            generation,
            generation_label: generation.display_name().to_owned(),
            firewire_guid: effective_guid.map(|g| format!("0x{}", g.to_hex_string())),
            is_mock,
            can_eject: !is_mock && cfg!(windows),
            track_count: 0,
            playlist_count: 0,
            storage: StorageBreakdown {
                total_bytes: 0,
                music_bytes: 0,
                artwork_bytes: 0,
                other_bytes: 0,
                free_bytes: 0,
            },
        },
        hashing_scheme,
        itdb,
        artwork_db,
    };
    connected.refresh_summary_metrics();
    Ok(connected)
}
