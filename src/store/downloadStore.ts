import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import {
  CrawlOptions,
  CrawlResult,
  Download,
  DownloadRequest,
  ProgressEvent,
  SchedulerState,
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
  /** Null until the scheduler first reports in. */
  scheduler: SchedulerState | null;
  /** Seconds left before auto-shutdown; null when no countdown is running. */
  shutdownCountdown: number | null;

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
  markPaused: (id: string) => void;
  markCancelled: (id: string) => void;
  setScheduler: (state: SchedulerState) => void;
  setShutdownCountdown: (seconds: number | null) => void;
  cancelShutdown: () => Promise<void>;
  crawlSite: (options: CrawlOptions) => Promise<CrawlResult>;
  addDownloads: (urls: string[], savePath: string) => Promise<string[]>;
  isTorrentSource: (source: string) => Promise<boolean>;
  addTorrent: (source: string, savePath: string) => Promise<string>;
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
  zeroLogMode: false,
  theme: "system",
  enableScheduler: false,
  schedulerStart: "02:00",
  schedulerStop: "07:00",
  schedulerPauseOnHighCpu: false,
  schedulerCpuThreshold: 80,
  schedulerPauseOnLowBattery: false,
  schedulerBatteryThreshold: 20,
  autoShutdown: false,
  torrentSavePath: "~/Downloads",
  llmEnabled: false,
  llmEndpoint: "http://localhost:11434/api/generate",
  llmModel: "llama3.2:1b",
};

/** Parse an integer setting, falling back when the stored value is unusable. */
function num(raw: Record<string, string>, key: string, fallback: number): number {
  const parsed = parseInt(raw[key] ?? "", 10);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function parseSettings(raw: Record<string, string>): Settings {
  const d = DEFAULT_SETTINGS;
  return {
    maxParallelDownloads: num(raw, "max_parallel_downloads", d.maxParallelDownloads),
    maxSegmentsPerDownload: num(raw, "max_segments_per_download", d.maxSegmentsPerDownload),
    defaultSavePath: raw["default_save_path"] ?? d.defaultSavePath,
    speedLimitKbps: num(raw, "speed_limit_kbps", d.speedLimitKbps),
    zeroLogMode: raw["zero_log_mode"] === "true",
    theme: (raw["theme"] as Settings["theme"]) ?? d.theme,

    enableScheduler: raw["enable_scheduler"] === "true",
    schedulerStart: raw["scheduler_start"] ?? d.schedulerStart,
    schedulerStop: raw["scheduler_stop"] ?? d.schedulerStop,
    schedulerPauseOnHighCpu: raw["scheduler_pause_on_high_cpu"] === "true",
    schedulerCpuThreshold: num(raw, "scheduler_cpu_threshold", d.schedulerCpuThreshold),
    schedulerPauseOnLowBattery: raw["scheduler_pause_on_low_battery"] === "true",
    schedulerBatteryThreshold: num(raw, "scheduler_battery_threshold", d.schedulerBatteryThreshold),

    // Absent must read as false: a missing key can never mean "power off".
    autoShutdown: raw["auto_shutdown"] === "true",

    torrentSavePath: raw["torrent_save_path"] ?? d.torrentSavePath,

    llmEnabled: raw["llm_enabled"] === "true",
    llmEndpoint: raw["llm_endpoint"] ?? d.llmEndpoint,
    llmModel: raw["llm_model"] ?? d.llmModel,
  };
}

export const useDownloadStore = create<DownloadState>((set, get) => ({
  downloads: [],
  settings: DEFAULT_SETTINGS,
  isLoading: false,
  error: null,
  pendingDownload: null,
  scheduler: null,
  shutdownCountdown: null,

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
        kind: "http",
        uploadedBytes: 0,
        uploadSpeedBps: 0,
        peersConnected: 0,
        peersTotal: 0,
      };

      set((state) => ({ downloads: [newDownload, ...state.downloads] }));
      return id;
    } catch (err) {
      set({ error: String(err) });
      throw err;
    }
  },

  isTorrentSource: async (source: string): Promise<boolean> => {
    return invoke<boolean>("cmd_is_torrent_source", { source });
  },

  // The row is added by the `download_added` event the backend emits once swarm
  // metadata arrives, so there is nothing to insert optimistically here — until
  // then we don't know the torrent's name or size.
  addTorrent: async (source: string, savePath: string): Promise<string> => {
    try {
      return await invoke<string>("cmd_add_torrent", { source, savePath });
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
      downloads: state.downloads.map((d) => {
        if (d.id !== event.id) return d;

        // Progress ticks report bytes, not intent. A seeding torrent and a paused
        // one both keep emitting, so only a download that was already running (or
        // waiting to run) may be moved into "downloading" — anything the user or
        // the scheduler stopped keeps the status it was given.
        const running = d.status === "downloading" || d.status === "queued";
        const status = running ? ("downloading" as const) : d.status;

        return {
          ...d,
          downloaded: event.downloadedBytes,
          totalBytes: event.totalBytes,
          speedBps: event.speedBps,
          etaSeconds: event.etaSeconds,
          status,
          uploadedBytes: event.uploadedBytes ?? d.uploadedBytes,
          uploadSpeedBps: event.uploadSpeedBps ?? d.uploadSpeedBps,
          peersConnected: event.peersConnected ?? d.peersConnected,
          peersTotal: event.peersTotal ?? d.peersTotal,
        };
      }),
    }));
  },

  markPaused: (id: string) => {
    set((state) => ({
      downloads: state.downloads.map((d) =>
        d.id === id ? { ...d, status: "paused" as const, speedBps: 0, etaSeconds: 0 } : d
      ),
    }));
  },

  markCancelled: (id: string) => {
    set((state) => ({
      downloads: state.downloads.map((d) =>
        d.id === id ? { ...d, status: "cancelled" as const, speedBps: 0, etaSeconds: 0 } : d
      ),
    }));
  },

  setScheduler: (scheduler: SchedulerState) => set({ scheduler }),

  setShutdownCountdown: (shutdownCountdown: number | null) => set({ shutdownCountdown }),

  cancelShutdown: async () => {
    // Clear locally first so the banner disappears on click rather than on the
    // next backend tick.
    set({ shutdownCountdown: null });
    await invoke("cmd_cancel_shutdown");
  },

  crawlSite: async (options: CrawlOptions): Promise<CrawlResult> => {
    try {
      return await invoke<CrawlResult>("cmd_crawl_site", { options });
    } catch (err) {
      set({ error: String(err) });
      throw err;
    }
  },

  addDownloads: async (urls: string[], savePath: string): Promise<string[]> => {
    try {
      const ids = await invoke<string[]>("cmd_add_downloads", {
        urls,
        savePath: savePath || get().settings.defaultSavePath,
      });
      await get().loadDownloads();
      return ids;
    } catch (err) {
      set({ error: String(err) });
      throw err;
    }
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
