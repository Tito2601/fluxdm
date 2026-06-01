import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  PieChart,
  Pie,
  Cell,
  Legend,
} from "recharts";
import { HardDrive, Download, Zap, Activity, AlertTriangle } from "lucide-react";
import {
  AnalyticsData,
  formatBytes,
  formatSpeed,
  CATEGORY_COLORS,
} from "../types";
import { useDownloadStore } from "../store/downloadStore";

const CATEGORY_ORDER = [
  "videos",
  "music",
  "documents",
  "software",
  "images",
  "archives",
  "other",
];

export default function AnalyticsDashboard() {
  const [analytics, setAnalytics] = useState<AnalyticsData | null>(null);
  const [loading, setLoading] = useState(true);
  const { downloads } = useDownloadStore();

  const activeCount = downloads.filter((d) => d.status === "downloading").length;

  useEffect(() => {
    const load = async () => {
      try {
        const data = await invoke<AnalyticsData>("cmd_get_analytics");
        setAnalytics(data);
      } catch (err) {
        console.error("Failed to load analytics:", err);
      } finally {
        setLoading(false);
      }
    };

    load();
    const interval = setInterval(load, 5000);
    return () => clearInterval(interval);
  }, []);

  // Build pie chart data from categories
  const pieData = CATEGORY_ORDER
    .filter((cat) => (analytics?.downloadsByCategory[cat] ?? 0) > 0)
    .map((cat) => ({
      name: cat.charAt(0).toUpperCase() + cat.slice(1),
      value: analytics?.downloadsByCategory[cat] ?? 0,
      color: CATEGORY_COLORS[cat] ?? "#6b7280",
    }));

  // Format speed history for chart
  const speedData = (analytics?.speedHistory ?? []).map((point) => ({
    time: new Date(point.timestamp * 1000).toLocaleTimeString("en", {
      hour: "2-digit",
      minute: "2-digit",
    }),
    speedMbps: parseFloat((point.speedBps / 1_000_000).toFixed(2)),
  }));

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-slate-500 text-sm animate-pulse">Loading analytics...</div>
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-6">
      {/* Stat Cards */}
      <div className="grid grid-cols-4 gap-4">
        <StatCard
          icon={<HardDrive size={18} className="text-blue-400" />}
          label="Total Downloaded"
          value={formatBytes(analytics?.totalDownloadedBytes ?? 0)}
          sublabel="all time"
        />
        <StatCard
          icon={<Download size={18} className="text-green-400" />}
          label="Downloads Today"
          value={String(analytics?.downloadsToday ?? 0)}
          sublabel="completed"
        />
        <StatCard
          icon={<Zap size={18} className="text-yellow-400" />}
          label="Avg Speed"
          value={formatSpeed(analytics?.avgSpeedBps ?? 0)}
          sublabel="overall average"
        />
        <StatCard
          icon={<Activity size={18} className="text-purple-400" />}
          label="Active Now"
          value={String(activeCount)}
          sublabel={`of ${downloads.length} total`}
        />
      </div>

      <div className="grid grid-cols-5 gap-4">
        {/* Speed History Chart */}
        <div className="col-span-3 bg-[#1e293b] rounded-xl border border-slate-700/50 p-4">
          <h3 className="text-sm font-medium text-slate-300 mb-4 flex items-center gap-2">
            <Activity size={14} />
            Speed History
          </h3>
          {speedData.length > 0 ? (
            <ResponsiveContainer width="100%" height={200}>
              <LineChart data={speedData}>
                <CartesianGrid strokeDasharray="3 3" stroke="#1e293b" />
                <XAxis
                  dataKey="time"
                  tick={{ fill: "#64748b", fontSize: 11 }}
                  tickLine={false}
                  axisLine={false}
                />
                <YAxis
                  tick={{ fill: "#64748b", fontSize: 11 }}
                  tickLine={false}
                  axisLine={false}
                  tickFormatter={(v: number) => `${v}M`}
                />
                <Tooltip
                  contentStyle={{
                    background: "#0f172a",
                    border: "1px solid #334155",
                    borderRadius: "8px",
                    fontSize: "12px",
                  }}
                  formatter={(v: number) => [`${v} MB/s`, "Speed"]}
                />
                <Line
                  type="monotone"
                  dataKey="speedMbps"
                  stroke="#3b82f6"
                  strokeWidth={2}
                  dot={false}
                  activeDot={{ r: 4, fill: "#3b82f6" }}
                />
              </LineChart>
            </ResponsiveContainer>
          ) : (
            <div className="flex items-center justify-center h-[200px] text-slate-600 text-sm">
              No speed history yet
            </div>
          )}
        </div>

        {/* Category Breakdown */}
        <div className="col-span-2 bg-[#1e293b] rounded-xl border border-slate-700/50 p-4">
          <h3 className="text-sm font-medium text-slate-300 mb-4 flex items-center gap-2">
            <Download size={14} />
            By Category
          </h3>
          {pieData.length > 0 ? (
            <ResponsiveContainer width="100%" height={200}>
              <PieChart>
                <Pie
                  data={pieData}
                  cx="50%"
                  cy="50%"
                  innerRadius={50}
                  outerRadius={75}
                  paddingAngle={3}
                  dataKey="value"
                >
                  {pieData.map((entry, index) => (
                    <Cell key={index} fill={entry.color} />
                  ))}
                </Pie>
                <Tooltip
                  contentStyle={{
                    background: "#0f172a",
                    border: "1px solid #334155",
                    borderRadius: "8px",
                    fontSize: "12px",
                  }}
                />
                <Legend
                  iconSize={8}
                  formatter={(value) => (
                    <span style={{ color: "#94a3b8", fontSize: "11px" }}>
                      {value}
                    </span>
                  )}
                />
              </PieChart>
            </ResponsiveContainer>
          ) : (
            <div className="flex items-center justify-center h-[200px] text-slate-600 text-sm">
              No downloads yet
            </div>
          )}
        </div>
      </div>

      {/* ISP Throttling Detector */}
      <ThrottlingDetector analytics={analytics} />
    </div>
  );
}

