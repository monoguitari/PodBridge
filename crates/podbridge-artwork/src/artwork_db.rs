// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use byteorder::{LittleEndian, ReadBytesExt};
use encoding_rs::UTF_16LE;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::ArtworkError;
use crate::formats::{reconcile_specs_with_mhif, ArtworkFamily, IthmbFormatSpec};
use crate::rgb565::{
    decode_slot_to_png, decode_source_artwork, encode_all_formats, EncodedThumbnail,
};

pub const DEFAULT_MHFD_HEADER_LEN: usize = 132;
pub const DEFAULT_MHII_ART_HEADER_LEN: usize = 152;
pub const DEFAULT_MHNI_HEADER_LEN: usize = 76;
pub const DEFAULT_MHIF_HEADER_LEN: usize = 124;
/// Maximum size per `.ithmb` file (`256 MiB`) before rolling over to `_2.ithmb`, `_3.ithmb`, etc.
pub const MAX_ITHMB_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// Single `.ithmb` thumbnail descriptor (`mhni` + child `mhod type=3` filename).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtworkThumbnailRef {
    pub format_id: u32,
    pub ithmb_offset: u32,
    pub img_size: u32,
    pub vertical_padding: i16,
    pub horizontal_padding: i16,
    pub height: u16,
    pub width: u16,
    /// Colon-prefixed `.ithmb` filename, e.g. `:F1055_1.ithmb`.
    pub filename: String,
    pub raw_mhni_header: Vec<u8>,
}

/// Opaque child `mhod` inside an `mhii` preserved verbatim (e.g. `mhod type=6` wrapping `mhaf`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawArtworkMhod {
    pub mhod_type: u16,
    pub raw_bytes: Vec<u8>,
}

/// Parsed `mhii` artwork item inside `ArtworkDB`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtworkItem {
    /// Unique `mhii.id` (`100..`), referenced by `iTunesDB` `mhit.mhii_link`.
    pub id: u32,
    /// Representative track `dbid` (`mhit + 0x70`).
    pub song_id: u64,
    pub orig_img_size: u32,
    pub ref_count: u32,
    pub thumbnails: Vec<ArtworkThumbnailRef>,
    pub extra_mhods: Vec<RawArtworkMhod>,
    pub raw_header: Vec<u8>,
}

/// `.ithmb` format entry inside `mhsd 3 -> mhlf -> mhif`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtworkFormatFileEntry {
    pub format_id: u32,
    pub img_size: u32,
    pub raw_header: Vec<u8>,
}

/// Complete in-memory `ArtworkDB` with SHA-256 deduplication and `.ithmb` slot allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtworkDatabase {
    pub version: u32,
    pub next_id: u32,
    pub raw_mhfd_header: Vec<u8>,
    pub images: Vec<ArtworkItem>,
    pub raw_mhsd2_payload: Vec<u8>,
    pub format_files: Vec<ArtworkFormatFileEntry>,
    /// Session-scoped SHA-256 -> `mhii.id` deduplication cache for newly added covers.
    sha256_to_id: HashMap<[u8; 32], u32>,
}

/// Pending `.ithmb` write generated before `ArtworkDB` is atomically committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingIthmbWrite {
    /// Bare filename inside `iPod_Control/Artwork/`, e.g. `F1055_1.ithmb`.
    pub bare_filename: String,
    pub offset: u32,
    pub bytes: Vec<u8>,
}

#[inline]
fn ensure_slice(buf: &[u8], start: usize, len: usize) -> Result<&[u8], ArtworkError> {
    let end = start.checked_add(len).ok_or(ArtworkError::UnexpectedEof {
        offset: start,
        needed: len,
        remaining: buf.len().saturating_sub(start),
    })?;
    if end > buf.len() {
        return Err(ArtworkError::UnexpectedEof {
            offset: start,
            needed: len,
            remaining: buf.len().saturating_sub(start),
        });
    }
    Ok(&buf[start..end])
}

#[inline]
fn read_u16_at(buf: &[u8], off: usize) -> u16 {
    if off + 2 <= buf.len() {
        (&buf[off..off + 2]).read_u16::<LittleEndian>().unwrap_or(0)
    } else {
        0
    }
}

#[inline]
fn read_i16_at(buf: &[u8], off: usize) -> i16 {
    if off + 2 <= buf.len() {
        (&buf[off..off + 2]).read_i16::<LittleEndian>().unwrap_or(0)
    } else {
        0
    }
}

#[inline]
fn read_u32_at(buf: &[u8], off: usize) -> u32 {
    if off + 4 <= buf.len() {
        (&buf[off..off + 4]).read_u32::<LittleEndian>().unwrap_or(0)
    } else {
        0
    }
}

