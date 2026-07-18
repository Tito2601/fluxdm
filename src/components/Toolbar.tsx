import {
  FolderOpen,
  Globe,
  Magnet,
  Pause,
  Play,
  Plus,
  Search,
  Square,
  Trash2,
} from "lucide-react";
import { Download } from "../types";

export type Pane = "downloads" | "analytics" | "settings";

const PANE_TITLES: Record<Pane, string> = {
  downloads: "Downloads",
  analytics: "Analytics",
  settings: "Settings",
};

interface Props {
  selected: Download | null;
  pane: Pane;
  query: string;
  onQueryChange: (q: string) => void;
  onAddUrl: () => void;
  onAddTorrent: () => void;
  onGrabSite: () => void;
  onResume: () => void;
  onPause: () => void;
  onStop: () => void;
  onDelete: () => void;
  onOpenFolder: () => void;
}

export default function Toolbar({
  selected,
  pane,
  query,
  onQueryChange,
  onAddUrl,
  onAddTorrent,
  onGrabSite,
  onResume,
  onPause,
  onStop,
  onDelete,
  onOpenFolder,
}: Props) {
  const status = selected?.status;

  // Each action is enabled only where it means something, so the toolbar itself
  // documents what a download can do right now.
  const canResume = !!selected && (status === "paused" || status === "failed");
  const canPause = !!selected && (status === "downloading" || status === "queued");
  const canStop = !!selected && (status === "downloading" || status === "queued" || status === "paused");
  const canOpen = !!selected && status === "completed";

  const onDownloads = pane === "downloads";

  return (
    <div className="flex items-center gap-1 border-b border-line bg-surface-2 px-2 py-1.5">
      {/* Adding a download is always available; it is what the app is for. */}
      <ToolButton label="Add URL" Icon={Plus} onClick={onAddUrl} primary />
      <ToolButton label="Add Torrent" Icon={Magnet} onClick={onAddTorrent} />
      <ToolButton label="Grab Site" Icon={Globe} onClick={onGrabSite} />

      {/* The rest of the toolbar acts on the selected row, so it is meaningless
          away from the table rather than merely disabled. */}
      {onDownloads ? (
        <>
          <Divider />

          <ToolButton label="Resume" Icon={Play} onClick={onResume} disabled={!canResume} />
          <ToolButton label="Pause" Icon={Pause} onClick={onPause} disabled={!canPause} />
          <ToolButton label="Stop" Icon={Square} onClick={onStop} disabled={!canStop} />
          <ToolButton label="Open Folder" Icon={FolderOpen} onClick={onOpenFolder} disabled={!canOpen} />
          <ToolButton label="Delete" Icon={Trash2} onClick={onDelete} disabled={!selected} danger />

          <Divider />

          <div className="relative ml-1 w-56">
            <Search size={13} className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-slate-500" />
            <input
              type="search"
              value={query}
              onChange={(e) => onQueryChange(e.target.value)}
              placeholder="Search downloads…"
              aria-label="Search downloads"
              className="w-full rounded-md border border-line bg-surface-1 py-1.5 pl-8 pr-2.5 text-xs text-slate-200 placeholder:text-slate-600 focus:border-accent focus:outline-none"
            />
          </div>
        </>
      ) : (
        <>
          <Divider />
          <span className="px-1 text-xs font-medium text-slate-300">{PANE_TITLES[pane]}</span>
        </>
      )}

      <div className="flex-1" />
    </div>
  );
}

function Divider() {
  return <div className="mx-1.5 h-6 w-px bg-line" />;
}

interface ToolButtonProps {
  label: string;
  Icon: typeof Plus;
  onClick: () => void;
  disabled?: boolean;
  primary?: boolean;
  danger?: boolean;
}

function ToolButton({ label, Icon, onClick, disabled, primary, danger }: ToolButtonProps) {
  const base =
    "flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-35";

  const tone = primary
    ? "bg-accent text-white hover:bg-accent/85"
    : danger
      ? "text-slate-400 hover:bg-danger/15 hover:text-danger"
      : "text-slate-400 hover:bg-surface-3 hover:text-slate-100";

  return (
    <button onClick={onClick} disabled={disabled} title={label} className={`${base} ${tone}`}>
      <Icon size={14} />
      <span className="hidden lg:inline">{label}</span>
    </button>
  );
}
