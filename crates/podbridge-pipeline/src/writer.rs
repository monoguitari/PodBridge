// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use podbridge_artwork::{flush_pending_ithmb_writes, serialize_artworkdb, PendingIthmbWrite};
use podbridge_device::ConnectedIpod;
use podbridge_itdb::{
    ipod_location_to_rel_path, serialize_itunesdb, unix_to_mac_timestamp, DuplicateIndex,
    NewTrackInput,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::backup::{create_rotating_backup, write_file_atomic};
use crate::error::PipelineError;
use crate::scanner::{scan_dropped_paths, TransferAction};
use crate::settings::TransferSettings;
use crate::transcode::{PreparedTrack, TranscodePool};

/// 2 MiB sequential copy buffer tuned to saturate USB 2.0 (~25–30 MB/s) without seek thrashing on
/// the iPod Classic's 1.8" 4200 RPM drive.
const SEQUENTIAL_COPY_BUF_BYTES: usize = 2 * 1024 * 1024;
/// Minimum interval between progress event emissions (`~30 Hz`) to keep IPC overhead near zero.
const PROGRESS_THROTTLE_INTERVAL: Duration = Duration::from_millis(33);

/// High-level transfer batch lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferPhase {
    Scanning,
    Transferring,
    CommittingDatabase,
    Completed,
    Cancelled,
    Failed,
}

/// Per-track status pill displayed in the bottom Transfer Queue drawer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferItemStatus {
    Queued,
    TranscodingAlac,
    TranscodingAac,
    Copying,
    SkippedDuplicate,
    Done,
    Error(String),
    Cancelled,
}

/// Single row in the UI Transfer Queue drawer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferQueueItem {
    pub index: usize,
    pub title: String,
    pub artist: String,
    pub source_filename: String,
    pub status: TransferItemStatus,
}

/// Throttled real-time progress snapshot pushed to the UI (`transfer://progress`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgressSnapshot {
    pub batch_id: u64,
    pub phase: TransferPhase,
    pub completed_files: usize,
    pub total_files: usize,
    pub skipped_duplicates: usize,
    pub bytes_written: u64,
    pub total_bytes_estimated: u64,
    pub throughput_mb_per_sec: f64,
    pub eta_seconds: Option<u64>,
    pub items: Vec<TransferQueueItem>,
}

/// Summary returned when a batch transfer commits cleanly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCommitSummary {
    pub added_tracks: usize,
    pub skipped_duplicates: usize,
    pub created_playlists: usize,
    pub bytes_written: u64,
    pub elapsed_ms: u64,
}

struct SequentialWriteResult {
    staged_itdb: podbridge_itdb::ItdbDatabase,
    staged_artdb: podbridge_artwork::ArtworkDatabase,
    pending_ithmb_writes: Vec<PendingIthmbWrite>,
    written_audio_files: Vec<PathBuf>,
    track_id_by_source: HashMap<PathBuf, u32>,
    completed_files: usize,
    bytes_written: u64,
}

fn current_mac_timestamp() -> u32 {
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    unix_to_mac_timestamp(unix)
}

/// Generates a collision-free iPod track path (`F00..F{N-1}/ABCD.<ext>`) and its colon-delimited
/// `iTunesDB` location (`:iPod_Control:Music:F00:ABCD.<ext>`).
fn allocate_ipod_music_slot(
    music_dir: &Path,
    dir_count: u32,
    folder_cursor: &mut u32,
    ext: &str,
) -> Result<(PathBuf, String), std::io::Error> {
    let count = dir_count.max(1);
    let folder_idx = *folder_cursor % count;
    *folder_cursor = folder_cursor.wrapping_add(1);

    let folder_name = format!("F{folder_idx:02}");
    let folder_path = music_dir.join(&folder_name);
    fs::create_dir_all(&folder_path)?;

    let mut rng = rand::thread_rng();
    loop {
        let code: String = (0..4)
            .map(|_| (b'A' + rng.gen_range(0..26)) as char)
            .collect();
        let fname = format!("{code}.{ext}");
        let full_path = folder_path.join(&fname);
        if !full_path.exists() {
            let colon_loc = format!(":iPod_Control:Music:{folder_name}:{fname}");
            return Ok((full_path, colon_loc));
        }
    }
}