#[inline]
fn read_u64_at(buf: &[u8], off: usize) -> u64 {
    if off + 8 <= buf.len() {
        (&buf[off..off + 8]).read_u64::<LittleEndian>().unwrap_or(0)
    } else {
        0
    }
}

fn check_magic(buf: &[u8], offset: usize, expected: &'static str) -> Result<(), ArtworkError> {
    let s = ensure_slice(buf, offset, 4)?;
    if s != expected.as_bytes() {
        let mut actual = [0u8; 4];
        actual.copy_from_slice(s);
        return Err(ArtworkError::InvalidMagic {
            offset,
            expected,
            actual,
        });
    }
    Ok(())
}

impl ArtworkDatabase {
    /// Creates a brand-new empty `ArtworkDB` (`version = 6`, `next_id = 100`) initialized for `specs`.
    #[must_use]
    pub fn new_empty(specs: &[IthmbFormatSpec]) -> Self {
        let mut raw_mhfd = vec![0u8; DEFAULT_MHFD_HEADER_LEN];
        raw_mhfd[0x00..0x04].copy_from_slice(b"mhfd");
        raw_mhfd[0x04..0x08].copy_from_slice(&(DEFAULT_MHFD_HEADER_LEN as u32).to_le_bytes());
        raw_mhfd[0x10..0x14].copy_from_slice(&6u32.to_le_bytes()); // version = 6 (>= 2 mandatory)
        raw_mhfd[0x14..0x18].copy_from_slice(&3u32.to_le_bytes()); // 3 mhsd children
        raw_mhfd[0x1C..0x20].copy_from_slice(&100u32.to_le_bytes()); // next_id = 100
        raw_mhfd[0x30..0x34].copy_from_slice(&2u32.to_le_bytes()); // unk7 = 2

        let mut mhla = vec![0u8; 92];
        mhla[0x00..0x04].copy_from_slice(b"mhla");
        mhla[0x04..0x08].copy_from_slice(&92u32.to_le_bytes());

        let format_files = specs
            .iter()
            .map(|s| {
                let mut hdr = vec![0u8; DEFAULT_MHIF_HEADER_LEN];
                hdr[0x00..0x04].copy_from_slice(b"mhif");
                hdr[0x04..0x08].copy_from_slice(&(DEFAULT_MHIF_HEADER_LEN as u32).to_le_bytes());
                hdr[0x08..0x0C].copy_from_slice(&(DEFAULT_MHIF_HEADER_LEN as u32).to_le_bytes());
                ArtworkFormatFileEntry {
                    format_id: s.format_id,
                    img_size: s.image_byte_size() as u32,
                    raw_header: hdr,
                }
            })
            .collect();

        Self {
            version: 6,
            next_id: 100,
            raw_mhfd_header: raw_mhfd,
            images: Vec::new(),
            raw_mhsd2_payload: mhla,
            format_files,
            sha256_to_id: HashMap::new(),
        }
    }

    /// Returns the reconciled `.ithmb` format specs combining `family` defaults with any `mhif`
    /// records already stored in this `ArtworkDB`.
    #[must_use]
    pub fn effective_specs(&self, family: ArtworkFamily) -> Vec<IthmbFormatSpec> {
        let mhif_pairs: Vec<(u32, u32)> = self
            .format_files
            .iter()
            .map(|f| (f.format_id, f.img_size))
            .collect();
        reconcile_specs_with_mhif(family.default_specs(), &mhif_pairs)
    }

    /// Registers compressed cover art (`JPEG`/`PNG`/`WebP`/`BMP`), deduplicating by SHA-256 within
    /// the session. If already registered, increments `ref_count` and returns `(mhii_id, empty_writes)`.
    /// Otherwise encodes all `specs`, allocates free `.ithmb` slots, and returns `(mhii_id, writes)`.
    pub fn register_artwork_bytes(
        &mut self,
        source_bytes: &[u8],
        song_dbid: u64,
        specs: &[IthmbFormatSpec],
    ) -> Result<(u32, Vec<PendingIthmbWrite>), ArtworkError> {
        let digest: [u8; 32] = Sha256::digest(source_bytes).into();
        if let Some(&existing_id) = self.sha256_to_id.get(&digest) {
            if let Some(item) = self.images.iter_mut().find(|im| im.id == existing_id) {
                item.ref_count = item.ref_count.saturating_add(1);
                return Ok((existing_id, Vec::new()));
            }
        }

        let decoded = decode_source_artwork(source_bytes)?;
        let encoded_thumbs = encode_all_formats(&decoded, specs);
        Ok(self.insert_encoded_thumbnails(
            Some(digest),
            source_bytes.len() as u32,
            song_dbid,
            encoded_thumbs,
        ))
    }

