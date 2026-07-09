import {
  BarChart3,
  CheckCircle2,
  DownloadCloud,
  Inbox,
  Magnet,
  Pause,
  Settings as SettingsIcon,
  XCircle,
} from "lucide-react";
import { CATEGORY_ICONS, Download } from "../types";
import { Pane } from "./Toolbar";

/** A saved view over the download list. */
export type Filter =
  | { kind: "all" }
  | { kind: "status"; status: Download["status"] }
  | { kind: "torrents" }
  | { kind: "category"; category: string };

export function filterKey(f: Filter): string {
  switch (f.kind) {
    case "all": return "all";
    case "torrents": return "torrents";
    case "status": return `status:${f.status}`;
    case "category": return `category:${f.category}`;
  }
}

export function matchesFilter(d: Download, f: Filter): boolean {
  switch (f.kind) {
    case "all": return true;
    case "torrents": return d.kind === "torrent";
    case "status": return d.status === f.status;
    case "category": return d.category === f.category;
  }
}

interface Props {
  downloads: Download[];
  active: Filter;
  onSelect: (f: Filter) => void;
  pane: Pane;
  onPane: (p: Pane) => void;
}

/**
 * The app's only navigation. It stays mounted in every pane — leaving Analytics
 * or Settings is always one click away, and nothing the user was looking at
 * disappears when they get there.
 */
export default function Sidebar({ downloads, active, onSelect, pane, onPane }: Props) {
  const count = (f: Filter) => downloads.filter((d) => matchesFilter(d, f)).length;

  // Only show categories that actually have downloads — an empty tree of every
  // possible category is noise.
  const categories = Array.from(new Set(downloads.map((d) => d.category))).sort();

  const activeKey = filterKey(active);
  const onDownloads = pane === "downloads";

  return (
    <nav className="flex h-full w-52 flex-shrink-0 flex-col overflow-y-auto border-r border-line bg-surface-1 py-3">
      <Section title="Navigation">
        <Item
          label="Downloads"
          Icon={Inbox}
          selected={onDownloads}
          onClick={() => onPane("downloads")}
        />
        <Item
          label="Analytics"
          Icon={BarChart3}
          selected={pane === "analytics"}
          onClick={() => onPane("analytics")}
        />
        <Item
          label="Settings"
          Icon={SettingsIcon}
          selected={pane === "settings"}
          onClick={() => onPane("settings")}
        />
      </Section>

      {/* Filters narrow the download table, so they only mean something beside it. */}
      {onDownloads && (
        <>
          <Section title="Filters">
            <Item
              label="All"
              Icon={Inbox}
              count={count({ kind: "all" })}
              selected={activeKey === "all"}
              onClick={() => onSelect({ kind: "all" })}
            />
            <Item
              label="Downloading"
              Icon={DownloadCloud}
              count={count({ kind: "status", status: "downloading" })}
              selected={activeKey === "status:downloading"}
              onClick={() => onSelect({ kind: "status", status: "downloading" })}
            />
            <Item
              label="Paused"
              Icon={Pause}
              count={count({ kind: "status", status: "paused" })}
              selected={activeKey === "status:paused"}
              onClick={() => onSelect({ kind: "status", status: "paused" })}
            />
            <Item
              label="Completed"
              Icon={CheckCircle2}
              count={count({ kind: "status", status: "completed" })}
              selected={activeKey === "status:completed"}
              onClick={() => onSelect({ kind: "status", status: "completed" })}
            />
            <Item
              label="Failed"
              Icon={XCircle}
              count={count({ kind: "status", status: "failed" })}
              selected={activeKey === "status:failed"}
              onClick={() => onSelect({ kind: "status", status: "failed" })}
            />
          </Section>

          <Section title="Torrents">
            <Item
              label="All torrents"
              Icon={Magnet}
              count={count({ kind: "torrents" })}
              selected={activeKey === "torrents"}
              onClick={() => onSelect({ kind: "torrents" })}
            />
          </Section>

          {categories.length > 0 && (
            <Section title="Categories">
              {categories.map((c) => (
                <Item
                  key={c}
                  label={c[0].toUpperCase() + c.slice(1)}
                  emoji={CATEGORY_ICONS[c] ?? CATEGORY_ICONS.other}
                  count={count({ kind: "category", category: c })}
                  selected={activeKey === `category:${c}`}
                  onClick={() => onSelect({ kind: "category", category: c })}
                />
              ))}
            </Section>
          )}
        </>
      )}
    </nav>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="mb-4">
      <h2 className="px-4 pb-1.5 text-[10px] font-semibold uppercase tracking-wider text-slate-500">
        {title}
      </h2>
      <ul className="space-y-0.5 px-2">{children}</ul>
    </div>
  );
}

interface ItemProps {
  label: string;
  selected: boolean;
  onClick: () => void;
  /** Omitted by navigation entries, which count nothing. */
  count?: number;
  Icon?: typeof Inbox;
  emoji?: string;
}

function Item({ label, count, selected, onClick, Icon, emoji }: ItemProps) {
  return (
    <li>
      <button
        onClick={onClick}
        aria-current={selected ? "page" : undefined}
        className={`group flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-[13px] transition-colors ${
          selected
            ? "bg-accent/15 text-accent-soft"
            : "text-slate-400 hover:bg-surface-3 hover:text-slate-200"
        }`}
      >
        {Icon ? <Icon size={14} className="flex-shrink-0" /> : <span className="w-3.5 text-center text-xs">{emoji}</span>}
        <span className="flex-1 truncate">{label}</span>
        {count !== undefined && count > 0 && (
          <span
            className={`tnum rounded px-1.5 text-[10px] ${
              selected ? "bg-accent/20 text-accent-soft" : "bg-surface-3 text-slate-500 group-hover:text-slate-400"
            }`}
          >
            {count}
          </span>
        )}
      </button>
    </li>
  );
}
