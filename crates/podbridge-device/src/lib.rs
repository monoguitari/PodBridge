// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

//! Connected iPod USB discovery, FireWire GUID resolution, Safe Eject, and Mock iPod mode.

pub mod error;
pub mod mock;
pub mod model;
pub mod sysinfo;
pub mod windows_usb;

pub use error::DeviceError;
pub use mock::{
    create_mock_ipod, open_ipod_at_path, ConnectedIpod, IpodDeviceSummary, StorageBreakdown,
};
pub use model::IpodGeneration;
pub use sysinfo::{read_sysinfo_metadata, resolve_hashing_scheme, SysInfoMetadata};
pub use windows_usb::{
    eject_usb_ipod, enumerate_connected_usb_ipods, parse_apple_usb_instance_id, query_volume_space,
    UsbDriveProbe,
};

/// Discovers and opens all physical USB-connected iPods currently mounted on Windows.
#[must_use]
pub fn discover_connected_ipods() -> Vec<ConnectedIpod> {
    enumerate_connected_usb_ipods()
        .into_iter()
        .filter_map(|probe| {
            open_ipod_at_path(
                &probe.drive_root,
                probe.usb_pid.map(|pid| (pid, probe.firewire_guid)),
                false,
                Some(&probe.volume_label),
            )
            .ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use podbridge_crypto::{verify_hash58, verify_hash72, FirewireGuid};

    use super::*;

    #[test]
    fn parses_real_windows_mb147ll_pnp_instance_id() {
        let (pid, guid) =
            parse_apple_usb_instance_id(r"USB\VID_05AC&PID_1261\000A27001358762E").unwrap();
        assert_eq!(pid, 0x1261);
        assert_eq!(guid.unwrap().to_hex_string(), "000A27001358762E");
        assert_eq!(
            IpodGeneration::detect(Some(pid), Some("xB147")),
            IpodGeneration::Classic6G
        );
    }

    #[test]
    fn initializes_and_reopens_mock_ipods_across_all_5_generations() {
        let guid = FirewireGuid::parse_hex("000A27001358762E").unwrap();
        let generations = [
            IpodGeneration::Video5G,
            IpodGeneration::Classic6G,
            IpodGeneration::Nano3G,
            IpodGeneration::Nano4G,
            IpodGeneration::Nano5G,
        ];

        for (idx, gen_model) in generations.into_iter().enumerate() {
            let dir = std::env::temp_dir().join(format!(
                "podbridge_mock_test_{}_{}",
                std::process::id(),
                idx
            ));
            let _ = std::fs::remove_dir_all(&dir);

            let created = create_mock_ipod(&dir, "Mock iPod", gen_model, guid).unwrap();
            assert_eq!(created.summary.generation, gen_model);
            assert!(created.summary.is_mock);
            assert_eq!(created.summary.track_count, 0);

            // Reopen without USB hint (verifying SysInfo / HashInfo resolution on disk)
            let reopened = open_ipod_at_path(&dir, None, true, None).unwrap();
            assert_eq!(reopened.summary.generation, gen_model);

            let raw_db = std::fs::read(reopened.itunes_db_path()).unwrap();
            match gen_model {
                IpodGeneration::Video5G => {}
                IpodGeneration::Nano5G => {
                    assert!(verify_hash72(&raw_db, None).unwrap());
                }
                _ => {
                    assert!(verify_hash58(&raw_db, guid).unwrap());
                }
            }

            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
