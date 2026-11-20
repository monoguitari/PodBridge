// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

import { useEffect, useState } from "react";
import { RotateCcw, X } from "lucide-react";
import { podbridgeApi } from "../api/bridge";
import type {
  AppSettings,
  BackupManifest,
  DuplicatePolicy,
  LosslessTarget,
} from "../types";
import { formatBytes } from "../utils/format";

interface SettingsModalProps {
  open: boolean;
  settings: AppSettings;
  activeDeviceId: string | null;
  onClose: () => void;
  onSaveSettings: (next: AppSettings) => void;
  onRestoreBackup: (backupId: string) => void;
}

export function SettingsModal({
  open,
  settings,
  activeDeviceId,
  onClose,
  onSaveSettings,
  onRestoreBackup,
}: SettingsModalProps) {
  const [draft, setDraft] = useState<AppSettings>(settings);
  const [backups, setBackups] = useState<BackupManifest[]>([]);

  useEffect(() => {
    setDraft(settings);
  }, [settings]);

  useEffect(() => {
    if (!open || !activeDeviceId) {
      setBackups([]);
      return;
    }
    void podbridgeApi.listBackups(activeDeviceId).then(setBackups);
  }, [open, activeDeviceId]);

  if (!open) return null;

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    onSaveSettings(draft);
    onClose();
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <form
        className="modal-card"
        style={{ maxWidth: 520 }}
        onClick={(e) => e.stopPropagation()}
        onSubmit={submit}
      >
        <div className="modal-header">
          <span style={{ fontWeight: 700, fontSize: 15 }}>
            Settings & Database Safety Backups
          </span>
          <button type="button" className="btn-icon" onClick={onClose}>
            <X size={15} />
          </button>
        </div>

        <label className="field-label">
          <span>Lossless Auto-Transcode Target (FLAC / APE / WV)</span>
          <select
            className="search-input"
            style={{ paddingLeft: 10 }}
            value={draft.transfer.lossless_target}
            onChange={(e) =>
              setDraft({
                ...draft,
                transfer: {
                  ...draft.transfer,
                  lossless_target: e.target.value as LosslessTarget,
                },
              })
            }
          >
            <option value="Alac">
              Apple Lossless (ALAC .m4a) — Recommended (Fast, 100% Lossless)
            </option>
            <option value="Aac">
              AAC (.m4a) — Saves iPod Drive Space
            </option>
          </select>
        </label>

        <label className="field-label">
          <span>AAC Transcode Bitrate (for OGG / Opus / WMA)</span>
          <select
            className="search-input"
            style={{ paddingLeft: 10 }}
            value={draft.transfer.aac_bitrate_kbps}
            onChange={(e) =>
              setDraft({
                ...draft,
                transfer: {
                  ...draft.transfer,
                  aac_bitrate_kbps: Number(e.target.value),
                },
              })
            }
          >
            <option value={192}>192 kbps AAC</option>
            <option value={256}>256 kbps AAC (Recommended)</option>
            <option value={320}>320 kbps AAC</option>
          </select>
        </label>

        <label className="field-label">
          <span>Duplicate Song Policy</span>
          <select
            className="search-input"
            style={{ paddingLeft: 10 }}
            value={draft.transfer.duplicate_policy}
            onChange={(e) =>
              setDraft({
                ...draft,
                transfer: {
                  ...draft.transfer,
                  duplicate_policy: e.target.value as DuplicatePolicy,
                },
              })
            }
          >
            <option value="Skip">
              Skip songs already on iPod (Recommended)
            </option>
            <option value="KeepBoth">Transfer duplicate copy anyway</option>
          </select>
        </label>

        {activeDeviceId && (
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            <span
              style={{
                fontSize: 11.5,
                fontWeight: 650,
                color: "var(--text-secondary)",
              }}
            >
              Automatic iTunesDB + ArtworkDB Backups (Last 10)
            </span>
            {backups.length === 0 ? (
              <div
                style={{
                  fontSize: 12,
                  color: "var(--text-muted)",
                  padding: "8px 0",
                }}
              >
                No backups created yet — a snapshot is saved automatically
                before every transfer or edit.
              </div>
            ) : (
              <div className="backup-list">
                {backups.map((b) => (
                  <div key={b.id} className="backup-row">
                    <div>
                      <div style={{ fontWeight: 600 }} className="tabular">
                        {new Date(b.created_unix_ms).toLocaleString()}
                      </div>
                      <div
                        className="tabular"
                        style={{ fontSize: 11, color: "var(--text-muted)" }}
                      >
                        {b.track_count} songs · {b.playlist_count} playlists ·{" "}
                        {formatBytes(b.itunesdb_bytes)}
                      </div>
                    </div>
                    <button
                      type="button"
                      className="btn"
                      onClick={() => {
                        onRestoreBackup(b.id);
                        onClose();
                      }}
                    >
                      <RotateCcw size={13} />
                      <span>Restore</span>
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

        <div className="modal-footer">
          <button type="button" className="btn" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="btn btn-primary">
            Save Preferences
          </button>
        </div>
      </form>
    </div>
  );
}
