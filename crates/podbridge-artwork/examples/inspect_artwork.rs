// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs;
use std::io::{self, Read};
use std::time::Instant;

use podbridge_artwork::{parse_artworkdb, serialize_artworkdb, ArtworkFamily};

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
    let adb = parse_artworkdb(&buf)?;
    let parse_time = t0.elapsed();

    let specs = adb.effective_specs(ArtworkFamily::Classic6G);
    let out = serialize_artworkdb(&adb);
    let reparsed = parse_artworkdb(&out)?;

    assert_eq!(reparsed.images.len(), adb.images.len());
    assert_eq!(reparsed.format_files.len(), adb.format_files.len());
    assert_eq!(out.len(), buf.len(), "byte length parity");

    println!(
        "ArtworkDB v{} OK in {:.2?}! images={} next_id={} formats={:?} exact_byte_match={}",
        adb.version,
        parse_time,
        adb.images.len(),
        adb.next_id,
        specs
            .iter()
            .map(|s| (s.format_id, s.image_byte_size()))
            .collect::<Vec<_>>(),
        out == buf
    );
    Ok(())
}