/// Executes a complete pipelined transfer batch onto `ipod`:
///
/// 1. Parallel `lofty` metadata/artwork scan + duplicate check.
/// 2. Multi-core FFmpeg transcode (`FLAC -> ALAC`, `OGG/Opus -> AAC`) + cover resize on PC SSD/RAM.
/// 3. Single sequential 2 MiB buffered USB writer to `iPod_Control/Music/F00..F49/`.
/// 4. Rotating local `iTunesDB`+`ArtworkDB` backup + atomic `.tmp -> fsync -> rename` commit.
///
/// If `cancel` triggers at any point before step 4's rename, all newly copied audio files are
/// rolled back so the iPod stays 100% consistent.
pub async fn execute_transfer_batch<F>(
    ipod: &mut ConnectedIpod,
    dropped_paths: &[PathBuf],
    settings: &TransferSettings,
    backup_root: &Path,
    explicit_ffmpeg: Option<&Path>,
    cancel: CancellationToken,
    on_progress: F,
) -> Result<BatchCommitSummary, PipelineError>
where
    F: Fn(TransferProgressSnapshot) + Send + Sync + 'static,
{
    let batch_start = Instant::now();
    let batch_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |d| d.as_millis() as u64);
    let progress_cb = Arc::new(on_progress);

    // Phase 1: Parallel Scan
    let existing_dup = DuplicateIndex::from_database(&ipod.itdb);
    let paths_vec = dropped_paths.to_vec();
    let settings_clone = settings.clone();
    let report = tokio::task::spawn_blocking(move || {
        scan_dropped_paths(&paths_vec, &settings_clone, &existing_dup)
    })
    .await
    .map_err(|e| PipelineError::Io(std::io::Error::other(e.to_string())))?;

    if cancel.is_cancelled() {
        return Err(PipelineError::Cancelled);
    }

    let mut items_ui: Vec<TransferQueueItem> = Vec::with_capacity(report.tracks.len());
    let mut active_work: Vec<(usize, crate::scanner::ScannedTrack)> = Vec::new();
    let mut track_id_by_source: HashMap<PathBuf, u32> = HashMap::new();
    let mut estimated_total_bytes = 0u64;

    for (idx, tr) in report.tracks.iter().enumerate() {
        let source_filename = tr
            .source_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("track")
            .to_owned();
        if let Some(dup_tid) = tr.duplicate_of_track_id {
            track_id_by_source.insert(tr.source_path.clone(), dup_tid);
            items_ui.push(TransferQueueItem {
                index: idx,
                title: tr.title.clone(),
                artist: tr.artist.clone(),
                source_filename,
                status: TransferItemStatus::SkippedDuplicate,
            });
        } else {
            // Estimate output size (ALAC ~65% of WAV / ~95% of FLAC, AAC ~32 KB/s)
            let est = match tr.action {
                TransferAction::CopyNative { .. } => tr.source_size,
                TransferAction::TranscodeAlac => tr.source_size,
                TransferAction::TranscodeAac { bitrate_kbps } => {
                    if tr.length_ms > 0 {
                        (u64::from(tr.length_ms) * u64::from(bitrate_kbps) * 125) / 1_000
                    } else {
                        tr.source_size / 3
                    }
                }
            };
            estimated_total_bytes = estimated_total_bytes.saturating_add(est.max(4_096));
            items_ui.push(TransferQueueItem {
                index: idx,
                title: tr.title.clone(),
                artist: tr.artist.clone(),
                source_filename,
                status: TransferItemStatus::Queued,
            });
            active_work.push((idx, tr.clone()));
        }
    }

    let total_to_transfer = active_work.len();
    let skipped_duplicates = report.skipped_duplicates;

    if estimated_total_bytes > ipod.summary.storage.free_bytes {
        return Err(PipelineError::InsufficientSpace {
            needed_bytes: estimated_total_bytes,
            free_bytes: ipod.summary.storage.free_bytes,
        });
    }

    let shared_items = Arc::new(Mutex::new(items_ui));
    let emit_snapshot = {
        let shared_items = Arc::clone(&shared_items);
        let progress_cb = Arc::clone(&progress_cb);
        move |phase: TransferPhase, done: usize, bytes_written: u64, mbps: f64, eta: Option<u64>| {
            let snapshot_items = shared_items
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            progress_cb(TransferProgressSnapshot {
                batch_id,
                phase,
                completed_files: done,
                total_files: total_to_transfer,
                skipped_duplicates,
                bytes_written,
                total_bytes_estimated: estimated_total_bytes.max(bytes_written),
                throughput_mb_per_sec: mbps,
                eta_seconds: eta,
                items: snapshot_items,
            });
        }
    };

    emit_snapshot(TransferPhase::Transferring, 0, 0, 0.0, None);

    // If nothing new needs copying (e.g., M3U import of already-on-iPod tracks), still commit playlists!
    if active_work.is_empty() && report.playlists.is_empty() {
        emit_snapshot(TransferPhase::Completed, 0, 0, 0.0, Some(0));
        return Ok(BatchCommitSummary {
            added_tracks: 0,
            skipped_duplicates,
            created_playlists: 0,
            bytes_written: 0,
            elapsed_ms: batch_start.elapsed().as_millis() as u64,
        });
    }

    // Phase 2 & 3: Producer pool -> Bounded Channel (cap = 8) -> Single Sequential Writer
    let artwork_specs = ipod
        .artwork_db
        .effective_specs(ipod.summary.generation.artwork_family());
    let pool = TranscodePool::new(explicit_ffmpeg);
    let (tx, mut rx) = mpsc::channel::<Result<PreparedTrack, (usize, PipelineError)>>(8);

    for (q_idx, track) in active_work {
        let tx_clone = tx.clone();
        let pool_clone = pool.clone();
        let specs_clone = artwork_specs.clone();
        let cancel_clone = cancel.clone();
        let items_ref = Arc::clone(&shared_items);
        let pre_status = match track.action {
            TransferAction::CopyNative { .. } => TransferItemStatus::Queued,
            TransferAction::TranscodeAlac => TransferItemStatus::TranscodingAlac,
            TransferAction::TranscodeAac { .. } => TransferItemStatus::TranscodingAac,
        };

        tokio::spawn(async move {
            if pre_status != TransferItemStatus::Queued {
                if let Ok(mut g) = items_ref.lock() {
                    g[q_idx].status = pre_status;
                }
            }
            let res = pool_clone
                .prepare_track(q_idx, track, specs_clone, cancel_clone)
                .await
                .map_err(|e| (q_idx, e));
            let _ = tx_clone.send(res).await;
        });
    }
    drop(tx);

    let music_dir = ipod.music_dir();
    let dir_count = ipod.summary.generation.music_dir_count();
    let mut staged_itdb = ipod.itdb.clone();
    let mut staged_artdb = ipod.artwork_db.clone();
    let mut pending_ithmb_writes: Vec<PendingIthmbWrite> = Vec::new();
    let mut written_audio_files: Vec<PathBuf> = Vec::new();
    let mut folder_cursor = staged_itdb.tracks.len() as u32;
    let now_mac = current_mac_timestamp();

    let emit_writer = emit_snapshot.clone();
    let items_writer = Arc::clone(&shared_items);
    let cancel_writer = cancel.clone();

    // Run sequential buffered disk I/O on a dedicated blocking thread reading from `rx`
    let writer_outcome = tokio::task::spawn_blocking(
        move || -> Result<SequentialWriteResult, (Vec<PathBuf>, PipelineError)> {
            let mut copy_buf = vec![0u8; SEQUENTIAL_COPY_BUF_BYTES];
            let mut bytes_written = 0u64;
            let mut completed_files = 0usize;
            let mut deferred_art_links: Vec<(usize, [u8; 32], u32)> = Vec::new();
            let io_start = Instant::now();
            let mut last_emit = Instant::now() - PROGRESS_THROTTLE_INTERVAL;

            while let Some(item_res) = rx.blocking_recv() {
                if cancel_writer.is_cancelled() {
                    return Err((written_audio_files, PipelineError::Cancelled));
                }

                let prepared = match item_res {
                    Ok(p) => p,
                    Err((_, PipelineError::Cancelled)) => {
                        return Err((written_audio_files, PipelineError::Cancelled));
                    }
                    Err((q_idx, err)) => {
                        if let Ok(mut g) = items_writer.lock() {
                            g[q_idx].status = TransferItemStatus::Error(err.to_string());
                        }
                        return Err((written_audio_files, err));
                    }
                };

                let q_idx = prepared.queue_index;
                if let Ok(mut g) = items_writer.lock() {
                    g[q_idx].status = TransferItemStatus::Copying;
                }

                let (dest_path, colon_loc) = match allocate_ipod_music_slot(
                    &music_dir,
                    dir_count,
                    &mut folder_cursor,
                    prepared.final_ext,
                ) {
                    Ok(pair) => pair,
                    Err(e) => return Err((written_audio_files, PipelineError::Io(e))),
                };

                written_audio_files.push(dest_path.clone());

                let copy_res = (|| -> Result<u64, std::io::Error> {
                    let mut src = File::open(prepared.audio_source.as_path())?;
                    let mut dst = File::create(&dest_path)?;
                    let mut copied_this_file = 0u64;
                    loop {
                        if cancel_writer.is_cancelled() {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::Interrupted,
                                "cancelled",
                            ));
                        }
                        let n = src.read(&mut copy_buf)?;
                        if n == 0 {
                            break;
                        }
                        dst.write_all(&copy_buf[..n])?;
                        copied_this_file += n as u64;
                        bytes_written += n as u64;

                        if last_emit.elapsed() >= PROGRESS_THROTTLE_INTERVAL {
                            let secs = io_start.elapsed().as_secs_f64().max(0.001);
                            let mbps = (bytes_written as f64 / (1024.0 * 1024.0)) / secs;
                            let rem = estimated_total_bytes.saturating_sub(bytes_written);
                            let eta = if mbps > 0.05 {
                                Some(((rem as f64 / (1024.0 * 1024.0)) / mbps).ceil() as u64)
                            } else {
                                None
                            };
                            emit_writer(
                                TransferPhase::Transferring,
                                completed_files,
                                bytes_written,
                                mbps,
                                eta,
                            );
                            last_emit = Instant::now();
                        }
                    }
                    dst.sync_data()?;
                    Ok(copied_this_file)
                })();

                let actual_copied = match copy_res {
                    Ok(n) => n,
                    Err(_) if cancel_writer.is_cancelled() => {
                        return Err((written_audio_files, PipelineError::Cancelled));
                    }
                    Err(e) => return Err((written_audio_files, PipelineError::Io(e))),
                };

                // Allocate / deduplicate ArtworkDB entry
                let next_dbid_preview = staged_itdb.alloc_unique_u64();
                let mut mhii_link = 0u32;
                let mut has_artwork = false;
                let mut orig_art_size = 0u32;

                let mut deferred_sha: Option<([u8; 32], u32)> = None;
                if let Some(prep_art) = prepared.prepared_artwork {
                    orig_art_size = prep_art.orig_img_size;
                    if let Some(thumbs) = prep_art.encoded_thumbs {
                        let (id, writes) = staged_artdb.insert_encoded_thumbnails(
                            Some(prep_art.sha256),
                            prep_art.orig_img_size,
                            next_dbid_preview,
                            thumbs,
                        );
                        mhii_link = id;
                        has_artwork = true;
                        pending_ithmb_writes.extend(writes);
                    } else if let Some(id) = staged_artdb.increment_ref_by_sha256(&prep_art.sha256)
                    {
                        mhii_link = id;
                        has_artwork = true;
                    } else {
                        deferred_sha = Some((prep_art.sha256, prep_art.orig_img_size));
                    }
                }

                let track_vec_idx = staged_itdb.tracks.len();
                let s = &prepared.scanned;
                let (new_tid, actual_dbid) = staged_itdb.add_track(NewTrackInput {
                    title: s.title.clone(),
                    artist: s.artist.clone(),
                    album_artist: s.album_artist.clone(),
                    album: s.album.clone(),
                    genre: s.genre.clone(),
                    composer: s.composer.clone(),
                    comment: s.comment.clone(),
                    location: colon_loc,
                    filetype: prepared.final_filetype,
                    filetype_desc: prepared.final_filetype_desc.to_owned(),
                    size: (actual_copied.min(u64::from(u32::MAX)) as u32).max(prepared.final_size),
                    length_ms: prepared.final_length_ms,
                    bitrate: prepared.final_bitrate_kbps,
                    sample_rate_hz: prepared.final_sample_rate_hz,
                    sample_count: 0,
                    track_nr: s.track_nr,
                    tracks_total: s.tracks_total,
                    disc_nr: s.disc_nr,
                    discs_total: s.discs_total,
                    year: s.year,
                    compilation: s.compilation,
                    has_artwork,
                    artwork_size: orig_art_size,
                    mhii_link,
                    timestamp_mac: now_mac,
                });

                // Patch song_id on newly created ArtworkItem to match the track's actual dbid
                if has_artwork {
                    if let Some(im) = staged_artdb.images.iter_mut().find(|i| i.id == mhii_link) {
                        if im.song_id == next_dbid_preview {
                            im.song_id = actual_dbid;
                        }
                    }
                }

                if let Some((sha, osize)) = deferred_sha {
                    deferred_art_links.push((track_vec_idx, sha, osize));
                }

                track_id_by_source.insert(s.source_path.clone(), new_tid);
                completed_files += 1;

                if let Ok(mut g) = items_writer.lock() {
                    g[q_idx].status = TransferItemStatus::Done;
                }

                let secs = io_start.elapsed().as_secs_f64().max(0.001);
                let mbps = (bytes_written as f64 / (1024.0 * 1024.0)) / secs;
                let rem = estimated_total_bytes.saturating_sub(bytes_written);
                let eta = if completed_files == total_to_transfer {
                    Some(0)
                } else if mbps > 0.05 {
                    Some(((rem as f64 / (1024.0 * 1024.0)) / mbps).ceil() as u64)
                } else {
                    None
                };
                emit_writer(
                    TransferPhase::Transferring,
                    completed_files,
                    bytes_written,
                    mbps,
                    eta,
                );
                last_emit = Instant::now();
            }

            for (t_idx, sha, osize) in deferred_art_links {
                if let Some(id) = staged_artdb.increment_ref_by_sha256(&sha) {
                    if let Some(tr) = staged_itdb.tracks.get_mut(t_idx) {
                        tr.has_artwork = true;
                        tr.artwork_count = 1;
                        tr.artwork_size = osize;
                        tr.mhii_link = id;
                    }
                }
            }

            Ok(SequentialWriteResult {
                staged_itdb,
                staged_artdb,
                pending_ithmb_writes,
                written_audio_files,
                track_id_by_source,
                completed_files,
                bytes_written,
            })
        },
    )
    .await
    .map_err(|e| PipelineError::Io(std::io::Error::other(e.to_string())))?;

    let SequentialWriteResult {
        mut staged_itdb,
        staged_artdb,
        pending_ithmb_writes,
        written_audio_files: written_files,
        track_id_by_source,
        completed_files: added_tracks,
        bytes_written,
    } = match writer_outcome {
        Ok(ok) => ok,
        Err((partial_files, err)) => {
            // Roll back all audio files copied in this aborted batch!
            for f in partial_files {
                let _ = fs::remove_file(f);
            }
            if matches!(err, PipelineError::Cancelled) {
                if let Ok(mut g) = shared_items.lock() {
                    for item in g.iter_mut() {
                        if matches!(
                            item.status,
                            TransferItemStatus::Queued
                                | TransferItemStatus::Copying
                                | TransferItemStatus::TranscodingAlac
                                | TransferItemStatus::TranscodingAac
                        ) {
                            item.status = TransferItemStatus::Cancelled;
                        }
                    }
                }
                emit_snapshot(TransferPhase::Cancelled, 0, 0, 0.0, None);
            } else {
                emit_snapshot(TransferPhase::Failed, 0, 0, 0.0, None);
            }
            return Err(err);
        }
    };

    // Create any imported M3U / M3U8 playlists
    let mut created_playlists = 0usize;
    for pl_spec in &report.playlists {
        let ordered_tids: Vec<u32> = pl_spec
            .source_paths
            .iter()
            .filter_map(|p| track_id_by_source.get(p).copied())
            .collect();
        let pid = staged_itdb.create_playlist(&pl_spec.name, now_mac);
        staged_itdb.set_playlist_tracks(pid, &ordered_tids, now_mac)?;
        created_playlists += 1;
    }

    // Phase 4: Rotating Backup + Atomic Commit
    emit_snapshot(
        TransferPhase::CommittingDatabase,
        added_tracks,
        bytes_written,
        0.0,
        Some(0),
    );

    let commit_res = (|| -> Result<(), PipelineError> {
        create_rotating_backup(ipod, backup_root)?;
        flush_pending_ithmb_writes(&ipod.artwork_dir(), &pending_ithmb_writes)?;

        let art_bytes = serialize_artworkdb(&staged_artdb);
        write_file_atomic(&ipod.artwork_db_path(), &art_bytes)?;

        let itdb_bytes = serialize_itunesdb(&mut staged_itdb, &ipod.hashing_scheme)?;
        write_file_atomic(&ipod.itunes_db_path(), &itdb_bytes)?;
        Ok(())
    })();

    if let Err(err) = commit_res {
        for f in written_files {
            let _ = fs::remove_file(f);
        }
        emit_snapshot(
            TransferPhase::Failed,
            added_tracks,
            bytes_written,
            0.0,
            None,
        );
        return Err(err);
    }

    ipod.itdb = staged_itdb;
    ipod.artwork_db = staged_artdb;
    ipod.refresh_summary_metrics();

    let total_secs = batch_start.elapsed().as_secs_f64().max(0.001);
    let final_mbps = (bytes_written as f64 / (1024.0 * 1024.0)) / total_secs;
    emit_snapshot(
        TransferPhase::Completed,
        added_tracks,
        bytes_written,
        final_mbps,
        Some(0),
    );

    Ok(BatchCommitSummary {
        added_tracks,
        skipped_duplicates,
        created_playlists,
        bytes_written,
        elapsed_ms: batch_start.elapsed().as_millis() as u64,
    })
}