    /// Inserts pre-encoded thumbnails (useful when the pipeline pre-encodes thumbnails on worker
    /// threads before handing off to the sequential USB writer).
    pub fn insert_encoded_thumbnails(
        &mut self,
        source_sha256: Option<[u8; 32]>,
        orig_img_size: u32,
        song_dbid: u64,
        encoded_thumbs: Vec<EncodedThumbnail>,
    ) -> (u32, Vec<PendingIthmbWrite>) {
        if let Some(digest) = source_sha256 {
            if let Some(&existing_id) = self.sha256_to_id.get(&digest) {
                if let Some(item) = self.images.iter_mut().find(|im| im.id == existing_id) {
                    item.ref_count = item.ref_count.saturating_add(1);
                    return (existing_id, Vec::new());
                }
            }
        }

        let mhii_id = self.next_id.max(100);
        self.next_id = mhii_id.saturating_add(1);

        let mut thumbs = Vec::with_capacity(encoded_thumbs.len());
        let mut writes = Vec::with_capacity(encoded_thumbs.len());

        for enc in encoded_thumbs {
            let slot_size = enc.bytes.len() as u32;
            self.ensure_format_file_entry(enc.format_id, slot_size);
            let (colon_name, bare_name, offset) = self.alloc_free_slot(enc.format_id, slot_size);

            let mut raw_mhni = vec![0u8; DEFAULT_MHNI_HEADER_LEN];
            raw_mhni[0x00..0x04].copy_from_slice(b"mhni");
            raw_mhni[0x04..0x08].copy_from_slice(&(DEFAULT_MHNI_HEADER_LEN as u32).to_le_bytes());

            thumbs.push(ArtworkThumbnailRef {
                format_id: enc.format_id,
                ithmb_offset: offset,
                img_size: slot_size,
                vertical_padding: enc.vertical_padding,
                horizontal_padding: enc.horizontal_padding,
                height: enc.height,
                width: enc.width,
                filename: colon_name,
                raw_mhni_header: raw_mhni,
            });
            writes.push(PendingIthmbWrite {
                bare_filename: bare_name,
                offset,
                bytes: enc.bytes,
            });
        }

        let mut raw_hdr = vec![0u8; DEFAULT_MHII_ART_HEADER_LEN];
        raw_hdr[0x00..0x04].copy_from_slice(b"mhii");
        raw_hdr[0x04..0x08].copy_from_slice(&(DEFAULT_MHII_ART_HEADER_LEN as u32).to_le_bytes());
        raw_hdr[0x3C..0x40].copy_from_slice(&1u32.to_le_bytes()); // unk_0x3C = 1
        raw_hdr[0x48..0x50].copy_from_slice(&0x7FF8_0000_0000_0000u64.to_le_bytes());
        raw_hdr[0x50..0x58].copy_from_slice(&0x7FF8_0000_0000_0000u64.to_le_bytes());

        self.images.push(ArtworkItem {
            id: mhii_id,
            song_id: song_dbid,
            orig_img_size,
            ref_count: 1,
            thumbnails: thumbs,
            extra_mhods: Vec::new(),
            raw_header: raw_hdr,
        });

        if let Some(digest) = source_sha256 {
            self.sha256_to_id.insert(digest, mhii_id);
        }

        (mhii_id, writes)
    }

    /// Increments `ref_count` and returns `Some(mhii_id)` if `sha256` was already registered in
    /// this session.
    pub fn increment_ref_by_sha256(&mut self, sha256: &[u8; 32]) -> Option<u32> {
        let id = *self.sha256_to_id.get(sha256)?;
        let item = self.images.iter_mut().find(|im| im.id == id)?;
        item.ref_count = item.ref_count.saturating_add(1);
        Some(id)
    }

    /// Decrements `ref_count` for `mhii_id` when a referencing track is deleted, removing the
    /// `ArtworkItem` (and freeing its `.ithmb` slots for reuse) when `ref_count` reaches `0`.
    pub fn release_artwork_ref(&mut self, mhii_id: u32) {
        if mhii_id == 0 {
            return;
        }
        if let Some(idx) = self.images.iter().position(|im| im.id == mhii_id) {
            if self.images[idx].ref_count > 1 {
                self.images[idx].ref_count -= 1;
            } else {
                self.images.remove(idx);
                self.sha256_to_id.retain(|_, &mut id| id != mhii_id);
            }
        }
    }

