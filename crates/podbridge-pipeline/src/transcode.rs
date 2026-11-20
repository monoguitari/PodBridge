// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use lofty::config::ParseOptions;
use lofty::file::AudioFile;
use lofty::probe::Probe;
use podbridge_artwork::{
    decode_source_artwork, encode_all_formats, EncodedThumbnail, IthmbFormatSpec,
};
use podbridge_itdb::FILETYPE_M4A;
use tempfile::{Builder, TempPath};
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::error::PipelineError;
use crate::scanner::{ScannedTrack, TransferAction};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Source of audio bytes ready to be streamed sequentially onto the iPod.
#[derive(Debug)]
pub enum PreparedAudioSource {
    /// Native audio file copied directly from its original PC path (zero temp disk copy).
    NativeFile(PathBuf),
    /// Transcoded `.m4a` stored on fast PC temp storage and auto-deleted on drop.
    TempTranscoded(TempPath),
}

impl PreparedAudioSource {
    #[must_use]
    pub fn as_path(&self) -> &Path {
        match self {
            Self::NativeFile(p) => p.as_path(),
            Self::TempTranscoded(tp) => tp.as_ref(),
        }
    }
}

/// Pre-encoded cover artwork ready for `ArtworkDatabase::insert_encoded_thumbnails`.
#[derive(Debug, Clone)]
pub struct PreparedArtwork {
    pub sha256: [u8; 32],
    pub orig_img_size: u32,
    /// `Some` for the first track encountering this `sha256`; `None` for subsequent tracks on the
    /// same album so Lanczos3 resizing runs only once per unique cover!
    pub encoded_thumbs: Option<Vec<EncodedThumbnail>>,
}

/// Fully prepared track ready for the single sequential USB writer consumer.
#[derive(Debug)]
pub struct PreparedTrack {
    pub queue_index: usize,
    pub scanned: ScannedTrack,
    pub audio_source: PreparedAudioSource,
    pub final_ext: &'static str,
    pub final_filetype: u32,
    pub final_filetype_desc: &'static str,
    pub final_size: u32,
    pub final_length_ms: u32,
    pub final_bitrate_kbps: u32,
    pub final_sample_rate_hz: u32,
    pub prepared_artwork: Option<PreparedArtwork>,
}

