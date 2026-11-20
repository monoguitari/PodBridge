// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::error::DeviceError;
use podbridge_crypto::FirewireGuid;

/// Hardware metadata discovered from Windows USB SetupAPI / CfgMgr32 for a mounted drive letter.
#[derive(Debug, Clone)]
pub struct UsbDriveProbe {
    pub drive_root: std::path::PathBuf,
    pub volume_label: String,
    pub usb_pid: Option<u16>,
    pub firewire_guid: Option<FirewireGuid>,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

/// Parses an Apple USB PnP Device Instance ID (`USB\VID_05AC&PID_1261\000A27001358762E`) into
/// `(usb_pid, Option<FirewireGuid>)`.
#[must_use]
pub fn parse_apple_usb_instance_id(instance_id: &str) -> Option<(u16, Option<FirewireGuid>)> {
    let up = instance_id.trim().to_ascii_uppercase();
    if !up.contains("VID_05AC&PID_") {
        return None;
    }
    let after_pid = up.split("VID_05AC&PID_").nth(1)?;
    let pid_hex = after_pid.get(0..4)?;
    let pid = u16::from_str_radix(pid_hex, 16).ok()?;

    let serial_seg = instance_id.rsplit('\\').next().unwrap_or("");
    let guid = if serial_seg.len() >= 16 {
        FirewireGuid::parse_hex(&serial_seg[0..16]).ok()
    } else {
        None
    };
    Some((pid, guid))
}

#[cfg(windows)]
mod imp {
    use std::ffi::OsString;
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStringExt;
    use std::path::{Path, PathBuf};

