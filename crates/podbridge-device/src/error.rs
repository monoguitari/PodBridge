// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use podbridge_artwork::ArtworkError;
use podbridge_crypto::CryptoError;
use podbridge_itdb::ItdbError;
use thiserror::Error;

/// Errors encountered during iPod discovery, metadata loading, Mock creation, or Safe Eject.
#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("path '{0}' does not contain an iPod_Control directory")]
    NotAnIpodRoot(String),
    #[error("missing FireWire GUID for {model}; ensure USB connection is direct or SysInfo contains FirewireGuid")]
    MissingFirewireGuid { model: &'static str },
    #[error("iPod Nano 5G requires an existing iTunes-signed iTunesDB or iPod_Control/Device/HashInfo seed (sync once with iTunes first)")]
    MissingNano5GHashSeed,
    #[error("Safe Eject is only supported for physical USB drives on Windows (got '{0}')")]
    EjectUnsupported(String),
    #[error("Windows Safe Eject failed: {0}")]
    EjectFailed(String),
    #[error("crypto error: {0}")]
    Crypto(#[from] CryptoError),
    #[error("iTunesDB error: {0}")]
    Itdb(#[from] ItdbError),
    #[error("ArtworkDB error: {0}")]
    Artwork(#[from] ArtworkError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
