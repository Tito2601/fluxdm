use anyhow::{Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{info, warn};
use uuid::Uuid;

use crate::engine::control::DownloadControl;
use crate::engine::http;
use crate::engine::throttle;

/// Batch writes so a stream of small chunks doesn't become a stream of syscalls.
const WRITE_BUFFER: usize = 512 * 1024;

/// Report progress at most once per this many bytes, per segment.
const PROGRESS_STEP: u64 = 64 * 1024;

/// Reports a segment's absolute byte count as it grows.
///
/// Absolute rather than a delta: a segment that restarts (a server ignoring our
/// `Range` header) rewinds to zero, and the aggregator must follow it down rather
/// than double-count the bytes it fetched twice.
pub type ProgressTx = UnboundedSender<(usize, u64)>;

/// Where a download's bytes accumulate before it is finished and renamed.
///
/// Segments write directly into this file at their own offsets, so a completed
/// download needs no merge pass — only a rename.
pub fn part_path(output_path: &str) -> PathBuf {
    PathBuf::from(format!("{}.fluxdm-part", output_path))
}

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SegmentStatus {
    Pending,
    Downloading,
    Completed,
    Failed,
    /// Stopped mid-transfer by a pause, a cancel, or the scheduler gate.
    /// Whatever bytes were fetched remain in the temp file.
    Interrupted,
}

impl SegmentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SegmentStatus::Pending     => "pending",
            SegmentStatus::Downloading => "downloading",
            SegmentStatus::Completed   => "completed",
            SegmentStatus::Failed      => "failed",
            // Persisted as `pending`: on the next run `resume::find_resumable_segments`
            // re-derives the true byte count from the temp file on disk, so the
            // distinction between "never started" and "stopped partway" is not
            // worth a column of its own.
            SegmentStatus::Interrupted => "pending",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub id:          String,
    pub download_id: String,
    pub index_num:   usize,
    pub byte_start:  u64,
    pub byte_end:    u64,
    pub downloaded:  u64,
    pub status:      SegmentStatus,
}

impl Segment {
    pub fn new(
        download_id: &str,
        index_num: usize,
        byte_start: u64,
        byte_end: u64,
    ) -> Self {
        Self {
            id:          Uuid::new_v4().to_string(),
            download_id: download_id.to_string(),
            index_num,
            byte_start,
            byte_end,
            downloaded:  0,
            status:      SegmentStatus::Pending,
        }
    }

    /// Total number of bytes this segment should download.
    pub fn expected_bytes(&self) -> u64 {
        self.byte_end - self.byte_start + 1
    }
}

// ── Download worker ───────────────────────────────────────────────────────────

