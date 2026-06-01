import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import {
  Download,
  DownloadRequest,
  ProgressEvent,
  Settings,
  StreamInfo,
  ThreatAnalysis,
  normalizeDownload,
} from "../types";

interface DownloadState {
  downloads: Download[];
  settings: Settings;
  isLoading: boolean;
  error: string | null;
  /** Set when the browser extension requests a download; cleared once the dialog opens. */
  pendingDownload: DownloadRequest | null;

  // Actions
  loadDownloads: () => Promise<void>;
  addDownload: (
    url: string,
    filename: string,
    savePath: string,
    headers?: Record<string, string>,
    cookies?: string
  ) => Promise<string>;
  pauseDownload: (id: string) => Promise<void>;
  resumeDownload: (id: string) => Promise<void>;
  cancelDownload: (id: string) => Promise<void>;
  deleteDownload: (id: string, deleteFile: boolean) => Promise<void>;
  updateProgress: (event: ProgressEvent) => void;
  addDownloadFromEvent: (raw: Record<string, unknown>) => void;
  markCompleted: (id: string) => void;
  markFailed: (id: string, error: string) => void;
  loadSettings: () => Promise<void>;
  updateSetting: (key: string, value: string) => Promise<void>;
  clearHistory: () => Promise<void>;
  getThreatDetails: (download: Download) => Promise<ThreatAnalysis>;
  setPendingDownload: (req: DownloadRequest | null) => void;
  testLlm: (endpoint: string, model: string) => Promise<string>;
  suggestFilename: (url: string, filename: string, mime: string | null) => Promise<string>;
  probeStream: (url: string) => Promise<StreamInfo>;
  addStreamDownload: (
    manifestUrl: string,
    reprId: string | null,
    streamType: string,
    filename: string,
    savePath: string
  ) => Promise<string>;
  clearError: () => void;
}

const DEFAULT_SETTINGS: Settings = {
  maxParallelDownloads: 3,
  maxSegmentsPerDownload: 8,
  defaultSavePath: "~/Downloads",
  speedLimitKbps: 0,
  enableScheduler: false,
  schedulerStart: "02:00",
  schedulerStop: "07:00",
  zeroLogMode: false,
  theme: "system",
  llmEnabled: false,
  llmEndpoint: "http://localhost:11434/api/generate",
  llmModel: "llama3.2:1b",
};

function parseSettings(raw: Record<string, string>): Settings {
  return {
    maxParallelDownloads: parseInt(raw["max_parallel_downloads"] ?? "3"),
    maxSegmentsPerDownload: parseInt(raw["max_segments_per_download"] ?? "8"),
    defaultSavePath: raw["default_save_path"] ?? "~/Downloads",
    speedLimitKbps: parseInt(raw["speed_limit_kbps"] ?? "0"),
    enableScheduler: raw["enable_scheduler"] === "true",
    schedulerStart: raw["scheduler_start"] ?? "02:00",
    schedulerStop: raw["scheduler_stop"] ?? "07:00",
    zeroLogMode: raw["zero_log_mode"] === "true",
    theme: (raw["theme"] as Settings["theme"]) ?? "system",
    llmEnabled: raw["llm_enabled"] === "true",
    llmEndpoint: raw["llm_endpoint"] ?? "http://localhost:11434/api/generate",
    llmModel: raw["llm_model"] ?? "llama3.2:1b",
  };
}

