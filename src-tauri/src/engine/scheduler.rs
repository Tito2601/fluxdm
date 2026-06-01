#![allow(dead_code)]
/// FluxDM Download Scheduler (Phase 6 — stub for Phase 1)
///
/// Will implement:
/// - Time-window based download gating (e.g., only 02:00–07:00)
/// - CPU load checks (pause if CPU > 80%)
/// - Battery checks (pause if battery < 20%)
/// - Metered connection detection

pub struct Scheduler {
    pub enabled: bool,
    pub start:   String, // "HH:MM"
    pub stop:    String, // "HH:MM"
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            enabled: false,
            start:   "02:00".to_string(),
            stop:    "07:00".to_string(),
        }
    }

    /// Returns `true` if downloads should proceed right now.
    pub fn should_download_now(&self) -> bool {
        if !self.enabled {
            return true; // scheduler off → always download
        }
        // TODO Phase 6: parse start/stop times and compare with current time
        true
    }
}