    fn ensure_format_file_entry(&mut self, format_id: u32, img_size: u32) {
        if !self.format_files.iter().any(|f| f.format_id == format_id) {
            let mut hdr = vec![0u8; DEFAULT_MHIF_HEADER_LEN];
            hdr[0x00..0x04].copy_from_slice(b"mhif");
            hdr[0x04..0x08].copy_from_slice(&(DEFAULT_MHIF_HEADER_LEN as u32).to_le_bytes());
            hdr[0x08..0x0C].copy_from_slice(&(DEFAULT_MHIF_HEADER_LEN as u32).to_le_bytes());
            self.format_files.push(ArtworkFormatFileEntry {
                format_id,
                img_size,
                raw_header: hdr,
            });
        }
    }

    /// Finds the lowest unused `(file_index, byte_offset)` slot for `format_id`.
    fn alloc_free_slot(&self, format_id: u32, slot_size: u32) -> (String, String, u32) {
        let mut occupied: HashSet<(u32, u32)> = HashSet::new();
        let prefix = format!("F{format_id}_");

        for im in &self.images {
            for th in &im.thumbnails {
                if th.format_id == format_id {
                    let bare = th.filename.trim_start_matches([':', '/', '\\']);
                    let file_num = bare
                        .strip_prefix(&prefix)
                        .and_then(|s| s.strip_suffix(".ithmb"))
                        .and_then(|n| n.parse::<u32>().ok())
                        .unwrap_or(1);
                    occupied.insert((file_num, th.ithmb_offset));
                }
            }
        }

        let step = slot_size.max(1);
        let mut file_idx = 1u32;
        loop {
            let mut offset = 0u64;
            while offset + u64::from(step) <= MAX_ITHMB_FILE_BYTES {
                let off32 = offset as u32;
                if !occupied.contains(&(file_idx, off32)) {
                    let bare = format!("F{format_id}_{file_idx}.ithmb");
                    let colon = format!(":{bare}");
                    return (colon, bare, off32);
                }
                offset += u64::from(step);
            }
            file_idx += 1;
        }
    }

    /// Reads a single thumbnail for `mhii_id` from `artwork_dir` (`iPod_Control/Artwork`) via
    /// direct `seek` and decodes it to PNG bytes in `< 1ms`.
    pub fn read_thumbnail_png(
        &self,
        artwork_dir: &Path,
        mhii_id: u32,
        preferred_min_width: u16,
    ) -> Result<Option<Vec<u8>>, ArtworkError> {
        let Some(item) = self.images.iter().find(|im| im.id == mhii_id) else {
            return Ok(None);
        };
        if item.thumbnails.is_empty() {
            return Ok(None);
        }

        // Choose smallest thumbnail >= preferred_min_width, or largest available
        let mut candidates: Vec<&ArtworkThumbnailRef> = item.thumbnails.iter().collect();
        candidates.sort_by_key(|t| t.width);
        let chosen = candidates
            .iter()
            .copied()
            .find(|t| t.width >= preferred_min_width)
            .or_else(|| candidates.last().copied())
            .expect("non-empty thumbnails");

        let bare = chosen.filename.trim_start_matches([':', '/', '\\']);
        let ithmb_path = artwork_dir.join(bare);
        if !ithmb_path.exists() {
            return Ok(None);
        }

        let mut file = fs::File::open(ithmb_path)?;
        file.seek(SeekFrom::Start(u64::from(chosen.ithmb_offset)))?;
        let mut slot = vec![0u8; chosen.img_size as usize];
        file.read_exact(&mut slot)?;

        let h = usize::from(chosen.height.max(1));
        let stride = slot.len() / h;
        let png = decode_slot_to_png(&slot, chosen.width, chosen.height, stride)?;
        Ok(Some(png))
    }
}

/// Applies pending `.ithmb` slot writes sequentially into `artwork_dir` (`iPod_Control/Artwork`).
pub fn flush_pending_ithmb_writes(
    artwork_dir: &Path,
    writes: &[PendingIthmbWrite],
) -> Result<(), ArtworkError> {
    if writes.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(artwork_dir)?;

    // Group writes by .ithmb filename so each file is opened, written sequentially, and fsynced once
    let mut grouped: HashMap<&str, Vec<&PendingIthmbWrite>> = HashMap::new();
    for w in writes {
        grouped.entry(&w.bare_filename).or_default().push(w);
    }

    let mut filenames: Vec<&str> = grouped.keys().copied().collect();
    filenames.sort_unstable();

    for name in filenames {
        let mut items = grouped.remove(name).unwrap_or_default();
        items.sort_by_key(|w| w.offset);

        let path = artwork_dir.join(name);
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;

        for w in items {
            file.seek(SeekFrom::Start(u64::from(w.offset)))?;
            file.write_all(&w.bytes)?;
        }
        file.sync_data()?;
    }

    Ok(())
}

