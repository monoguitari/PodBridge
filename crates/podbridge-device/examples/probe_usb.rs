// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use podbridge_device::{discover_connected_ipods, enumerate_connected_usb_ipods};

fn main() {
    let t0 = Instant::now();
    let probes = enumerate_connected_usb_ipods();
    let probe_elapsed = t0.elapsed();
    println!(
        "USB probe found {} drive(s) in {:.2?}: {:?}",
        probes.len(),
        probe_elapsed,
        probes
    );

    let t1 = Instant::now();
    let devices = discover_connected_ipods();
    let full_elapsed = t1.elapsed();
    for dev in &devices {
        println!(
            "Opened {:?} ({}) at {:?} | GUID={:?} | tracks={} | playlists={} | free={:.2} GiB / {:.2} GiB | total_open_time={:.2?}",
            dev.summary.name,
            dev.summary.generation_label,
            dev.summary.root_path,
            dev.summary.firewire_guid,
            dev.summary.track_count,
            dev.summary.playlist_count,
            dev.summary.storage.free_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            dev.summary.storage.total_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            full_elapsed
        );
    }
}
