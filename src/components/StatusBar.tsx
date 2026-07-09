import { ArrowDown, ArrowUp, CalendarClock, Zap } from "lucide-react";
import { Download, SchedulerState, formatSpeed } from "../types";

interface Props {
  downloads: Download[];
  scheduler: SchedulerState | null;
}

export default function StatusBar({ downloads, scheduler }: Props) {
  const active = downloads.filter((d) => d.status === "downloading");
  const downSpeed = downloads.reduce((sum, d) => sum + d.speedBps, 0);
  const upSpeed = downloads.reduce((sum, d) => sum + d.uploadSpeedBps, 0);

  // `scheduler` is null until the first tick; treat that as "running normally"
  // rather than flashing a hold state on startup.
  const held = scheduler?.open === false;

  return (
    <footer className="flex h-6 flex-shrink-0 items-center gap-4 border-t border-line bg-surface-0 px-3 text-[11px] text-slate-500">
      <span className="flex items-center gap-1.5">
        {held ? (
          <>
            <CalendarClock size={11} className="text-warn" />
            <span className="text-warn">{scheduler?.reason ?? "Downloads on hold"}</span>
          </>
        ) : (
          <>
            <Zap size={11} className={active.length > 0 ? "text-accent" : "text-slate-600"} />
            <span>
              {active.length > 0
                ? `${active.length} download${active.length > 1 ? "s" : ""} in progress`
                : "Idle"}
            </span>
          </>
        )}
      </span>

      <div className="flex-1" />

      <span className="tnum flex items-center gap-1" title="Total download speed">
        <ArrowDown size={11} className="text-accent" />
        {formatSpeed(downSpeed)}
      </span>
      <span className="tnum flex items-center gap-1" title="Total upload speed">
        <ArrowUp size={11} className="text-up" />
        {formatSpeed(upSpeed)}
      </span>
    </footer>
  );
}