/// Parses binary `ArtworkDB` bytes into an [`ArtworkDatabase`].
pub fn parse_artworkdb(bytes: &[u8]) -> Result<ArtworkDatabase, ArtworkError> {
    check_magic(bytes, 0, "mhfd")?;
    let head = ensure_slice(bytes, 0, 32)?;
    let hdr_len = read_u32_at(head, 0x04) as usize;
    let total_len = read_u32_at(head, 0x08) as usize;
    let version = read_u32_at(head, 0x10);
    let children_count = read_u32_at(head, 0x14) as usize;
    let next_id = read_u32_at(head, 0x1C).max(100);

    if hdr_len < 0x34 || total_len < hdr_len || total_len > bytes.len() {
        return Err(ArtworkError::InvalidChunkBounds {
            offset: 0,
            chunk: "mhfd",
            header_len: hdr_len,
            total_len,
        });
    }

    let raw_mhfd_header = bytes[0..hdr_len].to_vec();
    let mut images = Vec::new();
    let mut raw_mhsd2_payload = {
        let mut mhla = vec![0u8; 92];
        mhla[0x00..0x04].copy_from_slice(b"mhla");
        mhla[0x04..0x08].copy_from_slice(&92u32.to_le_bytes());
        mhla
    };
    let mut format_files = Vec::new();
    let mut max_seen_id = next_id.saturating_sub(1);

    let mut pos = hdr_len;
    for _ in 0..children_count {
        check_magic(bytes, pos, "mhsd")?;
        let sh = ensure_slice(bytes, pos, 16)?;
        let shdr = read_u32_at(sh, 0x04) as usize;
        let stot = read_u32_at(sh, 0x08) as usize;
        let stype = u32::from(read_u16_at(sh, 0x0C));

        if shdr < 16 || stot < shdr || pos + stot > total_len {
            return Err(ArtworkError::InvalidChunkBounds {
                offset: pos,
                chunk: "mhsd(ArtworkDB)",
                header_len: shdr,
                total_len: stot,
            });
        }

        let child_off = pos + shdr;
        let child_len = stot - shdr;
        match stype {
            1 => {
                images = parse_artwork_mhli(bytes, child_off, child_len, &mut max_seen_id)?;
            }
            2 => {
                raw_mhsd2_payload = bytes[child_off..child_off + child_len].to_vec();
            }
            3 => {
                format_files = parse_artwork_mhlf(bytes, child_off, child_len)?;
            }
            _ => {}
        }

        pos += stot;
    }

    Ok(ArtworkDatabase {
        version: version.max(2),
        next_id: next_id.max(max_seen_id.saturating_add(1)),
        raw_mhfd_header,
        images,
        raw_mhsd2_payload,
        format_files,
        sha256_to_id: HashMap::new(),
    })
}

fn parse_artwork_mhli(
    bytes: &[u8],
    offset: usize,
    section_len: usize,
    max_seen_id: &mut u32,
) -> Result<Vec<ArtworkItem>, ArtworkError> {
    check_magic(bytes, offset, "mhli")?;
    let head = ensure_slice(bytes, offset, 12)?;
    let hdr_len = read_u32_at(head, 0x04) as usize;
    let count = read_u32_at(head, 0x08) as usize;
    if hdr_len < 12 || hdr_len > section_len {
        return Err(ArtworkError::InvalidChunkBounds {
            offset,
            chunk: "mhli",
            header_len: hdr_len,
            total_len: section_len,
        });
    }

    let mut items = Vec::with_capacity(count);
    let mut pos = offset + hdr_len;
    let end = offset + section_len;

    for _ in 0..count {
        check_magic(bytes, pos, "mhii")?;
        let iprobe = ensure_slice(bytes, pos, 28)?;
        let ihdr = read_u32_at(iprobe, 0x04) as usize;
        let itot = read_u32_at(iprobe, 0x08) as usize;
        let children = read_u32_at(iprobe, 0x0C) as usize;
        if ihdr < 0x3C || itot < ihdr || pos + itot > end {
            return Err(ArtworkError::InvalidChunkBounds {
                offset: pos,
                chunk: "mhii",
                header_len: ihdr,
                total_len: itot,
            });
        }

        let raw_hdr = bytes[pos..pos + ihdr].to_vec();
        let id = read_u32_at(&raw_hdr, 0x10);
        *max_seen_id = (*max_seen_id).max(id);

        let mut thumbnails = Vec::new();
        let mut extra_mhods = Vec::new();

        let mut cpos = pos + ihdr;
        let iend = pos + itot;
        for _ in 0..children {
            check_magic(bytes, cpos, "mhod")?;
            let mhead = ensure_slice(bytes, cpos, 16)?;
            let mhdr = read_u32_at(mhead, 0x04) as usize;
            let mtot = read_u32_at(mhead, 0x08) as usize;
            let mtype = read_u16_at(mhead, 0x0C);
            if mhdr < 16 || mtot < mhdr || cpos + mtot > iend {
                return Err(ArtworkError::InvalidChunkBounds {
                    offset: cpos,
                    chunk: "mhod(mhii)",
                    header_len: mhdr,
                    total_len: mtot,
                });
            }

            if mtype == 2
                && cpos + mhdr + 4 <= iend
                && &bytes[cpos + mhdr..cpos + mhdr + 4] == b"mhni"
            {
                let thumb = parse_mhni(bytes, cpos + mhdr, mtot - mhdr)?;
                thumbnails.push(thumb);
            } else {
                extra_mhods.push(RawArtworkMhod {
                    mhod_type: mtype,
                    raw_bytes: bytes[cpos..cpos + mtot].to_vec(),
                });
            }
            cpos += mtot;
        }

        items.push(ArtworkItem {
            id,
            song_id: read_u64_at(&raw_hdr, 0x14),
            orig_img_size: read_u32_at(&raw_hdr, 0x30),
            ref_count: read_u32_at(&raw_hdr, 0x38).max(1),
            thumbnails,
            extra_mhods,
            raw_header: raw_hdr,
        });

        pos += itot;
    }

    Ok(items)
}

