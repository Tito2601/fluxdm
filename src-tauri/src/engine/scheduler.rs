//! Download scheduler.
//!
//! Decides, moment to moment, whether downloads are allowed to run. Three
//! independent guards can each veto:
//!
//! - **Time window** — only download between `scheduler_start` and `scheduler_stop`.
//! - **CPU load** — hold off while the machine is busy.
//! - **Battery** — hold off on low battery, unless plugged in.
//!
//! The guards are independent on purpose: wanting "never download below 20%
//! battery" has nothing to do with wanting "only download overnight", so each
//! has its own toggle rather than hanging off one master switch.
//!
//! The verdict is published through [`DownloadControl::set_gate_open`]. Running
//! downloads poll that gate and stop cleanly; queued ones simply aren't started.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Local, NaiveTime};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::engine::queue::DownloadQueue;
use crate::storage::db::Database;

/// How often the scheduler re-reads settings and re-evaluates its guards.
const TICK: std::time::Duration = std::time::Duration::from_secs(10);

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct SchedulerConfig {
    pub window_enabled: bool,
    pub start:          NaiveTime,
    pub stop:           NaiveTime,

    pub pause_on_high_cpu: bool,
    pub cpu_threshold:     f32, // percent, 0-100

    pub pause_on_low_battery: bool,
    pub battery_threshold:    f32, // percent, 0-100
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            window_enabled:       false,
            start:                NaiveTime::from_hms_opt(2, 0, 0).unwrap(),
            stop:                 NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
            pause_on_high_cpu:    false,
            cpu_threshold:        80.0,
            pause_on_low_battery: false,
            battery_threshold:    20.0,
        }
    }
}

impl SchedulerConfig {
    /// Build from the `settings` key/value table. Unparseable or missing values
    /// fall back to the default rather than disabling the scheduler outright.
    pub fn from_settings(s: &HashMap<String, String>) -> Self {
        let d = Self::default();
        let flag = |k: &str, fallback: bool| {
            s.get(k).map(|v| v == "true").unwrap_or(fallback)
        };
        let num = |k: &str, fallback: f32| {
            s.get(k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(fallback)
        };
        let time = |k: &str, fallback: NaiveTime| {
            s.get(k).and_then(|v| parse_hhmm(v)).unwrap_or(fallback)
        };

        Self {
            window_enabled:       flag("enable_scheduler", d.window_enabled),
            start:                time("scheduler_start", d.start),
            stop:                 time("scheduler_stop", d.stop),
            pause_on_high_cpu:    flag("scheduler_pause_on_high_cpu", d.pause_on_high_cpu),
            cpu_threshold:        num("scheduler_cpu_threshold", d.cpu_threshold),
            pause_on_low_battery: flag("scheduler_pause_on_low_battery", d.pause_on_low_battery),
            battery_threshold:    num("scheduler_battery_threshold", d.battery_threshold),
        }
    }
}

/// Parse `"HH:MM"` (24-hour). Returns `None` on anything malformed.
pub fn parse_hhmm(raw: &str) -> Option<NaiveTime> {
    let (h, m) = raw.trim().split_once(':')?;
    NaiveTime::from_hms_opt(h.trim().parse().ok()?, m.trim().parse().ok()?, 0)
}

/// Is `now` inside the window `[start, stop)`?
///
/// When `start > stop` the window wraps past midnight (e.g. 22:00 → 06:00).
/// A zero-length window (`start == stop`) means "no restriction" rather than
/// "never", since a window that can never open would silently wedge the queue.
pub fn within_window(now: NaiveTime, start: NaiveTime, stop: NaiveTime) -> bool {
    if start == stop {
        true
    } else if start < stop {
        now >= start && now < stop
    } else {
        now >= start || now < stop
    }
}

// ── Decision ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum BlockReason {
    OutsideWindow,
    CpuBusy(f32),
    BatteryLow(f32),
}

