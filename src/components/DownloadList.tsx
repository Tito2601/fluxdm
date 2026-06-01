import { Download, Filter, RefreshCw } from "lucide-react";
import { useState, useMemo } from "react";
import { useDownloadStore } from "../store/downloadStore";
import { DownloadStatus } from "../types";
import DownloadItem from "./DownloadItem";

type FilterTab = "all" | DownloadStatus;

const FILTER_TABS: { id: FilterTab; label: string }[] = [
  { id: "all", label: "All" },
  { id: "downloading", label: "Active" },
  { id: "queued", label: "Queued" },
  { id: "completed", label: "Completed" },
  { id: "paused", label: "Paused" },
  { id: "failed", label: "Failed" },
];

export default function DownloadList() {
  const { downloads, isLoading, loadDownloads } = useDownloadStore();
  const [filter, setFilter] = useState<FilterTab>("all");
  const [search, setSearch] = useState("");

  const filtered = useMemo(() => {
    return downloads.filter((d) => {
      const matchesFilter = filter === "all" || d.status === filter;
      const matchesSearch =
        !search ||
        d.filename.toLowerCase().includes(search.toLowerCase()) ||
        d.url.toLowerCase().includes(search.toLowerCase());
      return matchesFilter && matchesSearch;
    });
  }, [downloads, filter, search]);

  const counts: Record<FilterTab, number> = useMemo(() => {
    const c: Record<FilterTab, number> = {
      all: downloads.length,
      downloading: 0,
      queued: 0,
      completed: 0,
      paused: 0,
      failed: 0,
      cancelled: 0,
    };
    downloads.forEach((d) => {
      c[d.status] = (c[d.status] ?? 0) + 1;
    });
    return c;
  }, [downloads]);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-slate-500 text-sm animate-pulse">Loading downloads...</div>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      {/* Filter Bar */}
      <div className="flex items-center gap-2 px-4 py-2 border-b border-slate-800 bg-[#0c1524]">
        {/* Filter Tabs */}
        <div className="flex items-center gap-1">
          {FILTER_TABS.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setFilter(tab.id)}
              className={`px-3 py-1 text-xs rounded-md transition-colors ${
                filter === tab.id
                  ? "bg-slate-700 text-white"
                  : "text-slate-500 hover:text-slate-300"
              }`}
            >
              {tab.label}
              {counts[tab.id] > 0 && (
                <span className="ml-1 text-slate-400">({counts[tab.id]})</span>
              )}
            </button>
          ))}
        </div>

        <div className="flex-1" />

        {/* Search */}
        <div className="relative">
          <Filter
            size={12}
            className="absolute left-2.5 top-1/2 -translate-y-1/2 text-slate-500"
          />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Filter files..."
            className="bg-slate-800 border border-slate-700 rounded-lg pl-8 pr-3 py-1 text-xs text-slate-300 placeholder-slate-600 focus:outline-none focus:border-blue-500 w-40"
          />
        </div>

        {/* Refresh */}
        <button
          onClick={loadDownloads}
          className="p-1.5 text-slate-500 hover:text-slate-300 hover:bg-slate-800 rounded transition-colors"
          title="Refresh"
        >
          <RefreshCw size={13} />
        </button>
      </div>

      {/* Download Items */}
      <div className="flex-1 overflow-y-auto">
        {filtered.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full gap-3 text-slate-600">
            <Download size={48} strokeWidth={1} />
            <p className="text-sm">
              {search
                ? "No downloads match your search"
                : filter !== "all"
                ? `No ${filter} downloads`
                : "No downloads yet"}
            </p>
            {!search && filter === "all" && (
              <p className="text-xs text-slate-700">
                Click "Add Download" or install the browser extension to get started
              </p>
            )}
          </div>
        ) : (
          filtered.map((download) => (
            <DownloadItem key={download.id} download={download} />
          ))
        )}
      </div>

      {/* Bottom status bar */}
      {downloads.length > 0 && (
        <div className="flex items-center gap-4 px-4 py-1.5 border-t border-slate-800 bg-[#0c1524] text-xs text-slate-500">
          <span>
            {counts["downloading"]} active · {counts["completed"]} completed ·{" "}
            {counts["failed"]} failed
          </span>
        </div>
      )}
    </div>
  );
}
