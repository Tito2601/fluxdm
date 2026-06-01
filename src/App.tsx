import { useEffect, useState } from "react";
import { useTauriEvents } from "./hooks/useTauriEvents";
import { useDownloadStore } from "./store/downloadStore";
import DownloadList from "./components/DownloadList";
import AnalyticsDashboard from "./components/AnalyticsDashboard";
import SettingsPanel from "./components/SettingsPanel";
import QueuePanel from "./components/QueuePanel";
import StreamPicker from "./components/StreamPicker";
import {
  Download,
  BarChart3,
  Settings,
  Zap,
  Github,
  Plus,
  X,
  AlertCircle,
  Radio,
  Loader2,
  Sparkles,
} from "lucide-react";
import { StreamInfo, StreamQuality } from "./types";

type TabId = "downloads" | "analytics" | "settings";

function App() {
  const [activeTab, setActiveTab] = useState<TabId>("downloads");
  const [showAddDialog, setShowAddDialog] = useState(false);
  const [showQueue, setShowQueue] = useState(false);
  const [addUrl, setAddUrl] = useState("");
  const [addFilename, setAddFilename] = useState("");
  const [addSavePath, setAddSavePath] = useState("");
  // Stream detection state
  const [probing, setProbing] = useState(false);
  const [streamInfo, setStreamInfo] = useState<StreamInfo | null>(null);
  const [showStreamPicker, setShowStreamPicker] = useState(false);
  // LLM suggestion state
  const [suggestingName, setSuggestingName] = useState(false);

  const {
    loadDownloads,
    loadSettings,
    addDownload,
    probeStream,
    addStreamDownload,
    suggestFilename,
    settings,
    downloads,
    error,
    clearError,
    pendingDownload,
    setPendingDownload,
  } = useDownloadStore();

  // Set up Tauri event listeners
  useTauriEvents();

  // Load initial data
  useEffect(() => {
    loadDownloads();
    loadSettings();
  }, [loadDownloads, loadSettings]);

  // Browser extension sent a download request → open the Add dialog pre-filled
  useEffect(() => {
    if (!pendingDownload) return;
    setAddUrl(pendingDownload.url);
    setAddFilename(pendingDownload.filename);
    setAddSavePath(pendingDownload.savePath);
    setStreamInfo(null);
    setShowAddDialog(true);
    setPendingDownload(null);
  }, [pendingDownload, setPendingDownload]);

  const activeCount = downloads.filter((d) => d.status === "downloading").length;
  const queuedCount = downloads.filter(
    (d) => d.status === "queued" || d.status === "paused"
  ).length;

  const handleSuggestName = async () => {
    if (!addUrl) return;
    setSuggestingName(true);
    try {
      const suggested = await suggestFilename(addUrl, addFilename || addUrl.split("/").pop() || "file", null);
      setAddFilename(suggested);
    } catch {
      // LLM unavailable or disabled — silently ignore
    } finally {
      setSuggestingName(false);
    }
  };

  const resetAddDialog = () => {
    setAddUrl("");
    setAddFilename("");
    setAddSavePath("");
    setStreamInfo(null);
    setProbing(false);
    setShowAddDialog(false);
  };

  const handleDetectUrl = async () => {
    if (!addUrl) return;
    setProbing(true);
    setStreamInfo(null);
    try {
      const info = await probeStream(addUrl);
      setStreamInfo(info);
      if (info.streamType !== "direct") {
        // Suggest a filename with the right extension
        if (!addFilename) {
          const ext = info.streamType === "dash" ? ".mp4" : ".ts";
          const guessed = addUrl.split("/").filter(Boolean).pop()?.split("?")[0] ?? "stream";
          const base    = guessed.replace(/\.(m3u8|mpd)$/i, "");
          setAddFilename(`${base}${ext}`);
        }
        setShowStreamPicker(true);
      }
    } catch {
      // probe failed — treat as a direct download
      setStreamInfo({ streamType: "direct", qualities: [], durationSeconds: undefined, title: undefined });
    } finally {
      setProbing(false);
    }
  };

  const handleAddDownload = async () => {
    if (!addUrl) return;
    const filename = addFilename || addUrl.split("/").pop() || "download";
    const savePath = addSavePath || "C:\\Users\\Public\\Downloads";
    await addDownload(addUrl, filename, savePath, undefined, undefined);
    resetAddDialog();
  };

  const handleStreamConfirm = async (quality: StreamQuality, filename: string) => {
    if (!streamInfo) return;
    const savePath = addSavePath || "C:\\Users\\Public\\Downloads";
    await addStreamDownload(
      quality.url,
      quality.reprId ?? null,
      streamInfo.streamType,
      filename,
      savePath
    );
    setShowStreamPicker(false);
    resetAddDialog();
  };

  return (
    <div className="flex flex-col h-screen bg-[#0f172a] text-slate-100">
      {/* Title Bar */}
      <div
        data-tauri-drag-region
        className="flex items-center justify-between h-10 px-4 bg-[#020617] border-b border-slate-800 no-select"
      >
        <div className="flex items-center gap-2">
          <Zap size={16} className="text-blue-400" />
          <span className="text-sm font-semibold text-blue-400 tracking-wide">
            FluxDM
          </span>
          <span className="text-xs text-slate-500">v0.1.0</span>
        </div>
        <div className="flex items-center gap-3">
          {activeCount > 0 && (
            <span className="text-xs bg-blue-600 text-white px-2 py-0.5 rounded-full animate-pulse-download">
              {activeCount} active
            </span>
          )}
        </div>
      </div>

      {/* Navigation Tabs */}
      <div className="flex items-center gap-1 px-4 py-2 bg-[#0c1524] border-b border-slate-800">
        <button
          onClick={() => setActiveTab("downloads")}
          className={`flex items-center gap-2 px-4 py-2 rounded-md text-sm font-medium transition-colors ${
            activeTab === "downloads"
              ? "bg-blue-600 text-white"
              : "text-slate-400 hover:text-white hover:bg-slate-800"
          }`}
        >
          <Download size={14} />
          Downloads
          {downloads.length > 0 && (
            <span className="text-xs bg-slate-700 rounded-full px-1.5 py-0.5">
              {downloads.length}
            </span>
          )}
        </button>
        <button
          onClick={() => setActiveTab("analytics")}
          className={`flex items-center gap-2 px-4 py-2 rounded-md text-sm font-medium transition-colors ${
            activeTab === "analytics"
              ? "bg-blue-600 text-white"
              : "text-slate-400 hover:text-white hover:bg-slate-800"
          }`}
        >
          <BarChart3 size={14} />
          Analytics
        </button>
        <button
          onClick={() => setActiveTab("settings")}
          className={`flex items-center gap-2 px-4 py-2 rounded-md text-sm font-medium transition-colors ${
            activeTab === "settings"
              ? "bg-blue-600 text-white"
              : "text-slate-400 hover:text-white hover:bg-slate-800"
          }`}
        >
          <Settings size={14} />
          Settings
        </button>

        <div className="flex-1" />

        {/* Queue Toggle (only visible on Downloads tab) */}
        {activeTab === "downloads" && (
          <button
            onClick={() => setShowQueue((v) => !v)}
            className={`flex items-center gap-2 px-3 py-2 rounded-md text-sm font-medium transition-colors ${
              showQueue
                ? "bg-slate-700 text-white"
                : "text-slate-400 hover:text-white hover:bg-slate-800"
            }`}
            title="Toggle queue panel"
          >
            Queue
            {queuedCount > 0 && (
              <span className="text-xs bg-yellow-600/80 text-white px-1.5 py-0.5 rounded-full">
                {queuedCount}
              </span>
            )}
          </button>
        )}

        {/* Add Download Button */}
        <button
          onClick={() => setShowAddDialog(true)}
          className="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-500 rounded-md text-sm font-medium transition-colors"
        >
          <Plus size={14} />
          Add Download
        </button>
      </div>

      {/* Error Toast */}
      {error && (
        <div className="flex items-center gap-3 px-4 py-2.5 bg-red-900/40 border-b border-red-800/50 text-red-300 text-sm">
          <AlertCircle size={14} className="flex-shrink-0" />
          <span className="flex-1">{error}</span>
          <button
            onClick={clearError}
            className="p-0.5 hover:text-white transition-colors"
            title="Dismiss"
          >
            <X size={14} />
          </button>
        </div>
      )}

      {/* Main Content */}
      <div className="flex-1 overflow-hidden flex">
        <div className="flex-1 overflow-hidden">
          {activeTab === "downloads" && <DownloadList />}
          {activeTab === "analytics" && <AnalyticsDashboard />}
          {activeTab === "settings" && <SettingsPanel />}
        </div>

        {/* Queue Sidebar */}
        {activeTab === "downloads" && showQueue && (
          <div className="w-56 flex-shrink-0 border-l border-slate-800 bg-[#0c1524] overflow-y-auto">
            <QueuePanel />
          </div>
        )}
      </div>

      {/* Add Download Dialog */}
      {showAddDialog && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
          <div className="bg-[#1e293b] border border-slate-700 rounded-xl p-6 w-[480px] shadow-2xl">
            <h2 className="text-lg font-semibold mb-4 flex items-center gap-2">
              <Plus size={18} className="text-blue-400" />
              Add New Download
            </h2>

            <div className="space-y-4">
              {/* URL row with Detect button */}
              <div>
                <label className="block text-sm text-slate-400 mb-1">URL *</label>
                <div className="flex gap-2">
                  <input
                    type="url"
                    value={addUrl}
                    onChange={(e) => { setAddUrl(e.target.value); setStreamInfo(null); }}
                    onKeyDown={(e) => e.key === "Enter" && handleDetectUrl()}
                    placeholder="https://example.com/file.zip or stream.m3u8"
                    className="flex-1 bg-[#0f172a] border border-slate-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-blue-500"
                  />
                  <button
                    onClick={handleDetectUrl}
                    disabled={!addUrl || probing}
                    className="flex items-center gap-1.5 px-3 py-2 bg-slate-700 hover:bg-slate-600 disabled:opacity-50 disabled:cursor-not-allowed rounded-lg text-xs font-medium transition-colors flex-shrink-0"
                    title="Detect if URL is a stream (HLS/DASH)"
                  >
                    {probing ? (
                      <Loader2 size={12} className="animate-spin" />
                    ) : (
                      <Radio size={12} />
                    )}
                    {probing ? "Detecting…" : "Detect"}
                  </button>
                </div>

                {/* Stream detection result badge */}
                {streamInfo && streamInfo.streamType !== "direct" && (
                  <div
                    className="mt-1.5 flex items-center gap-1.5 text-xs text-blue-300 cursor-pointer hover:text-blue-200"
                    onClick={() => setShowStreamPicker(true)}
                  >
                    <Radio size={11} />
                    <span>
                      {streamInfo.streamType.toUpperCase()} stream — {streamInfo.qualities.length} {streamInfo.qualities.length === 1 ? "quality" : "qualities"} detected.{" "}
                      <span className="underline">Pick quality →</span>
                    </span>
                  </div>
                )}
                {streamInfo && streamInfo.streamType === "direct" && (
                  <div className="mt-1.5 text-xs text-slate-500">
                    ✓ Regular file — will be downloaded normally.
                  </div>
                )}
              </div>

              <div>
                <label className="block text-sm text-slate-400 mb-1">
                  Filename (optional)
                </label>
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={addFilename}
                    onChange={(e) => setAddFilename(e.target.value)}
                    placeholder="Auto-detected from URL"
                    className="flex-1 bg-[#0f172a] border border-slate-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-blue-500"
                  />
                  {settings.llmEnabled && (
                    <button
                      onClick={handleSuggestName}
                      disabled={!addUrl || suggestingName}
                      className="flex items-center gap-1 px-3 py-2 bg-purple-700/40 hover:bg-purple-700/60 border border-purple-600/40 disabled:opacity-50 disabled:cursor-not-allowed rounded-lg text-xs text-purple-300 transition-colors flex-shrink-0"
                      title="Ask local AI to suggest a better filename"
                    >
                      {suggestingName ? (
                        <Loader2 size={12} className="animate-spin" />
                      ) : (
                        <Sparkles size={12} />
                      )}
                      AI
                    </button>
                  )}
                </div>
              </div>

              <div>
                <label className="block text-sm text-slate-400 mb-1">
                  Save Path (optional)
                </label>
                <input
                  type="text"
                  value={addSavePath}
                  onChange={(e) => setAddSavePath(e.target.value)}
                  placeholder="C:\Users\Public\Downloads"
                  className="w-full bg-[#0f172a] border border-slate-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-blue-500"
                />
              </div>
            </div>

            <div className="flex justify-end gap-3 mt-6">
              <button
                onClick={resetAddDialog}
                className="px-4 py-2 text-sm text-slate-400 hover:text-white transition-colors"
              >
                Cancel
              </button>

              {/* If a stream was detected, prompt them to open the picker */}
              {streamInfo && streamInfo.streamType !== "direct" ? (
                <button
                  onClick={() => setShowStreamPicker(true)}
                  className="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-500 rounded-lg text-sm font-medium transition-colors"
                >
                  <Radio size={14} />
                  Pick Quality…
                </button>
              ) : (
                <button
                  onClick={handleAddDownload}
                  disabled={!addUrl}
                  className="px-4 py-2 bg-blue-600 hover:bg-blue-500 disabled:opacity-50 disabled:cursor-not-allowed rounded-lg text-sm font-medium transition-colors"
                >
                  Start Download
                </button>
              )}
            </div>
          </div>
        </div>
      )}

      {/* Stream quality picker modal */}
      {showStreamPicker && streamInfo && (
        <StreamPicker
          streamInfo={streamInfo}
          filename={addFilename}
          savePath={addSavePath}
          onConfirm={handleStreamConfirm}
          onCancel={() => setShowStreamPicker(false)}
        />
      )}

      {/* Status Bar */}
      <div className="flex items-center justify-between h-6 px-4 bg-[#020617] border-t border-slate-800 text-xs text-slate-500">
        <span>
          {activeCount > 0
            ? `${activeCount} download${activeCount > 1 ? "s" : ""} in progress`
            : "Ready"}
        </span>
        <span className="flex items-center gap-1">
          <Github size={10} />
          FluxDM by FluxDev
        </span>
      </div>
    </div>
  );
}

export default App;
