// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use thiserror::Error;

/// Errors produced during cover-art decoding/encoding, `.ithmb` I/O, or `ArtworkDB` parsing.
#[derive(Debug, Error)]
pub enum ArtworkError {
    #[error("unexpected EOF in ArtworkDB at offset 0x{offset:X}: needed {needed}, remaining {remaining}")]
    UnexpectedEof {
        offset: usize,
        needed: usize,
        remaining: usize,
    },
    #[error(
        "invalid ArtworkDB chunk magic at 0x{offset:X}: expected '{expected}', got {actual:?}"
    )]
    InvalidMagic {
        offset: usize,
        expected: &'static str,
        actual: [u8; 4],
    },
    #[error("invalid ArtworkDB chunk bounds at 0x{offset:X} ({chunk}): header={header_len}, total={total_len}")]
    InvalidChunkBounds {
        offset: usize,
        chunk: &'static str,
        header_len: usize,
        total_len: usize,
    },
    #[error("invalid .ithmb slot slice: got {actual} bytes, expected {expected}")]
    InvalidSlotSize { actual: usize, expected: usize },
    #[error("image decode/encode error: {0}")]
    Image(#[from] image::ImageError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
