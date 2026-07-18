import { useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Globe, Loader2, X } from "lucide-react";
import { useDownloadStore } from "../store/downloadStore";
import { DiscoveredFile } from "../types";

interface Props {
  onClose: () => void;
}

/** Filter presets. Empty `exts` means "every known downloadable type". */
const PRESETS: Array<{ id: string; label: string; exts: string[] }> = [
  { id: "all",     label: "Everything", exts: [] },
  { id: "images",  label: "Images",     exts: ["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp"] },
  { id: "video",   label: "Video",      exts: ["mp4", "mkv", "avi", "mov", "webm"] },
  { id: "audio",   label: "Audio",      exts: ["mp3", "flac", "wav", "m4a"] },
  { id: "docs",    label: "Documents",  exts: ["pdf", "epub", "mobi", "doc", "docx", "xls", "xlsx", "ppt", "pptx"] },
  { id: "archives", label: "Archives",  exts: ["zip", "rar", "7z", "gz", "bz2", "xz", "tar", "iso"] },
];

export default function SiteGrabber({ onClose }: Props) {
  const { settings, crawlSite, addDownloads } = useDownloadStore();

  const [url, setUrl] = useState("");
  const [depth, setDepth] = useState(1);
  const [sameHost, setSameHost] = useState(true);
  const [preset, setPreset] = useState("all");
  const [savePath, setSavePath] = useState("");

  const [scanning, setScanning] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [files, setFiles] = useState<DiscoveredFile[] | null>(null);
  const [truncated, setTruncated] = useState(false);
  const [pages, setPages] = useState(0);
  const [picked, setPicked] = useState<Set<string>>(new Set());

  const grouped = useMemo(() => {
    const by = new Map<string, DiscoveredFile[]>();
    for (const f of files ?? []) {
      const list = by.get(f.extension) ?? [];
      list.push(f);
      by.set(f.extension, list);
    }
    // Biggest groups first — that is almost always what the user came for.
    return [...by.entries()].sort((a, b) => b[1].length - a[1].length);
  }, [files]);

  const scan = async () => {
    if (!url.trim()) return;
    setScanning(true);
    setError(null);
    setFiles(null);
    try {
      const result = await crawlSite({
        url: url.trim(),
        depth,
        sameHostOnly: sameHost,
        extensions: PRESETS.find((p) => p.id === preset)?.exts ?? [],
      });
      setFiles(result.files);
      setPages(result.pagesVisited);
      setTruncated(result.truncated);
      // Nothing pre-selected: downloading is opt-in, and a crawl can return
      // hundreds of files.
      setPicked(new Set());
    } catch (err) {
      setError(String(err));
    } finally {
      setScanning(false);
    }
  };

  const toggle = (u: string) => {
    setPicked((prev) => {
      const next = new Set(prev);
      next.has(u) ? next.delete(u) : next.add(u);
      return next;
    });
  };

  const toggleGroup = (ext: string) => {
    const urls = (files ?? []).filter((f) => f.extension === ext).map((f) => f.url);
    const allOn = urls.every((u) => picked.has(u));
    setPicked((prev) => {
      const next = new Set(prev);
      for (const u of urls) allOn ? next.delete(u) : next.add(u);
      return next;
    });
  };

  const download = async () => {
    if (!picked.size) return;
    setBusy(true);
    try {
      await addDownloads([...picked], savePath || settings.defaultSavePath);
      onClose();
    } catch (err) {
      setError(String(err));
      setBusy(false);
    }
  };

  const browse = async () => {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === "string") setSavePath(dir);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
      <div className="flex max-h-[85vh] w-full max-w-2xl flex-col rounded-xl border border-slate-700 bg-[#0f172a] shadow-2xl">
        <div className="flex items-center gap-2.5 border-b border-slate-700 px-4 py-3">
          <Globe size={16} className="text-blue-400" />
          <h2 className="flex-1 text-sm font-semibold">Site Grabber</h2>
          <button onClick={onClose} className="p-0.5 text-slate-400 hover:text-white">
            <X size={16} />
          </button>
        </div>

        <div className="space-y-4 overflow-y-auto p-4">
          <div>
            <label className="mb-1.5 block text-xs font-medium text-slate-400">Page URL</label>
            <div className="flex gap-2">
              <input
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && scan()}
                placeholder="https://example.com/gallery"
                className="flex-1 rounded-lg border border-slate-700 bg-[#0b1220] px-3 py-2 text-sm focus:border-blue-500 focus:outline-none"
              />
              <button
                onClick={scan}
                disabled={scanning || !url.trim()}
                className="flex items-center gap-1.5 rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium hover:bg-blue-700 disabled:opacity-50"
              >
                {scanning ? <Loader2 size={14} className="animate-spin" /> : null}
                {scanning ? "Scanning…" : "Scan"}
              </button>
            </div>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="mb-1.5 block text-xs font-medium text-slate-400">
                Depth — {depth === 0 ? "this page only" : `${depth} level${depth > 1 ? "s" : ""}`}
              </label>
              <input
                type="range"
                min={0}
                max={3}
                value={depth}
                onChange={(e) => setDepth(Number(e.target.value))}
                className="w-full accent-blue-500"
              />
            </div>
            <div>
              <label className="mb-1.5 block text-xs font-medium text-slate-400">File type</label>
              <select
                value={preset}
                onChange={(e) => setPreset(e.target.value)}
                className="w-full rounded-lg border border-slate-700 bg-[#0b1220] px-3 py-2 text-sm focus:border-blue-500 focus:outline-none"
              >
                {PRESETS.map((p) => (
                  <option key={p.id} value={p.id}>{p.label}</option>
                ))}
              </select>
            </div>
          </div>

          <label className="flex items-start gap-2.5 text-sm">
            <input
              type="checkbox"
              checked={sameHost}
              onChange={(e) => setSameHost(e.target.checked)}
              className="mt-0.5 accent-blue-500"
            />
            <span>
              Stay on this site
              <span className="block text-xs text-slate-500">
                Following off-site links can turn one page into an unbounded crawl.
              </span>
            </span>
          </label>

          {error && (
            <div className="rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-xs text-danger">
              {error}
            </div>
          )}

          {files !== null && (
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-xs text-slate-400">
                <span>
                  {files.length} file{files.length === 1 ? "" : "s"} across {pages} page
                  {pages === 1 ? "" : "s"}
                </span>
                {picked.size > 0 && (
                  <span className="text-blue-400">· {picked.size} selected</span>
                )}
              </div>

              {truncated && (
                <div className="rounded-lg border border-amber-500/20 bg-amber-500/10 px-3 py-2 text-xs text-amber-400/90">
                  Stopped at the page limit — these results are partial. Narrow the
                  depth or file type for a complete scan.
                </div>
              )}

              {files.length === 0 && (
                <p className="py-6 text-center text-sm text-slate-500">
                  No matching files found on that page.
                </p>
              )}

              {grouped.map(([ext, group]) => (
                <div key={ext} className="overflow-hidden rounded-lg border border-slate-700">
                  <button
                    onClick={() => toggleGroup(ext)}
                    className="flex w-full items-center gap-2 bg-slate-800/60 px-3 py-2 text-left text-xs font-semibold hover:bg-slate-800"
                  >
                    <span className="uppercase text-blue-300">.{ext}</span>
                    <span className="text-slate-400">{group.length}</span>
                    <span className="flex-1" />
                    <span className="font-normal text-slate-500">
                      {group.every((f) => picked.has(f.url)) ? "Deselect all" : "Select all"}
                    </span>
                  </button>
                  <div className="max-h-40 overflow-y-auto">
                    {group.map((f) => (
                      <label
                        key={f.url}
                        className="flex items-center gap-2.5 border-t border-slate-800 px-3 py-1.5 text-xs hover:bg-slate-800/40"
                      >
                        <input
                          type="checkbox"
                          checked={picked.has(f.url)}
                          onChange={() => toggle(f.url)}
                          className="accent-blue-500"
                        />
                        <span className="min-w-0 flex-1 truncate" title={f.url}>
                          {f.filename}
                        </span>
                        {f.label && (
                          <span className="max-w-[40%] truncate text-slate-500" title={f.label}>
                            {f.label}
                          </span>
                        )}
                      </label>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        <div className="flex items-center gap-2 border-t border-slate-700 px-4 py-3">
          <button
            onClick={browse}
            className="flex items-center gap-1.5 rounded-lg border border-slate-700 px-3 py-2 text-xs hover:bg-slate-800"
            title="Choose save folder"
          >
            <FolderOpen size={13} />
            <span className="max-w-[200px] truncate">
              {savePath || settings.defaultSavePath}
            </span>
          </button>
          <span className="flex-1" />
          <button
            onClick={onClose}
            className="rounded-lg px-3 py-2 text-sm text-slate-400 hover:text-white"
          >
            Cancel
          </button>
          <button
            onClick={download}
            disabled={busy || picked.size === 0}
            className="rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium hover:bg-blue-700 disabled:opacity-50"
          >
            {busy ? "Adding…" : `Download ${picked.size || ""}`}
          </button>
        </div>
      </div>
    </div>
  );
}
