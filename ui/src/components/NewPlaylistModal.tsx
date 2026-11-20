// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { X } from "lucide-react";

interface NewPlaylistModalProps {
  open: boolean;
  onClose: () => void;
  onCreate: (name: string) => void;
}

export function NewPlaylistModal({
  open,
  onClose,
  onCreate,
}: NewPlaylistModalProps) {
  const [name, setName] = useState("");

  if (!open) return null;

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = name.trim();
    if (!trimmed) return;
    onCreate(trimmed);
    setName("");
    onClose();
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <form
        className="modal-card"
        style={{ maxWidth: 380 }}
        onClick={(e) => e.stopPropagation()}
        onSubmit={submit}
      >
        <div className="modal-header">
          <span style={{ fontWeight: 700, fontSize: 15 }}>New iPod Playlist</span>
          <button type="button" className="btn-icon" onClick={onClose}>
            <X size={15} />
          </button>
        </div>

        <label className="field-label">
          <span>Playlist Name</span>
          <input
            type="text"
            className="search-input"
            style={{ paddingLeft: 10 }}
            placeholder="e.g. Late Night Drive"
            value={name}
            autoFocus
            onChange={(e) => setName(e.target.value)}
          />
        </label>

        <div className="modal-footer">
          <button type="button" className="btn" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            className="btn btn-primary"
            disabled={!name.trim()}
          >
            Create Playlist
          </button>
        </div>
      </form>
    </div>
  );
}
