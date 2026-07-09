import { useState } from "react";
import { ChevronDown, ChevronUp, Copy, Check } from "lucide-react";
import ProgressBar from "./ProgressBar";
import StatusPill from "./StatusPill";
import {
  Download,
  formatBytes,
  formatEta,
  formatSpeed,
  progressPercent,
  shareRatio,
} from "../types";

interface Props {
  download: Download | null;
  collapsed: boolean;
  onToggle: () => void;
}

export default function DetailPanel({ download, collapsed, onToggle }: Props) {
  return (
    <section className="flex-shrink-0 border-t border-line bg-surface-1">
      <button
        onClick={onToggle}
        className="flex w-full items-center justify-between px-3 py-1.5 text-[10px] font-semibold uppercase tracking-wider text-slate-500 hover:text-slate-300"
        aria-expanded={!collapsed}
      >
        <span>Details</span>
        {collapsed ? <ChevronUp size={13} /> : <ChevronDown size={13} />}
      </button>

      {!collapsed && (
        <div className="max-h-56 overflow-y-auto px-3 pb-3">
          {download ? <Body download={download} /> : <Empty />}
        </div>
      )}
    </section>
  );
}

function Empty() {
  return (
    <p className="py-6 text-center text-xs text-slate-600">
      Select a download to see its details.
    </p>
  );
}

function Body({ download: d }: { download: Download }) {
  const isTorrent = d.kind === "torrent";

  return (
    <div className="space-y-3">
      <div>
        <div className="mb-1.5 flex items-center gap-2">
          <h3 className="truncate text-sm font-medium text-slate-100" title={d.filename}>
            {d.filename}
          </h3>
          <StatusPill download={d} />
        </div>
        <ProgressBar download={d} />
        <p className="tnum mt-1.5 text-[11px] text-slate-500">
          {formatBytes(d.downloaded)}
          {d.totalBytes > 0 && ` of ${formatBytes(d.totalBytes)} (${progressPercent(d).toFixed(1)}%)`}
        </p>
      </div>

      <dl className="grid grid-cols-2 gap-x-6 gap-y-1.5 sm:grid-cols-3 lg:grid-cols-4">
        <Field
          label="Download speed"
          value={
            d.status === "downloading"
              ? `${formatBytes(d.speedBps)}/s`
              : d.speedBps > 0
                ? formatSpeed(d.speedBps)
                : "—"
          }
        />
        <Field
          label="Time remaining"
          value={d.status === "downloading" ? formatEta(d.etaSeconds) : "—"}
        />
        <Field label="Category" value={d.category} />
        <Field label="Source" value={d.kind} />

        {isTorrent && (
          <>
            <Field label="Upload speed" value={d.uploadSpeedBps > 0 ? formatSpeed(d.uploadSpeedBps) : "—"} accent="up" />
            <Field label="Uploaded" value={formatBytes(d.uploadedBytes)} accent="up" />
            <Field label="Share ratio" value={shareRatio(d).toFixed(2)} accent="up" />
            <Field
              label="Peers"
              value={`${d.peersConnected} connected · ${d.peersTotal} discovered`}
            />
          </>
        )}

        {d.threatScore > 0 && <Field label="Threat score" value={`${d.threatScore} / 100`} />}
        {d.mimeType && <Field label="Type" value={d.mimeType} />}
      </dl>

      <div className="space-y-1.5 border-t border-line pt-2.5">
        <CopyRow label="Save path" value={`${d.savePath}`} />
        <CopyRow label={isTorrent ? "Magnet / source" : "URL"} value={d.url} />
        {d.infoHash && <CopyRow label="Info hash" value={d.infoHash} mono />}
        {d.checksum && <CopyRow label="SHA-256" value={d.checksum} mono />}
      </div>
    </div>
  );
}

function Field({
  label,
  value,
  accent,
}: {
  label: string;
  value: string;
  accent?: "up";
}) {
  return (
    <div>
      <dt className="text-[10px] uppercase tracking-wide text-slate-600">{label}</dt>
      <dd className={`tnum truncate text-xs ${accent === "up" ? "text-up" : "text-slate-300"}`} title={value}>
        {value}
      </dd>
    </div>
  );
}

function CopyRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  const [copied, setCopied] = useState(false);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      // Clipboard access can be denied; the value is still selectable by hand.
    }
  };

  return (
    <div className="flex items-center gap-2">
      <span className="w-24 flex-shrink-0 text-[10px] uppercase tracking-wide text-slate-600">
        {label}
      </span>
      <span
        className={`min-w-0 flex-1 truncate text-[11px] text-slate-400 ${mono ? "font-mono" : ""}`}
        title={value}
      >
        {value}
      </span>
      <button
        onClick={copy}
        title={`Copy ${label}`}
        className="flex-shrink-0 rounded p-1 text-slate-600 transition-colors hover:bg-surface-3 hover:text-slate-300"
      >
        {copied ? <Check size={11} className="text-ok" /> : <Copy size={11} />}
      </button>
    </div>
  );
}
