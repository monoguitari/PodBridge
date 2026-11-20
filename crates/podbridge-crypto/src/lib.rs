// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! Clean-room cryptographic signing (`hash58` and `hash72`) for iPod `iTunesDB` files.
//!
//! ### Dual-Signing Order Invariant
//! When an `iTunesDB` carries both `hash72` (`mhbd[0x72..0xA0]`) and `hash58`
//! (`mhbd[0x58..0x6C]`), **`hash72` must be computed and written first, followed by `hash58`**,
//! because `hash58` hashes over `mhbd[0x70..0xA0]` without zeroing it.

pub mod hash58;
pub mod hash72;

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

pub use hash58::{compute_hash58, derive_hash58_key, sign_hash58, verify_hash58};
pub use hash72::{
    compute_hash72_sha1, compute_hash72_signature, sign_hash72, verify_hash72, Hash72Info,
};

pub(crate) const MHBD_MAGIC: &[u8; 4] = b"mhbd";
/// Minimum `mhbd` buffer length required to access `0x00..0xA0` (`hash58` + `hash72` slots).
pub const MIN_MHBD_SIGNING_LEN: usize = 0xA0;

/// Errors produced during `iTunesDB` signing, verification, or device GUID / `HashInfo` parsing.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("iTunesDB buffer too small: {actual} bytes (need at least {minimum})")]
    BufferTooSmall { actual: usize, minimum: usize },
    #[error("buffer does not start with 'mhbd' header magic")]
    InvalidMagic,
    #[error("invalid 16-hex-character FireWire GUID: '{0}'")]
    InvalidGuidHex(String),
    #[error("invalid HashInfo file (expected 'HASHv0' header and >= 54 bytes)")]
    InvalidHashInfo,
    #[error("iTunesDB does not contain an active hash72 signature block")]
    Hash72NotPresent,
    #[error("hash72 AES-128-CBC block verification failed")]
    Hash72VerificationFailed,
}

/// 64-bit IEEE-1394 / USB iPod FireWire GUID stored in big-endian byte order.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FirewireGuid([u8; 8]);

impl FirewireGuid {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 8]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 8] {
        &self.0
    }

    /// Parses a 16-character hex FireWire GUID (with optional `0x` / `0X` prefix and whitespace).
    pub fn parse_hex(input: &str) -> Result<Self, CryptoError> {
        let trimmed = input.trim();
        let hex = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
            .unwrap_or(trimmed);

        if hex.len() != 16 {
            return Err(CryptoError::InvalidGuidHex(input.to_owned()));
        }

        let mut out = [0u8; 8];
        for (i, slot) in out.iter_mut().enumerate() {
            let pair = &hex[2 * i..2 * i + 2];
            *slot = u8::from_str_radix(pair, 16)
                .map_err(|_| CryptoError::InvalidGuidHex(input.to_owned()))?;
        }
        Ok(Self(out))
    }

    /// Formats the GUID as uppercase 16-digit hexadecimal (e.g. `000A27001358762E`).
    #[must_use]
    pub fn to_hex_string(&self) -> String {
        format!(
            "{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5], self.0[6], self.0[7]
        )
    }
}

impl fmt::Debug for FirewireGuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FirewireGuid(0x{})", self.to_hex_string())
    }
}

impl fmt::Display for FirewireGuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex_string())
    }
}

impl FromStr for FirewireGuid {
    type Err = CryptoError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_hex(s)
    }
}

/// Cryptographic signing mode required for a target iPod model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashingScheme {
    /// iPod 5G / 5.5G Video and earlier: no cryptographic signature required.
    None,
    /// iPod Classic 6G / 6.5G / 7G and iPod Nano 3G / 4G (`hash58`, plus optional companion
    /// `hash72` preserved from an iTunes-initialized database).
    Hash58 {
        guid: FirewireGuid,
        companion_hash72: Option<Hash72Info>,
    },
    /// iPod Nano 5G (`hash72`, plus optional `hash58` when FireWire GUID is known).
    Hash72 {
        info: Hash72Info,
        companion_guid: Option<FirewireGuid>,
    },
}

