import { useState, useEffect } from "react";
import {
  FolderOpen,
  Clock,
  ShieldCheck,
  Palette,
  Trash2,
  Save,
  Sparkles,
  CheckCircle2,
  XCircle,
  Loader2,
} from "lucide-react";
import { useDownloadStore } from "../store/downloadStore";

export default function SettingsPanel() {
  const { settings, updateSetting, clearHistory, testLlm } = useDownloadStore();

  const [savePath, setSavePath] = useState(settings.defaultSavePath);
  const [maxParallel, setMaxParallel] = useState(settings.maxParallelDownloads);
  const [maxSegments, setMaxSegments] = useState(settings.maxSegmentsPerDownload);
  const [speedLimit, setSpeedLimit] = useState(settings.speedLimitKbps);
  const [enableScheduler, setEnableScheduler] = useState(settings.enableScheduler);
  const [schedulerStart, setSchedulerStart] = useState(settings.schedulerStart);
  const [schedulerStop, setSchedulerStop] = useState(settings.schedulerStop);
  const [zeroLog, setZeroLog] = useState(settings.zeroLogMode);
  const [theme, setTheme] = useState(settings.theme);
  const [saved, setSaved] = useState(false);
  // LLM settings
  const [llmEnabled, setLlmEnabled]     = useState(settings.llmEnabled);
  const [llmEndpoint, setLlmEndpoint]   = useState(settings.llmEndpoint);
  const [llmModel, setLlmModel]         = useState(settings.llmModel);
  const [llmTesting, setLlmTesting]     = useState(false);
  const [llmTestResult, setLlmTestResult] = useState<{ ok: boolean; msg: string } | null>(null);

  useEffect(() => {
    setSavePath(settings.defaultSavePath);
    setMaxParallel(settings.maxParallelDownloads);
    setMaxSegments(settings.maxSegmentsPerDownload);
    setSpeedLimit(settings.speedLimitKbps);
    setEnableScheduler(settings.enableScheduler);
    setSchedulerStart(settings.schedulerStart);
    setSchedulerStop(settings.schedulerStop);
    setZeroLog(settings.zeroLogMode);
    setTheme(settings.theme);
    setLlmEnabled(settings.llmEnabled);
    setLlmEndpoint(settings.llmEndpoint);
    setLlmModel(settings.llmModel);
  }, [settings]);

  const handleSave = async () => {
    await updateSetting("default_save_path", savePath);
    await updateSetting("max_parallel_downloads", String(maxParallel));
    await updateSetting("max_segments_per_download", String(maxSegments));
    await updateSetting("speed_limit_kbps", String(speedLimit));
    await updateSetting("enable_scheduler", String(enableScheduler));
    await updateSetting("scheduler_start", schedulerStart);
    await updateSetting("scheduler_stop", schedulerStop);
    await updateSetting("zero_log_mode", String(zeroLog));
    await updateSetting("theme", theme);
    await updateSetting("llm_enabled", String(llmEnabled));
    await updateSetting("llm_endpoint", llmEndpoint);
    await updateSetting("llm_model", llmModel);
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  const handleTestLlm = async () => {
    setLlmTesting(true);
    setLlmTestResult(null);
    try {
      const reply = await testLlm(llmEndpoint, llmModel);
      setLlmTestResult({ ok: true, msg: `Connected — model replied: "${reply}"` });
    } catch (err) {
      setLlmTestResult({ ok: false, msg: String(err) });
    } finally {
      setLlmTesting(false);
    }
  };

  const handleClearHistory = async () => {
    if (confirm("Clear all history and remove completed / cancelled / failed downloads? This cannot be undone.")) {
      await clearHistory();
    }
  };

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-6 max-w-2xl">
      {/* Downloads Section */}
      <Section title="Downloads" icon={<FolderOpen size={15} />}>
        <div className="space-y-4">
          <FormRow label="Default Save Path">
            <div className="flex gap-2">
              <input
                type="text"
                value={savePath}
                onChange={(e) => setSavePath(e.target.value)}
                className="flex-1 bg-[#0f172a] border border-slate-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-blue-500"
              />
              <button className="px-3 py-2 bg-slate-700 hover:bg-slate-600 rounded-lg text-xs transition-colors">
                Browse
              </button>
            </div>
          </FormRow>

          <FormRow label={`Max Parallel Downloads: ${maxParallel}`}>
            <input
              type="range"
              min={1}
              max={10}
              value={maxParallel}
              onChange={(e) => setMaxParallel(Number(e.target.value))}
              className="w-full accent-blue-500"
            />
            <div className="flex justify-between text-xs text-slate-500 mt-1">
              <span>1</span>
              <span>10</span>
            </div>
          </FormRow>

          <FormRow label={`Segments per Download: ${maxSegments}`}>
            <input
              type="range"
              min={1}
              max={16}
              value={maxSegments}
              onChange={(e) => setMaxSegments(Number(e.target.value))}
              className="w-full accent-blue-500"
            />
            <div className="flex justify-between text-xs text-slate-500 mt-1">
              <span>1</span>
              <span>16 (max)</span>
            </div>
          </FormRow>

          <FormRow
            label={`Speed Limit: ${speedLimit === 0 ? "Unlimited" : `${speedLimit} KB/s`}`}
          >
            <input
              type="range"
              min={0}
              max={102400}
              step={512}
              value={speedLimit}
              onChange={(e) => setSpeedLimit(Number(e.target.value))}
              className="w-full accent-blue-500"
            />
            <div className="flex justify-between text-xs text-slate-500 mt-1">
              <span>Unlimited</span>
              <span>100 MB/s</span>
            </div>
          </FormRow>
        </div>
      </Section>

      {/* Scheduler Section */}
      <Section title="Scheduler" icon={<Clock size={15} />}>
        <div className="space-y-4">
          <Toggle
            label="Enable Smart Scheduler"
            description="Only download during specified time window"
            checked={enableScheduler}
            onChange={setEnableScheduler}
          />

          {enableScheduler && (
            <div className="grid grid-cols-2 gap-4">
              <FormRow label="Start Time">
                <input
                  type="time"
                  value={schedulerStart}
                  onChange={(e) => setSchedulerStart(e.target.value)}
                  className="w-full bg-[#0f172a] border border-slate-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-blue-500"
                />
              </FormRow>
              <FormRow label="Stop Time">
                <input
                  type="time"
                  value={schedulerStop}
                  onChange={(e) => setSchedulerStop(e.target.value)}
                  className="w-full bg-[#0f172a] border border-slate-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-blue-500"
                />
              </FormRow>
            </div>
          )}

          <div className="space-y-2">
            <p className="text-xs text-slate-500 font-medium">Conditions</p>
            {[
              "Only on unmetered connections",
              "Pause when CPU > 80%",
              "Pause when battery < 20%",
            ].map((condition) => (
              <label
                key={condition}
                className="flex items-center gap-2 text-sm text-slate-400 cursor-pointer"
              >
                <input
                  type="checkbox"
                  className="accent-blue-500"
                  disabled={!enableScheduler}
                />
                {condition}
              </label>
            ))}
          </div>
        </div>
      </Section>

      {/* Privacy Section */}
      <Section title="Privacy" icon={<ShieldCheck size={15} />}>
        <div className="space-y-4">
          <Toggle
            label="Zero-Log Mode"
            description="No download history is stored on disk"
            checked={zeroLog}
            onChange={setZeroLog}
          />
          <Toggle
            label="Encrypted Vault"
            description="Completed files are encrypted at rest (coming soon)"
            checked={false}
            onChange={() => {}}
            disabled
          />
          <div>
            <button
              onClick={handleClearHistory}
              className="flex items-center gap-2 px-4 py-2 bg-red-900/20 hover:bg-red-900/30 border border-red-800/40 text-red-400 rounded-lg text-sm transition-colors"
            >
              <Trash2 size={14} />
              Clear All History
            </button>
          </div>
        </div>
      </Section>

      {/* Appearance Section */}
      <Section title="Appearance" icon={<Palette size={15} />}>
        <div className="flex gap-3">
          {(["dark", "light", "system"] as const).map((t) => (
            <label
              key={t}
              className={`flex items-center gap-2 px-4 py-2 rounded-lg border cursor-pointer transition-colors ${
                theme === t
                  ? "border-blue-500 bg-blue-900/20 text-blue-400"
                  : "border-slate-700 text-slate-400 hover:border-slate-600"
              }`}
            >
              <input
                type="radio"
                name="theme"
                value={t}
                checked={theme === t}
                onChange={() => setTheme(t)}
                className="sr-only"
              />
              <span className="capitalize text-sm">{t}</span>
            </label>
          ))}
        </div>
      </Section>

      {/* AI Renaming Section */}
      <Section title="AI Filename Renaming" icon={<Sparkles size={15} />}>
        <div className="space-y-4">
          <Toggle
            label="Enable LLM Suggestions"
            description="Uses a local AI model to suggest cleaner filenames when adding downloads"
            checked={llmEnabled}
            onChange={setLlmEnabled}
          />

          <FormRow label="API Endpoint">
            <input
              type="text"
              value={llmEndpoint}
              onChange={(e) => setLlmEndpoint(e.target.value)}
              placeholder="http://localhost:11434/api/generate"
              className="w-full bg-[#0f172a] border border-slate-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-blue-500 font-mono"
            />
            <p className="text-[10px] text-slate-600 mt-1">
              Ollama: <code>http://localhost:11434/api/generate</code> ·
              OpenAI-compat: <code>http://localhost:1234/v1/chat/completions</code>
            </p>
          </FormRow>

          <FormRow label="Model">
            <input
              type="text"
              value={llmModel}
              onChange={(e) => setLlmModel(e.target.value)}
              placeholder="llama3.2:1b"
              className="w-full bg-[#0f172a] border border-slate-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-blue-500 font-mono"
            />
            <p className="text-[10px] text-slate-600 mt-1">
              Recommended fast models: <code>llama3.2:1b</code>, <code>phi3.5:mini</code>, <code>qwen2.5:1.5b</code>
            </p>
          </FormRow>

          <div className="flex items-center gap-3">
            <button
              onClick={handleTestLlm}
              disabled={llmTesting || !llmEndpoint}
              className="flex items-center gap-2 px-4 py-2 bg-slate-700 hover:bg-slate-600 disabled:opacity-50 disabled:cursor-not-allowed rounded-lg text-sm transition-colors"
            >
              {llmTesting ? (
                <Loader2 size={13} className="animate-spin" />
              ) : (
                <Sparkles size={13} />
              )}
              {llmTesting ? "Testing…" : "Test Connection"}
            </button>

            {llmTestResult && (
              <span
                className={`flex items-center gap-1.5 text-xs ${
                  llmTestResult.ok ? "text-green-400" : "text-red-400"
                }`}
              >
                {llmTestResult.ok ? (
                  <CheckCircle2 size={12} />
                ) : (
                  <XCircle size={12} />
                )}
                {llmTestResult.msg}
              </span>
            )}
          </div>
        </div>
      </Section>

      {/* Save Button */}
      <div>
        <button
          onClick={handleSave}
          className={`flex items-center gap-2 px-6 py-2.5 rounded-lg text-sm font-medium transition-all ${
            saved
              ? "bg-green-600 text-white"
              : "bg-blue-600 hover:bg-blue-500 text-white"
          }`}
        >
          <Save size={14} />
          {saved ? "Saved!" : "Save Settings"}
        </button>
      </div>
    </div>
  );
}

