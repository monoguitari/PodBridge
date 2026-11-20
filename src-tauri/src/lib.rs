// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

pub mod commands;
pub mod dto;
pub mod state;

use std::time::Duration;

use tauri::http::{Response, StatusCode};
use tauri::{Emitter, Manager};

use crate::state::{current_mounted_ipod_roots_fingerprint, AppState};

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(val) = u8::from_str_radix(hex, 16) {
                    out.push(val);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Starts the Tauri v2 desktop application.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_uri_scheme_protocol("ipodart", |ctx, request| {
            let uri = request.uri();
            let path = uri.path().trim_start_matches('/');
            let mut parts = path.splitn(2, '/');
            let raw_mhii = parts.next().unwrap_or("0");
            let raw_dev = parts.next().unwrap_or("");
            let mhii_id = raw_mhii.parse::<u32>().unwrap_or(0);
            let device_id = percent_decode(raw_dev);

            let min_w = uri
                .query()
                .and_then(|q| {
                    q.split('&')
                        .find_map(|kv| kv.strip_prefix("w="))
                        .and_then(|v| v.parse::<u16>().ok())
                })
                .unwrap_or(128);

            let state = ctx.app_handle().state::<AppState>();
            if let Some(png) = state.get_or_decode_artwork_png(&device_id, mhii_id, min_w) {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "image/png")
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Cache-Control", "public, max-age=3600")
                    .body(png)
                    .unwrap_or_else(|_| Response::new(Vec::new()))
            } else {
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .header("Access-Control-Allow-Origin", "*")
                    .body(Vec::new())
                    .unwrap_or_else(|_| Response::new(Vec::new()))
            }
        })
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("PodBridge"));
            app.manage(AppState::new(data_dir));

            // Spawn lightweight USB hotplug detector emitting `device://hotplug` on drive changes
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut prev = current_mounted_ipod_roots_fingerprint();
                let mut ticker = tokio::time::interval(Duration::from_millis(1200));
                loop {
                    ticker.tick().await;
                    let cur = current_mounted_ipod_roots_fingerprint();
                    if cur != prev {
                        prev = cur;
                        let _ = handle.emit("device://hotplug", ());
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_devices,
            commands::select_device,
            commands::get_library_snapshot,
            commands::create_mock_device,
            commands::open_mock_device,
            commands::enqueue_transfer,
            commands::cancel_active_transfer,
            commands::delete_tracks,
            commands::create_playlist,
            commands::rename_playlist,
            commands::set_playlist_tracks,
            commands::append_to_playlist,
            commands::delete_playlist,
            commands::safe_eject_device,
            commands::get_app_settings,
            commands::update_app_settings,
            commands::list_backups,
            commands::restore_backup,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run PodBridge Tauri application");
}