export const useDownloadStore = create<DownloadState>((set, get) => ({
  downloads: [],
  settings: DEFAULT_SETTINGS,
  isLoading: false,
  error: null,
  pendingDownload: null,

  loadDownloads: async () => {
    set({ isLoading: true, error: null });
    try {
      const raw = await invoke<Record<string, unknown>[]>("cmd_get_downloads");
      const downloads = raw.map(normalizeDownload);
      set({ downloads, isLoading: false });
    } catch (err) {
      set({ error: String(err), isLoading: false });
    }
  },

  addDownload: async (url, filename, savePath, headers, cookies) => {
    try {
      const id = await invoke<string>("cmd_add_download", {
        url,
        filename,
        savePath: savePath || get().settings.defaultSavePath,
        headers: headers ?? null,
        cookies: cookies ?? null,
      });

      // Optimistically add to list
      const newDownload: Download = {
        id,
        url,
        filename,
        savePath,
        totalBytes: 0,
        downloaded: 0,
        status: "queued",
        speedBps: 0,
        etaSeconds: 0,
        category: "other",
        threatScore: 0,
        numSegments: 8,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      };

      set((state) => ({ downloads: [newDownload, ...state.downloads] }));
      return id;
    } catch (err) {
      set({ error: String(err) });
      throw err;
    }
  },

  pauseDownload: async (id) => {
    try {
      await invoke("cmd_pause_download", { id });
      set((state) => ({
        downloads: state.downloads.map((d) =>
          d.id === id ? { ...d, status: "paused" as const } : d
        ),
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  resumeDownload: async (id) => {
    try {
      await invoke("cmd_resume_download", { id });
      set((state) => ({
        downloads: state.downloads.map((d) =>
          d.id === id ? { ...d, status: "queued" as const } : d
        ),
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  cancelDownload: async (id) => {
    try {
      await invoke("cmd_cancel_download", { id });
      set((state) => ({
        downloads: state.downloads.map((d) =>
          d.id === id ? { ...d, status: "cancelled" as const } : d
        ),
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  deleteDownload: async (id, deleteFile) => {
    try {
      await invoke("cmd_delete_download", { id, deleteFile });
      set((state) => ({
        downloads: state.downloads.filter((d) => d.id !== id),
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  addDownloadFromEvent: (raw: Record<string, unknown>) => {
    const download = normalizeDownload(raw);
    set((state) => {
      // Ignore if already in the list (e.g. added optimistically via cmd_add_download)
      if (state.downloads.some((d) => d.id === download.id)) return state;
      return { downloads: [download, ...state.downloads] };
    });
  },

  updateProgress: (event: ProgressEvent) => {
    set((state) => ({
      downloads: state.downloads.map((d) =>
        d.id === event.id
          ? {
              ...d,
              downloaded: event.downloadedBytes,
              totalBytes: event.totalBytes,
              speedBps: event.speedBps,
              etaSeconds: event.etaSeconds,
              status: "downloading" as const,
            }
          : d
      ),
    }));
  },

  markCompleted: (id: string) => {
    set((state) => ({
      downloads: state.downloads.map((d) =>
        d.id === id
          ? {
              ...d,
              status: "completed" as const,
              speedBps: 0,
              etaSeconds: 0,
              completedAt: new Date().toISOString(),
            }
          : d
      ),
    }));
  },

  markFailed: (id: string, error: string) => {
    console.error(`Download ${id} failed:`, error);
    set((state) => ({
      downloads: state.downloads.map((d) =>
        d.id === id
          ? { ...d, status: "failed" as const, speedBps: 0, etaSeconds: 0 }
          : d
      ),
    }));
  },

  loadSettings: async () => {
    try {
      const raw = await invoke<Record<string, string>>("cmd_get_settings");
      set({ settings: parseSettings(raw) });
    } catch (err) {
      console.error("Failed to load settings:", err);
    }
  },

  updateSetting: async (key, value) => {
    try {
      await invoke("cmd_update_setting", { key, value });
      await get().loadSettings();
    } catch (err) {
      set({ error: String(err) });
    }
  },

  clearHistory: async () => {
    try {
      await invoke("cmd_clear_history");
      // Reload so completed/cancelled/failed items disappear from the list
      await get().loadDownloads();
    } catch (err) {
      set({ error: String(err) });
    }
  },

  getThreatDetails: async (download: Download): Promise<ThreatAnalysis> => {
    return invoke<ThreatAnalysis>("cmd_get_threat_details", {
      url:      download.url,
      filename: download.filename,
      mime:     download.mimeType ?? null,
      referrer: download.referrer ?? null,
      fileSize: download.totalBytes > 0 ? download.totalBytes : null,
    });
  },

  testLlm: async (endpoint: string, model: string): Promise<string> => {
    return invoke<string>("cmd_test_llm", { endpoint, model });
  },

  suggestFilename: async (url: string, filename: string, mime: string | null): Promise<string> => {
    return invoke<string>("cmd_llm_suggest_name", { url, filename, mime });
  },

  probeStream: async (url: string): Promise<StreamInfo> => {
    return invoke<StreamInfo>("cmd_probe_stream", { url });
  },

  addStreamDownload: async (
    manifestUrl: string,
    reprId: string | null,
    streamType: string,
    filename: string,
    savePath: string
  ): Promise<string> => {
    try {
      return await invoke<string>("cmd_add_stream_download", {
        manifestUrl,
        reprId,
        streamType,
        filename,
        savePath: savePath || get().settings.defaultSavePath,
      });
    } catch (err) {
      set({ error: String(err) });
      throw err;
    }
  },

  setPendingDownload: (req) => set({ pendingDownload: req }),

  clearError: () => set({ error: null }),
}));
