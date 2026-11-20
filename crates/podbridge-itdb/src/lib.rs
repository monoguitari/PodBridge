// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! Binary `iTunesDB` parser, lossless model, playlist/M3U8 manager, and serializer.

pub mod chunks;
pub mod error;
pub mod parser;

pub use chunks::{
    mac_to_unix_timestamp, unix_to_mac_timestamp, AlbumItem, ArtistItem, ItdbDatabase, MhbdHeader,
    Playlist, PlaylistItem, RawMhod, RawMhsdSection, TrackRecord, DEFAULT_MHBD_HEADER_LEN,
    DEFAULT_MHIA_HEADER_LEN, DEFAULT_MHIP_HEADER_LEN, DEFAULT_MHIT_HEADER_LEN,
    DEFAULT_MHYP_HEADER_LEN, FILETYPE_AIFF, FILETYPE_M4A, FILETYPE_MP3, FILETYPE_WAV,
    MAC_EPOCH_OFFSET_SECS,
};
pub use error::ItdbError;
pub use parser::parse_itunesdb;

#[cfg(test)]
mod tests {
    use super::*;

    fn build_minimal_raw_itunesdb() -> Vec<u8> {
        let mut buf = Vec::new();

        // mhbd (244 bytes)
        let mut mhbd = MhbdHeader::default().raw_header;
        mhbd[0x14..0x18].copy_from_slice(&1u32.to_le_bytes()); // 1 mhsd child

        // mhod title ("Song A")
        let title_u16: Vec<u8> = "Song A".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut mhod_title = vec![0u8; 40 + title_u16.len()];
        let mhod_len = mhod_title.len() as u32;
        mhod_title[0x00..0x04].copy_from_slice(b"mhod");
        mhod_title[0x04..0x08].copy_from_slice(&24u32.to_le_bytes());
        mhod_title[0x08..0x0C].copy_from_slice(&mhod_len.to_le_bytes());
        mhod_title[0x0C..0x10].copy_from_slice(&1u32.to_le_bytes());
        mhod_title[0x18..0x1C].copy_from_slice(&1u32.to_le_bytes());
        mhod_title[0x1C..0x20].copy_from_slice(&(title_u16.len() as u32).to_le_bytes());
        mhod_title[0x20..0x24].copy_from_slice(&1u32.to_le_bytes());
        mhod_title[0x28..].copy_from_slice(&title_u16);

        // mhit (624 bytes + mhod_title)
        let mut mhit = TrackRecord::new(1001, 0xCAFE_BABE_1234_5678).raw_header;
        let mhit_tot = (mhit.len() + mhod_title.len()) as u32;
        mhit[0x08..0x0C].copy_from_slice(&mhit_tot.to_le_bytes());
        mhit[0x0C..0x10].copy_from_slice(&1u32.to_le_bytes());
        mhit[0x10..0x14].copy_from_slice(&1001u32.to_le_bytes());
        mhit[0x14..0x18].copy_from_slice(&1u32.to_le_bytes());
        mhit[0x18..0x1C].copy_from_slice(&FILETYPE_MP3.to_le_bytes());
        mhit[0x24..0x28].copy_from_slice(&4_500_000u32.to_le_bytes());
        mhit[0x28..0x2C].copy_from_slice(&215_000u32.to_le_bytes());
        mhit[0x3C..0x40].copy_from_slice(&(44_100u32 << 16).to_le_bytes());
        mhit[0x70..0x78].copy_from_slice(&0xCAFE_BABE_1234_5678u64.to_le_bytes());
        mhit[0xA4] = 1; // has_artwork = true

        // mhlt (92 bytes)
        let mut mhlt = vec![0u8; 92];
        mhlt[0x00..0x04].copy_from_slice(b"mhlt");
        mhlt[0x04..0x08].copy_from_slice(&92u32.to_le_bytes());
        mhlt[0x08..0x0C].copy_from_slice(&1u32.to_le_bytes());

        // mhsd type=1 (96 bytes)
        let mhsd_tot = (96 + mhlt.len() + mhit.len() + mhod_title.len()) as u32;
        let mut mhsd = vec![0u8; 96];
        mhsd[0x00..0x04].copy_from_slice(b"mhsd");
        mhsd[0x04..0x08].copy_from_slice(&96u32.to_le_bytes());
        mhsd[0x08..0x0C].copy_from_slice(&mhsd_tot.to_le_bytes());
        mhsd[0x0C..0x10].copy_from_slice(&1u32.to_le_bytes());

        let db_tot = (mhbd.len() as u32) + mhsd_tot;
        mhbd[0x08..0x0C].copy_from_slice(&db_tot.to_le_bytes());

        buf.extend_from_slice(&mhbd);
        buf.extend_from_slice(&mhsd);
        buf.extend_from_slice(&mhlt);
        buf.extend_from_slice(&mhit);
        buf.extend_from_slice(&mhod_title);
        buf
    }

    #[test]
    fn parses_synthetic_itunesdb_and_timestamps() {
        let raw = build_minimal_raw_itunesdb();
        let db = parse_itunesdb(&raw).unwrap();
        assert_eq!(db.header.version, 0x73);
        assert_eq!(db.tracks.len(), 1);
        let t = &db.tracks[0];
        assert_eq!(t.track_id, 1001);
        assert_eq!(t.title, "Song A");
        assert_eq!(t.size, 4_500_000);
        assert_eq!(t.length_ms, 215_000);
        assert_eq!(t.sample_rate_hz, 44_100);
        assert!(t.has_artwork);

        let unix = 1_700_000_000u64;
        assert_eq!(mac_to_unix_timestamp(unix_to_mac_timestamp(unix)), unix);
        assert_eq!(unix_to_mac_timestamp(0), 0);
    }

    #[test]
    fn rejects_invalid_magic_and_truncated_chunks() {
        let mut bad = build_minimal_raw_itunesdb();
        bad[0..4].copy_from_slice(b"nope");
        assert!(matches!(
            parse_itunesdb(&bad),
            Err(ItdbError::InvalidMagic {
                expected: "mhbd",
                ..
            })
        ));

        let truncated = &build_minimal_raw_itunesdb()[..100];
        assert!(matches!(
            parse_itunesdb(truncated),
            Err(ItdbError::InvalidChunkBounds { chunk: "mhbd", .. })
        ));
    }
}
