import { useState } from "react";
import {
  Video,
  Radio,
  CheckCircle2,
  Clock,
  Wifi,
  X,
} from "lucide-react";
import { StreamInfo, StreamQuality } from "../types";

interface StreamPickerProps {
  streamInfo: StreamInfo;
  filename:   string;
  savePath:   string;
  onConfirm:  (quality: StreamQuality, filename: string) => void;
  onCancel:   () => void;
}

const STREAM_TYPE_LABELS: Record<string, string> = {
  hls:  "HLS (HTTP Live Streaming)",
  dash: "MPEG-DASH",
};

function formatBandwidth(bps: number): string {
  if (bps <= 0) return "";
  if (bps >= 1_000_000) return `${(bps / 1_000_000).toFixed(1)} Mbps`;
  return `${(bps / 1000).toFixed(0)} Kbps`;
}

function formatDuration(secs: number): string {
  if (secs <= 0) return "";
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.round(secs % 60);
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

/**
 * Shown when the user adds a URL that resolves to an HLS or DASH stream.
 * Lets them pick a quality variant before the download begins.
 */
export default function StreamPicker({
  streamInfo,
  filename,
  savePath,
  onConfirm,
  onCancel,
}: StreamPickerProps) {
  // Default to the best quality (index 0 — sorted best-first by the backend)
  const [selected, setSelected] = useState<number>(0);
  const [editedFilename, setEditedFilename] = useState(filename);

  const sorted = [...streamInfo.qualities].sort((a, b) => b.bandwidth - a.bandwidth);
  const selectedQuality = sorted[selected] ?? sorted[0];

  const handleConfirm = () => {
    if (!selectedQuality) return;
    const ext = streamInfo.streamType === "dash" ? ".mp4" : ".ts";
    const finalName = editedFilename || `stream${ext}`;
    onConfirm(selectedQuality, finalName);
  };

  return (
    <div className="fixed inset-0 bg-black/70 flex items-center justify-center z-50">
      <div className="bg-[#1e293b] border border-slate-700 rounded-xl w-[480px] shadow-2xl overflow-hidden">

        {/* Header */}
        <div className="flex items-center justify-between px-5 py-4 bg-[#0c1524] border-b border-slate-800">
          <div className="flex items-center gap-2">
            <Radio size={16} className="text-blue-400" />
            <span className="text-sm font-semibold">Stream Detected</span>
          </div>
          <button
            onClick={onCancel}
            className="text-slate-500 hover:text-white transition-colors"
          >
            <X size={16} />
          </button>
        </div>

        {/* Stream meta */}
        <div className="px-5 py-3 flex items-center gap-4 text-xs text-slate-400 border-b border-slate-800 bg-[#0f172a]/50">
          <span className="flex items-center gap-1">
            <Video size={12} className="text-blue-400" />
            {STREAM_TYPE_LABELS[streamInfo.streamType] ?? streamInfo.streamType.toUpperCase()}
          </span>
          {streamInfo.durationSeconds && streamInfo.durationSeconds > 0 && (
            <span className="flex items-center gap-1">
              <Clock size={12} />
              {formatDuration(streamInfo.durationSeconds)}
            </span>
          )}
          <span className="flex items-center gap-1">
            <Wifi size={12} />
            {streamInfo.qualities.length} {streamInfo.qualities.length === 1 ? "quality" : "qualities"}
          </span>
        </div>

        {/* Quality selector */}
        <div className="px-5 py-4">
          <p className="text-xs text-slate-500 mb-3">
            Select quality — the first option is the highest available.
          </p>

          <div className="space-y-2 max-h-52 overflow-y-auto pr-1">
            {sorted.map((q, i) => {
              const isSelected = selected === i;
              return (
                <button
                  key={q.index}
                  onClick={() => setSelected(i)}
                  className={`w-full flex items-center justify-between px-3 py-2.5 rounded-lg border transition-all text-left ${
                    isSelected
                      ? "bg-blue-600/20 border-blue-500 text-white"
                      : "bg-slate-800/40 border-slate-700 text-slate-300 hover:border-slate-500 hover:bg-slate-800/70"
                  }`}
                >
                  <div className="flex items-center gap-2.5">
                    <div
                      className={`w-4 h-4 rounded-full border-2 flex items-center justify-center flex-shrink-0 transition-colors ${
                        isSelected ? "border-blue-400" : "border-slate-600"
                      }`}
                    >
                      {isSelected && (
                        <div className="w-2 h-2 rounded-full bg-blue-400" />
                      )}
                    </div>
                    <div>
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium">{q.label}</span>
                        {i === 0 && (
                          <span className="text-[10px] bg-blue-600/40 text-blue-300 px-1.5 py-0.5 rounded font-medium">
                            BEST
                          </span>
                        )}
                      </div>
                      {q.resolution && (
                        <span className="text-xs text-slate-500">{q.resolution}</span>
                      )}
                    </div>
                  </div>

                  <div className="text-right flex-shrink-0 ml-3">
                    {q.bandwidth > 0 && (
                      <span className="text-xs text-slate-400 font-mono">
                        {formatBandwidth(q.bandwidth)}
                      </span>
                    )}
                    {q.codecs && (
                      <div className="text-[10px] text-slate-600 truncate max-w-[100px]">
                        {q.codecs}
                      </div>
                    )}
                  </div>
                </button>
              );
            })}
          </div>
        </div>

        {/* Filename editor */}
        <div className="px-5 pb-3">
          <label className="block text-xs text-slate-400 mb-1">Output filename</label>
          <input
            type="text"
            value={editedFilename}
            onChange={(e) => setEditedFilename(e.target.value)}
            className="w-full bg-[#0f172a] border border-slate-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-blue-500"
          />
          <p className="text-[10px] text-slate-600 mt-1">
            Saved to: {savePath || "default save path"}
          </p>
        </div>

        {/* Actions */}
        <div className="flex items-center justify-between px-5 py-4 bg-[#0c1524] border-t border-slate-800">
          <button
            onClick={onCancel}
            className="px-4 py-2 text-sm text-slate-400 hover:text-white transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleConfirm}
            disabled={!selectedQuality}
            className="flex items-center gap-2 px-5 py-2 bg-blue-600 hover:bg-blue-500 disabled:opacity-50 disabled:cursor-not-allowed rounded-lg text-sm font-medium transition-colors"
          >
            <CheckCircle2 size={14} />
            Download {selectedQuality?.label ?? ""}
          </button>
        </div>
      </div>
    </div>
  );
}
