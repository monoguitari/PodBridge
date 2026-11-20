// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs;
use std::io::{self, Read};
use std::time::Instant;

use podbridge_itdb::parse_itunesdb;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = env::args().nth(1).unwrap_or_else(|| "-".to_owned());
    let buf = if arg == "-" {
        let mut data = Vec::new();
        io::stdin().read_to_end(&mut data)?;
        data
    } else {
        fs::read(arg)?
    };

    let t0 = Instant::now();
    let db = parse_itunesdb(&buf)?;
    let elapsed = t0.elapsed();

    println!(
        "Parsed iTunesDB v0x{:X} in {:.2?}: sections={:?}, tracks={}, albums={}, playlists(s2)={}, podcast_pls(s3)={}, category_pls(s5)={}, hash72_embedded={}",
        db.header.version,
        elapsed,
        db.section_order,
        db.tracks.len(),
        db.albums.len(),
        db.playlists.len(),
        db.podcast_playlists.len(),
        db.category_playlists.len(),
        db.embedded_hash72.is_some()
    );
    if let Some(first) = db.tracks.first() {
        println!(
            "  First track: id={} title={:?} artist={:?} album={:?} loc={:?} art={}",
            first.track_id,
            first.title,
            first.artist,
            first.album,
            first.location,
            first.has_artwork
        );
    }
    for pl in &db.playlists {
        println!(
            "  Playlist: {:?} (master={}, items={})",
            pl.name,
            pl.is_master,
            pl.items.len()
        );
    }
    Ok(())
}