/// Locates the `ffmpeg` sidecar or system executable.
pub fn resolve_ffmpeg_binary(explicit_override: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit_override {
        if p.is_file() {
            return Some(p.to_path_buf());
        }
    }

    if let Ok(exe) = env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let sidecar_candidates = [
                exe_dir.join("ffmpeg.exe"),
                exe_dir.join("ffmpeg-x86_64-pc-windows-msvc.exe"),
                exe_dir.join("binaries").join("ffmpeg.exe"),
                exe_dir
                    .join("binaries")
                    .join("ffmpeg-x86_64-pc-windows-msvc.exe"),
                exe_dir.join("ffmpeg"),
            ];
            for cand in sidecar_candidates {
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }

    if let Ok(home) = env::var("HOME") {
        let local_ff = PathBuf::from(home)
            .join(".local")
            .join("bin")
            .join("ffmpeg");
        if local_ff.is_file() {
            return Some(local_ff);
        }
    }

    let bin_names: &[&str] = if cfg!(windows) {
        &["ffmpeg.exe", "ffmpeg"]
    } else {
        &["ffmpeg"]
    };
    if let Some(path_os) = env::var_os("PATH") {
        for dir in env::split_paths(&path_os) {
            for name in bin_names {
                let cand = dir.join(name);
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }

    None
}

/// Bounded multi-core FFmpeg + artwork preparation pool.
#[derive(Clone)]
pub struct TranscodePool {
    semaphore: Arc<Semaphore>,
    ffmpeg_path: Option<PathBuf>,
    seen_art_hashes: Arc<Mutex<HashSet<[u8; 32]>>>,
}

impl TranscodePool {
    /// Creates a new pool with concurrency bounded to `(logical_cores - 1).clamp(1, 8)`.
    #[must_use]
    pub fn new(explicit_ffmpeg: Option<&Path>) -> Self {
        let cores = std::thread::available_parallelism().map_or(4, usize::from);
        let workers = cores.saturating_sub(1).clamp(1, 8);
        Self {
            semaphore: Arc::new(Semaphore::new(workers)),
            ffmpeg_path: resolve_ffmpeg_binary(explicit_ffmpeg),
            seen_art_hashes: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Prepares a single non-duplicate [`ScannedTrack`] (transcoding via FFmpeg if required and
    /// resizing unique cover art on a blocking worker thread), respecting `cancel`.
    pub async fn prepare_track(
        &self,
        queue_index: usize,
        track: ScannedTrack,
        artwork_specs: Vec<IthmbFormatSpec>,
        cancel: CancellationToken,
    ) -> Result<PreparedTrack, PipelineError> {
        if cancel.is_cancelled() {
            return Err(PipelineError::Cancelled);
        }

        let _permit = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(PipelineError::Cancelled),
            res = self.semaphore.clone().acquire_owned() => {
                res.map_err(|_| PipelineError::Cancelled)?
            }
        };

        // Pre-encode cover art only on the FIRST occurrence of its SHA-256 across the batch
        let prepared_artwork = if let Some(ref art) = track.artwork {
            let is_first = {
                let mut guard = self
                    .seen_art_hashes
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.insert(art.sha256)
            };
            let orig_img_size = art.bytes.len() as u32;
            let sha256 = art.sha256;
            if is_first && !artwork_specs.is_empty() {
                let raw_art = art.bytes.clone();
                let specs_clone = artwork_specs;
                let encoded = tokio::task::spawn_blocking(move || {
                    decode_source_artwork(&raw_art)
                        .map(|img| encode_all_formats(&img, &specs_clone))
                })
                .await
                .map_err(|e| PipelineError::Io(std::io::Error::other(e.to_string())))?;
                // Tolerate malformed cover images gracefully without failing the audio transfer
                let thumbs = encoded.ok();
                thumbs.map(|t| PreparedArtwork {
                    sha256,
                    orig_img_size,
                    encoded_thumbs: Some(t),
                })
            } else if !artwork_specs.is_empty() {
                Some(PreparedArtwork {
                    sha256,
                    orig_img_size,
                    encoded_thumbs: None,
                })
            } else {
                None
            }
        } else {
            None
        };

        match track.action {
            TransferAction::CopyNative {
                ipod_ext,
                filetype,
                filetype_desc,
            } => {
                let final_size = track.source_size.min(u64::from(u32::MAX)) as u32;
                let final_length_ms = track.length_ms;
                let final_bitrate_kbps = track.bitrate_kbps;
                let final_sample_rate_hz = track.sample_rate_hz;
                let src_path = track.source_path.clone();
                Ok(PreparedTrack {
                    queue_index,
                    scanned: track,
                    audio_source: PreparedAudioSource::NativeFile(src_path),
                    final_ext: ipod_ext,
                    final_filetype: filetype,
                    final_filetype_desc: filetype_desc,
                    final_size,
                    final_length_ms,
                    final_bitrate_kbps,
                    final_sample_rate_hz,
                    prepared_artwork,
                })
            }
            TransferAction::TranscodeAlac => {
                let ff = self
                    .ffmpeg_path
                    .as_deref()
                    .ok_or(PipelineError::FfmpegNotFound)?;
                let temp_path =
                    run_ffmpeg_transcode(ff, &track.source_path, TranscodeCodec::Alac, &cancel)
                        .await?;
                let (size, len_ms, br, sr) = probe_transcoded_m4a(&temp_path, &track);
                Ok(PreparedTrack {
                    queue_index,
                    scanned: track,
                    audio_source: PreparedAudioSource::TempTranscoded(temp_path),
                    final_ext: "m4a",
                    final_filetype: FILETYPE_M4A,
                    final_filetype_desc: "Apple Lossless audio file",
                    final_size: size,
                    final_length_ms: len_ms,
                    final_bitrate_kbps: br,
                    final_sample_rate_hz: sr,
                    prepared_artwork,
                })
            }
            TransferAction::TranscodeAac { bitrate_kbps } => {
                let ff = self
                    .ffmpeg_path
                    .as_deref()
                    .ok_or(PipelineError::FfmpegNotFound)?;
                let temp_path = run_ffmpeg_transcode(
                    ff,
                    &track.source_path,
                    TranscodeCodec::Aac { bitrate_kbps },
                    &cancel,
                )
                .await?;
                let (size, len_ms, br, sr) = probe_transcoded_m4a(&temp_path, &track);
                Ok(PreparedTrack {
                    queue_index,
                    scanned: track,
                    audio_source: PreparedAudioSource::TempTranscoded(temp_path),
                    final_ext: "m4a",
                    final_filetype: FILETYPE_M4A,
                    final_filetype_desc: "AAC audio file",
                    final_size: size,
                    final_length_ms: len_ms,
                    final_bitrate_kbps: if br > 0 { br } else { bitrate_kbps },
                    final_sample_rate_hz: sr,
                    prepared_artwork,
                })
            }
        }
    }
}

#[derive(Clone, Copy)]
enum TranscodeCodec {
    Alac,
    Aac { bitrate_kbps: u32 },
}

async fn run_ffmpeg_transcode(
    ffmpeg_bin: &Path,
    source_path: &Path,
    codec: TranscodeCodec,
    cancel: &CancellationToken,
) -> Result<TempPath, PipelineError> {
    let temp_file = Builder::new()
        .prefix("podbridge_tx_")
        .suffix(".m4a")
        .tempfile()?;
    let temp_path = temp_file.into_temp_path();

    let mut cmd = Command::new(ffmpeg_bin);
    cmd.kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .arg("-y")
        .arg("-v")
        .arg("error")
        .arg("-i")
        .arg(source_path)
        .arg("-vn");

    match codec {
        TranscodeCodec::Alac => {
            cmd.args(["-c:a", "alac", "-movflags", "+faststart"]);
        }
        TranscodeCodec::Aac { bitrate_kbps } => {
            cmd.arg("-c:a")
                .arg("aac")
                .arg("-b:a")
                .arg(format!("{bitrate_kbps}k"))
                .args(["-movflags", "+faststart"]);
        }
    }
    cmd.arg(temp_path.as_os_str());

    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let child = cmd.spawn()?;
    let output = tokio::select! {
        biased;
        () = cancel.cancelled() => {
            // Dropping `child` with `kill_on_drop(true)` terminates the FFmpeg subprocess immediately,
            // and dropping `temp_path` deletes the partial output file.
            return Err(PipelineError::Cancelled);
        }
        res = child.wait_with_output() => res?,
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(PipelineError::TranscodeFailed {
            path: source_path.to_string_lossy().into_owned(),
            stderr,
        });
    }

    Ok(temp_path)
}

fn probe_transcoded_m4a(temp_path: &Path, fallback: &ScannedTrack) -> (u32, u32, u32, u32) {
    let size = fs::metadata(temp_path).map_or(0, |m| m.len().min(u64::from(u32::MAX)) as u32);
    let mut len_ms = fallback.length_ms;
    let mut br = fallback.bitrate_kbps;
    let mut sr = fallback.sample_rate_hz;

    if let Some(tf) = Probe::open(temp_path)
        .ok()
        .and_then(|p| p.options(ParseOptions::new()).read().ok())
    {
        let props = tf.properties();
        let d = props.duration().as_millis() as u32;
        if d > 0 {
            len_ms = d;
        }
        if let Some(b) = props.audio_bitrate().or_else(|| props.overall_bitrate()) {
            if b > 0 {
                br = b;
            }
        }
        if let Some(s) = props.sample_rate() {
            if s > 0 {
                sr = s;
            }
        }
    }

    (size, len_ms, br, sr)
}
