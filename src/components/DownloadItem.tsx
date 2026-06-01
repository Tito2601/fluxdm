import {
  Pause,
  Play,
  X,
  Folder,
  FileText,
  CheckCircle2,
  AlertCircle,
  Clock,
  Ban,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { Download, formatBytes, formatSpeed, formatEta, CATEGORY_ICONS } from "../types";
import { useDownloadStore } from "../store/downloadStore";
import ThreatBadge from "./ThreatBadge";

interface DownloadItemProps {
  download: Download;
}

const STATUS_CONFIG = {
  queued: { color: "text-yellow-400", bg: "bg-yellow-900/20", label: "Queued", icon: Clock },
  downloading: { color: "text-blue-400", bg: "bg-blue-900/20", label: "Downloading", icon: null },
  paused: { color: "text-slate-400", bg: "bg-slate-800/50", label: "Paused", icon: Pause },
  completed: { color: "text-green-400", bg: "bg-green-900/20", label: "Completed", icon: CheckCircle2 },
  failed: { color: "text-red-400", bg: "bg-red-900/20", label: "Failed", icon: AlertCircle },
  cancelled: { color: "text-slate-500", bg: "bg-slate-900/50", label: "Cancelled", icon: Ban },
} as const;

export default function DownloadItem({ download }: DownloadItemProps) {
  const { pauseDownload, resumeDownload, cancelDownload, deleteDownload } = useDownloadStore();

  const progress = download.totalBytes > 0
    ? Math.min(100, (download.downloaded / download.totalBytes) * 100)
    : 0;

  const statusCfg = STATUS_CONFIG[download.status] ?? STATUS_CONFIG.queued;
  const categoryIcon = CATEGORY_ICONS[download.category] ?? "📎";

  const handleOpenFile = async () => {
    try {
      await invoke("cmd_open_file", {
        path: `${download.savePath}/${download.filename}`,
      });
    } catch (err) {
      console.error("Failed to open file:", err);
    }
  };

  const handleOpenFolder = async () => {
    try {
      await invoke("cmd_open_folder", {
        path: `${download.savePath}/${download.filename}`,
      });
    } catch (err) {
      console.error("Failed to open folder:", err);
    }
  };

  return (
    <div className={`px-4 py-3 border-b border-slate-800/60 hover:bg-slate-800/20 transition-colors group ${statusCfg.bg}`}>
      <div className="flex items-center gap-3">
        {/* File Category Icon */}
        <div className="w-8 h-8 flex items-center justify-center text-lg flex-shrink-0">
          {categoryIcon}
        </div>

        {/* File Info + Progress */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <span
              className="text-sm font-medium text-slate-100 truncate max-w-xs"
              title={download.filename}
            >
              {download.filename}
            </span>
            <ThreatBadge download={download} />

            {/* Status Badge */}
            <span className={`text-xs font-medium ${statusCfg.color} ml-auto flex-shrink-0`}>
              {statusCfg.label}
            </span>
          </div>

          {/* Progress Bar */}
          <div className="relative h-1.5 bg-slate-700 rounded-full overflow-hidden">
            <div
              className={`absolute left-0 top-0 h-full rounded-full transition-all duration-300 ${
                download.status === "downloading"
                  ? "bg-blue-500"
                  : download.status === "completed"
                  ? "bg-green-500"
                  : download.status === "failed"
                  ? "bg-red-500"
                  : "bg-slate-500"
              }`}
              style={{ width: `${download.status === "completed" ? 100 : progress}%` }}
            />
          </div>

          {/* Stats Row */}
          <div className="flex items-center gap-3 mt-1 text-xs text-slate-500">
            <span>
              {formatBytes(download.downloaded)}
              {download.totalBytes > 0 ? ` / ${formatBytes(download.totalBytes)}` : ""}
            </span>

            {download.status === "downloading" && (
              <>
                <span className="text-blue-400 font-medium">
                  {formatSpeed(download.speedBps)}
                </span>
                {download.etaSeconds > 0 && (
                  <span className="text-slate-400">
                    {formatEta(download.etaSeconds)} remaining
                  </span>
                )}
              </>
            )}

            <span className="ml-auto capitalize text-slate-600">
              {download.category}
            </span>
          </div>
        </div>

        {/* Action Buttons */}
        <div className="flex items-center gap-1 flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
          {/* Pause / Resume */}
          {download.status === "downloading" && (
            <button
              onClick={() => pauseDownload(download.id)}
              className="p-1.5 text-slate-400 hover:text-yellow-400 hover:bg-slate-700 rounded transition-colors"
              title="Pause"
            >
              <Pause size={14} />
            </button>
          )}

          {(download.status === "paused" || download.status === "queued") && (
            <button
              onClick={() => resumeDownload(download.id)}
              className="p-1.5 text-slate-400 hover:text-green-400 hover:bg-slate-700 rounded transition-colors"
              title="Resume"
            >
              <Play size={14} />
            </button>
          )}

          {/* Open File (completed only) */}
          {download.status === "completed" && (
            <button
              onClick={handleOpenFile}
              className="p-1.5 text-slate-400 hover:text-blue-400 hover:bg-slate-700 rounded transition-colors"
              title="Open file"
            >
              <FileText size={14} />
            </button>
          )}

          {/* Open Folder */}
          <button
            onClick={handleOpenFolder}
            className="p-1.5 text-slate-400 hover:text-blue-400 hover:bg-slate-700 rounded transition-colors"
            title="Open folder"
          >
            <Folder size={14} />
          </button>

          {/* Cancel / Delete */}
          {(download.status === "downloading" || download.status === "queued" || download.status === "paused") && (
            <button
              onClick={() => cancelDownload(download.id)}
              className="p-1.5 text-slate-400 hover:text-red-400 hover:bg-slate-700 rounded transition-colors"
              title="Cancel"
            >
              <X size={14} />
            </button>
          )}
          {(download.status === "completed" || download.status === "failed" || download.status === "cancelled") && (
            <button
              onClick={() => deleteDownload(download.id, false)}
              className="p-1.5 text-slate-400 hover:text-red-400 hover:bg-slate-700 rounded transition-colors"
              title="Remove from list"
            >
              <X size={14} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