/// Deletes `track_ids` from `ipod`, backing up `iTunesDB` + `ArtworkDB` first, deleting the
/// corresponding audio files from `iPod_Control/Music/`, releasing `ArtworkDB` slot references,
/// and atomically committing both databases.
pub fn delete_tracks_atomic(
    ipod: &mut ConnectedIpod,
    track_ids: &[u32],
    backup_root: &Path,
) -> Result<usize, PipelineError> {
    if track_ids.is_empty() {
        return Ok(0);
    }

    create_rotating_backup(ipod, backup_root)?;

    let mut staged_itdb = ipod.itdb.clone();
    let mut staged_artdb = ipod.artwork_db.clone();

    let removed = staged_itdb.remove_tracks(track_ids);
    if removed.is_empty() {
        return Ok(0);
    }

    for r in &removed {
        if r.mhii_link != 0 {
            staged_artdb.release_artwork_ref(r.mhii_link);
        }
    }

    let art_bytes = serialize_artworkdb(&staged_artdb);
    write_file_atomic(&ipod.artwork_db_path(), &art_bytes)?;

    let itdb_bytes = serialize_itunesdb(&mut staged_itdb, &ipod.hashing_scheme)?;
    write_file_atomic(&ipod.itunes_db_path(), &itdb_bytes)?;

    // Delete underlying audio files only AFTER the database commit succeeds!
    for r in &removed {
        let rel = ipod_location_to_rel_path(&r.location);
        if !rel.as_os_str().is_empty() {
            let full = ipod.summary.root_path.join(rel);
            let _ = fs::remove_file(full);
        }
    }

    ipod.itdb = staged_itdb;
    ipod.artwork_db = staged_artdb;
    ipod.refresh_summary_metrics();
    Ok(removed.len())
}

/// Atomically persists in-memory playlist mutations (`create_playlist`, `rename_playlist`,
/// `set_playlist_tracks`, `delete_playlist`) to disk with a rotating backup.
pub fn commit_database_atomic(
    ipod: &mut ConnectedIpod,
    backup_root: &Path,
) -> Result<(), PipelineError> {
    create_rotating_backup(ipod, backup_root)?;
    let itdb_bytes = serialize_itunesdb(&mut ipod.itdb, &ipod.hashing_scheme)?;
    write_file_atomic(&ipod.itunes_db_path(), &itdb_bytes)?;
    ipod.refresh_summary_metrics();
    Ok(())
}
