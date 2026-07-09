import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertCircle, X, Zap } from "lucide-react";

import { useTauriEvents } from "./hooks/useTauriEvents";
import { useDownloadStore } from "./store/downloadStore";

import AddDialog, { AddMode } from "./components/AddDialog";
import AnalyticsDashboard from "./components/AnalyticsDashboard";
import DetailPanel from "./components/DetailPanel";
import DownloadTable from "./components/DownloadTable";
import SettingsPanel from "./components/SettingsPanel";
import Sidebar, { Filter, matchesFilter } from "./components/Sidebar";
import StatusBar from "./components/StatusBar";
import Toolbar, { Pane } from "./components/Toolbar";

function App() {
  const [pane, setPane] = useState<Pane>("downloads");
  const [filter, setFilter] = useState<Filter>({ kind: "all" });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [detailsCollapsed, setDetailsCollapsed] = useState(false);
  const [addMode, setAddMode] = useState<AddMode | null>(null);

  const {
    downloads,
    scheduler,
    error,
    clearError,
    loadDownloads,
    loadSettings,
    pauseDownload,
    resumeDownload,
    cancelDownload,
    deleteDownload,
    pendingDownload,
    setPendingDownload,
  } = useDownloadStore();

  useTauriEvents();

  useEffect(() => {
    loadDownloads();
    loadSettings();
  }, [loadDownloads, loadSettings]);

  // The browser extension asked to download something — open the dialog prefilled.
  useEffect(() => {
    if (pendingDownload) setAddMode("url");
  }, [pendingDownload]);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return downloads
      .filter((d) => matchesFilter(d, filter))
      .filter((d) => !q || d.filename.toLowerCase().includes(q) || d.url.toLowerCase().includes(q));
  }, [downloads, filter, query]);

  // Look the selection up in the full list: it stays valid even when the current
  // filter or search no longer shows it, so the toolbar doesn't go dead.
  const selected = useMemo(
    () => downloads.find((d) => d.id === selectedId) ?? null,
    [downloads, selectedId]
  );

  const closeDialog = () => {
    setAddMode(null);
    setPendingDownload(null);
  };

  const openFolder = async () => {
    if (!selected) return;
    await invoke("cmd_open_folder", { path: `${selected.savePath}/${selected.filename}` });
  };

  const openFile = async (path: string) => {
    await invoke("cmd_open_file", { path }).catch(() => {});
  };

  return (
    <div className="flex h-screen flex-col bg-surface-1 text-slate-200">
      {/* Title bar */}
      <header
        data-tauri-drag-region
        className="no-select flex h-9 flex-shrink-0 items-center gap-2 border-b border-line bg-surface-0 px-3"
      >
        <Zap size={14} className="text-accent" />
        <span className="text-xs font-semibold tracking-wide text-slate-200">FluxDM</span>
        <span className="text-[10px] text-slate-600">v0.1.0</span>
      </header>

      <Toolbar
        selected={selected}
        pane={pane}
        query={query}
        onQueryChange={setQuery}
        onAddUrl={() => setAddMode("url")}
        onAddTorrent={() => setAddMode("torrent")}
        onResume={() => selected && resumeDownload(selected.id)}
        onPause={() => selected && pauseDownload(selected.id)}
        onStop={() => selected && cancelDownload(selected.id)}
        onDelete={() => selected && deleteDownload(selected.id, false)}
        onOpenFolder={openFolder}
      />

      {error && (
        <div className="flex flex-shrink-0 items-center gap-2.5 border-b border-danger/40 bg-danger/10 px-3 py-2 text-xs text-danger">
          <AlertCircle size={13} className="flex-shrink-0" />
          <span className="flex-1">{error}</span>
          <button onClick={clearError} title="Dismiss" className="p-0.5 hover:text-white">
            <X size={13} />
          </button>
        </div>
      )}

      <main className="flex flex-1 overflow-hidden">
        <Sidebar
          downloads={downloads}
          active={filter}
          onSelect={setFilter}
          pane={pane}
          onPane={setPane}
        />

        <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
          {pane === "downloads" && (
            <>
              <div className="min-h-0 flex-1">
                <DownloadTable
                  downloads={visible}
                  selectedId={selectedId}
                  onSelect={setSelectedId}
                  onActivate={(d) =>
                    d.status === "completed" && openFile(`${d.savePath}/${d.filename}`)
                  }
                />
              </div>
              <DetailPanel
                download={selected}
                collapsed={detailsCollapsed}
                onToggle={() => setDetailsCollapsed((v) => !v)}
              />
            </>
          )}

          {pane === "analytics" && (
            <div className="flex-1 overflow-y-auto">
              <AnalyticsDashboard />
            </div>
          )}
          {pane === "settings" && (
            <div className="flex-1 overflow-y-auto">
              <SettingsPanel />
            </div>
          )}
        </div>
      </main>

      <StatusBar downloads={downloads} scheduler={scheduler} />

      {addMode && (
        <AddDialog
          mode={addMode}
          initial={pendingDownload ?? undefined}
          onClose={closeDialog}
        />
      )}
    </div>
  );
}

export default App;
