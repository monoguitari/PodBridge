// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use podbridge_crypto::CryptoError;
use thiserror::Error;

/// Errors produced while parsing, mutating, or serializing an `iTunesDB`.
#[derive(Debug, Error)]
pub enum ItdbError {
    #[error("unexpected EOF at offset 0x{offset:X}: needed {needed} bytes, remaining {remaining}")]
    UnexpectedEof {
        offset: usize,
        needed: usize,
        remaining: usize,
    },
    #[error("invalid chunk magic at offset 0x{offset:X}: expected '{expected}', got {actual:?}")]
    InvalidMagic {
        offset: usize,
        expected: &'static str,
        actual: [u8; 4],
    },
    #[error("invalid chunk bounds at offset 0x{offset:X} ({chunk}): header_len={header_len}, total_len={total_len}")]
    InvalidChunkBounds {
        offset: usize,
        chunk: &'static str,
        header_len: usize,
        total_len: usize,
    },
    #[error("missing required track list section (mhsd type=1)")]
    MissingTrackSection,
    #[error("playlist persistent_id 0x{0:016X} not found")]
    PlaylistNotFound(u64),
    #[error("cannot delete or rename the Master Library playlist")]
    CannotDeleteMasterPlaylist,
    #[error("cryptographic signing error: {0}")]
    Crypto(#[from] CryptoError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
