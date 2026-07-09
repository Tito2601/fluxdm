import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Loader2, Magnet, Radio, Sparkles, X } from "lucide-react";
import StreamPicker from "./StreamPicker";
import { useDownloadStore } from "../store/downloadStore";
import { StreamInfo, StreamQuality } from "../types";

export type AddMode = "url" | "torrent";

interface Props {
  mode: AddMode;
  /** Prefills from the browser extension, if any. */
  initial?: { url: string; filename: string; savePath: string };
  onClose: () => void;
}

export default function AddDialog({ mode, initial, onClose }: Props) {
  const {
    settings,
    addDownload,
    addTorrent,
    isTorrentSource,
    probeStream,
    addStreamDownload,
    suggestFilename,
  } = useDownloadStore();

  const [source, setSource] = useState(initial?.url ?? "");
  const [filename, setFilename] = useState(initial?.filename ?? "");
  const [savePath, setSavePath] = useState(initial?.savePath ?? "");

  const [torrentMode, setTorrentMode] = useState(mode === "torrent");
  const [streamInfo, setStreamInfo] = useState<StreamInfo | null>(null);
  const [showPicker, setShowPicker] = useState(false);

  const [probing, setProbing] = useState(false);
  const [suggesting, setSuggesting] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const defaultPath = torrentMode ? settings.torrentSavePath : settings.defaultSavePath;

  // Pasting a magnet link into the URL dialog should just work, rather than
  // failing later with a confusing HTTP error. The backend decides what counts
  // as a torrent so the rule lives in exactly one place.
  useEffect(() => {
    if (mode === "torrent" || !source.trim()) return;
    let cancelled = false;
    isTorrentSource(source).then((yes) => {
      if (!cancelled && yes) setTorrentMode(true);
    });
    return () => { cancelled = true; };
  }, [source, mode, isTorrentSource]);

  const browseFolder = async () => {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked === "string") setSavePath(picked);
  };

  const browseTorrentFile = async () => {
    const picked = await open({
      multiple: false,
      filters: [{ name: "Torrent", extensions: ["torrent"] }],
    });
    if (typeof picked === "string") setSource(picked);
  };

  const detectStream = async () => {
    if (!source) return;
    setProbing(true);
    setStreamInfo(null);
    try {
      const info = await probeStream(source);
      setStreamInfo(info);
      if (info.streamType !== "direct") {
        if (!filename) {
          const ext = info.streamType === "dash" ? ".mp4" : ".ts";
          const guessed = source.split("/").filter(Boolean).pop()?.split("?")[0] ?? "stream";
          setFilename(`${guessed.replace(/\.(m3u8|mpd)$/i, "")}${ext}`);
        }
        setShowPicker(true);
      }
    } catch {
      setStreamInfo({ streamType: "direct", qualities: [] });
    } finally {
      setProbing(false);
    }
  };

  const suggest = async () => {
    if (!source) return;
    setSuggesting(true);
    try {
      setFilename(await suggestFilename(source, filename || source.split("/").pop() || "file", null));
    } catch {
      // The local model is optional; leave the field as the user typed it.
    } finally {
      setSuggesting(false);
    }
  };

  const submit = async () => {
    if (!source.trim()) return;
    setBusy(true);
    setError(null);
    try {
      if (torrentMode) {
        await addTorrent(source.trim(), savePath || defaultPath);
      } else {
        const name = filename || source.split("/").pop() || "download";
        await addDownload(source.trim(), name, savePath || defaultPath);
      }
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const confirmStream = async (quality: StreamQuality, name: string) => {
    if (!streamInfo) return;
    setBusy(true);
    try {
      await addStreamDownload(
        quality.url,
        quality.reprId ?? null,
        streamInfo.streamType,
        name,
        savePath || defaultPath
      );
      onClose();
    } catch (err) {
      setError(String(err));
      setBusy(false);
    }
  };

  if (showPicker && streamInfo) {
    return (
      <StreamPicker
        streamInfo={streamInfo}
        filename={filename}
        savePath={savePath}
        onConfirm={confirmStream}
        onCancel={() => setShowPicker(false)}
      />
    );
  }

  const isStream = streamInfo && streamInfo.streamType !== "direct";

  return (
    <Overlay onClose={onClose}>
      <header className="mb-4 flex items-center gap-2">
        {torrentMode ? <Magnet size={16} className="text-up" /> : <Radio size={16} className="text-accent" />}
        <h2 className="text-sm font-semibold text-slate-100">
          {torrentMode ? "Add Torrent" : "Add Download"}
        </h2>
        <div className="flex-1" />
        <button onClick={onClose} className="rounded p-1 text-slate-500 hover:bg-surface-3 hover:text-slate-200">
          <X size={14} />
        </button>
      </header>

      <div className="space-y-3.5">
        <Field label={torrentMode ? "Magnet link or .torrent file" : "URL"}>
          <div className="flex gap-2">
            <input
              autoFocus
              value={source}
              onChange={(e) => { setSource(e.target.value); setStreamInfo(null); }}
              onKeyDown={(e) => e.key === "Enter" && submit()}
              placeholder={torrentMode ? "magnet:?xt=urn:btih:… or a local .torrent path" : "https://example.com/file.zip"}
              className={inputClass}
            />
            {torrentMode ? (
              <SmallButton onClick={browseTorrentFile} Icon={FolderOpen} label="Browse" />
            ) : (
              <SmallButton
                onClick={detectStream}
                Icon={probing ? Loader2 : Radio}
                label={probing ? "Detecting…" : "Detect"}
                disabled={!source || probing}
                spinning={probing}
              />
            )}
          </div>

          {isStream && (
            <button
              onClick={() => setShowPicker(true)}
              className="mt-1.5 flex items-center gap-1.5 text-[11px] text-accent-soft hover:underline"
            >
              <Radio size={11} />
              {streamInfo!.streamType.toUpperCase()} stream · {streamInfo!.qualities.length} qualities — pick one
            </button>
          )}
          {streamInfo?.streamType === "direct" && (
            <p className="mt-1.5 text-[11px] text-slate-500">Regular file — will download normally.</p>
          )}
          {torrentMode && mode === "url" && (
            <p className="mt-1.5 text-[11px] text-up">Detected a torrent — switched to the torrent engine.</p>
          )}
        </Field>

        {!torrentMode && (
          <Field label="Filename" hint="optional">
            <div className="flex gap-2">
              <input
                value={filename}
                onChange={(e) => setFilename(e.target.value)}
                placeholder="Auto-detected from the URL"
                className={inputClass}
              />
              {settings.llmEnabled && (
                <SmallButton
                  onClick={suggest}
                  Icon={suggesting ? Loader2 : Sparkles}
                  label="AI"
                  disabled={!source || suggesting}
                  spinning={suggesting}
                />
              )}
            </div>
          </Field>
        )}

        <Field label="Save to" hint="optional">
          <div className="flex gap-2">
            <input
              value={savePath}
              onChange={(e) => setSavePath(e.target.value)}
              placeholder={defaultPath}
              className={inputClass}
            />
            <SmallButton onClick={browseFolder} Icon={FolderOpen} label="Browse" />
          </div>
        </Field>

        {error && (
          <p className="rounded border border-danger/40 bg-danger/10 px-2.5 py-1.5 text-[11px] text-danger">
            {error}
          </p>
        )}
      </div>

      <footer className="mt-5 flex items-center justify-end gap-2">
        {busy && torrentMode && (
          <span className="mr-auto text-[11px] text-slate-500">
            Fetching torrent metadata from the swarm…
          </span>
        )}
        <button onClick={onClose} className="px-3 py-1.5 text-xs text-slate-400 hover:text-slate-200">
          Cancel
        </button>
        <button
          onClick={isStream ? () => setShowPicker(true) : submit}
          disabled={!source.trim() || busy}
          className="flex items-center gap-1.5 rounded-md bg-accent px-3.5 py-1.5 text-xs font-medium text-white transition-colors hover:bg-accent/85 disabled:cursor-not-allowed disabled:opacity-40"
        >
          {busy && <Loader2 size={12} className="animate-spin" />}
          {isStream ? "Pick Quality…" : torrentMode ? "Add Torrent" : "Start Download"}
        </button>
      </footer>
    </Overlay>
  );
}

const inputClass =
  "flex-1 rounded-md border border-line bg-surface-0 px-2.5 py-1.5 text-xs text-slate-200 placeholder:text-slate-600 focus:border-accent focus:outline-none";

function Overlay({ children, onClose }: { children: React.ReactNode; onClose: () => void }) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/65 p-4"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="w-[520px] max-w-full rounded-xl border border-line-strong bg-surface-2 p-5 shadow-2xl"
      >
        {children}
      </div>
    </div>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <label className="mb-1 flex items-baseline gap-1.5 text-[11px] text-slate-400">
        {label}
        {hint && <span className="text-[10px] text-slate-600">{hint}</span>}
      </label>
      {children}
    </div>
  );
}

function SmallButton({
  onClick,
  Icon,
  label,
  disabled,
  spinning,
}: {
  onClick: () => void;
  Icon: typeof Radio;
  label: string;
  disabled?: boolean;
  spinning?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="flex flex-shrink-0 items-center gap-1.5 rounded-md border border-line bg-surface-3 px-2.5 py-1.5 text-[11px] font-medium text-slate-300 transition-colors hover:bg-surface-4 disabled:cursor-not-allowed disabled:opacity-40"
    >
      <Icon size={12} className={spinning ? "animate-spin" : ""} />
      {label}
    </button>
  );
}
