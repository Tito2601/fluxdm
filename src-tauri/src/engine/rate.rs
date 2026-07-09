//! Smoothing for reported transfer rates.
//!
//! Both engines sample a cumulative byte counter on a fixed tick and divide by the
//! elapsed time. That raw sample is jumpy: a torrent's `progress_bytes` only moves
//! when a piece finishes verifying, so a one-second window holds either zero pieces
//! or several. Reporting it straight makes the UI alternate between "—" and a spike.
//!
//! An exponential moving average keeps the number readable without lagging far
//! behind a genuine change in speed.

/// Exponential moving average of a byte rate.
pub struct Ema {
    /// Weight of the newest sample. Higher reacts faster and smooths less.
    alpha: f64,
    value: Option<f64>,
}

impl Ema {
    pub fn new(alpha: f64) -> Self {
        Self { alpha, value: None }
    }

    /// Fold in a fresh bytes/sec sample and return the smoothed rate.
    ///
    /// The first sample is taken as-is: averaging it against zero would halve the
    /// speed shown for the first second of every download.
    pub fn update(&mut self, sample: f64) -> u64 {
        let next = match self.value {
            Some(prev) => self.alpha * sample + (1.0 - self.alpha) * prev,
            None => sample,
        };
        self.value = Some(next);
        next.max(0.0) as u64
    }

    /// Forget the history — used when a transfer pauses, so the rate doesn't
    /// resume from a stale average.
    pub fn reset(&mut self) {
        self.value = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sample_passes_through() {
        let mut ema = Ema::new(0.3);
        assert_eq!(ema.update(1000.0), 1000);
    }

    #[test]
    fn alternating_zeros_and_spikes_stay_near_the_mean() {
        // A torrent delivering 1 MB every other tick averages 500 KB/s.
        let mut ema = Ema::new(0.3);
        let mut last = 0;
        for i in 0..60 {
            last = ema.update(if i % 2 == 0 { 1_000_000.0 } else { 0.0 });
        }
        assert!(last > 300_000 && last < 700_000, "rate settled at {}", last);
        // The point of the exercise: it never reads as a dead zero.
        assert!(last > 0);
    }

    #[test]
    fn converges_on_a_steady_rate() {
        let mut ema = Ema::new(0.3);
        let mut last = 0;
        for _ in 0..50 {
            last = ema.update(2_000_000.0);
        }
        assert!((last as i64 - 2_000_000).abs() < 1_000);
    }

    #[test]
    fn reset_forgets_history() {
        let mut ema = Ema::new(0.3);
        ema.update(5_000_000.0);
        ema.reset();
        assert_eq!(ema.update(100.0), 100);
    }
}
