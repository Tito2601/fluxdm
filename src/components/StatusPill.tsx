import {
  ArrowUpCircle,
  CheckCircle2,
  CircleSlash,
  Clock,
  DownloadCloud,
  Pause,
  XCircle,
} from "lucide-react";
import { Download, DownloadStatus } from "../types";

const STYLES: Record<DownloadStatus, { label: string; className: string; Icon: typeof Clock }> = {
  queued: { label: "Queued", className: "text-slate-400 bg-slate-400/10", Icon: Clock },
  downloading: { label: "Downloading", className: "text-accent bg-accent/10", Icon: DownloadCloud },
  paused: { label: "Paused", className: "text-slate-300 bg-slate-500/15", Icon: Pause },
  completed: { label: "Completed", className: "text-ok bg-ok/10", Icon: CheckCircle2 },
  failed: { label: "Failed", className: "text-danger bg-danger/10", Icon: XCircle },
  cancelled: { label: "Cancelled", className: "text-slate-500 bg-slate-600/15", Icon: CircleSlash },
};

export default function StatusPill({ download }: { download: Download }) {
  // A finished torrent is still doing work — it's uploading to the swarm. Saying
  // "Completed" would hide the fact that it's using bandwidth.
  const seeding = download.kind === "torrent" && download.status === "completed";

  const { label, className, Icon } = seeding
    ? { label: "Seeding", className: "text-up bg-up/10", Icon: ArrowUpCircle }
    : STYLES[download.status];

  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded px-2 py-0.5 text-[11px] font-medium ${className}`}
    >
      <Icon size={12} />
      {label}
    </span>
  );
}