fn parse_mhni(
    bytes: &[u8],
    offset: usize,
    max_len: usize,
) -> Result<ArtworkThumbnailRef, ArtworkError> {
    check_magic(bytes, offset, "mhni")?;
    let head = ensure_slice(bytes, offset, 44)?;
    let hdr_len = read_u32_at(head, 0x04) as usize;
    let tot_len = read_u32_at(head, 0x08) as usize;
    let children = read_u32_at(head, 0x0C) as usize;
    if hdr_len < 44 || tot_len < hdr_len || tot_len > max_len {
        return Err(ArtworkError::InvalidChunkBounds {
            offset,
            chunk: "mhni",
            header_len: hdr_len,
            total_len: tot_len,
        });
    }

    let raw_mhni = bytes[offset..offset + hdr_len].to_vec();
    let format_id = read_u32_at(&raw_mhni, 0x10);
    let mut filename = format!(":F{format_id}_1.ithmb");

    let mut cpos = offset + hdr_len;
    let end = offset + tot_len;
    for _ in 0..children {
        check_magic(bytes, cpos, "mhod")?;
        let mhead = ensure_slice(bytes, cpos, 16)?;
        let mhdr = read_u32_at(mhead, 0x04) as usize;
        let mtot = read_u32_at(mhead, 0x08) as usize;
        let mtype = read_u16_at(mhead, 0x0C);
        if mhdr < 24 || mtot < mhdr || cpos + mtot > end {
            return Err(ArtworkError::InvalidChunkBounds {
                offset: cpos,
                chunk: "mhod(mhni)",
                header_len: mhdr,
                total_len: mtot,
            });
        }
        if mtype == 3 && mtot >= 36 {
            let str_byte_len = read_u32_at(bytes, cpos + 0x18) as usize;
            let encoding = read_u32_at(bytes, cpos + 0x1C);
            let s_start = cpos + 36;
            let s_end = s_start.saturating_add(str_byte_len).min(cpos + mtot);
            let raw_str = &bytes[s_start..s_end];
            filename = if encoding == 2 {
                let (cow, _, _) = UTF_16LE.decode(raw_str);
                cow.into_owned()
            } else {
                String::from_utf8_lossy(raw_str).into_owned()
            };
        }
        cpos += mtot;
    }

    Ok(ArtworkThumbnailRef {
        format_id,
        ithmb_offset: read_u32_at(&raw_mhni, 0x14),
        img_size: read_u32_at(&raw_mhni, 0x18),
        vertical_padding: read_i16_at(&raw_mhni, 0x1C),
        horizontal_padding: read_i16_at(&raw_mhni, 0x1E),
        height: read_u16_at(&raw_mhni, 0x20),
        width: read_u16_at(&raw_mhni, 0x22),
        filename,
        raw_mhni_header: raw_mhni,
    })
}

