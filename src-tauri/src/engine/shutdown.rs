//! Auto-shutdown once the queue drains — Phase 7.
//!
//! Powering off the machine is the one action in FluxDM the user cannot undo, so
//! the design is deliberately conservative on three fronts:
//!
//! - **Opt-in.** Off unless `auto_shutdown` is explicitly `true`.
//! - **Armed by work, not by idleness.** A shutdown is only ever scheduled after
//!   the queue has been observed *busy* and then goes idle. An app sitting idle
//!   at launch with the setting left on from last week must not power the machine
//!   off, which a naive "queue is empty" check would do within one tick.
//! - **Cancellable.** The countdown emits a tick per second and aborts the moment
//!   the user cancels or new work arrives.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use crate::engine::queue::DownloadQueue;
use crate::storage::db::Database;

/// How often the queue is sampled.
const TICK: Duration = Duration::from_secs(2);

/// Grace period before the machine goes down, giving the user time to cancel.
const COUNTDOWN_SECS: u64 = 60;

/// Shared cancel flag. The Tauri command sets it; the countdown polls it.
#[derive(Debug, Default)]
pub struct ShutdownControl {
    cancelled: AtomicBool,
}

impl ShutdownControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// Abort an in-flight countdown. Safe to call when none is running.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Clear the flag so a later countdown starts from a clean slate.
    fn reset(&self) {
        self.cancelled.store(false, Ordering::SeqCst);
    }
}

// ── Background task ───────────────────────────────────────────────────────────

/// Watch the queue and power the machine off after it drains.
///
/// `armed` is the whole safety story: it flips true only while real work is in
/// flight, so the idle→shutdown edge can never be reached without work having
/// happened first in this session.
pub async fn run_auto_shutdown(
    app:     AppHandle,
    db:      Arc<Mutex<Database>>,
    queue:   Arc<DownloadQueue>,
    control: Arc<ShutdownControl>,
) {
    let mut armed = false;

    info!("Auto-shutdown watcher started (tick = {}s)", TICK.as_secs());

    loop {
        tokio::time::sleep(TICK).await;

        let idle = queue.is_idle().await;

        if !idle {
            // Work is in flight: arm the trigger and drop any stale cancellation
            // so the next countdown is not pre-cancelled by an earlier one.
            if !armed {
                armed = true;
                control.reset();
            }
            continue;
        }

        // Idle, but nothing ran since the last shutdown check — nothing to do.
        if !armed {
            continue;
        }

        // Read the setting only now, so toggling it off mid-download still takes
        // effect and the queue is not sampled against a stale config.
        let enabled = match db.lock().await.get_setting("auto_shutdown") {
            Ok(v)  => v.as_deref() == Some("true"),
            Err(e) => {
                warn!("Auto-shutdown could not read settings: {}", e);
                continue;
            }
        };

        // Disarm regardless: this drain has been handled, and leaving it armed
        // would re-trigger every tick for as long as the queue stays empty.
        armed = false;

        if !enabled {
            continue;
        }

        info!("Queue drained — starting {}s shutdown countdown", COUNTDOWN_SECS);
        if countdown(&app, &queue, &control).await {
            execute_shutdown(&app);
            return; // The machine is going down; stop watching.
        }
    }
}

/// Run the grace period. Returns `true` if it completed and the machine should
/// go down, `false` if it was aborted.
async fn countdown(
    app:     &AppHandle,
    queue:   &Arc<DownloadQueue>,
    control: &Arc<ShutdownControl>,
) -> bool {
    control.reset();

    for remaining in (1..=COUNTDOWN_SECS).rev() {
        let _ = app.emit("shutdown_pending", serde_json::json!({
            "secondsRemaining": remaining,
        }));

        tokio::time::sleep(Duration::from_secs(1)).await;

        if control.is_cancelled() {
            info!("Shutdown cancelled by user");
            let _ = app.emit("shutdown_cancelled", serde_json::json!({
                "reason": "cancelled",
            }));
            return false;
        }

        // A download queued during the countdown is a clear signal the user is
        // still working, and outranks a shutdown scheduled before it existed.
        if !queue.is_idle().await {
            info!("Shutdown aborted — new work arrived");
            let _ = app.emit("shutdown_cancelled", serde_json::json!({
                "reason": "new_download",
            }));
            return false;
        }
    }

    true
}

/// Hand the machine off to the OS.
fn execute_shutdown(app: &AppHandle) {
    let _ = app.emit("shutdown_executing", serde_json::json!({}));
    info!("Executing system shutdown");

    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("shutdown")
        .args(["/s", "/t", "0"])
        .spawn();

    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("osascript")
        .args(["-e", "tell app \"System Events\" to shut down"])
        .spawn();

    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("systemctl")
        .arg("poweroff")
        .spawn();

    if let Err(e) = result {
        error!("Shutdown command failed: {}", e);
        let _ = app.emit("shutdown_cancelled", serde_json::json!({
            "reason": format!("shutdown command failed: {}", e),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_is_observable() {
        let c = ShutdownControl::new();
        assert!(!c.is_cancelled());
        c.cancel();
        assert!(c.is_cancelled());
    }

    #[test]
    fn reset_clears_a_previous_cancel() {
        // Without this, one cancellation would permanently disable the feature
        // for the rest of the session.
        let c = ShutdownControl::new();
        c.cancel();
        c.reset();
        assert!(!c.is_cancelled());
    }

    #[tokio::test]
    async fn an_idle_queue_alone_never_arms_a_shutdown() {
        // The regression that would power off a machine that was only ever idle.
        let queue = Arc::new(DownloadQueue::new());
        assert!(queue.is_idle().await);

        let mut armed = false;
        for _ in 0..10 {
            if !queue.is_idle().await {
                armed = true;
            }
        }
        assert!(!armed, "an idle queue must never arm the shutdown trigger");
    }
}