/// Signs an assembled `iTunesDB` byte buffer in place according to `scheme`.
///
/// Enforces the mandatory `hash72`-before-`hash58` ordering whenever both signatures are active.
pub fn sign_itunesdb(buf: &mut [u8], scheme: &HashingScheme) -> Result<(), CryptoError> {
    if buf.len() < MIN_MHBD_SIGNING_LEN {
        return Err(CryptoError::BufferTooSmall {
            actual: buf.len(),
            minimum: MIN_MHBD_SIGNING_LEN,
        });
    }
    if &buf[0..4] != MHBD_MAGIC {
        return Err(CryptoError::InvalidMagic);
    }

    match scheme {
        HashingScheme::None => {
            buf[0x30..0x32].copy_from_slice(&0u16.to_le_bytes());
            buf[0x58..0x6C].fill(0);
            buf[0x70..0xA0].fill(0);
            Ok(())
        }
        HashingScheme::Hash58 {
            guid,
            companion_hash72,
        } => {
            // Pre-set hashing_scheme = 1 BEFORE hash72 so hash72's SHA-1 sees 0x30 = 1!
            buf[0x30..0x32].copy_from_slice(&1u16.to_le_bytes());
            if let Some(info) = companion_hash72 {
                sign_hash72(buf, info)?;
            } else {
                buf[0x70..0xA0].fill(0);
            }
            sign_hash58(buf, *guid)?;
            Ok(())
        }
        HashingScheme::Hash72 {
            info,
            companion_guid,
        } => {
            if companion_guid.is_some() {
                buf[0x30..0x32].copy_from_slice(&1u16.to_le_bytes());
            }
            sign_hash72(buf, info)?;
            if let Some(guid) = companion_guid {
                sign_hash58(buf, *guid)?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_synthetic_itunesdb(payload_seed: u8) -> Vec<u8> {
        let mut buf = vec![0u8; 512];
        let total = buf.len() as u32;
        buf[0x00..0x04].copy_from_slice(b"mhbd");
        buf[0x04..0x08].copy_from_slice(&244u32.to_le_bytes());
        buf[0x08..0x0C].copy_from_slice(&total.to_le_bytes());
        buf[0x0C..0x10].copy_from_slice(&1u32.to_le_bytes());
        buf[0x10..0x14].copy_from_slice(&0x73u32.to_le_bytes());
        buf[0x18..0x20].copy_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());
        buf[0x32..0x3A].copy_from_slice(&0xAABB_CCDD_EEFF_0011u64.to_le_bytes());
        for (idx, b) in buf[244..].iter_mut().enumerate() {
            *b = payload_seed.wrapping_add(idx as u8);
        }
        buf
    }

    #[test]
    fn parses_and_formats_firewire_guid_with_zero_byte_lcm_vector() {
        let guid = FirewireGuid::parse_hex("0x000a27001358762e").unwrap();
        assert_eq!(guid.to_hex_string(), "000A27001358762E");

        // Verified ground-truth key_20 vector from MB147LL hardware (tests zero-byte LCM = 1 rule)
        let key = derive_hash58_key(guid);
        let expected_hex = "49ba1726f3054f6556f98e1ecd7dc79ddfdca80c";
        let actual_hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(actual_hex, expected_hex);
    }

    #[test]
    fn rejects_malformed_guid_and_short_buffers() {
        assert!(matches!(
            FirewireGuid::parse_hex("0x1234"),
            Err(CryptoError::InvalidGuidHex(_))
        ));
        assert!(matches!(
            FirewireGuid::parse_hex("000A2700135876ZZ"),
            Err(CryptoError::InvalidGuidHex(_))
        ));

        let guid = FirewireGuid::parse_hex("000A27001358762E").unwrap();
        let mut short = vec![0u8; 64];
        assert_eq!(
            sign_hash58(&mut short, guid),
            Err(CryptoError::BufferTooSmall {
                actual: 64,
                minimum: MIN_MHBD_SIGNING_LEN
            })
        );
    }

    #[test]
    fn signs_verifies_and_preserves_zeroed_header_fields_for_hash58() {
        let guid = FirewireGuid::parse_hex("000A27001358762E").unwrap();
        let mut db = make_synthetic_itunesdb(0x42);

        let orig_db_id = db[0x18..0x20].to_vec();
        let orig_unk_32 = db[0x32..0x46].to_vec();

        sign_itunesdb(
            &mut db,
            &HashingScheme::Hash58 {
                guid,
                companion_hash72: None,
            },
        )
        .unwrap();

        assert_eq!(&db[0x18..0x20], orig_db_id.as_slice());
        assert_eq!(&db[0x32..0x46], orig_unk_32.as_slice());
        assert!(verify_hash58(&db, guid).unwrap());

        // Changing db_id does NOT invalidate hash58 because 0x18..0x20 is zeroed before hashing
        db[0x18] ^= 0xFF;
        assert!(verify_hash58(&db, guid).unwrap());

        // Corrupting payload bytes MUST fail verification
        db[300] ^= 0x01;
        assert!(!verify_hash58(&db, guid).unwrap());
    }

    #[test]
    fn signs_extracts_and_dual_signs_hash72_then_hash58() {
        let guid = FirewireGuid::parse_hex("000A27001358762E").unwrap();
        let info = Hash72Info {
            uuid: {
                let mut u = [0u8; 20];
                u[0..8].copy_from_slice(guid.as_bytes());
                u
            },
            rndpart: [
                0xD5, 0xEE, 0x87, 0xA0, 0x28, 0x7F, 0xEE, 0xF6, 0x51, 0x50, 0xD2, 0x4E,
            ],
            iv: [
                0x26, 0xB5, 0x17, 0xB4, 0x84, 0x71, 0x88, 0x91, 0x99, 0x81, 0x9C, 0xC9, 0x1F, 0x80,
                0xD0, 0x4E,
            ],
        };

        // Round-trip HashInfo file serialization (54 packed bytes + 56 padded compatibility)
        let packed = info.to_hash_info_bytes();
        assert_eq!(packed.len(), 54);
        assert_eq!(Hash72Info::from_hash_info_bytes(&packed).unwrap(), info);
        let mut padded = packed.to_vec();
        padded.extend_from_slice(&[0, 0]);
        assert_eq!(Hash72Info::from_hash_info_bytes(&padded).unwrap(), info);

        // Dual-sign (hash72 first, hash58 second)
        let mut db = make_synthetic_itunesdb(0x99);
        sign_itunesdb(
            &mut db,
            &HashingScheme::Hash58 {
                guid,
                companion_hash72: Some(info),
            },
        )
        .unwrap();

        assert!(verify_hash58(&db, guid).unwrap());
        assert!(verify_hash72(&db, Some(&info)).unwrap());
        assert!(verify_hash72(&db, None).unwrap());

        // Recover Hash72Info from the signed DB without prior HashInfo file!
        let extracted = Hash72Info::extract_from_itunesdb(&db, Some(guid)).unwrap();
        assert_eq!(extracted, info);

        // Mutate payload, re-sign using extracted Hash72Info, and verify both hashes stay valid
        db[260] = 0xAA;
        assert!(!verify_hash58(&db, guid).unwrap());
        assert!(!verify_hash72(&db, Some(&extracted)).unwrap());

        sign_itunesdb(
            &mut db,
            &HashingScheme::Hash58 {
                guid,
                companion_hash72: Some(extracted),
            },
        )
        .unwrap();
        assert!(verify_hash58(&db, guid).unwrap());
        assert!(verify_hash72(&db, Some(&extracted)).unwrap());
    }
}
