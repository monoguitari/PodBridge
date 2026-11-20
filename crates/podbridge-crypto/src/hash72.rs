// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use aes::cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes128;
use sha1::{Digest, Sha1};

use crate::{CryptoError, FirewireGuid, MHBD_MAGIC, MIN_MHBD_SIGNING_LEN};

/// Fixed 16-byte AES-128 key used by `hash72`.
const HASH72_AES_KEY: [u8; 16] = [
    0x61, 0x8C, 0xA1, 0x0D, 0xC7, 0xF5, 0x7F, 0xD3, 0xB4, 0x72, 0x3E, 0x08, 0x15, 0x74, 0x63, 0xD7,
];

const HASH_INFO_MAGIC: &[u8; 6] = b"HASHv0";
const HASH_INFO_PACKED_LEN: usize = 54;
const HASH72_MARKER: [u8; 2] = [0x01, 0x00];

/// Device-bound `hash72` parameters (`HashInfo` / extracted `iTunesDB` seed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hash72Info {
    pub uuid: [u8; 20],
    pub rndpart: [u8; 12],
    pub iv: [u8; 16],
}

impl Hash72Info {
    /// Parses a `HashInfo` file (`iPod_Control/Device/HashInfo`, 54 packed bytes or >=56 padded).
    pub fn from_hash_info_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        if bytes.len() < HASH_INFO_PACKED_LEN || &bytes[0..6] != HASH_INFO_MAGIC {
            return Err(CryptoError::InvalidHashInfo);
        }
        let mut uuid = [0u8; 20];
        let mut rndpart = [0u8; 12];
        let mut iv = [0u8; 16];
        uuid.copy_from_slice(&bytes[0x06..0x1A]);
        rndpart.copy_from_slice(&bytes[0x1A..0x26]);
        iv.copy_from_slice(&bytes[0x26..0x36]);
        Ok(Self { uuid, rndpart, iv })
    }

    /// Serializes this `Hash72Info` into the packed 54-byte `HashInfo` on-disk layout.
    #[must_use]
    pub fn to_hash_info_bytes(&self) -> [u8; HASH_INFO_PACKED_LEN] {
        let mut out = [0u8; HASH_INFO_PACKED_LEN];
        out[0x00..0x06].copy_from_slice(HASH_INFO_MAGIC);
        out[0x06..0x1A].copy_from_slice(&self.uuid);
        out[0x1A..0x26].copy_from_slice(&self.rndpart);
        out[0x26..0x36].copy_from_slice(&self.iv);
        out
    }

    /// Extracts `(iv, rndpart)` directly from an existing, valid `hash72`-signed `iTunesDB`.
    ///
    /// Decrypts the 2-block AES-128-CBC ciphertext at `mhbd[0x80..0xA0]` against the database's
    /// SHA-1 digest and verifies Block 2 (`db_sha1[16..20] || rndpart`) before succeeding.
    pub fn extract_from_itunesdb(
        buf: &[u8],
        guid: Option<FirewireGuid>,
    ) -> Result<Self, CryptoError> {
        if buf.len() < MIN_MHBD_SIGNING_LEN {
            return Err(CryptoError::BufferTooSmall {
                actual: buf.len(),
                minimum: MIN_MHBD_SIGNING_LEN,
            });
        }
        if &buf[0..4] != MHBD_MAGIC {
            return Err(CryptoError::InvalidMagic);
        }
        if buf[0x72..0x74] != HASH72_MARKER {
            return Err(CryptoError::Hash72NotPresent);
        }

        let db_sha1 = compute_hash72_sha1(buf, false)?;

        let mut rndpart = [0u8; 12];
        let mut c1 = [0u8; 16];
        let mut c2 = [0u8; 16];
        rndpart.copy_from_slice(&buf[0x74..0x80]);
        c1.copy_from_slice(&buf[0x80..0x90]);
        c2.copy_from_slice(&buf[0x90..0xA0]);

        let cipher = Aes128::new(GenericArray::from_slice(&HASH72_AES_KEY));

        let mut d1 = GenericArray::clone_from_slice(&c1);
        cipher.decrypt_block(&mut d1);

        let mut d2 = GenericArray::clone_from_slice(&c2);
        cipher.decrypt_block(&mut d2);

        // Verify Block 2: AES_Dec(C2) ^ C1 == db_sha1[16..20] || rndpart[0..12]
        let mut p2_actual = [0u8; 16];
        for i in 0..16 {
            p2_actual[i] = d2[i] ^ c1[i];
        }
        if p2_actual[0..4] != db_sha1[16..20] || p2_actual[4..16] != rndpart {
            return Err(CryptoError::Hash72VerificationFailed);
        }

        let mut iv = [0u8; 16];
        for i in 0..16 {
            iv[i] = d1[i] ^ db_sha1[i];
        }

        let mut uuid = [0u8; 20];
        if let Some(g) = guid {
            uuid[0..8].copy_from_slice(g.as_bytes());
        }

        Ok(Self { uuid, rndpart, iv })
    }
}

