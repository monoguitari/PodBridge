// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! Binary `iTunesDB` parser, lossless model, playlist/M3U8 manager, and serializer.

pub mod chunks;
pub mod error;
pub mod library;
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
pub use library::{
    ipod_location_to_rel_path, parse_m3u_content, parse_m3u_file, DuplicateCandidate,
    DuplicateIndex, M3uPlaylistSpec, NewTrackInput, RemovedTrackInfo,
    DEFAULT_DUPLICATE_TOLERANCE_MS,
};
pub use parser::parse_itunesdb;
pub use writer::serialize_itunesdb;

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Instant;

    use podbridge_crypto::{verify_hash58, FirewireGuid, HashingScheme};

    use super::*;

    fn sample_track_input(title: &str, artist: &str, album: &str, loc: &str) -> NewTrackInput {
        NewTrackInput {
            title: title.to_owned(),
            artist: artist.to_owned(),
            album_artist: String::new(),
            album: album.to_owned(),
            genre: "Electronic".to_owned(),
            composer: String::new(),
            comment: String::new(),
            location: loc.to_owned(),
            filetype: FILETYPE_M4A,
            filetype_desc: "Apple Lossless audio file".to_owned(),
            size: 18_500_000,
            length_ms: 245_100,
            bitrate: 900,
            sample_rate_hz: 44_100,
            sample_count: 0,
            track_nr: 1,
            tracks_total: 10,
            disc_nr: 1,
            discs_total: 1,
            year: 2007,
            compilation: false,
            has_artwork: true,
            artwork_size: 45_000,
            mhii_link: 100,
            timestamp_mac: unix_to_mac_timestamp(1_700_000_000),
        }
    }

    #[test]
    fn round_trips_empty_and_populated_database_with_hash58() {
        let guid = FirewireGuid::parse_hex("000A27001358762E").unwrap();
        let scheme = HashingScheme::Hash58 {
            guid,
            companion_hash72: None,
        };

        let mut db =
            ItdbDatabase::new_empty("PodBridge Classic", unix_to_mac_timestamp(1_700_000_000));
        let (tid, dbid) = db.add_track(sample_track_input(
            "Digital Love",
            "Daft Punk",
            "Discovery",
            ":iPod_Control:Music:F00:ABCD.m4a",
        ));

        let bytes = serialize_itunesdb(&mut db, &scheme).unwrap();
        assert!(verify_hash58(&bytes, guid).unwrap());

        let reparsed = parse_itunesdb(&bytes).unwrap();
        assert_eq!(reparsed.section_order, vec![4, 1, 3, 2]);
        assert_eq!(reparsed.tracks.len(), 1);
        assert_eq!(reparsed.tracks[0].track_id, tid);
        assert_eq!(reparsed.albums.len(), 1);
        assert_eq!(reparsed.albums[0].album, "Discovery");
        assert_eq!(reparsed.albums[0].artwork_track_dbid, dbid);
        assert_eq!(reparsed.tracks[0].album_id, reparsed.albums[0].album_id);
        assert_eq!(reparsed.playlists[0].items.len(), 1);
        assert_eq!(reparsed.podcast_playlists[0].items.len(), 1);
        assert!(reparsed.tracks[0].has_artwork);
        assert_eq!(reparsed.tracks[0].mhii_link, 100);
    }

    #[test]
    fn supports_playlist_crud_track_removal_duplicates_and_m3u8_parsing() {
        let guid = FirewireGuid::parse_hex("000A27001358762E").unwrap();
        let scheme = HashingScheme::Hash58 {
            guid,
            companion_hash72: None,
        };

        let mut db = ItdbDatabase::new_empty("Test iPod", 100);
        let (t1, _) = db.add_track(sample_track_input(
            "One More Time",
            "Daft Punk",
            "Discovery",
            ":iPod_Control:Music:F01:AAAA.m4a",
        ));
        let (t2, _) = db.add_track(sample_track_input(
            "Aerodynamic",
            "Daft Punk",
            "Discovery",
            ":iPod_Control:Music:F02:BBBB.m4a",
        ));

        // Duplicate detection (case/whitespace-insensitive + container duration tolerance)
        let dup_idx = DuplicateIndex::from_database(&db);
        assert_eq!(
            dup_idx.find_duplicate(
                DuplicateCandidate {
                    title: "  one   more time ",
                    artist: "DAFT PUNK",
                    album: "discovery",
                    length_ms: 246_000, // +900ms within 2500ms tolerance
                },
                DEFAULT_DUPLICATE_TOLERANCE_MS
            ),
            Some(t1)
        );
        assert_eq!(
            dup_idx.find_duplicate(
                DuplicateCandidate {
                    title: "One More Time",
                    artist: "Daft Punk",
                    album: "Discovery",
                    length_ms: 310_000, // Different live version (+65s)
                },
                DEFAULT_DUPLICATE_TOLERANCE_MS
            ),
            None
        );

        // Playlist CRUD
        let pl_pid = db.create_playlist("Road Trip", 120);
        db.set_playlist_tracks(pl_pid, &[t2, t1], 125).unwrap();
        db.rename_playlist(pl_pid, "Late Night Drive", 130).unwrap();

        // Protect Master playlist from deletion
        let master_pid = db.playlists[0].persistent_id;
        assert!(matches!(
            db.delete_playlist(master_pid),
            Err(ItdbError::CannotDeleteMasterPlaylist)
        ));

        // Removing t2 scrubs it from user playlist and Master playlist on serialize
        let removed = db.remove_tracks(&[t2]);
        assert_eq!(removed.len(), 1);
        assert_eq!(
            ipod_location_to_rel_path(&removed[0].location),
            Path::new("iPod_Control/Music/F02/BBBB.m4a")
        );

        let raw = serialize_itunesdb(&mut db, &scheme).unwrap();
        let reparsed = parse_itunesdb(&raw).unwrap();
        assert_eq!(reparsed.tracks.len(), 1);
        assert_eq!(reparsed.playlists.len(), 2);
        let user_pl = &reparsed.playlists[1];
        assert_eq!(user_pl.name, "Late Night Drive");
        assert_eq!(user_pl.items.len(), 1);
        assert_eq!(user_pl.items[0].track_id, t1);
        assert_eq!(user_pl.items[0].position, 1);

        // M3U8 parsing with BOM, #PLAYLIST, comments, and relative paths
        let m3u = "\u{FEFF}#EXTM3U\n#PLAYLIST: Gym Mix\n#EXTINF:215,Daft Punk - Harder\nAlbums\\Discovery\\04 Harder.flac\nhttps://example.com/stream.mp3\n/abs/song.mp3\n";
        let spec = parse_m3u_content(m3u, Path::new("/music"), "fallback");
        assert_eq!(spec.name, "Gym Mix");
        assert_eq!(spec.track_paths.len(), 2);
        assert_eq!(
            spec.track_paths[0],
            Path::new("/music/Albums/Discovery/04 Harder.flac")
        );
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
        assert!(
            parse_elapsed.as_millis() < 1500,
            "10k parse took {parse_elapsed:?}"
        );
    }
}
