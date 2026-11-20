// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs;
use std::io::{self, Read};
use std::time::Instant;

use podbridge_crypto::{
    sign_itunesdb, verify_hash58, verify_hash72, FirewireGuid, Hash72Info, HashingScheme,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: cargo run -p podbridge-crypto --example verify_db -- <path|-> <GUID>");
        std::process::exit(2);
    }

    let buf = if args[1] == "-" {
        let mut data = Vec::new();
        io::stdin().read_to_end(&mut data)?;
        data
    } else {
        fs::read(&args[1])?
    };

    let guid = FirewireGuid::parse_hex(&args[2])?;

    let t0 = Instant::now();
    let h58_ok = verify_hash58(&buf, guid)?;
    let h72_extracted = Hash72Info::extract_from_itunesdb(&buf, Some(guid)).ok();
    let verify_elapsed = t0.elapsed();

    println!(
        "Loaded {} bytes | GUID=0x{} | hash58_valid={} | hash72_extracted={} | verify_time={:.2?}",
        buf.len(),
        guid,
        h58_ok,
        h72_extracted.is_some(),
        verify_elapsed
    );

    if !h58_ok {
        return Err("hash58 verification failed".into());
    }

    // Verify in-memory re-signing produces byte-identical signatures
    let mut clone = buf.clone();
    clone[0x58..0x6C].fill(0);
    if h72_extracted.is_some() {
        clone[0x72..0xA0].fill(0);
    }
    let t1 = Instant::now();
    sign_itunesdb(
        &mut clone,
        &HashingScheme::Hash58 {
            guid,
            companion_hash72: h72_extracted,
        },
    )?;
    let sign_elapsed = t1.elapsed();

    assert_eq!(&clone[0x58..0x6C], &buf[0x58..0x6C], "hash58 byte mismatch");
    if let Some(ref info) = h72_extracted {
        assert_eq!(&clone[0x72..0xA0], &buf[0x72..0xA0], "hash72 byte mismatch");
        assert!(verify_hash72(&clone, Some(info))?);
    }
    println!("Byte-identical re-sign verified in {sign_elapsed:.2?}!");
    Ok(())
}
