use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

/// Why a running download was asked to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interrupt {
    /// User paused it, or the scheduler closed the gate.
    Paused,
    /// User cancelled it — partial data should be discarded.
    Cancelled,
}

/// Cooperative stop signals shared between the queue, the scheduler and the
/// in-flight download tasks.
///
/// Downloads poll [`Self::interrupt_reason`] between chunks. This is the only
/// way a *running* transfer can be stopped: dropping the task would leave the
/// segment temp files and the DB rows inconsistent, so instead each worker
/// unwinds cleanly and reports [`SegmentStatus::Interrupted`].
///
/// Reads vastly outnumber writes (once per chunk vs. once per user action), so
/// this uses `RwLock` rather than an async mutex — polling must never await.
pub struct DownloadControl {
    paused:    RwLock<HashSet<String>>,
    cancelled: RwLock<HashSet<String>>,
    /// Downloads the scheduler stopped on its own. Re-queued when the gate reopens.
    auto_paused: RwLock<HashSet<String>>,
    /// `false` while the scheduler forbids downloading (outside its window,
    /// CPU too busy, battery too low).
    gate_open: AtomicBool,
}

/// `RwLock` poisoning only means some other thread panicked while holding the
/// lock. The sets are plain `HashSet<String>` with no cross-field invariants,
/// so the data is still sound and we can safely carry on.
macro_rules! read_or_recover {
    ($lock:expr) => {
        $lock.read().unwrap_or_else(|e| e.into_inner())
    };
}
macro_rules! write_or_recover {
    ($lock:expr) => {
        $lock.write().unwrap_or_else(|e| e.into_inner())
    };
}

impl DownloadControl {
    pub fn new() -> Self {
        Self {
            paused:      RwLock::new(HashSet::new()),
            cancelled:   RwLock::new(HashSet::new()),
            auto_paused: RwLock::new(HashSet::new()),
            gate_open:   AtomicBool::new(true),
        }
    }

    // ── Pause / resume ────────────────────────────────────────────────────

    pub fn pause(&self, id: &str) {
        write_or_recover!(self.paused).insert(id.to_string());
    }

    pub fn unpause(&self, id: &str) {
        write_or_recover!(self.paused).remove(id);
        write_or_recover!(self.auto_paused).remove(id);
    }

    pub fn is_paused(&self, id: &str) -> bool {
        read_or_recover!(self.paused).contains(id)
    }

    // ── Cancel ────────────────────────────────────────────────────────────

    pub fn cancel(&self, id: &str) {
        write_or_recover!(self.cancelled).insert(id.to_string());
    }

    pub fn is_cancelled(&self, id: &str) -> bool {
        read_or_recover!(self.cancelled).contains(id)
    }

    /// Drop all signals for a download that has reached a terminal state.
    pub fn clear(&self, id: &str) {
        write_or_recover!(self.paused).remove(id);
        write_or_recover!(self.cancelled).remove(id);
        write_or_recover!(self.auto_paused).remove(id);
    }

    // ── Scheduler gate ────────────────────────────────────────────────────

    pub fn gate_open(&self) -> bool {
        self.gate_open.load(Ordering::Relaxed)
    }

    pub fn set_gate_open(&self, open: bool) {
        self.gate_open.store(open, Ordering::Relaxed);
    }

    /// Record that the scheduler (not the user) paused this download.
    pub fn mark_auto_paused(&self, id: &str) {
        write_or_recover!(self.auto_paused).insert(id.to_string());
    }

    /// Drain the set of scheduler-paused downloads, clearing their paused flag.
    /// Called when the gate reopens so they can be re-queued.
    pub fn take_auto_paused(&self) -> Vec<String> {
        let ids: Vec<String> = write_or_recover!(self.auto_paused).drain().collect();
        let mut paused = write_or_recover!(self.paused);
        for id in &ids {
            paused.remove(id);
        }
        ids
    }

    // ── Polled by in-flight downloads ─────────────────────────────────────

    /// Returns `Some(reason)` when the download identified by `id` must stop now.
    ///
    /// A closed scheduler gate reads as [`Interrupt::Paused`]: the bytes already
    /// on disk stay put and the transfer resumes when the gate reopens.
    pub fn interrupt_reason(&self, id: &str) -> Option<Interrupt> {
        if self.is_cancelled(id) {
            Some(Interrupt::Cancelled)
        } else if self.is_paused(id) || !self.gate_open() {
            Some(Interrupt::Paused)
        } else {
            None
        }
    }
}

impl Default for DownloadControl {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_interrupt_when_idle() {
        let c = DownloadControl::new();
        assert_eq!(c.interrupt_reason("a"), None);
    }

    #[test]
    fn cancel_outranks_pause() {
        let c = DownloadControl::new();
        c.pause("a");
        c.cancel("a");
        assert_eq!(c.interrupt_reason("a"), Some(Interrupt::Cancelled));
    }

    #[test]
    fn closed_gate_pauses_every_download() {
        let c = DownloadControl::new();
        c.set_gate_open(false);
        assert_eq!(c.interrupt_reason("anything"), Some(Interrupt::Paused));
    }

    #[test]
    fn signals_are_per_download() {
        let c = DownloadControl::new();
        c.pause("a");
        assert_eq!(c.interrupt_reason("a"), Some(Interrupt::Paused));
        assert_eq!(c.interrupt_reason("b"), None);
    }

    #[test]
    fn take_auto_paused_drains_and_unpauses() {
        let c = DownloadControl::new();
        c.pause("a");
        c.mark_auto_paused("a");
        c.pause("b"); // user-paused, must survive

        let ids = c.take_auto_paused();
        assert_eq!(ids, vec!["a".to_string()]);
        assert!(!c.is_paused("a"), "scheduler-paused download should be released");
        assert!(c.is_paused("b"), "user-paused download must stay paused");
        assert!(c.take_auto_paused().is_empty(), "drain must be idempotent");
    }

    #[test]
    fn clear_removes_all_signals() {
        let c = DownloadControl::new();
        c.pause("a");
        c.cancel("a");
        c.clear("a");
        assert_eq!(c.interrupt_reason("a"), None);
    }
}
