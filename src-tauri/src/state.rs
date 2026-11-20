// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use podbridge_device::{discover_connected_ipods, ConnectedIpod};
use tokio_util::sync::CancellationToken;

use crate::dto::AppSettings;

/// Shared application state managed by Tauri.
pub struct AppState {
    pub app_data_dir: PathBuf,
    pub devices: Mutex<HashMap<String, ConnectedIpod>>,
    pub selected_device_id: Mutex<Option<String>>,
    pub active_cancel: Mutex<Option<CancellationToken>>,
    pub settings: Mutex<AppSettings>,
    /// In-memory decoded PNG thumbnail cache (`(device_id, mhii_id, size_bucket)` -> PNG bytes).
    pub art_png_cache: Mutex<HashMap<(String, u32, u16), Vec<u8>>>,
}

impl AppState {
    #[must_use]
    pub fn new(app_data_dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&app_data_dir);
        let settings_path = app_data_dir.join("settings.json");
        let settings = fs::read(&settings_path)
            .ok()
            .and_then(|raw| serde_json::from_slice::<AppSettings>(&raw).ok())
            .unwrap_or_default();

        let mut map = HashMap::new();
        let mut first_id = None;
        for ipod in discover_connected_ipods() {
            if first_id.is_none() {
                first_id = Some(ipod.summary.id.clone());
            }
            map.insert(ipod.summary.id.clone(), ipod);
        }

        Self {
            app_data_dir,
            devices: Mutex::new(map),
            selected_device_id: Mutex::new(first_id),
            active_cancel: Mutex::new(None),
            settings: Mutex::new(settings),
            art_png_cache: Mutex::new(HashMap::new()),
        }
    }

    #[must_use]
    pub fn backup_root(&self) -> PathBuf {
        self.app_data_dir.join("backups")
    }

    pub fn save_settings(&self, new_settings: AppSettings) -> Result<(), String> {
        let raw = serde_json::to_vec_pretty(&new_settings).map_err(|e| e.to_string())?;
        let path = self.app_data_dir.join("settings.json");
        fs::write(path, raw).map_err(|e| e.to_string())?;
        let mut guard = self
            .settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = new_settings;
        Ok(())
    }

    /// Re-scans physical USB drives while preserving any opened Mock iPod sessions.
    pub fn refresh_usb_devices(&self) {
        let fresh_usb = discover_connected_ipods();
        let mut devices = self
            .devices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // Remove stale USB devices whose root path is no longer mounted
        let fresh_ids: std::collections::HashSet<String> =
            fresh_usb.iter().map(|d| d.summary.id.clone()).collect();
        devices.retain(|id, dev| dev.summary.is_mock || fresh_ids.contains(id));

        for ipod in fresh_usb {
            devices.entry(ipod.summary.id.clone()).or_insert(ipod);
        }

        let mut sel = self
            .selected_device_id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if sel.as_ref().is_some_and(|id| !devices.contains_key(id)) {
            *sel = None;
        }
        if sel.is_none() {
            // Prefer physical USB iPods first, then Mock iPods
            *sel = devices
                .values()
                .find(|d| !d.summary.is_mock)
                .or_else(|| devices.values().next())
                .map(|d| d.summary.id.clone());
        }
    }

    /// Resolves a PNG thumbnail for `(device_id, mhii_id, min_width)` with in-memory caching.
    pub fn get_or_decode_artwork_png(
        &self,
        device_id: &str,
        mhii_id: u32,
        min_width: u16,
    ) -> Option<Vec<u8>> {
        if mhii_id == 0 {
            return None;
        }
        let bucket = if min_width > 140 { 320 } else { 128 };
        let key = (device_id.to_owned(), mhii_id, bucket);

        if let Some(cached) = self
            .art_png_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            .cloned()
        {
            return Some(cached);
        }

        let (art_dir, art_db) = {
            let guard = self
                .devices
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let dev = guard.get(device_id)?;
            (dev.artwork_dir(), dev.artwork_db.clone())
        };

        let png = art_db
            .read_thumbnail_png(&art_dir, mhii_id, bucket)
            .ok()??;
        let mut cache = self
            .art_png_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.len() > 2_000 {
            cache.clear();
        }
        cache.insert(key, png.clone());
        Some(png)
    }

    pub fn invalidate_device_art_cache(&self, device_id: &str) {
        let mut cache = self
            .art_png_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.retain(|(dev_id, _, _), _| dev_id != device_id);
    }
}

/// Helper returning mounted `<Drive>:\iPod_Control` roots on Windows for zero-disk-churn hotplug
/// change detection.
#[must_use]
pub fn current_mounted_ipod_roots_fingerprint() -> Vec<PathBuf> {
    podbridge_device::enumerate_connected_usb_ipods()
        .into_iter()
        .map(|p| p.drive_root)
        .collect()
}

pub fn parse_hex_u64(hex: &str) -> Result<u64, String> {
    let clean = hex.trim().trim_start_matches("0x").trim_start_matches("0X");
    u64::from_str_radix(clean, 16).map_err(|_| format!("invalid persistent ID '{hex}'"))
}

pub fn ensure_parent_exists(p: &Path) -> Result<(), String> {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    Ok(())
}