function StatCard({
  icon,
  label,
  value,
  sublabel,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  sublabel: string;
}) {
  return (
    <div className="bg-[#1e293b] rounded-xl border border-slate-700/50 p-4">
      <div className="flex items-center gap-2 mb-2">
        {icon}
        <span className="text-xs text-slate-500">{label}</span>
      </div>
      <div className="text-xl font-bold text-slate-100">{value}</div>
      <div className="text-xs text-slate-500 mt-0.5">{sublabel}</div>
    </div>
  );
}

function ThrottlingDetector({ analytics }: { analytics: AnalyticsData | null }) {
  if (!analytics || analytics.speedHistory.length < 10) return null;

  const history = analytics.speedHistory;
  const avgSpeed = history.reduce((s, p) => s + p.speedBps, 0) / history.length;

  // Check evening hours (7pm-9pm = 19-21)
  const eveningPoints = history.filter((p) => {
    const hour = new Date(p.timestamp * 1000).getHours();
    return hour >= 19 && hour <= 21;
  });

  if (eveningPoints.length === 0) return null;

  const eveningAvg = eveningPoints.reduce((s, p) => s + p.speedBps, 0) / eveningPoints.length;
  const isThrottled = eveningAvg < avgSpeed * 0.3;

  if (!isThrottled) return null;

  return (
    <div className="bg-yellow-900/20 border border-yellow-800/40 rounded-xl p-4 flex items-start gap-3">
      <AlertTriangle size={16} className="text-yellow-400 mt-0.5 flex-shrink-0" />
      <div>
        <p className="text-sm font-medium text-yellow-400">
          ISP Throttling Detected
        </p>
        <p className="text-xs text-slate-400 mt-1">
          Your evening speeds (7–9 PM) are{" "}
          <strong>
            {Math.round((1 - eveningAvg / avgSpeed) * 100)}% slower
          </strong>{" "}
          than your daily average. This is a strong indicator of ISP throttling.
          Consider scheduling large downloads during off-peak hours using the
          Scheduler in Settings.
        </p>
      </div>
    </div>
  );
}