fn parse_artwork_mhlf(
    bytes: &[u8],
    offset: usize,
    section_len: usize,
) -> Result<Vec<ArtworkFormatFileEntry>, ArtworkError> {
    check_magic(bytes, offset, "mhlf")?;
    let head = ensure_slice(bytes, offset, 12)?;
    let hdr_len = read_u32_at(head, 0x04) as usize;
    let count = read_u32_at(head, 0x08) as usize;
    if hdr_len < 12 || hdr_len > section_len {
        return Err(ArtworkError::InvalidChunkBounds {
            offset,
            chunk: "mhlf",
            header_len: hdr_len,
            total_len: section_len,
        });
    }

    let mut files = Vec::with_capacity(count);
    let mut pos = offset + hdr_len;
    let end = offset + section_len;

    for _ in 0..count {
        check_magic(bytes, pos, "mhif")?;
        let probe = ensure_slice(bytes, pos, 24)?;
        let fhdr = read_u32_at(probe, 0x04) as usize;
        let ftot = read_u32_at(probe, 0x08) as usize;
        if fhdr < 24 || ftot < fhdr || pos + ftot > end {
            return Err(ArtworkError::InvalidChunkBounds {
                offset: pos,
                chunk: "mhif",
                header_len: fhdr,
                total_len: ftot,
            });
        }
        files.push(ArtworkFormatFileEntry {
            format_id: read_u32_at(probe, 0x10),
            img_size: read_u32_at(probe, 0x14),
            raw_header: bytes[pos..pos + ftot].to_vec(),
        });
        pos += ftot;
    }

    Ok(files)
}

/// Serializes an [`ArtworkDatabase`] into its exact on-disk binary representation.
#[must_use]
pub fn serialize_artworkdb(db: &ArtworkDatabase) -> Vec<u8> {
    let mhfd_len = db.raw_mhfd_header.len().max(DEFAULT_MHFD_HEADER_LEN);
    let mut mhfd = db.raw_mhfd_header.clone();
    mhfd.resize(mhfd_len, 0);
    mhfd[0x00..0x04].copy_from_slice(b"mhfd");
    mhfd[0x04..0x08].copy_from_slice(&(mhfd_len as u32).to_le_bytes());
    mhfd[0x10..0x14].copy_from_slice(&db.version.max(2).to_le_bytes());
    mhfd[0x14..0x18].copy_from_slice(&3u32.to_le_bytes());
    mhfd[0x1C..0x20].copy_from_slice(&db.next_id.max(100).to_le_bytes());
    mhfd[0x30..0x34].copy_from_slice(&2u32.to_le_bytes());

    let mut sections = Vec::new();

    // mhsd 1: mhli -> mhii
    write_art_mhsd(&mut sections, 1, |out| {
        let mhli_start = out.len();
        out.extend_from_slice(b"mhli");
        out.extend_from_slice(&92u32.to_le_bytes());
        out.extend_from_slice(&(db.images.len() as u32).to_le_bytes());
        out.resize(mhli_start + 92, 0);

        for im in &db.images {
            let istart = out.len();
            let ihdr_len = im.raw_header.len().max(DEFAULT_MHII_ART_HEADER_LEN);
            out.extend_from_slice(&im.raw_header);
            out.resize(istart + ihdr_len, 0);

            let hdr = &mut out[istart..istart + ihdr_len];
            hdr[0x00..0x04].copy_from_slice(b"mhii");
            hdr[0x04..0x08].copy_from_slice(&(ihdr_len as u32).to_le_bytes());
            hdr[0x10..0x14].copy_from_slice(&im.id.to_le_bytes());
            hdr[0x14..0x1C].copy_from_slice(&im.song_id.to_le_bytes());
            hdr[0x30..0x34].copy_from_slice(&im.orig_img_size.to_le_bytes());
            hdr[0x38..0x3C].copy_from_slice(&im.ref_count.to_le_bytes());

            let mut children = 0u32;
            for th in &im.thumbnails {
                write_mhod_type2_mhni(out, th);
                children += 1;
            }
            for extra in &im.extra_mhods {
                out.extend_from_slice(&extra.raw_bytes);
                children += 1;
            }

            let itot = (out.len() - istart) as u32;
            out[istart + 0x08..istart + 0x0C].copy_from_slice(&itot.to_le_bytes());
            out[istart + 0x0C..istart + 0x10].copy_from_slice(&children.to_le_bytes());
        }
    });

    // mhsd 2: mhla
    write_art_mhsd(&mut sections, 2, |out| {
        out.extend_from_slice(&db.raw_mhsd2_payload);
    });

    // mhsd 3: mhlf -> mhif
    write_art_mhsd(&mut sections, 3, |out| {
        let mhlf_start = out.len();
        out.extend_from_slice(b"mhlf");
        out.extend_from_slice(&92u32.to_le_bytes());
        out.extend_from_slice(&(db.format_files.len() as u32).to_le_bytes());
        out.resize(mhlf_start + 92, 0);

        for f in &db.format_files {
            let fstart = out.len();
            let fhdr_len = f.raw_header.len().max(DEFAULT_MHIF_HEADER_LEN);
            out.extend_from_slice(&f.raw_header);
            out.resize(fstart + fhdr_len, 0);
            out[fstart..fstart + 4].copy_from_slice(b"mhif");
            out[fstart + 0x04..fstart + 0x08].copy_from_slice(&(fhdr_len as u32).to_le_bytes());
            out[fstart + 0x08..fstart + 0x0C].copy_from_slice(&(fhdr_len as u32).to_le_bytes());
            out[fstart + 0x10..fstart + 0x14].copy_from_slice(&f.format_id.to_le_bytes());
            out[fstart + 0x14..fstart + 0x18].copy_from_slice(&f.img_size.to_le_bytes());
        }
    });

    let total_len = (mhfd.len() + sections.len()) as u32;
    mhfd[0x08..0x0C].copy_from_slice(&total_len.to_le_bytes());

    let mut out = Vec::with_capacity(total_len as usize);
    out.extend_from_slice(&mhfd);
    out.extend_from_slice(&sections);
    out
}

