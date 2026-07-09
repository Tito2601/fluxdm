import { ArrowUp, Magnet, Radio, Users } from "lucide-react";
import ProgressBar from "./ProgressBar";
import StatusPill from "./StatusPill";
import ThreatBadge from "./ThreatBadge";
import {
  CATEGORY_ICONS,
  Download,
  formatBytes,
  formatEta,
  formatSpeed,
  progressPercent,
} from "../types";

interface Props {
  downloads: Download[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onActivate: (d: Download) => void;
}

export default function DownloadTable({ downloads, selectedId, onSelect, onActivate }: Props) {
  if (downloads.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-slate-600">
        <Magnet size={28} className="opacity-40" />
        <p className="text-sm">Nothing here yet</p>
        <p className="text-xs text-slate-700">Add a URL, a magnet link, or a .torrent file to begin.</p>
      </div>
    );
  }

  return (
    <div className="h-full overflow-auto">
      <table className="w-full border-collapse text-left text-xs">
        <thead className="sticky top-0 z-10 bg-surface-2 text-[10px] uppercase tracking-wider text-slate-500">
          <tr className="border-b border-line">
            <Th className="w-[34%] pl-3">Name</Th>
            <Th className="w-[9%]">Size</Th>
            <Th className="w-[16%]">Progress</Th>
            <Th className="w-[11%]">Status</Th>
            <Th className="w-[9%] text-right">Speed</Th>
            <Th className="w-[8%] text-right">Upload</Th>
            <Th className="w-[6%] text-right">Peers</Th>
            <Th className="w-[7%] pr-3 text-right">ETA</Th>
          </tr>
        </thead>
        <tbody>
          {downloads.map((d) => (
            <Row
              key={d.id}
              download={d}
              selected={d.id === selectedId}
              onSelect={() => onSelect(d.id)}
              onActivate={() => onActivate(d)}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Th({ className = "", children }: { className?: string; children: React.ReactNode }) {
  return <th scope="col" className={`px-2 py-2 font-semibold ${className}`}>{children}</th>;
}

interface RowProps {
  download: Download;
  selected: boolean;
  onSelect: () => void;
  onActivate: () => void;
}

function Row({ download: d, selected, onSelect, onActivate }: RowProps) {
  const isTorrent = d.kind === "torrent";
  const seeding = isTorrent && d.status === "completed";
  const active = d.status === "downloading";

  return (
    <tr
      onClick={onSelect}
      onDoubleClick={onActivate}
      aria-selected={selected}
      className={`cursor-default border-b border-line/60 transition-colors ${
        selected ? "bg-accent/10" : "hover:bg-surface-2"
      }`}
    >
      {/* Name */}
      <td className="max-w-0 px-2 py-2 pl-3">
        <div className="flex items-center gap-2">
          <span className="flex-shrink-0 text-sm" aria-hidden>
            {CATEGORY_ICONS[d.category] ?? CATEGORY_ICONS.other}
          </span>
          <span className="truncate text-slate-200" title={d.filename}>
            {d.filename}
          </span>
          {isTorrent && <Magnet size={11} className="flex-shrink-0 text-up" aria-label="Torrent" />}
          {d.kind === "stream" && <Radio size={11} className="flex-shrink-0 text-accent-soft" aria-label="Stream" />}
          {d.threatScore > 60 && <ThreatBadge download={d} />}
        </div>
      </td>

      {/* Size */}
      <td className="tnum px-2 py-2 text-slate-400">
        {d.totalBytes > 0 ? formatBytes(d.totalBytes) : "—"}
      </td>

      {/* Progress */}
      <td className="px-2 py-2">
        <div className="flex items-center gap-2">
          <ProgressBar download={d} compact />
          <span className="tnum w-9 flex-shrink-0 text-right text-[11px] text-slate-500">
            {d.totalBytes > 0 ? `${progressPercent(d).toFixed(0)}%` : "—"}
          </span>
        </div>
      </td>

      {/* Status */}
      <td className="px-2 py-2"><StatusPill download={d} /></td>

      {/* Download speed. A running transfer always shows a number — briefly reading
          "0 B/s" is honest, whereas blanking to "—" every idle tick reads as flicker. */}
      <td className="tnum px-2 py-2 text-right text-slate-300">
        {active ? `${formatBytes(d.speedBps)}/s` : d.speedBps > 0 ? formatSpeed(d.speedBps) : "—"}
      </td>

      {/* Upload speed — torrents only */}
      <td className="tnum px-2 py-2 text-right">
        {isTorrent && d.uploadSpeedBps > 0 ? (
          <span className="inline-flex items-center gap-1 text-up">
            <ArrowUp size={10} />
            {formatSpeed(d.uploadSpeedBps)}
          </span>
        ) : (
          <span className="text-slate-600">—</span>
        )}
      </td>

      {/* Peers — torrents only */}
      <td className="tnum px-2 py-2 text-right">
        {isTorrent ? (
          <span
            className="inline-flex items-center gap-1 text-slate-400"
            title={`${d.peersConnected} connected of ${d.peersTotal} discovered`}
          >
            <Users size={10} />
            {d.peersConnected}
          </span>
        ) : (
          <span className="text-slate-600">—</span>
        )}
      </td>

      {/* ETA — a seeding torrent has no finish line */}
      <td className="tnum px-2 py-2 pr-3 text-right text-slate-400">
        {seeding ? "∞" : active ? formatEta(d.etaSeconds) : "—"}
      </td>
    </tr>
  );
}
