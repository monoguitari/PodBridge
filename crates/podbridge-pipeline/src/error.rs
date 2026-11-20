// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use podbridge_artwork::ArtworkError;
use podbridge_itdb::ItdbError;
use thiserror::Error;

/// Errors produced by the audio scanner, FFmpeg transcoder, or sequential iPod writer.
#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("transfer cancelled by user")]
    Cancelled,
    #[error("unsupported audio file '{path}'")]
    UnsupportedFormat { path: String },
    #[error("FFmpeg binary not found (checked sidecar and system PATH)")]
    FfmpegNotFound,
    #[error("FFmpeg transcode failed for '{path}': {stderr}")]
    TranscodeFailed { path: String, stderr: String },
    #[error(
        "insufficient free space on iPod: need {needed_bytes} bytes, available {free_bytes} bytes"
    )]
    InsufficientSpace { needed_bytes: u64, free_bytes: u64 },
    #[error("backup '{0}' not found")]
    BackupNotFound(String),
    #[error("iTunesDB error: {0}")]
    Itdb(#[from] ItdbError),
    #[error("ArtworkDB error: {0}")]
    Artwork(#[from] ArtworkError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
