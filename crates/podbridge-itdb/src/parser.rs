// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use byteorder::{LittleEndian, ReadBytesExt};
use encoding_rs::UTF_16LE;
use podbridge_crypto::Hash72Info;

use crate::chunks::{
    AlbumItem, ArtistItem, ItdbDatabase, MhbdHeader, Playlist, PlaylistItem, RawMhod,
    RawMhsdSection, TrackRecord,
};
use crate::error::ItdbError;

#[inline]
fn ensure_slice(buf: &[u8], start: usize, len: usize) -> Result<&[u8], ItdbError> {
    let end = start.checked_add(len).ok_or(ItdbError::UnexpectedEof {
        offset: start,
        needed: len,
        remaining: buf.len().saturating_sub(start),
    })?;
    if end > buf.len() {
        return Err(ItdbError::UnexpectedEof {
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
fn read_i32_at(buf: &[u8], off: usize) -> i32 {
    if off + 4 <= buf.len() {
        (&buf[off..off + 4]).read_i32::<LittleEndian>().unwrap_or(0)
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

fn check_magic(buf: &[u8], offset: usize, expected: &'static str) -> Result<(), ItdbError> {
    let magic_slice = ensure_slice(buf, offset, 4)?;
    if magic_slice != expected.as_bytes() {
        let mut actual = [0u8; 4];
        actual.copy_from_slice(magic_slice);
        return Err(ItdbError::InvalidMagic {
            offset,
            expected,
            actual,
        });
    }
    Ok(())
}

/// Parses an entire binary `iTunesDB` slice into an [`ItdbDatabase`].
pub fn parse_itunesdb(bytes: &[u8]) -> Result<ItdbDatabase, ItdbError> {
    check_magic(bytes, 0, "mhbd")?;
    let hdr_probe = ensure_slice(bytes, 0, 24)?;
    let header_len = read_u32_at(hdr_probe, 0x04) as usize;
    let total_len = read_u32_at(hdr_probe, 0x08) as usize;
    if header_len < 0x68 || total_len < header_len || total_len > bytes.len() {
        return Err(ItdbError::InvalidChunkBounds {
            offset: 0,
            chunk: "mhbd",
            header_len,
            total_len,
        });
    }

    let raw_hdr = bytes[0..header_len].to_vec();
    let mut db_lang = [0u8; 2];
    if header_len >= 0x48 {
        db_lang.copy_from_slice(&raw_hdr[0x46..0x48]);
    } else {
        db_lang = *b"en";
    }

    let header = MhbdHeader {
        version: read_u32_at(&raw_hdr, 0x10),
        db_id: read_u64_at(&raw_hdr, 0x18),
        platform: read_u16_at(&raw_hdr, 0x20),
        unk_0x22: read_u16_at(&raw_hdr, 0x22),
        library_id: read_u64_at(&raw_hdr, 0x24),
        hashing_scheme: read_u16_at(&raw_hdr, 0x30),
        db_language: db_lang,
        pid: read_u64_at(&raw_hdr, 0x48),
        tz_offset: read_i32_at(&raw_hdr, 0x6C),
        hash72_type: read_u16_at(&raw_hdr, 0x70),
        audio_language: if header_len >= 0xA2 {
            read_i16_at(&raw_hdr, 0xA0)
        } else {
            -1
        },
        subtitle_language: if header_len >= 0xA4 {
            read_i16_at(&raw_hdr, 0xA2)
        } else {
            -1
        },
        raw_header: raw_hdr,
    };

    let embedded_hash72 = Hash72Info::extract_from_itunesdb(&bytes[0..total_len], None).ok();
    let children_count = read_u32_at(bytes, 0x14) as usize;

    let mut albums = Vec::new();
    let mut tracks = Vec::new();
    let mut podcast_playlists = Vec::new();
    let mut playlists = Vec::new();
    let mut category_playlists = Vec::new();
    let mut artists = Vec::new();
    let mut section_order = Vec::with_capacity(children_count);
    let mut extra_sections = Vec::new();
    let mut has_track_section = false;
    let mut max_catalog_id = 1000u32;

    let mut pos = header_len;
    for _ in 0..children_count {
        check_magic(bytes, pos, "mhsd")?;
        let mhsd_head = ensure_slice(bytes, pos, 16)?;
        let mhsd_hdr_len = read_u32_at(mhsd_head, 0x04) as usize;
        let mhsd_total_len = read_u32_at(mhsd_head, 0x08) as usize;
        let section_type = read_u32_at(mhsd_head, 0x0C);

        if mhsd_hdr_len < 16 || mhsd_total_len < mhsd_hdr_len {
            return Err(ItdbError::InvalidChunkBounds {
                offset: pos,
                chunk: "mhsd",
                header_len: mhsd_hdr_len,
                total_len: mhsd_total_len,
            });
        }
        let section_slice = ensure_slice(bytes, pos, mhsd_total_len)?;
        section_order.push(section_type);

        let child_offset = pos + mhsd_hdr_len;
        let child_len = mhsd_total_len - mhsd_hdr_len;

        match section_type {
            1 => {
                has_track_section = true;
                tracks = parse_mhlt(bytes, child_offset, child_len, &mut max_catalog_id)?;
            }
            2 => {
                playlists = parse_mhlp(bytes, child_offset, child_len, &mut max_catalog_id)?;
            }
            3 => {
                podcast_playlists =
                    parse_mhlp(bytes, child_offset, child_len, &mut max_catalog_id)?;
            }
            4 => {
                albums = parse_mhla(bytes, child_offset, child_len, &mut max_catalog_id)?;
            }
            5 if child_len >= 4 && &bytes[child_offset..child_offset + 4] == b"mhlp" => {
                // On Classic 6G (version=0x73), mhsd type=5 is mhlp (Category Smart Playlists)
                category_playlists =
                    parse_mhlp(bytes, child_offset, child_len, &mut max_catalog_id)?;
            }
            8 if child_len >= 4 && &bytes[child_offset..child_offset + 4] == b"mhli" => {
                artists = parse_mhli_artists(bytes, child_offset, child_len, &mut max_catalog_id)?;
            }
            _ => {
                extra_sections.push(RawMhsdSection {
                    section_type,
                    raw_bytes: section_slice.to_vec(),
                });
            }
        }

        pos += mhsd_total_len;
    }

    if !has_track_section {
        return Err(ItdbError::MissingTrackSection);
    }

    Ok(ItdbDatabase {
        header,
        albums,
        tracks,
        podcast_playlists,
        playlists,
        category_playlists,
        artists,
        section_order,
        extra_sections,
        embedded_hash72,
        next_catalog_id: max_catalog_id.saturating_add(1),
        dirty_library_indices: false,
    })
}

fn parse_string_mhod_payload(mhod_slice: &[u8], hdr_len: usize, mhod_type: u32) -> Option<String> {
    // Podcast URLs (15, 16) store raw UTF-8 directly after the 24-byte mhod header
    if mhod_type == 15 || mhod_type == 16 {
        let raw = &mhod_slice[hdr_len..];
        return Some(String::from_utf8_lossy(raw).into_owned());
    }

    if mhod_slice.len() < hdr_len + 16 {
        return None;
    }
    let encoding = read_u32_at(mhod_slice, hdr_len);
    let str_byte_len = read_u32_at(mhod_slice, hdr_len + 4) as usize;
    let str_start = hdr_len + 16;
    let str_end = str_start.checked_add(str_byte_len)?.min(mhod_slice.len());
    let payload = &mhod_slice[str_start..str_end];

    let decoded = if encoding == 2 || encoding == 0 {
        String::from_utf8_lossy(payload).into_owned()
    } else {
        let (cow, _, _) = UTF_16LE.decode(payload);
        cow.into_owned()
    };
    Some(decoded)
}

fn parse_mhlt(
    bytes: &[u8],
    offset: usize,
    section_len: usize,
    max_catalog_id: &mut u32,
) -> Result<Vec<TrackRecord>, ItdbError> {
    check_magic(bytes, offset, "mhlt")?;
    let head = ensure_slice(bytes, offset, 12)?;
    let hdr_len = read_u32_at(head, 0x04) as usize;
    let track_count = read_u32_at(head, 0x08) as usize;
    if hdr_len < 12 || hdr_len > section_len {
        return Err(ItdbError::InvalidChunkBounds {
            offset,
            chunk: "mhlt",
            header_len: hdr_len,
            total_len: section_len,
        });
    }

    let mut tracks = Vec::with_capacity(track_count);
    let mut pos = offset + hdr_len;
    let end = offset + section_len;

    for _ in 0..track_count {
        check_magic(bytes, pos, "mhit")?;
        let mhit_probe = ensure_slice(bytes, pos, 16)?;
        let mhit_hdr_len = read_u32_at(mhit_probe, 0x04) as usize;
        let mhit_tot_len = read_u32_at(mhit_probe, 0x08) as usize;
        let num_mhods = read_u32_at(mhit_probe, 0x0C) as usize;

        if mhit_hdr_len < 0x9C || mhit_tot_len < mhit_hdr_len || pos + mhit_tot_len > end {
            return Err(ItdbError::InvalidChunkBounds {
                offset: pos,
                chunk: "mhit",
                header_len: mhit_hdr_len,
                total_len: mhit_tot_len,
            });
        }

        let raw_hdr = bytes[pos..pos + mhit_hdr_len].to_vec();
        let track_id = read_u32_at(&raw_hdr, 0x10);
        let album_id = read_u32_at(&raw_hdr, 0x120);
        let artist_id = read_u32_at(&raw_hdr, 0x1E0);
        let composer_id = read_u32_at(&raw_hdr, 0x1F4);
        *max_catalog_id = (*max_catalog_id)
            .max(track_id)
            .max(album_id)
            .max(artist_id)
            .max(composer_id);

        let mut track = TrackRecord {
            track_id,
            visible: read_u32_at(&raw_hdr, 0x14),
            filetype: read_u32_at(&raw_hdr, 0x18),
            type1: raw_hdr.get(0x1C).copied().unwrap_or(0),
            type2: raw_hdr.get(0x1D).copied().unwrap_or(0),
            compilation: raw_hdr.get(0x1E).copied().unwrap_or(0) != 0,
            rating: raw_hdr.get(0x1F).copied().unwrap_or(0),
            time_modified: read_u32_at(&raw_hdr, 0x20),
            size: read_u32_at(&raw_hdr, 0x24),
            length_ms: read_u32_at(&raw_hdr, 0x28),
            track_nr: read_u32_at(&raw_hdr, 0x2C),
            tracks_total: read_u32_at(&raw_hdr, 0x30),
            year: read_u32_at(&raw_hdr, 0x34),
            bitrate: read_u32_at(&raw_hdr, 0x38),
            sample_rate_hz: read_u32_at(&raw_hdr, 0x3C) >> 16,
            volume: read_i32_at(&raw_hdr, 0x40),
            start_time: read_u32_at(&raw_hdr, 0x44),
            stop_time: read_u32_at(&raw_hdr, 0x48),
            soundcheck: read_u32_at(&raw_hdr, 0x4C),
            play_count: read_u32_at(&raw_hdr, 0x50),
            play_count2: read_u32_at(&raw_hdr, 0x54),
            last_played: read_u32_at(&raw_hdr, 0x58),
            disc_nr: read_u32_at(&raw_hdr, 0x5C),
            discs_total: read_u32_at(&raw_hdr, 0x60),
            time_added: read_u32_at(&raw_hdr, 0x68),
            bookmark_time: read_u32_at(&raw_hdr, 0x6C),
            dbid: read_u64_at(&raw_hdr, 0x70),
            checked: raw_hdr.get(0x78).copied().unwrap_or(0),
            bpm: read_u16_at(&raw_hdr, 0x7A),
            artwork_count: read_u16_at(&raw_hdr, 0x7C),
            artwork_size: read_u32_at(&raw_hdr, 0x80),
            time_released: read_u32_at(&raw_hdr, 0x8C),
            explicit_flag: read_u16_at(&raw_hdr, 0x92),
            skip_count: read_u32_at(&raw_hdr, 0x98),
            recent_skip_count: read_u32_at(&raw_hdr, 0x9C),
            last_skipped: read_u32_at(&raw_hdr, 0xA0),
            has_artwork: raw_hdr.get(0xA4).copied().unwrap_or(2) == 1,
            skip_when_shuffling: raw_hdr.get(0xA5).copied().unwrap_or(0) != 0,
            remember_playback_position: raw_hdr.get(0xA6).copied().unwrap_or(0) != 0,
            podcast_flag: raw_hdr.get(0xA7).copied().unwrap_or(0),
            lyrics_flag: raw_hdr.get(0xB0).copied().unwrap_or(0) != 0,
            movie_flag: raw_hdr.get(0xB1).copied().unwrap_or(0) != 0,
            pregap: read_u32_at(&raw_hdr, 0xB8),
            sample_count: read_u64_at(&raw_hdr, 0xBC),
            postgap: read_u32_at(&raw_hdr, 0xC8),
            mediatype: {
                let mt = read_u32_at(&raw_hdr, 0xD0);
                if mt == 0 {
                    1
                } else {
                    mt
                }
            },
            gapless_data: read_u32_at(&raw_hdr, 0xF8),
            gapless_track_flag: read_u16_at(&raw_hdr, 0x100),
            gapless_album_flag: read_u16_at(&raw_hdr, 0x102),
            album_id,
            mhii_link: read_u32_at(&raw_hdr, 0x160),
            artist_id,
            composer_id,
            title: String::new(),
            location: String::new(),
            album: String::new(),
            artist: String::new(),
            genre: String::new(),
            filetype_desc: String::new(),
            comment: String::new(),
            composer: String::new(),
            grouping: String::new(),
            description: String::new(),
            album_artist: String::new(),
            sort_artist: String::new(),
            sort_title: String::new(),
            sort_album: String::new(),
            sort_album_artist: String::new(),
            sort_composer: String::new(),
            copyright: String::new(),
            raw_header: raw_hdr,
            extra_mhods: Vec::new(),
        };

        let mut mpos = pos + mhit_hdr_len;
        let mhit_end = pos + mhit_tot_len;
        for _ in 0..num_mhods {
            check_magic(bytes, mpos, "mhod")?;
            let mhead = ensure_slice(bytes, mpos, 16)?;
            let mhdr = read_u32_at(mhead, 0x04) as usize;
            let mtot = read_u32_at(mhead, 0x08) as usize;
            let mtype = read_u32_at(mhead, 0x0C);
            if mhdr < 16 || mtot < mhdr || mpos + mtot > mhit_end {
                return Err(ItdbError::InvalidChunkBounds {
                    offset: mpos,
                    chunk: "mhod(track)",
                    header_len: mhdr,
                    total_len: mtot,
                });
            }
            let mslice = &bytes[mpos..mpos + mtot];
            match mtype {
                1 => {
                    track.title = parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                2 => {
                    track.location =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                3 => {
                    track.album = parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                4 => {
                    track.artist =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                5 => {
                    track.genre = parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                6 => {
                    track.filetype_desc =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                8 => {
                    track.comment =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                12 => {
                    track.composer =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                13 => {
                    track.grouping =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                14 => {
                    track.description =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                22 => {
                    track.album_artist =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                23 => {
                    track.sort_artist =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                27 => {
                    track.sort_title =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                28 => {
                    track.sort_album =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                29 => {
                    track.sort_album_artist =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                30 => {
                    track.sort_composer =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                39 => {
                    track.copyright =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                _ => track.extra_mhods.push(RawMhod {
                    mhod_type: mtype,
                    raw_bytes: mslice.to_vec(),
                }),
            }
            mpos += mtot;
        }

        tracks.push(track);
        pos += mhit_tot_len;
    }

    Ok(tracks)
}

fn parse_mhla(
    bytes: &[u8],
    offset: usize,
    section_len: usize,
    max_catalog_id: &mut u32,
) -> Result<Vec<AlbumItem>, ItdbError> {
    check_magic(bytes, offset, "mhla")?;
    let head = ensure_slice(bytes, offset, 12)?;
    let hdr_len = read_u32_at(head, 0x04) as usize;
    let album_count = read_u32_at(head, 0x08) as usize;
    if hdr_len < 12 || hdr_len > section_len {
        return Err(ItdbError::InvalidChunkBounds {
            offset,
            chunk: "mhla",
            header_len: hdr_len,
            total_len: section_len,
        });
    }

    let mut albums = Vec::with_capacity(album_count);
    let mut pos = offset + hdr_len;
    let end = offset + section_len;

    for _ in 0..album_count {
        check_magic(bytes, pos, "mhia")?;
        let probe = ensure_slice(bytes, pos, 16)?;
        let ihdr = read_u32_at(probe, 0x04) as usize;
        let itot = read_u32_at(probe, 0x08) as usize;
        let num_mhods = read_u32_at(probe, 0x0C) as usize;
        if ihdr < 0x28 || itot < ihdr || pos + itot > end {
            return Err(ItdbError::InvalidChunkBounds {
                offset: pos,
                chunk: "mhia",
                header_len: ihdr,
                total_len: itot,
            });
        }
        let raw_hdr = bytes[pos..pos + ihdr].to_vec();
        let album_id = read_u32_at(&raw_hdr, 0x10);
        *max_catalog_id = (*max_catalog_id).max(album_id);

        let mut item = AlbumItem {
            album_id,
            sql_id: read_u64_at(&raw_hdr, 0x14),
            unk_0x1c: read_u32_at(&raw_hdr, 0x1C),
            artwork_track_dbid: read_u64_at(&raw_hdr, 0x20),
            album: String::new(),
            album_artist: String::new(),
            sort_album_artist: String::new(),
            raw_header: raw_hdr,
            extra_mhods: Vec::new(),
        };

        let mut mpos = pos + ihdr;
        let iend = pos + itot;
        for _ in 0..num_mhods {
            check_magic(bytes, mpos, "mhod")?;
            let mhead = ensure_slice(bytes, mpos, 16)?;
            let mhdr = read_u32_at(mhead, 0x04) as usize;
            let mtot = read_u32_at(mhead, 0x08) as usize;
            let mtype = read_u32_at(mhead, 0x0C);
            if mhdr < 16 || mtot < mhdr || mpos + mtot > iend {
                return Err(ItdbError::InvalidChunkBounds {
                    offset: mpos,
                    chunk: "mhod(mhia)",
                    header_len: mhdr,
                    total_len: mtot,
                });
            }
            let mslice = &bytes[mpos..mpos + mtot];
            match mtype {
                200 => {
                    item.album = parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                201 => {
                    item.album_artist =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                202 => {
                    item.sort_album_artist =
                        parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default()
                }
                _ => item.extra_mhods.push(RawMhod {
                    mhod_type: mtype,
                    raw_bytes: mslice.to_vec(),
                }),
            }
            mpos += mtot;
        }
        albums.push(item);
        pos += itot;
    }

    Ok(albums)
}

fn parse_mhli_artists(
    bytes: &[u8],
    offset: usize,
    section_len: usize,
    max_catalog_id: &mut u32,
) -> Result<Vec<ArtistItem>, ItdbError> {
    check_magic(bytes, offset, "mhli")?;
    let head = ensure_slice(bytes, offset, 12)?;
    let hdr_len = read_u32_at(head, 0x04) as usize;
    let count = read_u32_at(head, 0x08) as usize;
    if hdr_len < 12 || hdr_len > section_len {
        return Err(ItdbError::InvalidChunkBounds {
            offset,
            chunk: "mhli(artists)",
            header_len: hdr_len,
            total_len: section_len,
        });
    }

    let mut artists = Vec::with_capacity(count);
    let mut pos = offset + hdr_len;
    let end = offset + section_len;

    for _ in 0..count {
        check_magic(bytes, pos, "mhii")?;
        let probe = ensure_slice(bytes, pos, 16)?;
        let ihdr = read_u32_at(probe, 0x04) as usize;
        let itot = read_u32_at(probe, 0x08) as usize;
        let num_mhods = read_u32_at(probe, 0x0C) as usize;
        if ihdr < 0x20 || itot < ihdr || pos + itot > end {
            return Err(ItdbError::InvalidChunkBounds {
                offset: pos,
                chunk: "mhii(artist)",
                header_len: ihdr,
                total_len: itot,
            });
        }
        let raw_hdr = bytes[pos..pos + ihdr].to_vec();
        let artist_id = read_u32_at(&raw_hdr, 0x10);
        *max_catalog_id = (*max_catalog_id).max(artist_id);

        let mut item = ArtistItem {
            artist_id,
            sql_id: read_u64_at(&raw_hdr, 0x14),
            unk_0x1c: read_u32_at(&raw_hdr, 0x1C),
            artist: String::new(),
            raw_header: raw_hdr,
            extra_mhods: Vec::new(),
        };

        let mut mpos = pos + ihdr;
        let iend = pos + itot;
        for _ in 0..num_mhods {
            check_magic(bytes, mpos, "mhod")?;
            let mhead = ensure_slice(bytes, mpos, 16)?;
            let mhdr = read_u32_at(mhead, 0x04) as usize;
            let mtot = read_u32_at(mhead, 0x08) as usize;
            let mtype = read_u32_at(mhead, 0x0C);
            if mhdr < 16 || mtot < mhdr || mpos + mtot > iend {
                return Err(ItdbError::InvalidChunkBounds {
                    offset: mpos,
                    chunk: "mhod(mhii)",
                    header_len: mhdr,
                    total_len: mtot,
                });
            }
            let mslice = &bytes[mpos..mpos + mtot];
            if mtype == 300 {
                item.artist = parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default();
            } else {
                item.extra_mhods.push(RawMhod {
                    mhod_type: mtype,
                    raw_bytes: mslice.to_vec(),
                });
            }
            mpos += mtot;
        }
        artists.push(item);
        pos += itot;
    }

    Ok(artists)
}

fn parse_mhlp(
    bytes: &[u8],
    offset: usize,
    section_len: usize,
    max_catalog_id: &mut u32,
) -> Result<Vec<Playlist>, ItdbError> {
    check_magic(bytes, offset, "mhlp")?;
    let head = ensure_slice(bytes, offset, 12)?;
    let hdr_len = read_u32_at(head, 0x04) as usize;
    let playlist_count = read_u32_at(head, 0x08) as usize;
    if hdr_len < 12 || hdr_len > section_len {
        return Err(ItdbError::InvalidChunkBounds {
            offset,
            chunk: "mhlp",
            header_len: hdr_len,
            total_len: section_len,
        });
    }

    let mut playlists = Vec::with_capacity(playlist_count);
    let mut pos = offset + hdr_len;
    let end = offset + section_len;

    for _ in 0..playlist_count {
        check_magic(bytes, pos, "mhyp")?;
        let probe = ensure_slice(bytes, pos, 20)?;
        let phdr = read_u32_at(probe, 0x04) as usize;
        let ptot = read_u32_at(probe, 0x08) as usize;
        let mhod_count = read_u32_at(probe, 0x0C) as usize;
        let mhip_count = read_u32_at(probe, 0x10) as usize;

        if phdr < 0x30 || ptot < phdr || pos + ptot > end {
            return Err(ItdbError::InvalidChunkBounds {
                offset: pos,
                chunk: "mhyp",
                header_len: phdr,
                total_len: ptot,
            });
        }
        let raw_hdr = bytes[pos..pos + phdr].to_vec();
        let mut pl = Playlist {
            name: String::new(),
            is_master: raw_hdr.get(0x14).copied().unwrap_or(0) != 0,
            timestamp: read_u32_at(&raw_hdr, 0x18),
            timestamp_modified: read_u32_at(&raw_hdr, 0x58),
            persistent_id: read_u64_at(&raw_hdr, 0x1C),
            podcast_flag: raw_hdr.get(0x2A).copied().unwrap_or(0),
            group_flag: raw_hdr.get(0x2B).copied().unwrap_or(0),
            sort_order: read_u32_at(&raw_hdr, 0x2C),
            distinguished_kind: read_u16_at(&raw_hdr, 0x50),
            items: Vec::with_capacity(mhip_count),
            non_title_mhods: Vec::new(),
            raw_header: raw_hdr,
        };

        let mut cpos = pos + phdr;
        let pend = pos + ptot;

        for _ in 0..mhod_count {
            check_magic(bytes, cpos, "mhod")?;
            let mhead = ensure_slice(bytes, cpos, 16)?;
            let mhdr = read_u32_at(mhead, 0x04) as usize;
            let mtot = read_u32_at(mhead, 0x08) as usize;
            let mtype = read_u32_at(mhead, 0x0C);
            if mhdr < 16 || mtot < mhdr || cpos + mtot > pend {
                return Err(ItdbError::InvalidChunkBounds {
                    offset: cpos,
                    chunk: "mhod(mhyp)",
                    header_len: mhdr,
                    total_len: mtot,
                });
            }
            let mslice = &bytes[cpos..cpos + mtot];
            if mtype == 1 {
                pl.name = parse_string_mhod_payload(mslice, mhdr, mtype).unwrap_or_default();
            } else {
                pl.non_title_mhods.push(RawMhod {
                    mhod_type: mtype,
                    raw_bytes: mslice.to_vec(),
                });
            }
            cpos += mtot;
        }

        for idx in 0..mhip_count {
            check_magic(bytes, cpos, "mhip")?;
            let iprobe = ensure_slice(bytes, cpos, 16)?;
            let ihdr = read_u32_at(iprobe, 0x04) as usize;
            let itot = read_u32_at(iprobe, 0x08) as usize;
            let imhods = read_u32_at(iprobe, 0x0C) as usize;
            if ihdr < 0x24 || itot < ihdr || cpos + itot > pend {
                return Err(ItdbError::InvalidChunkBounds {
                    offset: cpos,
                    chunk: "mhip",
                    header_len: ihdr,
                    total_len: itot,
                });
            }
            let iraw = bytes[cpos..cpos + ihdr].to_vec();
            let group_id = read_u32_at(&iraw, 0x14);
            *max_catalog_id = (*max_catalog_id).max(group_id);

            let mut position = (idx + 1) as u32;
            let mut group_title = None;
            let mut extra_mhods = Vec::new();

            let mut mpos = cpos + ihdr;
            let iend = cpos + itot;
            for _ in 0..imhods {
                check_magic(bytes, mpos, "mhod")?;
                let mhead = ensure_slice(bytes, mpos, 16)?;
                let mhdr = read_u32_at(mhead, 0x04) as usize;
                let mtot = read_u32_at(mhead, 0x08) as usize;
                let mtype = read_u32_at(mhead, 0x0C);
                if mhdr < 16 || mtot < mhdr || mpos + mtot > iend {
                    return Err(ItdbError::InvalidChunkBounds {
                        offset: mpos,
                        chunk: "mhod(mhip)",
                        header_len: mhdr,
                        total_len: mtot,
                    });
                }
                let mslice = &bytes[mpos..mpos + mtot];
                if mtype == 100 && mtot >= 0x1C {
                    position = read_u32_at(mslice, 0x18);
                } else if mtype == 1 {
                    group_title = parse_string_mhod_payload(mslice, mhdr, mtype);
                } else {
                    extra_mhods.push(RawMhod {
                        mhod_type: mtype,
                        raw_bytes: mslice.to_vec(),
                    });
                }
                mpos += mtot;
            }

            pl.items.push(PlaylistItem {
                podcast_group_flag: read_u16_at(&iraw, 0x10),
                group_id,
                track_id: read_u32_at(&iraw, 0x18),
                timestamp: read_u32_at(&iraw, 0x1C),
                podcast_group_ref: read_u32_at(&iraw, 0x20),
                order_index: read_u32_at(&iraw, 0x24),
                track_dbid: read_u64_at(&iraw, 0x2C),
                item_pid: read_u64_at(&iraw, 0x3C),
                position,
                group_title,
                raw_header: iraw,
                extra_mhods,
            });

            cpos += itot;
        }

        playlists.push(pl);
        pos += ptot;
    }

    Ok(playlists)
}