/// Download a single byte-range segment straight into its slice of `dest`.
///
/// `dest` is the shared, pre-allocated destination file: every segment seeks to
/// its own `byte_start` and writes in place, so the bytes are never copied a
/// second time. Nothing else may write inside `[byte_start, byte_end]`.
///
/// Resumes from `segment.downloaded`, which the caller restores from the database.
/// Retries up to 3 times on network failure.
///
/// Polls `control` between chunks; when it signals a stop, the buffer is flushed
/// and the segment returns with [`SegmentStatus::Interrupted`] rather than an
/// error, so the caller can distinguish "user stopped it" from "it broke".
///
/// `progress` receives this segment's running byte total as chunks land, which is
/// what lets the UI show a live rate instead of one jump per finished segment.
pub async fn download_segment(
    mut segment: Segment,
    url: &str,
    cookies: Option<&str>,
    control: &DownloadControl,
    progress: &ProgressTx,
    dest: &Path,
) -> Result<Segment> {
    use reqwest::header::RANGE;

    let client = http::client();

    let expected = segment.expected_bytes();
    let mut last_error = String::new();

    // Bytes of this segment already in `dest`. Carried across attempts, and reset
    // to zero if a server forces us to restart the slice.
    let mut written = segment.downloaded.min(expected);

    for attempt in 1..=3_u8 {
        // Don't open a connection we're about to abandon.
        if control.interrupt_reason(&segment.download_id).is_some() {
            segment.downloaded = written;
            segment.status     = SegmentStatus::Interrupted;
            return Ok(segment);
        }

        // Already have the whole slice — no request needed.
        if written >= expected {
            info!("Segment {} already complete ({} bytes)", segment.index_num, written);
            let _ = progress.send((segment.index_num, written));
            segment.downloaded = written;
            segment.status     = SegmentStatus::Completed;
            return Ok(segment);
        }

        let range_start  = segment.byte_start + written;
        let range_header = format!("bytes={}-{}", range_start, segment.byte_end);

        if written > 0 {
            info!(
                "Segment {}: resuming from offset {} ({} bytes already written)",
                segment.index_num, range_start, written
            );
        }

        let mut req = client
            .get(url)
            .header(RANGE, &range_header)
            .header("User-Agent", "FluxDM/0.1");

        if let Some(cookie) = cookies {
            req = req.header("Cookie", cookie);
        }

        match req.send().await {
            Ok(response) => {
                let http_status = response.status().as_u16();

                // We asked for a range and got the whole file. Its first byte is
                // byte zero of the download, not of this segment, so nothing in
                // the body can be trusted at our offset — start the slice over.
                if written > 0 && http_status == 200 {
                    warn!(
                        "Segment {}: server returned 200 for partial range, restarting segment",
                        segment.index_num
                    );
                    written = 0;
                    // Those bytes are void; walk the reported total back down.
                    let _ = progress.send((segment.index_num, 0));
                    last_error = "Server returned 200 for partial range".to_string();
                    if attempt < 3 {
                        tokio::time::sleep(
                            std::time::Duration::from_secs(2 * attempt as u64),
                        )
                        .await;
                    }
                    continue;
                }

                if !response.status().is_success() && http_status != 206 {
                    let err = format!("HTTP {}", response.status());
                    warn!("Segment {}: HTTP error on attempt {}: {}", segment.index_num, attempt, err);
                    last_error = err;
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    continue;
                }

                // Seek to where this segment left off inside the shared file.
                let mut raw = tokio::fs::OpenOptions::new()
                    .write(true)
                    .open(dest)
                    .await
                    .with_context(|| format!("Cannot open {} for writing", dest.display()))?;
                raw.seek(SeekFrom::Start(range_start)).await?;
                let mut file = BufWriter::with_capacity(WRITE_BUFFER, raw);

                let mut stream      = response.bytes_stream();
                let mut downloaded  = written;
                let mut stream_ok   = true;
                let mut interrupted = false;
                let mut last_report = written;

                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => {
                            // A server that ignores `Range` sends more than we
                            // asked for. Writing it would overrun into the next
                            // segment's bytes, so stop at our boundary.
                            let room = expected - downloaded;
                            let take = (chunk.len() as u64).min(room) as usize;

                            throttle::consume(take as u64).await;
                            file.write_all(&chunk[..take]).await?;
                            downloaded += take as u64;

                            // Coarse enough that a fast link doesn't flood the
                            // channel, fine enough that the UI still looks live.
                            if downloaded - last_report >= PROGRESS_STEP {
                                last_report = downloaded;
                                let _ = progress.send((segment.index_num, downloaded));
                            }

                            if downloaded >= expected {
                                break;
                            }

                            // Stop between chunks so the written bytes always end
                            // on a boundary that `Range` can resume from.
                            if control.interrupt_reason(&segment.download_id).is_some() {
                                interrupted = true;
                                break;
                            }
                        }
                        Err(e) => {
                            last_error = e.to_string();
                            warn!(
                                "Segment {}: stream error on attempt {}: {}",
                                segment.index_num, attempt, last_error
                            );
                            stream_ok = false;
                            break;
                        }
                    }
                }

                // Drain the buffer, so what a retry or a resume believes is on
                // disk actually is.
                file.flush().await?;
                let _ = progress.send((segment.index_num, downloaded));
                written = downloaded;

                if interrupted {
                    info!(
                        "Segment {} interrupted at {} / {} bytes",
                        segment.index_num, downloaded, expected
                    );
                    segment.downloaded = downloaded;
                    segment.status     = SegmentStatus::Interrupted;
                    return Ok(segment);
                }

                // Only a full slice counts. A stream that ends early leaves a hole,
                // and the retry below picks up exactly where it stopped.
                if downloaded >= expected {
                    info!("Segment {} complete: {} bytes", segment.index_num, downloaded);
                    segment.downloaded = downloaded;
                    segment.status     = SegmentStatus::Completed;
                    return Ok(segment);
                }

                if stream_ok {
                    last_error = format!(
                        "Stream ended early: {} of {} bytes",
                        downloaded, expected
                    );
                    warn!("Segment {}: {}", segment.index_num, last_error);
                }
            }
            Err(e) => {
                last_error = e.to_string();
                warn!(
                    "Segment {}: network error on attempt {}: {}",
                    segment.index_num, attempt, e
                );
            }
        }

        if attempt < 3 {
            tokio::time::sleep(std::time::Duration::from_secs(2 * attempt as u64)).await;
        }
    }

    segment.downloaded = written;
    segment.status = SegmentStatus::Failed;
    Err(anyhow::anyhow!(
        "Segment {} failed after 3 attempts: {}",
        segment.index_num,
        last_error
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    const BODY: usize = 1024 * 1024;
    const CHUNK: usize = 16 * 1024;

    /// Deterministic, position-dependent bytes: writing a segment at the wrong
    /// offset produces a file that fails to compare equal.
    fn body() -> Arc<Vec<u8>> {
        Arc::new((0..BODY).map(|i| (i % 251) as u8).collect())
    }

    /// A range-honouring HTTP server that dribbles the requested slice out in
    /// many small chunks, the way a real connection delivers one.
    async fn serve(body: Arc<Vec<u8>>) -> String {
        serve_with(body, true).await
    }

    /// `pace` inserts a small sleep between chunks. Windows' timer granularity is
    /// ~15 ms, so those sleeps dominate the wall clock — any test that measures
    /// throughput must turn them off or it will pass without measuring anything.
    async fn serve_with(body: Arc<Vec<u8>>, pace: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                let (mut sock, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => return,
                };
                let body = body.clone();

                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();

                    // "Range: bytes=start-end"
                    let (start, end) = req
                        .lines()
                        .find_map(|l| l.to_lowercase().strip_prefix("range: bytes=").map(String::from))
                        .and_then(|r| {
                            let (a, b) = r.trim().split_once('-')?;
                            Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
                        })
                        .unwrap_or((0, body.len() - 1));

                    let slice = &body[start..=end.min(body.len() - 1)];
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        slice.len()
                    );
                    if sock.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    for piece in slice.chunks(CHUNK) {
                        if sock.write_all(piece).await.is_err() {
                            return;
                        }
                        if pace {
                            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                        }
                    }
                    let _ = sock.flush().await;
                });
            }
        });

        format!("http://{}/file.bin", addr)
    }

    /// Pre-allocate the destination the way `multi_segment_download` does.
    fn dest_file(len: u64) -> PathBuf {
        let path = std::env::temp_dir().join(format!("fluxdm-test-{}.part", Uuid::new_v4()));
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(len).unwrap();
        path
    }

    /// The bug this guards: progress used to be reported once, when the whole
    /// segment finished, so an in-flight download looked frozen.
    #[tokio::test]
    async fn reports_progress_while_the_segment_streams() {
        let url = serve(body()).await;
        let dest = dest_file(BODY as u64);
        let control = DownloadControl::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let segment = Segment::new("job", 0, 0, BODY as u64 - 1);
        let seg = download_segment(segment, &url, None, &control, &tx, &dest)
            .await
            .expect("segment must download");
        drop(tx);

        assert_eq!(seg.status, SegmentStatus::Completed);
        assert_eq!(seg.downloaded, BODY as u64);

        let mut updates = Vec::new();
        while let Some((index, bytes)) = rx.recv().await {
            assert_eq!(index, 0);
            updates.push(bytes);
        }

        assert!(updates.len() > 4, "expected a stream of updates, got {}", updates.len());
        assert!(
            updates.windows(2).all(|w| w[1] >= w[0]),
            "reported byte counts must never go backwards: {:?}",
            updates
        );
        assert_eq!(*updates.last().unwrap(), BODY as u64, "final count is exact");

        let _ = std::fs::remove_file(dest);
    }

    /// The merge pass is gone: segments must land at their own offsets in one
    /// shared file, and the result must be byte-for-byte the original.
    #[tokio::test]
    async fn concurrent_segments_assemble_the_file_in_place() {
        let expected = body();
        let url = serve(expected.clone()).await;
        let dest = dest_file(BODY as u64);
        let control = Arc::new(DownloadControl::new());
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        // Four segments, written concurrently, no temp files anywhere.
        let bounds = [
            (0u64, 262_143u64),
            (262_144, 524_287),
            (524_288, 786_431),
            (786_432, BODY as u64 - 1),
        ];

        let mut tasks = Vec::new();
        for (i, (start, end)) in bounds.into_iter().enumerate() {
            let url = url.clone();
            let dest = dest.clone();
            let control = control.clone();
            let tx = tx.clone();
            tasks.push(tokio::spawn(async move {
                let seg = Segment::new("job", i, start, end);
                download_segment(seg, &url, None, &control, &tx, &dest).await
            }));
        }

        for t in tasks {
            let seg = t.await.unwrap().expect("segment must download");
            assert_eq!(seg.status, SegmentStatus::Completed);
        }

        let written = std::fs::read(&dest).unwrap();
        assert_eq!(written.len(), BODY, "file is exactly the declared size");
        assert!(written == *expected, "assembled bytes differ from the source");

        let _ = std::fs::remove_file(dest);
    }

    /// The `speed_limit_kbps` setting reaches the byte loop.
    ///
    /// Ignored by default: it mutates the process-wide limit, which would throttle
    /// any other download test running alongside it. Run on its own with
    /// `cargo test -- --ignored --test-threads=1`.
    #[tokio::test]
    #[ignore]
    async fn a_speed_limit_paces_a_real_segment() {
        // No artificial pacing in the server: the only thing that may slow this
        // transfer down is the limiter.
        let url = serve_with(body(), false).await;
        let control = DownloadControl::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        // Baseline: how fast is 1 MB over loopback with no limit at all?
        let dest = dest_file(BODY as u64);
        throttle::set_limit_kbps(0);
        let start = std::time::Instant::now();
        download_segment(Segment::new("job", 0, 0, BODY as u64 - 1), &url, None, &control, &tx, &dest)
            .await
            .expect("baseline segment must download");
        let unlimited = start.elapsed();
        let _ = std::fs::remove_file(dest);

        // 1 MB at 256 KB/s: 256 KB of burst, then 768 KB at 256 KB/s ≈ 3 s.
        let dest = dest_file(BODY as u64);
        throttle::set_limit_kbps(256);
        let start = std::time::Instant::now();
        let seg = download_segment(
            Segment::new("job", 0, 0, BODY as u64 - 1),
            &url, None, &control, &tx, &dest,
        )
        .await
        .expect("throttled segment must download");
        let limited = start.elapsed();
        throttle::set_limit_kbps(0);

        assert_eq!(seg.downloaded, BODY as u64, "throttling must not lose bytes");
        assert!(
            limited >= std::time::Duration::from_secs(2),
            "1 MB at 256 KB/s finished in {:?} — the limit is not applied",
            limited
        );
        assert!(
            limited > unlimited * 4,
            "limited {:?} vs unlimited {:?} — no measurable throttling",
            limited,
            unlimited
        );

        let _ = std::fs::remove_file(dest);
    }

    /// A resumed segment must not re-fetch what it already has, and must land the
    /// remainder at the right offset rather than at the start of its slice.
    #[tokio::test]
    async fn a_resumed_segment_fills_only_the_gap() {
        let expected = body();
        let url = serve(expected.clone()).await;
        let dest = dest_file(BODY as u64);
        let control = DownloadControl::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        // Pretend a previous run wrote the first half of the whole file.
        let half = BODY as u64 / 2;
        {
            use std::io::Write as _;
            let mut f = std::fs::OpenOptions::new().write(true).open(&dest).unwrap();
            f.write_all(&expected[..half as usize]).unwrap();
        }

        let mut segment = Segment::new("job", 0, 0, BODY as u64 - 1);
        segment.downloaded = half;

        let seg = download_segment(segment, &url, None, &control, &tx, &dest)
            .await
            .expect("segment must resume");

        assert_eq!(seg.status, SegmentStatus::Completed);
        assert_eq!(seg.downloaded, BODY as u64);

        let written = std::fs::read(&dest).unwrap();
        assert!(written == *expected, "resumed file differs from the source");

        let _ = std::fs::remove_file(dest);
    }
}
