// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use podbridge_crypto::FirewireGuid;
use podbridge_device::{create_mock_ipod, eject_usb_ipod, open_ipod_at_path, IpodGeneration};
use podbridge_itdb::unix_to_mac_timestamp;
use podbridge_pipeline::{
    commit_database_atomic, delete_tracks_atomic, execute_transfer_batch, list_device_backups,
    restore_device_backup, BackupManifest, BatchCommitSummary,
};
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;

use crate::dto::{build_library_snapshot, AppSettings, DevicesPayload, LibrarySnapshot};
use crate::state::{ensure_parent_exists, parse_hex_u64, AppState};

fn now_mac() -> u32 {
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    unix_to_mac_timestamp(unix)
}

#[tauri::command]
pub fn list_devices(state: State<'_, AppState>) -> Result<DevicesPayload, String> {
    state.refresh_usb_devices();
    let guard = state
        .devices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut devices: Vec<_> = guard.values().map(|d| d.summary.clone()).collect();
    devices.sort_by(|a, b| {
        a.is_mock
            .cmp(&b.is_mock)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let selected = state
        .selected_device_id
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    Ok(DevicesPayload {
        devices,
        selected_device_id: selected,
    })
}

#[tauri::command]
pub fn select_device(
    device_id: String,
    state: State<'_, AppState>,
) -> Result<LibrarySnapshot, String> {
    let guard = state
        .devices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ipod = guard
        .get(&device_id)
        .ok_or_else(|| format!("device '{device_id}' is not connected"))?;
    let snap = build_library_snapshot(ipod);
    drop(guard);

    *state
        .selected_device_id
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(device_id);
    Ok(snap)
}

#[tauri::command]
pub fn get_library_snapshot(
    device_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Option<LibrarySnapshot>, String> {
    let target_id = device_id.or_else(|| {
        state
            .selected_device_id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    });
    let Some(id) = target_id else {
        return Ok(None);
    };
    let guard = state
        .devices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(guard.get(&id).map(build_library_snapshot))
}

#[tauri::command]
pub fn create_mock_device(
    path: String,
    name: String,
    generation: IpodGeneration,
    state: State<'_, AppState>,
) -> Result<LibrarySnapshot, String> {
    let root = PathBuf::from(&path);
    ensure_parent_exists(&root)?;
    let guid = FirewireGuid::parse_hex("000A27001358762E").map_err(|e| e.to_string())?;
    let label = if name.trim().is_empty() {
        "Mock iPod"
    } else {
        name.trim()
    };
    let ipod = create_mock_ipod(&root, label, generation, guid).map_err(|e| e.to_string())?;
    let id = ipod.summary.id.clone();
    let snap = build_library_snapshot(&ipod);

    state
        .devices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id.clone(), ipod);
    *state
        .selected_device_id
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(id);
    Ok(snap)
}

#[tauri::command]
pub fn open_mock_device(
    path: String,
    state: State<'_, AppState>,
) -> Result<LibrarySnapshot, String> {
    let root = PathBuf::from(&path);
    let ipod = open_ipod_at_path(&root, None, true, None).map_err(|e| e.to_string())?;
    let id = ipod.summary.id.clone();
    let snap = build_library_snapshot(&ipod);

    state
        .devices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id.clone(), ipod);
    *state
        .selected_device_id
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(id);
    Ok(snap)
}

#[tauri::command]
pub async fn enqueue_transfer(
    app: AppHandle,
    device_id: String,
    paths: Vec<String>,
    state: State<'_, AppState>,
) -> Result<BatchCommitSummary, String> {
    let mut ipod_clone = {
        let guard = state
            .devices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard
            .get(&device_id)
            .cloned()
            .ok_or_else(|| format!("device '{device_id}' not found"))?
    };

    let settings = state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .transfer
        .clone();
    let backup_root = state.backup_root();

    let cancel = CancellationToken::new();
    {
        let mut active = state
            .active_cancel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(prev) = active.replace(cancel.clone()) {
            prev.cancel();
        }
    }

    let path_bufs: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let app_for_events = app.clone();
    let result = execute_transfer_batch(
        &mut ipod_clone,
        &path_bufs,
        &settings,
        &backup_root,
        None,
        cancel,
        move |snap| {
            let _ = app_for_events.emit("transfer://progress", &snap);
        },
    )
    .await;

    *state
        .active_cancel
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;

    let summary = result.map_err(|e| e.to_string())?;

    state.invalidate_device_art_cache(&device_id);
    state
        .devices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(device_id, ipod_clone);

    Ok(summary)
}

#[tauri::command]
pub fn cancel_active_transfer(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(tok) = state
        .active_cancel
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    {
        tok.cancel();
    }
    Ok(())
}

#[tauri::command]
pub fn delete_tracks(
    device_id: String,
    track_ids: Vec<u32>,
    state: State<'_, AppState>,
) -> Result<LibrarySnapshot, String> {
    let backup_root = state.backup_root();
    let mut guard = state
        .devices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ipod = guard
        .get_mut(&device_id)
        .ok_or_else(|| format!("device '{device_id}' not found"))?;

    delete_tracks_atomic(ipod, &track_ids, &backup_root).map_err(|e| e.to_string())?;
    let snap = build_library_snapshot(ipod);
    drop(guard);

    state.invalidate_device_art_cache(&device_id);
    Ok(snap)
}

#[tauri::command]
pub fn create_playlist(
    device_id: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<LibrarySnapshot, String> {
    let backup_root = state.backup_root();
    let mut guard = state
        .devices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ipod = guard
        .get_mut(&device_id)
        .ok_or_else(|| format!("device '{device_id}' not found"))?;

    ipod.itdb.create_playlist(name.trim(), now_mac());
    commit_database_atomic(ipod, &backup_root).map_err(|e| e.to_string())?;
    Ok(build_library_snapshot(ipod))
}

#[tauri::command]
pub fn rename_playlist(
    device_id: String,
    persistent_id_hex: String,
    new_name: String,
    state: State<'_, AppState>,
) -> Result<LibrarySnapshot, String> {
    let pid = parse_hex_u64(&persistent_id_hex)?;
    let backup_root = state.backup_root();
    let mut guard = state
        .devices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ipod = guard
        .get_mut(&device_id)
        .ok_or_else(|| format!("device '{device_id}' not found"))?;

    ipod.itdb
        .rename_playlist(pid, new_name.trim(), now_mac())
        .map_err(|e| e.to_string())?;
    commit_database_atomic(ipod, &backup_root).map_err(|e| e.to_string())?;
    Ok(build_library_snapshot(ipod))
}

#[tauri::command]
pub fn set_playlist_tracks(
    device_id: String,
    persistent_id_hex: String,
    track_ids: Vec<u32>,
    state: State<'_, AppState>,
) -> Result<LibrarySnapshot, String> {
    let pid = parse_hex_u64(&persistent_id_hex)?;
    let backup_root = state.backup_root();
    let mut guard = state
        .devices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ipod = guard
        .get_mut(&device_id)
        .ok_or_else(|| format!("device '{device_id}' not found"))?;

    ipod.itdb
        .set_playlist_tracks(pid, &track_ids, now_mac())
        .map_err(|e| e.to_string())?;
    commit_database_atomic(ipod, &backup_root).map_err(|e| e.to_string())?;
    Ok(build_library_snapshot(ipod))
}

#[tauri::command]
pub fn append_to_playlist(
    device_id: String,
    persistent_id_hex: String,
    track_ids: Vec<u32>,
    state: State<'_, AppState>,
) -> Result<LibrarySnapshot, String> {
    let pid = parse_hex_u64(&persistent_id_hex)?;
    let backup_root = state.backup_root();
    let mut guard = state
        .devices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ipod = guard
        .get_mut(&device_id)
        .ok_or_else(|| format!("device '{device_id}' not found"))?;

    ipod.itdb
        .append_tracks_to_playlist(pid, &track_ids, now_mac())
        .map_err(|e| e.to_string())?;
    commit_database_atomic(ipod, &backup_root).map_err(|e| e.to_string())?;
    Ok(build_library_snapshot(ipod))
}

#[tauri::command]
pub fn delete_playlist(
    device_id: String,
    persistent_id_hex: String,
    state: State<'_, AppState>,
) -> Result<LibrarySnapshot, String> {
    let pid = parse_hex_u64(&persistent_id_hex)?;
    let backup_root = state.backup_root();
    let mut guard = state
        .devices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ipod = guard
        .get_mut(&device_id)
        .ok_or_else(|| format!("device '{device_id}' not found"))?;

    ipod.itdb.delete_playlist(pid).map_err(|e| e.to_string())?;
    commit_database_atomic(ipod, &backup_root).map_err(|e| e.to_string())?;
    Ok(build_library_snapshot(ipod))
}

#[tauri::command]
pub fn safe_eject_device(device_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let (root_path, is_mock) = {
        let guard = state
            .devices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dev = guard
            .get(&device_id)
            .ok_or_else(|| format!("device '{device_id}' not found"))?;
        (dev.summary.root_path.clone(), dev.summary.is_mock)
    };

    if !is_mock {
        eject_usb_ipod(&root_path).map_err(|e| e.to_string())?;
    }

    state.invalidate_device_art_cache(&device_id);
    state
        .devices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&device_id);
    let mut sel = state
        .selected_device_id
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if sel.as_deref() == Some(device_id.as_str()) {
        *sel = None;
    }
    Ok(())
}

#[tauri::command]
pub fn get_app_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone())
}

#[tauri::command]
pub fn update_app_settings(
    settings: AppSettings,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    state.save_settings(settings.clone())?;
    Ok(settings)
}

#[tauri::command]
pub fn list_backups(
    device_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<BackupManifest>, String> {
    let backup_root = state.backup_root();
    let guard = state
        .devices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ipod = guard
        .get(&device_id)
        .ok_or_else(|| format!("device '{device_id}' not found"))?;
    Ok(list_device_backups(ipod, &backup_root))
}

#[tauri::command]
pub fn restore_backup(
    device_id: String,
    backup_id: String,
    state: State<'_, AppState>,
) -> Result<LibrarySnapshot, String> {
    let backup_root = state.backup_root();
    let mut guard = state
        .devices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ipod = guard
        .get_mut(&device_id)
        .ok_or_else(|| format!("device '{device_id}' not found"))?;
    restore_device_backup(ipod, &backup_root, &backup_id).map_err(|e| e.to_string())?;
    let snap = build_library_snapshot(ipod);
    drop(guard);

    state.invalidate_device_art_cache(&device_id);
    Ok(snap)
}
