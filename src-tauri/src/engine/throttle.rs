//! Global download rate limit.
//!
//! One token bucket shared by every HTTP transfer, so `speed_limit_kbps` caps the
//! app as a whole rather than each segment independently — eight segments at
//! "1 MB/s" must add up to 1 MB/s, not 8.
//!
//! Torrents are limited separately, inside librqbit's own scheduler; see
//! [`TorrentEngine::set_download_limit`](crate::engine::torrent::TorrentEngine::set_download_limit).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

/// The settings key this limit is stored under.
pub const SETTING_KEY: &str = "speed_limit_kbps";

/// Never sleep longer than this in one go, so a limit raised mid-download takes
/// effect promptly instead of after a long pre-computed wait.
const MAX_SLEEP: Duration = Duration::from_millis(250);

/// A token bucket that paces callers to `limit_bps` bytes per second.
///
/// Zero means unlimited, which is also the default and the common case.
struct Limiter {
    limit_bps: AtomicU64,
    bucket:    Mutex<Bucket>,
}

struct Bucket {
    tokens: f64,
    last:   Instant,
}

impl Limiter {
    fn new() -> Self {
        Self {
            limit_bps: AtomicU64::new(0),
            bucket:    Mutex::new(Bucket { tokens: 0.0, last: Instant::now() }),
        }
    }

    fn set_bps(&self, bps: u64) {
        self.limit_bps.store(bps, Ordering::Relaxed);
    }

    fn bps(&self) -> u64 {
        self.limit_bps.load(Ordering::Relaxed)
    }

    /// Wait until `bytes` may be transferred, then account for them.
    async fn consume(&self, bytes: u64) {
        loop {
            let rate = self.bps();
            if rate == 0 || bytes == 0 {
                return;
            }

            let wait = {
                let mut b = self.bucket.lock().await;

                let now     = Instant::now();
                let elapsed = now.saturating_duration_since(b.last).as_secs_f64();
                b.last = now;

                // Allow one second of burst. The floor of `bytes` matters: a chunk
                // larger than the whole per-second allowance would otherwise never
                // fit in the bucket and this loop would never terminate.
                let capacity = (rate as f64).max(bytes as f64);
                b.tokens = (b.tokens + elapsed * rate as f64).min(capacity);

                if b.tokens >= bytes as f64 {
                    b.tokens -= bytes as f64;
                    return;
                }

                let deficit = bytes as f64 - b.tokens;
                Duration::from_secs_f64(deficit / rate as f64).min(MAX_SLEEP)
            };

            tokio::time::sleep(wait).await;
        }
    }
}

fn limiter() -> &'static Limiter {
    static LIMITER: OnceLock<Limiter> = OnceLock::new();
    LIMITER.get_or_init(Limiter::new)
}

/// Interpret a stored `speed_limit_kbps` value. Missing or unusable means unlimited.
fn parse_kbps(value: Option<&str>) -> u64 {
    value
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// Set the ceiling in KB/s. Zero disables it.
pub fn set_limit_kbps(kbps: u64) {
    limiter().set_bps(kbps.saturating_mul(1024));
}

/// Current ceiling in bytes/sec; zero means unlimited.
pub fn limit_bps() -> u64 {
    limiter().bps()
}

/// Wait until `bytes` may be transferred, then account for them.
///
/// Returns immediately when no limit is set, which is the common case — the cost
/// of the check is one relaxed atomic load per chunk.
pub async fn consume(bytes: u64) {
    limiter().consume(bytes).await
}

/// Read `speed_limit_kbps` and apply it to both engines.
///
/// Call at startup and whenever the setting changes.
pub fn apply_from_settings(
    value: Option<&str>,
    torrents: Option<&crate::engine::torrent::TorrentEngine>,
) {
    let kbps = parse_kbps(value);
    set_limit_kbps(kbps);

    if let Some(engine) = torrents {
        engine.set_download_limit(limit_bps());
    }

    if kbps == 0 {
        tracing::info!("Speed limit disabled");
    } else {
        tracing::info!("Speed limit set to {} KB/s", kbps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests drive their own `Limiter` rather than the process-wide one: mutating
    /// the global limit would throttle whatever other tests are downloading at
    /// the time.
    #[test]
    fn unusable_settings_mean_unlimited() {
        assert_eq!(parse_kbps(None), 0);
        assert_eq!(parse_kbps(Some("")), 0);
        assert_eq!(parse_kbps(Some("not a number")), 0);
        assert_eq!(parse_kbps(Some("-5")), 0);
        assert_eq!(parse_kbps(Some("0")), 0);
        assert_eq!(parse_kbps(Some(" 512 ")), 512);
    }

    #[tokio::test]
    async fn an_unlimited_bucket_never_sleeps() {
        let l = Limiter::new();
        let start = Instant::now();
        for _ in 0..100 {
            l.consume(1024 * 1024).await;
        }
        assert!(start.elapsed() < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn a_limit_paces_the_transfer() {
        let l = Limiter::new();
        l.set_bps(64 * 1024);

        l.consume(64 * 1024).await; // drain the one-second burst

        let start = Instant::now();
        l.consume(64 * 1024).await;
        let elapsed = start.elapsed();

        assert!(
            elapsed >= Duration::from_millis(500),
            "64 KB at 64 KB/s should take about a second, took {:?}",
            elapsed
        );
    }

    /// Eight segments sharing a 64 KB/s ceiling must together take about a
    /// second per 64 KB — not a second each in parallel.
    #[tokio::test]
    async fn the_ceiling_is_shared_across_callers() {
        let l = std::sync::Arc::new(Limiter::new());
        l.set_bps(64 * 1024);
        l.consume(64 * 1024).await; // drain the burst

        let start = Instant::now();
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let l = l.clone();
            tasks.push(tokio::spawn(async move { l.consume(8 * 1024).await }));
        }
        for t in tasks {
            t.await.unwrap();
        }

        // 8 x 8 KB = 64 KB at 64 KB/s ≈ 1s, regardless of concurrency.
        assert!(
            start.elapsed() >= Duration::from_millis(500),
            "concurrent callers escaped the shared ceiling: {:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn a_chunk_larger_than_the_whole_allowance_still_completes() {
        let l = Limiter::new();
        l.set_bps(1024); // 1 KB/s, asked for 4 KB
        tokio::time::timeout(Duration::from_secs(15), l.consume(4096))
            .await
            .expect("consume must terminate for an oversized chunk");
    }
}
