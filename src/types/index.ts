// ============================================================
// FluxDM Shared TypeScript Types
// ============================================================

export type DownloadStatus =
  | "queued"
  | "downloading"
  | "paused"
  | "completed"
  | "failed"
  | "cancelled";

/** Which engine owns a download. */
export type DownloadKind = "http" | "stream" | "torrent";

export interface Download {
  id: string;
  url: string;
  filename: string;
  savePath: string;
  totalBytes: number;
  downloaded: number;
  status: DownloadStatus;
  speedBps: number;
  etaSeconds: number;
  category: string;
  threatScore: number; // 0-100; show warning badge if > 60
  mimeType?: string;
  checksum?: string;
  sourceUrl?: string;
  referrer?: string;
  numSegments: number;
  createdAt: string;
  updatedAt: string;
  completedAt?: string;

  // Torrent-only; zero / undefined for other kinds.
  kind: DownloadKind;
  infoHash?: string;
  uploadedBytes: number;
  uploadSpeedBps: number;
  /** Peers currently connected. */
  peersConnected: number;
  /** Distinct peers discovered in the swarm. */
  peersTotal: number;
}

export interface ProgressEvent {
  id: string;
  downloadedBytes: number;
  totalBytes: number;
  speedBps: number;
  etaSeconds: number;

  // Present only for torrents.
  uploadedBytes?: number;
  uploadSpeedBps?: number;
  peersConnected?: number;
  peersTotal?: number;
}

/** Emitted when the scheduler opens or closes the download gate. */
export interface SchedulerState {
  open: boolean;
  /** Why downloading is on hold. Null when the gate is open. */
  reason: string | null;
}

export interface TorrentFile {
  name: string;
  length: number;
}

export interface TorrentAdded {
  name: string;
  totalBytes: number;
  infoHash: string;
  files: TorrentFile[];
}

export interface CompleteEvent {
  id: string;
  savePath: string;
  checksum: string;
}

export interface ErrorEvent {
  id: string;
  error: string;
}

export interface AnalyticsData {
  totalDownloadedBytes: number;
  downloadsToday: number;
  avgSpeedBps: number;
  downloadsByCategory: Record<string, number>;
  speedHistory: SpeedDataPoint[];
}

export interface SpeedDataPoint {
  timestamp: number;
  speedBps: number;
}

export interface Settings {
  maxParallelDownloads: number;
  maxSegmentsPerDownload: number;
  defaultSavePath: string;
  speedLimitKbps: number;
  zeroLogMode: boolean;
  theme: "light" | "dark" | "system";

  // Scheduler — each guard is independent of the others.
  enableScheduler: boolean;
  schedulerStart: string;
  schedulerStop: string;
  schedulerPauseOnHighCpu: boolean;
  schedulerCpuThreshold: number;
  schedulerPauseOnLowBattery: boolean;
  schedulerBatteryThreshold: number;

  /** Power off the machine once the queue drains. Off by default. */
  autoShutdown: boolean;

  // Torrent
  torrentSavePath: string;

  // LLM settings
  llmEnabled: boolean;
  llmEndpoint: string;
  llmModel: string;
}

// ── AI types ──────────────────────────────────────────────────────────────────

export interface ThreatFactor {
  name:   string;
  delta:  number;  // positive = more risky, negative = safer
  reason: string;
}

export interface ThreatAnalysis {
  score:   number;
  factors: ThreatFactor[];
}

// ── Browser extension download request ────────────────────────────────────────

/** Emitted by the HTTP server when the browser extension sends a download URL.
 *  The UI opens the Add Download dialog pre-filled with these values so the
 *  user can confirm / change the filename and save path before starting. */
export interface DownloadRequest {
  url:          string;
  filename:     string;
  savePath:     string;
  mimeType?:    string;
  referrer?:    string;
  pageUrl?:     string;
  threatScore?: number;
  category?:    string;
  fileSize?:    number;
  /** Request headers the extension captured for this URL. */
  headers?:     Record<string, string>;
  /** Cookie header for this URL's origin. */
  cookies?:     string;
}

// ── Site grabber ───────────────────────────────────────────────────────────────

export interface CrawlOptions {
  url: string;
  /** Link levels to follow. 0 = the starting page only. */
  depth: number;
  sameHostOnly: boolean;
  /** Extensions without dots. Empty = every known downloadable type. */
  extensions: string[];
}

export interface DiscoveredFile {
  url:       string;
  filename:  string;
  extension: string;
  label:     string | null;
  source:    string;
}