    use windows::core::PCWSTR;
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        CM_Get_Device_IDW, CM_Get_Parent, CM_Request_Device_EjectW, SetupDiDestroyDeviceInfoList,
        SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
        CR_SUCCESS, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, PNP_VETO_TYPE, SP_DEVICE_INTERFACE_DATA,
        SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA,
    };
    use windows::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FlushFileBuffers, GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives,
        GetVolumeInformationW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };
    use windows::Win32::System::Ioctl::{
        FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, GUID_DEVINTERFACE_DISK,
        IOCTL_STORAGE_EJECT_MEDIA, IOCTL_STORAGE_GET_DEVICE_NUMBER, STORAGE_DEVICE_NUMBER,
    };
    use windows::Win32::System::IO::DeviceIoControl;

    use super::{parse_apple_usb_instance_id, UsbDriveProbe};
    use crate::error::DeviceError;
    use podbridge_crypto::FirewireGuid;

    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;

    fn to_wide_nul(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn enumerate_windows_ipod_drives() -> Vec<UsbDriveProbe> {
        let mut results = Vec::new();
        // SAFETY: GetLogicalDrives takes no arguments and returns a bitmask of active drive letters.
        let mask = unsafe { GetLogicalDrives() };
        for bit in 0..26u32 {
            if (mask & (1 << bit)) == 0 {
                continue;
            }
            let letter = (b'A' + bit as u8) as char;
            let root_str = format!("{letter}:\\");
            let root_wide = to_wide_nul(&root_str);

            // SAFETY: root_wide is a valid NUL-terminated UTF-16 drive root path.
            let dtype = unsafe { GetDriveTypeW(PCWSTR(root_wide.as_ptr())) };
            if dtype != DRIVE_REMOVABLE && dtype != DRIVE_FIXED {
                continue;
            }

            let root_path = PathBuf::from(&root_str);
            if !root_path.join("iPod_Control").is_dir() {
                continue;
            }

            let volume_label =
                query_volume_label(&root_wide).unwrap_or_else(|| format!("iPod ({letter}:)"));
            let (total_bytes, free_bytes) = query_disk_space(&root_wide).unwrap_or((0, 0));
            let (usb_pid, firewire_guid) = resolve_usb_pid_and_guid_for_drive(letter)
                .map_or((None, None), |(pid, g, _)| (Some(pid), g));

            results.push(UsbDriveProbe {
                drive_root: root_path,
                volume_label,
                usb_pid,
                firewire_guid,
                total_bytes,
                free_bytes,
            });
        }
        results
    }

    pub fn query_path_space_windows(path: &Path) -> Option<(u64, u64)> {
        let wide = to_wide_nul(&path.to_string_lossy());
        query_disk_space(&wide)
    }

    fn query_volume_label(root_wide: &[u16]) -> Option<String> {
        let mut name_buf = [0u16; 261];
        // SAFETY: name_buf is stack-allocated with length passed explicitly.
        let ok = unsafe {
            GetVolumeInformationW(
                PCWSTR(root_wide.as_ptr()),
                Some(&mut name_buf),
                None,
                None,
                None,
                None,
            )
        };
        if ok.is_err() {
            return None;
        }
        let nul_pos = name_buf
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(name_buf.len());
        let label = OsString::from_wide(&name_buf[..nul_pos])
            .to_string_lossy()
            .trim()
            .to_owned();
        if label.is_empty() {
            None
        } else {
            Some(label)
        }
    }

    fn query_disk_space(path_wide: &[u16]) -> Option<(u64, u64)> {
        let mut free_avail = 0u64;
        let mut total = 0u64;
        let mut total_free = 0u64;
        // SAFETY: Output pointers reference valid stack u64 variables.
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                PCWSTR(path_wide.as_ptr()),
                Some(&mut free_avail),
                Some(&mut total),
                Some(&mut total_free),
            )
        };
        if ok.is_ok() {
            Some((total, free_avail))
        } else {
            None
        }
    }

    fn get_volume_device_number(drive_letter: char) -> Option<u32> {
        let vol_path = to_wide_nul(&format!("\\\\.\\{drive_letter}:"));
        // SAFETY: Opening volume handle with 0 desired access requires no admin privileges.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(vol_path.as_ptr()),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .ok()?;
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }

        let mut sdn: STORAGE_DEVICE_NUMBER = unsafe { zeroed() };
        let mut bytes_ret = 0u32;
        // SAFETY: sdn buffer size matches STORAGE_DEVICE_NUMBER.
        let res = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_STORAGE_GET_DEVICE_NUMBER,
                None,
                0,
                Some((&mut sdn as *mut STORAGE_DEVICE_NUMBER).cast()),
                size_of::<STORAGE_DEVICE_NUMBER>() as u32,
                Some(&mut bytes_ret),
                None,
            )
        };
        let _ = unsafe { CloseHandle(handle) };
        res.ok()?;
        Some(sdn.DeviceNumber)
    }

    /// Maps `drive_letter` (`D`) -> `STORAGE_DEVICE_NUMBER` -> `SetupDi` disk interface ->
    /// walks `CM_Get_Parent` up to `USB\VID_05AC&PID_xxxx\<GUID>`, returning `(pid, guid, usb_devinst)`.
    fn resolve_usb_pid_and_guid_for_drive(
        drive_letter: char,
    ) -> Option<(u16, Option<FirewireGuid>, u32)> {
        let target_dev_num = get_volume_device_number(drive_letter)?;

        // SAFETY: Enumerates present disk device interfaces.
        let dev_info = unsafe {
            SetupDiGetClassDevsW(
                Some(&GUID_DEVINTERFACE_DISK),
                PCWSTR::null(),
                None,
                DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
            )
        }
        .ok()?;

        let mut member_idx = 0u32;
        let mut found = None;

        loop {
            let mut iface_data = SP_DEVICE_INTERFACE_DATA {
                cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..unsafe { zeroed() }
            };
            // SAFETY: Valid dev_info set and initialized SP_DEVICE_INTERFACE_DATA.
            let enum_ok = unsafe {
                SetupDiEnumDeviceInterfaces(
                    dev_info,
                    None,
                    &GUID_DEVINTERFACE_DISK,
                    member_idx,
                    &mut iface_data,
                )
            };
            if enum_ok.is_err() {
                break;
            }
            member_idx += 1;

            let mut required_size = 0u32;
            let _ = unsafe {
                SetupDiGetDeviceInterfaceDetailW(
                    dev_info,
                    &iface_data,
                    None,
                    0,
                    Some(&mut required_size),
                    None,
                )
            };
            if required_size < size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32 {
                continue;
            }

            let mut detail_buf = vec![0u8; required_size as usize];
            let detail_ptr = detail_buf
                .as_mut_ptr()
                .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
            unsafe {
                (*detail_ptr).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            }
            let mut devinfo_data = SP_DEVINFO_DATA {
                cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
                ..unsafe { zeroed() }
            };

            let detail_ok = unsafe {
                SetupDiGetDeviceInterfaceDetailW(
                    dev_info,
                    &iface_data,
                    Some(detail_ptr),
                    required_size,
                    None,
                    Some(&mut devinfo_data),
                )
            };
            if detail_ok.is_err() {
                continue;
            }

            // Open the disk interface path to compare its STORAGE_DEVICE_NUMBER
            let path_ptr = unsafe { (*detail_ptr).DevicePath.as_ptr() };
            let disk_handle = unsafe {
                CreateFileW(
                    PCWSTR(path_ptr),
                    0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                )
            };
            let Ok(h) = disk_handle else { continue };
            if h == INVALID_HANDLE_VALUE {
                continue;
            }

            let mut sdn: STORAGE_DEVICE_NUMBER = unsafe { zeroed() };
            let mut ret = 0u32;
            let io_ok = unsafe {
                DeviceIoControl(
                    h,
                    IOCTL_STORAGE_GET_DEVICE_NUMBER,
                    None,
                    0,
                    Some((&mut sdn as *mut STORAGE_DEVICE_NUMBER).cast()),
                    size_of::<STORAGE_DEVICE_NUMBER>() as u32,
                    Some(&mut ret),
                    None,
                )
            };
            let _ = unsafe { CloseHandle(h) };
            if io_ok.is_err() || sdn.DeviceNumber != target_dev_num {
                continue;
            }

            // Walk CM_Get_Parent up the PnP tree to find the root USB\VID_05AC&PID_xxxx\<16-HEX-GUID>
            // (Composite iPods expose USB\VID_05AC&PID_1261&MI_00\... one level below the root USB node,
            // so keep walking up to capture both the 16-hex GUID and the ejectable root DEVINST!)
            let mut cur_inst = devinfo_data.DevInst;
            for _ in 0..8 {
                let mut id_buf = [0u16; 512];
                let cr = unsafe { CM_Get_Device_IDW(cur_inst, &mut id_buf, 0) };
                if cr == CR_SUCCESS {
                    let nul = id_buf.iter().position(|&c| c == 0).unwrap_or(id_buf.len());
                    let id_str = OsString::from_wide(&id_buf[..nul])
                        .to_string_lossy()
                        .into_owned();
                    if let Some((pid, guid)) = parse_apple_usb_instance_id(&id_str) {
                        let keep_prev_guid = found.and_then(|(_, g, _)| g);
                        found = Some((pid, guid.or(keep_prev_guid), cur_inst));
                        if guid.is_some() && !id_str.to_ascii_uppercase().contains("&MI_") {
                            break;
                        }
                    }
                }
                let mut parent = 0u32;
                if unsafe { CM_Get_Parent(&mut parent, cur_inst, 0) } != CR_SUCCESS {
                    break;
                }
                cur_inst = parent;
            }
            if found.is_some() {
                break;
            }
        }

        let _ = unsafe { SetupDiDestroyDeviceInfoList(dev_info) };
        found
    }

    pub fn safe_eject_windows_drive(drive_root: &Path) -> Result<(), DeviceError> {
        let s = drive_root.to_string_lossy();
        let letter = s
            .chars()
            .next()
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_uppercase())
            .ok_or_else(|| DeviceError::EjectUnsupported(s.into_owned()))?;

        let usb_inst = resolve_usb_pid_and_guid_for_drive(letter).map(|(_, _, inst)| inst);

        // 1. Flush, lock, dismount, and issue IOCTL_STORAGE_EJECT_MEDIA on the volume
        let vol_wide = to_wide_nul(&format!("\\\\.\\{letter}:"));
        if let Ok(handle) = unsafe {
            CreateFileW(
                PCWSTR(vol_wide.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        } {
            if handle != INVALID_HANDLE_VALUE {
                let mut ret = 0u32;
                unsafe {
                    let _ = FlushFileBuffers(handle);
                    let _ = DeviceIoControl(
                        handle,
                        FSCTL_LOCK_VOLUME,
                        None,
                        0,
                        None,
                        0,
                        Some(&mut ret),
                        None,
                    );
                    let _ = DeviceIoControl(
                        handle,
                        FSCTL_DISMOUNT_VOLUME,
                        None,
                        0,
                        None,
                        0,
                        Some(&mut ret),
                        None,
                    );
                    let _ = DeviceIoControl(
                        handle,
                        IOCTL_STORAGE_EJECT_MEDIA,
                        None,
                        0,
                        None,
                        0,
                        Some(&mut ret),
                        None,
                    );
                    let _ = CloseHandle(handle);
                }
            }
        }

        // 2. Request PnP device ejection on the parent USB node so the iPod screen shows "OK to disconnect"
        if let Some(devinst) = usb_inst {
            let mut veto_type = PNP_VETO_TYPE::default();
            let mut veto_name = [0u16; 260];
            let cr = unsafe {
                CM_Request_Device_EjectW(devinst, Some(&mut veto_type), Some(&mut veto_name), 0)
            };
            if cr != CR_SUCCESS {
                let nul = veto_name.iter().position(|&c| c == 0).unwrap_or(0);
                let reason = OsString::from_wide(&veto_name[..nul])
                    .to_string_lossy()
                    .into_owned();
                return Err(DeviceError::EjectFailed(format!(
                    "CFG_MGR status 0x{:X} (veto {:?}: {})",
                    cr.0, veto_type, reason
                )));
            }
        }

        Ok(())
    }
}

#[must_use]
pub fn enumerate_connected_usb_ipods() -> Vec<UsbDriveProbe> {
    #[cfg(windows)]
    {
        imp::enumerate_windows_ipod_drives()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[must_use]
pub fn query_volume_space(path: &std::path::Path) -> (u64, u64) {
    #[cfg(windows)]
    {
        imp::query_path_space_windows(path).unwrap_or((80_000_000_000, 40_000_000_000))
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        (80_000_000_000, 60_000_000_000)
    }
}

pub fn eject_usb_ipod(drive_root: &std::path::Path) -> Result<(), DeviceError> {
    #[cfg(windows)]
    {
        imp::safe_eject_windows_drive(drive_root)
    }
    #[cfg(not(windows))]
    {
        Err(DeviceError::EjectUnsupported(
            drive_root.to_string_lossy().into_owned(),
        ))
    }
}
