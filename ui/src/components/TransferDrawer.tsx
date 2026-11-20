// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { ChevronDown, ChevronUp, Loader2, X, XCircle } from "lucide-react";
import type { TransferItemStatus, TransferProgressSnapshot } from "../types";
import { formatBytes, formatEtaSeconds } from "../utils/format";

interface TransferDrawerProps {
  progress: TransferProgressSnapshot | null;
  onCancel: () => void;
  onDismiss: () => void;
}

function renderStatusPill(status: TransferItemStatus) {
  if (status === "Queued") {
    return <span className="q-pill q-muted">Queued</span>;
  }
  if (status === "TranscodingAlac") {
    return <span className="q-pill q-warn">Transcoding ALAC</span>;
  }
  if (status === "TranscodingAac") {
    return <span className="q-pill q-warn">Transcoding AAC</span>;
  }
  if (status === "Copying") {
    return <span className="q-pill q-active">Copying USB</span>;
  }
  if (status === "SkippedDuplicate") {
    return <span className="q-pill q-muted">Skipped Duplicate</span>;
  }
  if (status === "Done") {
    return <span className="q-pill q-ok">Done</span>;
  }
  if (status === "Cancelled") {
    return <span className="q-pill q-muted">Cancelled</span>;
  }
  return (
    <span className="q-pill q-err" title={status.Error}>
      Error
    </span>
  );
}

export function TransferDrawer({
  progress,
  onCancel,
  onDismiss,
}: TransferDrawerProps) {
  const [expanded, setExpanded] = useState(true);

  if (!progress) return null;

  const isRunning =
    progress.phase === "Scanning" ||
    progress.phase === "Transferring" ||
    progress.phase === "CommittingDatabase";

  const pct =
    progress.total_bytes_estimated > 0
      ? Math.min(
          100,
          (progress.bytes_written / progress.total_bytes_estimated) * 100,
        )
      : progress.total_files > 0
        ? Math.min(100, (progress.completed_files / progress.total_files) * 100)
        : 100;

  const phaseHeadline = (() => {
    switch (progress.phase) {
      case "Scanning":
        return "Scanning audio tags & artwork…";
      case "Transferring":
        return `Transferring ${progress.completed_files} / ${progress.total_files}`;
      case "CommittingDatabase":
        return "Signing & committing iTunesDB + ArtworkDB…";
      case "Completed":
        return `Transfer complete · ${progress.completed_files} added`;
      case "Cancelled":
        return "Transfer cancelled (rolled back cleanly)";
      case "Failed":
        return "Transfer encountered an error";
    }
  })();

  return (
    <div className="transfer-drawer">
      <div className="transfer-header">
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            minWidth: 0,
            flex: 1,
          }}
        >
          {isRunning && <Loader2 size={15} className="spin" />}
          <span style={{ fontWeight: 650 }} className="tabular">
            {phaseHeadline}
          </span>

          {progress.skipped_duplicates > 0 && (
            <span className="q-pill q-muted tabular">
              {progress.skipped_duplicates} dup skipped
            </span>
          )}

          {isRunning && progress.throughput_mb_per_sec > 0.05 && (
            <span
              className="tabular"
              style={{ color: "var(--text-secondary)", fontSize: 12 }}
            >
              {progress.throughput_mb_per_sec.toFixed(1)} MB/s ·{" "}
              {formatBytes(progress.bytes_written)} ·{" "}
              {formatEtaSeconds(progress.eta_seconds)}
            </span>
          )}
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          {isRunning ? (
            <button
              type="button"
              className="btn btn-danger"
              style={{ padding: "4px 9px", fontSize: 11.5 }}
              onClick={onCancel}
            >
              <XCircle size={13} />
              <span>Cancel</span>
            </button>
          ) : (
            <button
              type="button"
              className="btn-icon"
              title="Dismiss queue"
              onClick={onDismiss}
            >
              <X size={14} />
            </button>
          )}

          <button
            type="button"
            className="btn-icon"
            onClick={() => setExpanded((e) => !e)}
            title={expanded ? "Collapse queue" : "Expand queue"}
          >
            {expanded ? <ChevronDown size={15} /> : <ChevronUp size={15} />}
          </button>
        </div>
      </div>

      <div className="transfer-progress-track">
        <div
          className="transfer-progress-fill"
          style={{ width: `${isRunning ? Math.max(pct, 3) : 100}%` }}
        />
      </div>

      {expanded && progress.items.length > 0 && (
        <div className="transfer-items-list">
          {progress.items.map((item) => (
            <div key={item.index} className="transfer-item-row">
              <div style={{ minWidth: 0, flex: 1 }}>
                <span style={{ fontWeight: 550 }}>{item.title}</span>
                {item.artist && (
                  <span style={{ color: "var(--text-muted)" }}>
                    {" "}
                    — {item.artist}
                  </span>
                )}
              </div>
              {renderStatusPill(item.status)}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
