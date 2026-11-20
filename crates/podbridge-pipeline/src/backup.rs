// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use podbridge_artwork::parse_artworkdb;
use podbridge_device::ConnectedIpod;
use podbridge_itdb::parse_itunesdb;
use serde::{Deserialize, Serialize};

use crate::error::PipelineError;

/// Maximum number of rotating `iTunesDB` + `ArtworkDB` snapshots retained per iPod device.
pub const MAX_BACKUPS_PER_DEVICE: usize = 10;

/// Metadata stored alongside a timestamped `iTunesDB` + `ArtworkDB` backup snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub id: String,
    pub device_name: String,
    pub created_unix_ms: u64,
    pub track_count: usize,
    pub playlist_count: usize,
    pub itunesdb_bytes: u64,
}

fn sanitize_device_key(ipod: &ConnectedIpod) -> String {
    if let Some(ref guid) = ipod.summary.firewire_guid {
        guid.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
    } else {
        ipod.summary
            .id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect()
    }
}

fn device_backup_dir(backup_root: &Path, ipod: &ConnectedIpod) -> PathBuf {
    backup_root.join(sanitize_device_key(ipod))
}

/// Writes `bytes` to `<target>.tmp`, calls `sync_all()`, and atomically renames into `target`.
pub fn write_file_atomic(target: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_name = format!(
        ".{}.podbridge.tmp",
        target
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("database")
    );
    let tmp_path = target.with_file_name(tmp_name);

    {
        let mut f = File::create(&tmp_path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp_path, target)?;
    Ok(())
}

/// Snapshots the current on-disk `iTunesDB` and `ArtworkDB` of `ipod` into `backup_root` (rotating
/// older backups beyond `MAX_BACKUPS_PER_DEVICE`) and updates `iTunesDB.bak` on the iPod.
pub fn create_rotating_backup(
    ipod: &ConnectedIpod,
    backup_root: &Path,
) -> Result<Option<BackupManifest>, PipelineError> {
    let itdb_src = ipod.itunes_db_path();
    if !itdb_src.is_file() {
        return Ok(None);
    }

    let raw_itdb = fs::read(&itdb_src)?;

    // Keep iTunesDB.bak on the iPod updated to match Apple iTunes behavior
    let on_device_bak = itdb_src.with_extension("bak");
    let _ = write_file_atomic(&on_device_bak, &raw_itdb);

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let dev_dir = device_backup_dir(backup_root, ipod);
    let mut id = format!("{now_ms}");
    let mut snap_dir = dev_dir.join(&id);
    let mut suffix = 1u32;
    while snap_dir.exists() {
        id = format!("{now_ms}_{suffix}");
        snap_dir = dev_dir.join(&id);
        suffix += 1;
    }

    fs::create_dir_all(&snap_dir)?;
    write_file_atomic(&snap_dir.join("iTunesDB"), &raw_itdb)?;

    let art_src = ipod.artwork_db_path();
    if art_src.is_file() {
        if let Ok(raw_art) = fs::read(&art_src) {
            write_file_atomic(&snap_dir.join("ArtworkDB"), &raw_art)?;
        }
    }

    let manifest = BackupManifest {
        id,
        device_name: ipod.summary.name.clone(),
        created_unix_ms: now_ms,
        track_count: ipod.summary.track_count,
        playlist_count: ipod.summary.playlist_count,
        itunesdb_bytes: raw_itdb.len() as u64,
    };
    let manifest_json =
        serde_json::to_vec_pretty(&manifest).map_err(|e| std::io::Error::other(e.to_string()))?;
    write_file_atomic(&snap_dir.join("manifest.json"), &manifest_json)?;

    prune_old_backups(&dev_dir);
    Ok(Some(manifest))
}

fn prune_old_backups(dev_dir: &Path) {
    let Ok(rd) = fs::read_dir(dev_dir) else {
        return;
    };
    let mut snaps: Vec<(u64, PathBuf)> = rd
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let p = e.path();
            let raw = fs::read(p.join("manifest.json")).ok()?;
            let m: BackupManifest = serde_json::from_slice(&raw).ok()?;
            Some((m.created_unix_ms, p))
        })
        .collect();

    snaps.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (_, old_path) in snaps.into_iter().skip(MAX_BACKUPS_PER_DEVICE) {
        let _ = fs::remove_dir_all(old_path);
    }
}

/// Lists available backup snapshots for `ipod` in reverse chronological order (newest first).
#[must_use]
pub fn list_device_backups(ipod: &ConnectedIpod, backup_root: &Path) -> Vec<BackupManifest> {
    let dev_dir = device_backup_dir(backup_root, ipod);
    let Ok(rd) = fs::read_dir(&dev_dir) else {
        return Vec::new();
    };
    let mut list: Vec<BackupManifest> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let raw = fs::read(e.path().join("manifest.json")).ok()?;
            serde_json::from_slice(&raw).ok()
        })
        .collect();
    list.sort_by_key(|a| std::cmp::Reverse(a.created_unix_ms));
    list
}

/// Atomically restores `backup_id` onto `ipod` and reloads its in-memory databases.
pub fn restore_device_backup(
    ipod: &mut ConnectedIpod,
    backup_root: &Path,
    backup_id: &str,
) -> Result<(), PipelineError> {
    // Sanitize backup_id so path traversal is impossible
    if backup_id.is_empty()
        || backup_id.contains('/')
        || backup_id.contains('\\')
        || backup_id.contains("..")
    {
        return Err(PipelineError::BackupNotFound(backup_id.to_owned()));
    }

    let snap_dir = device_backup_dir(backup_root, ipod).join(backup_id);
    let snap_itdb = snap_dir.join("iTunesDB");
    if !snap_itdb.is_file() {
        return Err(PipelineError::BackupNotFound(backup_id.to_owned()));
    }

    let raw_itdb = fs::read(&snap_itdb)?;
    let parsed_itdb = parse_itunesdb(&raw_itdb)?;

    let snap_art = snap_dir.join("ArtworkDB");
    let parsed_art = if snap_art.is_file() {
        let raw_art = fs::read(&snap_art)?;
        let adb = parse_artworkdb(&raw_art)?;
        write_file_atomic(&ipod.artwork_db_path(), &raw_art)?;
        Some(adb)
    } else {
        None
    };

    write_file_atomic(&ipod.itunes_db_path(), &raw_itdb)?;
    ipod.itdb = parsed_itdb;
    if let Some(adb) = parsed_art {
        ipod.artwork_db = adb;
    }
    ipod.refresh_summary_metrics();
    Ok(())
}
