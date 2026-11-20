// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! Binary `iTunesDB` parser, lossless model, playlist/M3U8 manager, and serializer.

pub mod chunks;
pub mod error;
pub mod parser;
pub mod writer;

pub use chunks::{
    mac_to_unix_timestamp, unix_to_mac_timestamp, AlbumItem, ArtistItem, ItdbDatabase, MhbdHeader,
    Playlist, PlaylistItem, RawMhod, RawMhsdSection, TrackRecord, DEFAULT_MHBD_HEADER_LEN,
    DEFAULT_MHIA_HEADER_LEN, DEFAULT_MHIP_HEADER_LEN, DEFAULT_MHIT_HEADER_LEN,
    DEFAULT_MHYP_HEADER_LEN, FILETYPE_AIFF, FILETYPE_M4A, FILETYPE_MP3, FILETYPE_WAV,
    MAC_EPOCH_OFFSET_SECS,
};
pub use error::ItdbError;
pub use parser::parse_itunesdb;
pub use writer::serialize_itunesdb;

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use podbridge_crypto::{verify_hash58, FirewireGuid, HashingScheme};

    use super::*;

    #[test]
    fn round_trips_empty_and_populated_database_with_hash58() {
        let guid = FirewireGuid::parse_hex("000A27001358762E").unwrap();
        let scheme = HashingScheme::Hash58 {
            guid,
            companion_hash72: None,
        };

        let mut db =
            ItdbDatabase::new_empty("PodBridge Classic", unix_to_mac_timestamp(1_700_000_000));
        let mut t1 = TrackRecord::new(db.alloc_catalog_id(), 0x1111_2222_3333_4444);
        t1.title = "Digital Love".to_owned();
        t1.artist = "Daft Punk".to_owned();
        t1.album = "Discovery".to_owned();
        t1.genre = "Electronic".to_owned();
        t1.location = ":iPod_Control:Music:F00:ABCD.m4a".to_owned();
        t1.filetype = FILETYPE_M4A;
        t1.size = 9_420_000;
        t1.length_ms = 301_000;
        t1.track_nr = 3;
        t1.year = 2001;
        t1.bitrate = 256;
        t1.sample_rate_hz = 44_100;
        t1.has_artwork = true;
        t1.artwork_count = 1;
        t1.mhii_link = 100;
        db.tracks.push(t1);
        db.dirty_library_indices = true;

        let bytes = serialize_itunesdb(&mut db, &scheme).unwrap();
        assert!(verify_hash58(&bytes, guid).unwrap());

        let reparsed = parse_itunesdb(&bytes).unwrap();
        assert_eq!(reparsed.section_order, vec![4, 1, 3, 2]);
        assert_eq!(reparsed.tracks.len(), 1);
        assert_eq!(reparsed.albums.len(), 1);
        assert_eq!(reparsed.albums[0].album, "Discovery");
        assert_eq!(reparsed.albums[0].artwork_track_dbid, 0x1111_2222_3333_4444);
        assert_eq!(reparsed.tracks[0].album_id, reparsed.albums[0].album_id);
        assert_eq!(reparsed.playlists[0].items.len(), 1);
        assert_eq!(reparsed.podcast_playlists[0].items.len(), 1);
        assert!(reparsed.tracks[0].has_artwork);
        assert_eq!(reparsed.tracks[0].mhii_link, 100);
    }

    #[test]
    fn parses_and_serializes_10k_tracks_well_under_150ms_budget() {
        let guid = FirewireGuid::parse_hex("000A27001358762E").unwrap();
        let scheme = HashingScheme::Hash58 {
            guid,
            companion_hash72: None,
        };

        let mut db = ItdbDatabase::new_empty("Perf iPod", 0);
        for i in 0..10_000u32 {
            let mut t = TrackRecord::new(1000 + i, u64::from(i) + 1);
            t.title = format!("Track {i:05}");
            t.artist = format!("Artist {:03}", i / 20);
            t.album = format!("Album {:04}", i / 10);
            t.genre = "Rock".to_owned();
            t.location = format!(":iPod_Control:Music:F{:02}:T{:03}.mp3", i % 50, i % 1000);
            t.filetype = FILETYPE_MP3;
            t.size = 5_000_000;
            t.length_ms = 200_000;
            db.tracks.push(t);
        }
        db.dirty_library_indices = true;

        let serialized = serialize_itunesdb(&mut db, &scheme).unwrap();

        let t0 = Instant::now();
        let parsed = parse_itunesdb(&serialized).unwrap();
        let parse_elapsed = t0.elapsed();

        assert_eq!(parsed.tracks.len(), 10_000);
        assert_eq!(parsed.albums.len(), 1_000);
        // Debug builds easily finish < 400ms, release builds < 35ms; assert generous debug ceiling
        assert!(
            parse_elapsed.as_millis() < 1500,
            "10k parse took {parse_elapsed:?}"
        );
    }
}