function Section({
  title,
  icon,
  children,
}: {
  title: string;
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="bg-[#1e293b] rounded-xl border border-slate-700/50 p-5">
      <h3 className="text-sm font-semibold text-slate-200 mb-4 flex items-center gap-2">
        <span className="text-blue-400">{icon}</span>
        {title}
      </h3>
      {children}
    </div>
  );
}

function FormRow({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <label className="block text-xs text-slate-400 mb-1.5">{label}</label>
      {children}
    </div>
  );
}

function Toggle({
  label,
  description,
  checked,
  onChange,
  disabled = false,
}: {
  label: string;
  description: string;
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <div className="flex items-start justify-between gap-4">
      <div>
        <p className={`text-sm ${disabled ? "text-slate-500" : "text-slate-300"}`}>{label}</p>
        <p className="text-xs text-slate-500 mt-0.5">{description}</p>
      </div>
      <button
        onClick={() => !disabled && onChange(!checked)}
        disabled={disabled}
        className={`relative flex-shrink-0 w-10 h-6 rounded-full transition-colors ${
          checked ? "bg-blue-600" : "bg-slate-700"
        } ${disabled ? "opacity-40 cursor-not-allowed" : "cursor-pointer"}`}
      >
        <span
          className={`absolute top-1 left-1 w-4 h-4 bg-white rounded-full transition-transform ${
            checked ? "translate-x-4" : ""
          }`}
        />
      </button>
    </div>
  );
}
