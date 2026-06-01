import { List, ArrowUp, ArrowDown } from "lucide-react";
import { useDownloadStore } from "../store/downloadStore";

/**
 * QueuePanel — shows the current download queue order and allows reordering.
 * Phase 1 scaffold (reorder via Rust command will be wired in Phase 2 polish).
 */
export default function QueuePanel() {
  const { downloads } = useDownloadStore();
  const queued = downloads.filter(
    (d) => d.status === "queued" || d.status === "paused"
  );

  if (queued.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-32 text-slate-600 gap-2">
        <List size={24} strokeWidth={1} />
        <p className="text-xs">Queue is empty</p>
      </div>
    );
  }

  return (
    <div className="p-4">
      <h3 className="text-sm font-medium text-slate-300 mb-3 flex items-center gap-2">
        <List size={14} />
        Queue ({queued.length})
      </h3>
      <div className="space-y-1">
        {queued.map((d, i) => (
          <div
            key={d.id}
            className="flex items-center gap-2 p-2 bg-slate-800/40 rounded-lg text-xs text-slate-400"
          >
            <span className="text-slate-600 w-5 text-right">{i + 1}.</span>
            <span className="flex-1 truncate">{d.filename}</span>
            <div className="flex gap-1">
              {i > 0 && (
                <button className="p-0.5 hover:text-slate-200 transition-colors">
                  <ArrowUp size={12} />
                </button>
              )}
              {i < queued.length - 1 && (
                <button className="p-0.5 hover:text-slate-200 transition-colors">
                  <ArrowDown size={12} />
                </button>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
