// Copyright 2026 PodBridge Contributors
// SPDX-License-Identifier: Apache-2.0

import { useState } from "react";
import { FolderOpen, X } from "lucide-react";
import { podbridgeApi } from "../api/bridge";
import type { IpodGeneration } from "../types";

interface MockDeviceModalProps {
  open: boolean;
  onClose: () => void;
  onCreateMock: (path: string, name: string, gen: IpodGeneration) => void;
  onOpenExistingMock: (path: string) => void;
}

const GENERATIONS: { value: IpodGeneration; label: string }[] = [
  { value: "Classic6G", label: "iPod Classic 6G (MB147LL · hash58)" },
  { value: "Classic6_5G", label: "iPod Classic 6.5G (120GB · hash58)" },
  { value: "Classic7G", label: "iPod Classic 7G (160GB · hash58)" },
  { value: "Video5G", label: "iPod Video 5G / 5.5G (No hash)" },
  { value: "Nano3G", label: "iPod Nano 3G (hash58)" },
  { value: "Nano4G", label: "iPod Nano 4G (hash58)" },
  { value: "Nano5G", label: "iPod Nano 5G (hash72)" },
];

export function MockDeviceModal({
  open,
  onClose,
  onCreateMock,
  onOpenExistingMock,
}: MockDeviceModalProps) {
  const [mode, setMode] = useState<"create" | "open">("create");
  const [folderPath, setFolderPath] = useState("");
  const [deviceName, setDeviceName] = useState("Mock Classic 6G");
  const [generation, setGeneration] = useState<IpodGeneration>("Classic6G");

  if (!open) return null;

  const browseFolder = async () => {
    const picked = await podbridgeApi.pickFolderDialog(
      mode === "create"
        ? "Choose Empty Directory for Mock iPod"
        : "Choose Existing Mock iPod Root",
    );
    if (picked) setFolderPath(picked);
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = folderPath.trim();
    if (!trimmed) return;
    if (mode === "create") {
      onCreateMock(trimmed, deviceName.trim() || "Mock iPod", generation);
    } else {
      onOpenExistingMock(trimmed);
    }
    onClose();
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <form
        className="modal-card"
        style={{ maxWidth: 460 }}
        onClick={(e) => e.stopPropagation()}
        onSubmit={handleSubmit}
      >
        <div className="modal-header">
          <span style={{ fontWeight: 700, fontSize: 15 }}>
            Mock iPod Folder Mode
          </span>
          <button type="button" className="btn-icon" onClick={onClose}>
            <X size={15} />
          </button>
        </div>

        <div style={{ display: "flex", gap: 6 }}>
          <button
            type="button"
            className={`btn ${mode === "create" ? "btn-primary" : ""}`}
            style={{ flex: 1 }}
            onClick={() => setMode("create")}
          >
            Initialize New Mock
          </button>
          <button
            type="button"
            className={`btn ${mode === "open" ? "btn-primary" : ""}`}
            style={{ flex: 1 }}
            onClick={() => setMode("open")}
          >
            Open Existing Folder
          </button>
        </div>

        <label className="field-label">
          <span>Target Directory Path</span>
          <div style={{ display: "flex", gap: 6 }}>
            <input
              type="text"
              className="search-input"
              style={{ paddingLeft: 10, flex: 1 }}
              placeholder="C:\Users\You\MockPod"
              value={folderPath}
              onChange={(e) => setFolderPath(e.target.value)}
            />
            <button type="button" className="btn" onClick={() => void browseFolder()}>
              <FolderOpen size={14} />
              <span>Browse</span>
            </button>
          </div>
        </label>

        {mode === "create" && (
          <>
            <label className="field-label">
              <span>Device Name</span>
              <input
                type="text"
                className="search-input"
                style={{ paddingLeft: 10 }}
                value={deviceName}
                onChange={(e) => setDeviceName(e.target.value)}
              />
            </label>

            <label className="field-label">
              <span>Hardware Profile</span>
              <select
                className="search-input"
                style={{ paddingLeft: 10 }}
                value={generation}
                onChange={(e) => setGeneration(e.target.value as IpodGeneration)}
              >
                {GENERATIONS.map((g) => (
                  <option key={g.value} value={g.value}>
                    {g.label}
                  </option>
                ))}
              </select>
            </label>
          </>
        )}

        <div className="modal-footer">
          <button type="button" className="btn" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            className="btn btn-primary"
            disabled={!folderPath.trim()}
          >
            {mode === "create" ? "Create & Open Mock" : "Open Mock iPod"}
          </button>
        </div>
      </form>
    </div>
  );
}