/// Computes the 20-byte SHA-1 digest used by `hash72` without copying `buf`.
///
/// Zeroes `0x18..0x20` (`db_id`), `0x58..0x6C` (`hash58`), and `0x72..0xA0` (`hash72`).
/// When `force_flag_3` is true, hashes `0x70..0x72` as `3u16` (little-endian).
pub fn compute_hash72_sha1(buf: &[u8], force_flag_3: bool) -> Result<[u8; 20], CryptoError> {
    if buf.len() < MIN_MHBD_SIGNING_LEN {
        return Err(CryptoError::BufferTooSmall {
            actual: buf.len(),
            minimum: MIN_MHBD_SIGNING_LEN,
        });
    }
    if &buf[0..4] != MHBD_MAGIC {
        return Err(CryptoError::InvalidMagic);
    }

    let mut hasher = Sha1::new();
    hasher.update(&buf[0x00..0x18]);
    hasher.update([0u8; 8]); // 0x18..0x20: db_id zeroed
    hasher.update(&buf[0x20..0x58]);
    hasher.update([0u8; 20]); // 0x58..0x6C: hash58 zeroed
    hasher.update(&buf[0x6C..0x70]);
    if force_flag_3 {
        hasher.update(3u16.to_le_bytes());
    } else {
        hasher.update(&buf[0x70..0x72]);
    }
    hasher.update([0u8; 46]); // 0x72..0xA0: hash72 zeroed
    hasher.update(&buf[0xA0..]);

    Ok(hasher.finalize().into())
}

/// Computes the 46-byte `hash72` signature block for `mhbd[0x72..0xA0]`.
pub fn compute_hash72_signature(buf: &[u8], info: &Hash72Info) -> Result<[u8; 46], CryptoError> {
    let db_sha1 = compute_hash72_sha1(buf, true)?;
    let cipher = Aes128::new(GenericArray::from_slice(&HASH72_AES_KEY));

    let mut block1 = [0u8; 16];
    for i in 0..16 {
        block1[i] = db_sha1[i] ^ info.iv[i];
    }
    let mut c1 = GenericArray::clone_from_slice(&block1);
    cipher.encrypt_block(&mut c1);

    let mut p2 = [0u8; 16];
    p2[0..4].copy_from_slice(&db_sha1[16..20]);
    p2[4..16].copy_from_slice(&info.rndpart);

    let mut block2 = [0u8; 16];
    for i in 0..16 {
        block2[i] = p2[i] ^ c1[i];
    }
    let mut c2 = GenericArray::clone_from_slice(&block2);
    cipher.encrypt_block(&mut c2);

    let mut sig = [0u8; 46];
    sig[0..2].copy_from_slice(&HASH72_MARKER);
    sig[2..14].copy_from_slice(&info.rndpart);
    sig[14..30].copy_from_slice(&c1);
    sig[30..46].copy_from_slice(&c2);
    Ok(sig)
}

/// Signs `buf` in place with `hash72`, setting `mhbd[0x70..0x72] = 3` and writing the
/// 46-byte signature at `mhbd[0x72..0xA0]`.
pub fn sign_hash72(buf: &mut [u8], info: &Hash72Info) -> Result<[u8; 46], CryptoError> {
    let sig = compute_hash72_signature(buf, info)?;
    buf[0x70..0x72].copy_from_slice(&3u16.to_le_bytes());
    buf[0x72..0xA0].copy_from_slice(&sig);
    Ok(sig)
}

/// Verifies whether `buf` contains a valid `hash72` signature matching `info` (or self-consistent
/// if `info` is extracted).
pub fn verify_hash72(buf: &[u8], info: Option<&Hash72Info>) -> Result<bool, CryptoError> {
    if let Some(known) = info {
        if buf.len() < MIN_MHBD_SIGNING_LEN || &buf[0..4] != MHBD_MAGIC {
            return Err(CryptoError::InvalidMagic);
        }
        if buf[0x72..0x74] != HASH72_MARKER {
            return Ok(false);
        }
        let db_sha1 = compute_hash72_sha1(buf, false)?;
        let cipher = Aes128::new(GenericArray::from_slice(&HASH72_AES_KEY));

        let mut block1 = [0u8; 16];
        for i in 0..16 {
            block1[i] = db_sha1[i] ^ known.iv[i];
        }
        let mut c1 = GenericArray::clone_from_slice(&block1);
        cipher.encrypt_block(&mut c1);

        let mut p2 = [0u8; 16];
        p2[0..4].copy_from_slice(&db_sha1[16..20]);
        p2[4..16].copy_from_slice(&known.rndpart);
        let mut block2 = [0u8; 16];
        for i in 0..16 {
            block2[i] = p2[i] ^ c1[i];
        }
        let mut c2 = GenericArray::clone_from_slice(&block2);
        cipher.encrypt_block(&mut c2);

        Ok(buf[0x74..0x80] == known.rndpart
            && buf[0x80..0x90] == c1[..]
            && buf[0x90..0xA0] == c2[..])
    } else {
        match Hash72Info::extract_from_itunesdb(buf, None) {
            Ok(_) => Ok(true),
            Err(CryptoError::Hash72NotPresent | CryptoError::Hash72VerificationFailed) => Ok(false),
            Err(e) => Err(e),
        }
    }
}
