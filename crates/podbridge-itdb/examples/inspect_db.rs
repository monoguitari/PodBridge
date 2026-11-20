// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs;
use std::io::{self, Read};
use std::time::Instant;

use podbridge_crypto::{verify_hash58, verify_hash72, FirewireGuid, HashingScheme};
use podbridge_itdb::{parse_itunesdb, serialize_itunesdb};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = env::args().nth(1).unwrap_or_else(|| "-".to_owned());
    let guid_hex = env::args()
        .nth(2)
        .unwrap_or_else(|| "000A27001358762E".to_owned());
    let guid = FirewireGuid::parse_hex(&guid_hex)?;

    let buf = if arg == "-" {
        let mut data = Vec::new();
        io::stdin().read_to_end(&mut data)?;
        data
    } else {
        fs::read(arg)?
    };

    let t0 = Instant::now();
    let mut db = parse_itunesdb(&buf)?;
    let parse_elapsed = t0.elapsed();

    let orig_track_count = db.tracks.len();
    let orig_playlist_count = db.playlists.len();
    let orig_cat_count = db.category_playlists.len();

    db.dirty_library_indices = true;
    let t1 = Instant::now();
    let out = serialize_itunesdb(
        &mut db,
        &HashingScheme::Hash58 {
            guid,
            companion_hash72: None,
        },
    )?;
    let write_elapsed = t1.elapsed();

    assert!(verify_hash58(&out, guid)?, "round-trip hash58 invalid");
    if let Some(ref h72) = db.embedded_hash72 {
        assert!(verify_hash72(&out, Some(h72))?, "round-trip hash72 invalid");
    }

    let reparsed = parse_itunesdb(&out)?;
    assert_eq!(reparsed.tracks.len(), orig_track_count);
    assert_eq!(reparsed.playlists.len(), orig_playlist_count);
    assert_eq!(reparsed.category_playlists.len(), orig_cat_count);

    println!(
        "Round-trip OK! tracks={} albums={} parse={:.2?} rebuild+serialize+dual_sign={:.2?} out_bytes={}",
        reparsed.tracks.len(),
        reparsed.albums.len(),
        parse_elapsed,
        write_elapsed,
        out.len()
    );
    Ok(())
}
