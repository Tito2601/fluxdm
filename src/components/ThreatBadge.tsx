import { useState } from "react";
import { ShieldAlert, ShieldCheck, ChevronDown, X, Loader2 } from "lucide-react";
import { Download, ThreatAnalysis, ThreatFactor } from "../types";
import { useDownloadStore } from "../store/downloadStore";

interface ThreatBadgeProps {
  download: Download;
  showSafe?: boolean;
}

/**
 * Displays a threat warning badge.
 * Score > 60 → Warning. Score > 80 → Danger.
 * Click to expand a factor-by-factor breakdown (fetched on demand).
 */
export default function ThreatBadge({ download, showSafe = false }: ThreatBadgeProps) {
  const score = download.threatScore;
  const [expanded, setExpanded]     = useState(false);
  const [analysis, setAnalysis]     = useState<ThreatAnalysis | null>(null);
  const [loading,  setLoading]      = useState(false);

  const { getThreatDetails } = useDownloadStore();

  if (score <= 60) {
    if (!showSafe) return null;
    return (
      <span className="inline-flex items-center gap-1 text-xs bg-green-900/30 text-green-400 border border-green-800 px-2 py-0.5 rounded-full">
        <ShieldCheck size={10} />
        Safe
      </span>
    );
  }

  const isDanger = score > 80;

  const handleClick = async (e: React.MouseEvent) => {
    e.stopPropagation();
    if (expanded) { setExpanded(false); return; }

    setExpanded(true);
    if (!analysis) {
      setLoading(true);
      try {
        const result = await getThreatDetails(download);
        setAnalysis(result);
      } catch {
        // silently ignore; badge still shows
      } finally {
        setLoading(false);
      }
    }
  };

  return (
    <span className="relative inline-block">
      {/* Badge button */}
      <button
        onClick={handleClick}
        className={`inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded-full border transition-colors ${
          isDanger
            ? "bg-red-900/30 text-red-400 border-red-800 hover:bg-red-900/50"
            : "bg-yellow-900/30 text-yellow-400 border-yellow-800 hover:bg-yellow-900/50"
        }`}
        title={`Threat score: ${score}/100 — click for details`}
      >
        <ShieldAlert size={10} />
        {isDanger ? "HIGH RISK" : "Suspicious"}
        <span className="font-mono opacity-70">{score}</span>
        <ChevronDown size={9} className={`transition-transform ${expanded ? "rotate-180" : ""}`} />
      </button>

      {/* Dropdown */}
      {expanded && (
        <div
          className="absolute left-0 top-full mt-1 z-50 w-72 bg-[#1e293b] border border-slate-700 rounded-xl shadow-2xl p-3"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div className="flex items-center justify-between mb-2">
            <span className={`text-xs font-semibold ${isDanger ? "text-red-400" : "text-yellow-400"}`}>
              {isDanger ? "⚠ High-risk file" : "⚠ Suspicious file"} — score {score}/100
            </span>
            <button onClick={() => setExpanded(false)} className="text-slate-500 hover:text-white">
              <X size={12} />
            </button>
          </div>

          {loading && (
            <div className="flex items-center gap-2 text-slate-500 text-xs py-2">
              <Loader2 size={12} className="animate-spin" />
              Analysing…
            </div>
          )}

          {analysis && !loading && (
            <div className="space-y-1.5">
              {analysis.factors.map((f, i) => (
                <FactorRow key={i} factor={f} />
              ))}
            </div>
          )}

          <p className="mt-2 text-xs text-slate-600 border-t border-slate-700 pt-2">
            This is heuristic-based scoring. Always verify files from unfamiliar sources.
          </p>
        </div>
      )}
    </span>
  );
}

function FactorRow({ factor }: { factor: ThreatFactor }) {
  const isRisk    = factor.delta > 0;
  const color     = isRisk ? "text-red-400" : "text-green-400";
  const deltaStr  = isRisk ? `+${factor.delta}` : String(factor.delta);

  return (
    <div className="flex items-start gap-2">
      <span className={`font-mono text-xs w-8 text-right flex-shrink-0 mt-0.5 ${color}`}>
        {deltaStr}
      </span>
      <div className="min-w-0">
        <div className={`text-xs font-medium ${color}`}>{factor.name}</div>
        <div className="text-xs text-slate-500 leading-tight">{factor.reason}</div>
      </div>
    </div>
  );
}