impl BlockReason {
    /// Short human-readable text for the UI status bar.
    pub fn message(&self) -> String {
        match self {
            BlockReason::OutsideWindow  => "Outside scheduled hours".to_string(),
            BlockReason::CpuBusy(pct)   => format!("CPU busy ({:.0}%)", pct),
            BlockReason::BatteryLow(p)  => format!("Battery low ({:.0}%)", p),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GateDecision {
    Allow,
    Block(BlockReason),
}

// ── Scheduler ─────────────────────────────────────────────────────────────────

pub struct Scheduler {
    /// Kept alive across ticks: CPU usage is a delta between two refreshes, so a
    /// fresh `System` every tick would always report 0%.
    sys: sysinfo::System,
}

impl Scheduler {
    pub fn new() -> Self {
        Self { sys: sysinfo::System::new() }
    }

    /// Evaluate every guard. The first veto wins.
    pub fn evaluate(&mut self, cfg: &SchedulerConfig) -> GateDecision {
        if cfg.window_enabled && !within_window(Local::now().time(), cfg.start, cfg.stop) {
            return GateDecision::Block(BlockReason::OutsideWindow);
        }

        if cfg.pause_on_high_cpu {
            let usage = self.cpu_usage();
            if usage > cfg.cpu_threshold {
                return GateDecision::Block(BlockReason::CpuBusy(usage));
            }
        }

        if cfg.pause_on_low_battery {
            if let Some(pct) = battery_charge_percent() {
                if pct < cfg.battery_threshold {
                    return GateDecision::Block(BlockReason::BatteryLow(pct));
                }
            }
        }

        GateDecision::Allow
    }

    /// System-wide CPU usage, 0-100.
    ///
    /// `sysinfo` derives this from the delta since the previous refresh. The
    /// scheduler ticks far less often than `MINIMUM_CPU_UPDATE_INTERVAL`, so a
    /// single refresh per tick always spans a valid measurement window.
    fn cpu_usage(&mut self) -> f32 {
        self.sys.refresh_cpu_usage();
        self.sys.global_cpu_usage()
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Battery charge as a percentage, or `None` when there is no battery, the
/// battery can't be read, or the machine is running on external power.
///
/// Returning `None` while charging is what lets the battery guard mean
/// "don't drain the battery" rather than "don't download on a laptop".
fn battery_charge_percent() -> Option<f32> {
    use starship_battery::{Manager, State};

    let manager = Manager::new().ok()?;
    let battery = manager.batteries().ok()?.next()?.ok()?;

    match battery.state() {
        State::Charging | State::Full => None,
        _ => Some(battery.state_of_charge().value * 100.0),
    }
}

// ── Background task ───────────────────────────────────────────────────────────

/// Poll the guards forever, opening and closing the download gate.
///
/// On close, the downloads that were running are tagged as scheduler-paused; on
/// reopen they are re-queued. Downloads the *user* paused are left alone — the
/// scheduler must never resume something the user deliberately stopped.
pub async fn run_scheduler(
    app:   AppHandle,
    db:    Arc<Mutex<Database>>,
    queue: Arc<DownloadQueue>,
) {
    let mut scheduler = Scheduler::new();
    let control       = queue.control();

    info!("Scheduler started (tick = {}s)", TICK.as_secs());

    loop {
        tokio::time::sleep(TICK).await;

        let settings = match db.lock().await.get_all_settings() {
            Ok(s) => s,
            Err(e) => {
                warn!("Scheduler could not read settings: {}", e);
                continue;
            }
        };

        let cfg      = SchedulerConfig::from_settings(&settings);
        let decision = scheduler.evaluate(&cfg);
        let was_open = control.gate_open();

        match (&decision, was_open) {
            // Gate closing.
            (GateDecision::Block(reason), true) => {
                info!("Scheduler closing download gate: {}", reason.message());

                // Tag what's running now so it can be restarted later. The
                // downloads themselves notice the closed gate and stop on their own.
                for id in queue.active_ids().await {
                    control.mark_auto_paused(&id);
                }
                control.set_gate_open(false);
                emit_state(&app, false, Some(reason.message()));
            }

            // Gate opening.
            (GateDecision::Allow, false) => {
                info!("Scheduler reopening download gate");
                control.set_gate_open(true);

                let resumed = control.take_auto_paused();
                if !resumed.is_empty() {
                    requeue(&db, &queue, &resumed).await;
                    info!("Scheduler re-queued {} download(s)", resumed.len());
                }
                emit_state(&app, true, None);
            }

            // No change.
            (GateDecision::Block(reason), false) => {
                debug!("Gate still closed: {}", reason.message());
            }
            (GateDecision::Allow, true) => {}
        }
    }
}

/// Reload the given downloads from the DB and push them back onto the queue.
///
/// Skips anything that reached a terminal state while the gate was shut: a
/// download the user cancelled between the close and the reopen must stay
/// cancelled, and a torrent is owned by its own session rather than the queue.
async fn requeue(db: &Arc<Mutex<Database>>, queue: &Arc<DownloadQueue>, ids: &[String]) {
    use crate::engine::downloader::{DownloadKind, DownloadStatus};

    let jobs = {
        let db_lock = db.lock().await;
        match db_lock.get_all_downloads() {
            Ok(all) => {
                let selected: Vec<_> = all
                    .into_iter()
                    .filter(|j| ids.contains(&j.id))
                    .filter(|j| j.kind != DownloadKind::Torrent)
                    .filter(|j| matches!(j.status, DownloadStatus::Paused | DownloadStatus::Queued))
                    .collect();
                for job in &selected {
                    let _ = db_lock.update_download_status(&job.id, "queued");
                }
                selected
            }
            Err(e) => {
                warn!("Scheduler could not reload downloads: {}", e);
                return;
            }
        }
    };

    for mut job in jobs {
        job.status = DownloadStatus::Queued;
        queue.enqueue(job).await;
    }
}

fn emit_state(app: &AppHandle, open: bool, reason: Option<String>) {
    let _ = app.emit(
        "scheduler_state",
        serde_json::json!({ "open": open, "reason": reason }),
    );
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn parses_hhmm() {
        assert_eq!(parse_hhmm("02:00"), Some(t(2, 0)));
        assert_eq!(parse_hhmm("23:59"), Some(t(23, 59)));
        assert_eq!(parse_hhmm(" 7:05 "), Some(t(7, 5)));
    }

    #[test]
    fn rejects_malformed_hhmm() {
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("02:60"), None);
        assert_eq!(parse_hhmm("0200"), None);
        assert_eq!(parse_hhmm(""), None);
        assert_eq!(parse_hhmm("aa:bb"), None);
    }

    #[test]
    fn daytime_window_is_half_open() {
        let (start, stop) = (t(2, 0), t(7, 0));
        assert!(!within_window(t(1, 59), start, stop));
        assert!(within_window(t(2, 0), start, stop), "start is inclusive");
        assert!(within_window(t(6, 59), start, stop));
        assert!(!within_window(t(7, 0), start, stop), "stop is exclusive");
    }

    #[test]
    fn window_wraps_past_midnight() {
        let (start, stop) = (t(22, 0), t(6, 0));
        assert!(within_window(t(22, 0), start, stop));
        assert!(within_window(t(23, 59), start, stop));
        assert!(within_window(t(0, 0), start, stop), "midnight is inside");
        assert!(within_window(t(5, 59), start, stop));
        assert!(!within_window(t(6, 0), start, stop));
        assert!(!within_window(t(12, 0), start, stop));
    }

    #[test]
    fn zero_length_window_never_blocks() {
        let same = t(3, 0);
        assert!(within_window(t(3, 0), same, same));
        assert!(within_window(t(15, 0), same, same));
    }

    #[test]
    fn disabled_window_allows_any_time() {
        let cfg = SchedulerConfig { window_enabled: false, ..Default::default() };
        assert_eq!(Scheduler::new().evaluate(&cfg), GateDecision::Allow);
    }

    #[test]
    fn config_falls_back_on_garbage_values() {
        let mut s = HashMap::new();
        s.insert("enable_scheduler".into(), "true".into());
        s.insert("scheduler_start".into(), "not-a-time".into());
        s.insert("scheduler_cpu_threshold".into(), "abc".into());

        let cfg = SchedulerConfig::from_settings(&s);
        let d   = SchedulerConfig::default();

        assert!(cfg.window_enabled, "valid keys still apply");
        assert_eq!(cfg.start, d.start, "bad time falls back to default");
        assert_eq!(cfg.cpu_threshold, d.cpu_threshold, "bad number falls back");
    }

    #[test]
    fn config_reads_all_keys() {
        let mut s = HashMap::new();
        s.insert("enable_scheduler".into(), "true".into());
        s.insert("scheduler_start".into(), "22:30".into());
        s.insert("scheduler_stop".into(), "06:15".into());
        s.insert("scheduler_pause_on_high_cpu".into(), "true".into());
        s.insert("scheduler_cpu_threshold".into(), "65".into());
        s.insert("scheduler_pause_on_low_battery".into(), "true".into());
        s.insert("scheduler_battery_threshold".into(), "35".into());

        let cfg = SchedulerConfig::from_settings(&s);
        assert_eq!(cfg.start, t(22, 30));
        assert_eq!(cfg.stop, t(6, 15));
        assert!(cfg.pause_on_high_cpu);
        assert_eq!(cfg.cpu_threshold, 65.0);
        assert!(cfg.pause_on_low_battery);
        assert_eq!(cfg.battery_threshold, 35.0);
    }

    #[test]
    fn cpu_guard_blocks_above_threshold() {
        // Threshold of -1% is unreachable from below, so any reading trips it.
        let cfg = SchedulerConfig {
            pause_on_high_cpu: true,
            cpu_threshold: -1.0,
            ..Default::default()
        };
        assert!(matches!(
            Scheduler::new().evaluate(&cfg),
            GateDecision::Block(BlockReason::CpuBusy(_))
        ));
    }

    #[test]
    fn cpu_guard_allows_below_threshold() {
        let cfg = SchedulerConfig {
            pause_on_high_cpu: true,
            cpu_threshold: 1000.0, // no machine reports 1000% of total capacity
            ..Default::default()
        };
        assert_eq!(Scheduler::new().evaluate(&cfg), GateDecision::Allow);
    }
}
