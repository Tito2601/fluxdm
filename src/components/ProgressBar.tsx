import { Download, DownloadStatus, progressPercent } from "../types";

/** Bar fill colour per status. Seeding torrents are amber, matching upload stats. */
const FILL: Record<DownloadStatus, string> = {
  downloading: "bg-accent",
  queued: "bg-accent-dim",
  paused: "bg-slate-500",
  completed: "bg-ok",
  failed: "bg-danger",
  cancelled: "bg-slate-600",
};

interface Props {
  download: Download;
  /** Renders a slimmer bar for dense table rows. */
  compact?: boolean;
}

export default function ProgressBar({ download, compact = false }: Props) {
  const pct = progressPercent(download);
  const active = download.status === "downloading";

  // A completed torrent that is still seeding shows a full amber bar: the bar
  // means "this file's state", and seeding is the state that matters here.
  const seeding = download.kind === "torrent" && download.status === "completed";
  const fill = seeding ? "bg-up" : FILL[download.status];

  // Size is unknown until a magnet resolves metadata, or when a server sends no
  // Content-Length. A percentage would be a lie, so sweep instead.
  const indeterminate = active && download.totalBytes === 0;

  const height = compact ? "h-1.5" : "h-2.5";

  return (
    <div
      className={`relative w-full ${height} overflow-hidden rounded-full bg-surface-0 ring-1 ring-inset ring-line`}
      role="progressbar"
      aria-valuenow={indeterminate ? undefined : Math.round(pct)}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-label={`${download.filename} progress`}
    >
      {indeterminate ? (
        <div className="absolute inset-y-0 left-0 w-1/4 animate-indeterminate rounded-full bg-accent/70" />
      ) : (
        <div
          className={`h-full rounded-full transition-[width] duration-300 ease-out ${fill}`}
          style={{ width: `${pct}%` }}
        >
          {active && (
            <div className="progress-stripes h-full w-full animate-progress-stripes rounded-full" />
          )}
        </div>
      )}
    </div>
  );
}