export interface CrawlResult {
  files:        DiscoveredFile[];
  pagesVisited: number;
  /** True when a cap cut the crawl short, so results are not exhaustive. */
  truncated:    boolean;
}

// ── Stream types ───────────────────────────────────────────────────────────────

export interface StreamQuality {
  index: number;
  label: string;
  bandwidth: number;
  resolution?: string;
  codecs?: string;
  /** HLS: media playlist URL. DASH: MPD URL (same for all qualities). */
  url: string;
  /** DASH-only: representation ID. */
  reprId?: string;
}

export interface StreamInfo {
  /** `"hls"` | `"dash"` | `"direct"` (plain file, not a stream). */
  streamType: string;
  qualities: StreamQuality[];
  durationSeconds?: number;
  title?: string;
}

export interface DuplicateCheck {
  isUrlDuplicate:       boolean;
  previousFilename?:    string;
  previousSavePath?:    string;
  previousCompletedAt?: string;
  fileExists:           boolean;
  outputPath:           string;
}

export type FileCategory =
  | "videos"
  | "music"
  | "documents"
  | "software"
  | "images"
  | "archives"
  | "other";

export const CATEGORY_ICONS: Record<string, string> = {
  videos: "🎬",
  music: "🎵",
  documents: "📄",
  software: "💿",
  images: "🖼️",
  archives: "📦",
  other: "📎",
};

export const CATEGORY_COLORS: Record<string, string> = {
  videos: "#ef4444",
  music: "#a855f7",
  documents: "#3b82f6",
  software: "#22c55e",
  images: "#f97316",
  archives: "#eab308",
  other: "#6b7280",
};

// Utility: format bytes to human-readable
export function formatBytes(bytes: number): string {
  if (!bytes || !isFinite(bytes) || bytes <= 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1);
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

// Utility: format speed
export function formatSpeed(bps: number): string {
  if (!bps || !isFinite(bps) || bps <= 0) return "—";
  return `${formatBytes(bps)}/s`;
}

// Utility: format ETA
export function formatEta(seconds: number): string {
  if (seconds <= 0 || !isFinite(seconds)) return "—";
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) {
    const m = Math.floor(seconds / 60);
    const s = Math.round(seconds % 60);
    return `${m}m ${s}s`;
  }
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return `${h}h ${m}m`;
}

/** Percentage complete, 0-100. Zero-size downloads read as 0 rather than NaN. */
export function progressPercent(d: Download): number {
  if (!d.totalBytes) return d.status === "completed" ? 100 : 0;
  return Math.min(100, (d.downloaded / d.totalBytes) * 100);
}

/** Share ratio for a torrent — uploaded over downloaded. */
export function shareRatio(d: Download): number {
  if (!d.downloaded) return 0;
  return d.uploadedBytes / d.downloaded;
}

/** A download is doing work right now (and so should animate). */
export function isActive(d: Download): boolean {
  return d.status === "downloading" || d.status === "queued";
}

// Utility: convert raw Rust DownloadJob to Download type
export function normalizeDownload(raw: Record<string, unknown>): Download {
  return {
    id: raw.id as string,
    url: raw.url as string,
    filename: raw.filename as string,
    savePath: raw.save_path as string,
    totalBytes: (raw.total_bytes as number) ?? 0,
    downloaded: (raw.downloaded as number) ?? 0,
    status: (raw.status as DownloadStatus) ?? "queued",
    speedBps: (raw.speed_bps as number) ?? 0,
    etaSeconds: 0,
    category: (raw.category as string) ?? "other",
    threatScore: (raw.threat_score as number) ?? 0,
    mimeType: raw.mime_type as string | undefined,
    checksum: raw.checksum as string | undefined,
    sourceUrl: raw.source_url as string | undefined,
    referrer: raw.referrer as string | undefined,
    numSegments: (raw.num_segments as number) ?? 8,
    createdAt: raw.created_at as string,
    updatedAt: raw.updated_at as string,
    completedAt: raw.completed_at as string | undefined,

    kind: (raw.kind as DownloadKind) ?? "http",
    infoHash: raw.info_hash as string | undefined,
    uploadedBytes: (raw.uploaded_bytes as number) ?? 0,
    uploadSpeedBps: (raw.upload_speed_bps as number) ?? 0,
    peersConnected: (raw.peers_connected as number) ?? 0,
    peersTotal: (raw.peers_total as number) ?? 0,
  };
}