fn write_art_mhsd<F: FnOnce(&mut Vec<u8>)>(out: &mut Vec<u8>, index: u16, writer: F) {
    let start = out.len();
    out.extend_from_slice(b"mhsd");
    out.extend_from_slice(&96u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&u32::from(index).to_le_bytes());
    out.resize(start + 96, 0);
    writer(out);
    let tot = (out.len() - start) as u32;
    out[start + 0x08..start + 0x0C].copy_from_slice(&tot.to_le_bytes());
}

fn write_mhod_type2_mhni(out: &mut Vec<u8>, th: &ArtworkThumbnailRef) {
    // Container mhod type=2 (24 bytes)
    let m2_start = out.len();
    out.extend_from_slice(b"mhod");
    out.extend_from_slice(&24u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.resize(m2_start + 24, 0);

    // mhni (76 bytes)
    let mhni_start = out.len();
    let nhdr_len = th.raw_mhni_header.len().max(DEFAULT_MHNI_HEADER_LEN);
    out.extend_from_slice(&th.raw_mhni_header);
    out.resize(mhni_start + nhdr_len, 0);
    {
        let nhdr = &mut out[mhni_start..mhni_start + nhdr_len];
        nhdr[0x00..0x04].copy_from_slice(b"mhni");
        nhdr[0x04..0x08].copy_from_slice(&(nhdr_len as u32).to_le_bytes());
        nhdr[0x0C..0x10].copy_from_slice(&1u32.to_le_bytes());
        nhdr[0x10..0x14].copy_from_slice(&th.format_id.to_le_bytes());
        nhdr[0x14..0x18].copy_from_slice(&th.ithmb_offset.to_le_bytes());
        nhdr[0x18..0x1C].copy_from_slice(&th.img_size.to_le_bytes());
        nhdr[0x1C..0x1E].copy_from_slice(&th.vertical_padding.to_le_bytes());
        nhdr[0x1E..0x20].copy_from_slice(&th.horizontal_padding.to_le_bytes());
        nhdr[0x20..0x22].copy_from_slice(&th.height.to_le_bytes());
        nhdr[0x22..0x24].copy_from_slice(&th.width.to_le_bytes());
        nhdr[0x28..0x2C].copy_from_slice(&th.img_size.to_le_bytes()); // img_size2 mirror!
    }

    // Child mhod type=3 (36B header+subheader + UTF-16LE filename + 4-byte alignment padding)
    let m3_start = out.len();
    let utf16: Vec<u8> = th
        .filename
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let unpadded = 36 + utf16.len();
    let padded = (unpadded + 3) & !3;
    let pad_len = (padded - unpadded) as u8;

    out.extend_from_slice(b"mhod");
    out.extend_from_slice(&24u32.to_le_bytes());
    out.extend_from_slice(&(padded as u32).to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes());
    out.push(0);
    out.push(pad_len);
    out.extend_from_slice(&[0u8; 8]);
    out.extend_from_slice(&(utf16.len() as u32).to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes()); // encoding = 2 (UTF-16LE in ArtworkDB!)
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&utf16);
    out.resize(m3_start + padded, 0);

    let mhni_tot = (out.len() - mhni_start) as u32;
    out[mhni_start + 0x08..mhni_start + 0x0C].copy_from_slice(&mhni_tot.to_le_bytes());

    let m2_tot = (out.len() - m2_start) as u32;
    out[m2_start + 0x08..m2_start + 0x0C].copy_from_slice(&m2_tot.to_le_bytes());
}
