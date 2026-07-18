use anyhow::{Context, Result};
use chrono::Utc;
use futures::StreamExt;
use reqwest::header::{ACCEPT_RANGES, CONTENT_LENGTH};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::Mutex;
use tokio::time::MissedTickBehavior;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::engine::control::{DownloadControl, Interrupt};
use crate::engine::http;
use crate::engine::merger::{calculate_sha256, finalize_download};
use crate::engine::rate::Ema;
use crate::engine::segment::{download_segment, part_path, Segment, SegmentStatus};
use crate::engine::throttle;
use crate::storage::db::Database;

// ── Outcome ───────────────────────────────────────────────────────────────────

/// How a transfer ended. Distinguishes a deliberate stop from a failure so the
/// caller can persist `paused` / `cancelled` instead of `failed`.
enum DownloadOutcome {
    Finished(String), // SHA-256 of the merged file
    Stopped(Interrupt),
}

// ── Status ────────────────────────────────────────────────────────────────────

/// Download status — serializes as a plain lowercase string ("queued", "downloading", etc.)
/// so the frontend can compare it directly to the TypeScript string-union type.
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed(String),
    Cancelled,
}

impl serde::Serialize for DownloadStatus {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for DownloadStatus {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(Self::from_str(&raw))
    }
}

impl DownloadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            DownloadStatus::Queued      => "queued",
            DownloadStatus::Downloading => "downloading",
            DownloadStatus::Paused      => "paused",
            DownloadStatus::Completed   => "completed",
            DownloadStatus::Failed(_)   => "failed",
            DownloadStatus::Cancelled   => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "queued"      => DownloadStatus::Queued,
            "downloading" => DownloadStatus::Downloading,
            "paused"      => DownloadStatus::Paused,
            "completed"   => DownloadStatus::Completed,
            "cancelled"   => DownloadStatus::Cancelled,
            _             => DownloadStatus::Failed("Unknown".to_string()),
        }
    }
}

// ── Kind ──────────────────────────────────────────────────────────────────────

/// Which engine owns a download. Serializes as a plain lowercase string,
/// matching the `DownloadKind` TypeScript union.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadKind {
    /// Segmented or single-stream HTTP.
    Http,
    /// HLS / DASH media stream.
    Stream,
    /// BitTorrent swarm.
    Torrent,
}

impl DownloadKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            DownloadKind::Http    => "http",
            DownloadKind::Stream  => "stream",
            DownloadKind::Torrent => "torrent",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "stream"  => DownloadKind::Stream,
            "torrent" => DownloadKind::Torrent,
            _         => DownloadKind::Http,
        }
    }
}

impl serde::Serialize for DownloadKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for DownloadKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::from_str(&String::deserialize(d)?))
    }
}

// ── Job ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadJob {
    pub id:           String,
    pub url:          String,
    pub filename:     String,
    pub save_path:    String,
    pub total_bytes:  u64,
    pub downloaded:   u64,
    pub status:       DownloadStatus,
    pub speed_bps:    u64,
    pub num_segments: u8,
    pub category:     String,
    pub threat_score: u8,
    pub source_url:   Option<String>,
    pub referrer:     Option<String>,
    pub mime_type:    Option<String>,
    pub checksum:     Option<String>,
    pub created_at:   String,
    pub updated_at:   String,
    pub completed_at: Option<String>,

    // ── Torrent-specific ──────────────────────────────────────────────────
    // Zero / None for every other kind.
    pub kind:             DownloadKind,
    pub info_hash:        Option<String>,
    pub uploaded_bytes:   u64,
    pub upload_speed_bps: u64,
    /// Peers currently connected.
    pub peers_connected:  u32,
    /// Distinct peers discovered in the swarm so far.
    pub peers_total:      u32,

    // Not persisted to DB:
    #[serde(skip_serializing)]
    pub segments: Vec<Segment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers:  Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cookies:  Option<String>,
}

impl DownloadJob {
    pub fn new(
        url: String,
        filename: String,
        save_path: String,
        headers: Option<serde_json::Value>,
        cookies: Option<String>,
    ) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            id:           Uuid::new_v4().to_string(),
            url,
            filename,
            save_path,
            total_bytes:  0,
            downloaded:   0,
            status:       DownloadStatus::Queued,
            speed_bps:    0,
            num_segments: 8,
            category:     "other".to_string(),
            threat_score: 0,
            source_url:   None,
            referrer:     None,
            mime_type:    None,
            checksum:     None,
            created_at:   now.clone(),
            updated_at:   now,
            completed_at: None,

            kind:             DownloadKind::Http,
            info_hash:        None,
            uploaded_bytes:   0,
            upload_speed_bps: 0,
            peers_connected:  0,
            peers_total:      0,

            segments:     Vec::new(),
            headers,
            cookies,
        }
    }
}

// ── Progress event emitted to the React UI ────────────────────────────────────

/// Emitted as `"download_progress"`.
/// MUST use camelCase so the TypeScript `ProgressEvent` interface maps directly
/// without an explicit normalisation step.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub id:               String,
    pub downloaded_bytes: u64,
    pub total_bytes:      u64,
    pub speed_bps:        u64,
    pub eta_seconds:      u64,

    // Torrent-only. Omitted entirely for HTTP and stream downloads so the
    // frontend can tell "not a torrent" from "a torrent with zero peers".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uploaded_bytes:   Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_speed_bps: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peers_connected:  Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peers_total:      Option<u32>,
}

impl ProgressEvent {
    /// A plain HTTP/stream progress tick, with the torrent fields left unset.
    pub fn plain(id: String, downloaded_bytes: u64, total_bytes: u64, speed_bps: u64, eta_seconds: u64) -> Self {
        Self {
            id,
            downloaded_bytes,
            total_bytes,
            speed_bps,
            eta_seconds,
            uploaded_bytes:   None,
            upload_speed_bps: None,
            peers_connected:  None,
            peers_total:      None,
        }
    }
}

// ── Segment splitter ──────────────────────────────────────────────────────────

/// Divide `total_bytes` into `num_segments` equal ranges.
/// The last segment absorbs any remainder bytes.
pub fn split_into_segments(download_id: &str, total_bytes: u64, num_segments: u8) -> Vec<Segment> {
    let n = num_segments as u64;
    let seg_size = total_bytes / n;
    let mut segments = Vec::with_capacity(num_segments as usize);

    for i in 0..n {
        let byte_start = i * seg_size;
        let byte_end = if i == n - 1 {
            total_bytes - 1
        } else {
            (i + 1) * seg_size - 1
        };
        segments.push(Segment::new(download_id, i as usize, byte_start, byte_end));
    }

    segments
}

// ── Core download orchestrator ────────────────────────────────────────────────

/// Orchestrate a complete download: HEAD probe → multi-segment fetch → merge → checksum.
pub async fn start_download(
    mut job: DownloadJob,
    app_handle: AppHandle,
    db: Arc<Mutex<Database>>,
    control: Arc<DownloadControl>,
) -> Result<()> {
    info!("Starting download: {} → {}/{}", job.url, job.save_path, job.filename);

    let client = http::client();

    // ── Step 1: HEAD request ──────────────────────────────────────────────
    // A metadata probe should not hang the queue, so it keeps a hard deadline
    // even though the shared client sets none for body transfers.
    let head = http::apply_captured(
        client.head(&job.url).timeout(std::time::Duration::from_secs(30)),
        job.headers.as_ref(),
        job.cookies.as_deref(),
    )
    .send()
    .await?;

    let content_length = head
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    let supports_ranges = head
        .headers()
        .get(ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_lowercase() == "bytes")
        .unwrap_or(false);

    let mime_type = head
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());

    info!(
        "HEAD → size={} bytes, range={}, mime={:?}",
        content_length, supports_ranges, mime_type
    );

    // A binary filename answered with a web page means the link never resolved to
    // the file: expired signed URL, hotlink guard, or a login wall. The body would
    // be an error page, so fail here rather than write it to disk under the
    // expected filename.
    if let Some(mime) = mime_type.as_deref() {
        if is_markup(mime) && expects_binary(&job.filename) {
            anyhow::bail!(
                "server returned a web page ({}) instead of '{}' — the link may have \
                 expired or require signing in",
                mime,
                job.filename
            );
        }
    }

    job.total_bytes = content_length;
    job.mime_type   = mime_type.clone();
    job.status      = DownloadStatus::Downloading;

    // AI: categorize + threat score
    job.category = crate::ai::categorizer::categorize_download(
        &job.url,
        &job.filename,
        mime_type.as_deref().unwrap_or(""),
    );
    job.threat_score = crate::ai::threat::calculate_threat_score(
        &job.url,
        &job.filename,
        mime_type.as_deref().unwrap_or(""),
        job.referrer.as_deref().unwrap_or(""),
    );

    if job.threat_score > 60 {
        warn!(
            "High threat score {} for '{}' — user notified via UI badge",
            job.threat_score, job.filename
        );
    }

    // Persist initial state
    { db.lock().await.upsert_download(&job)?; }

    // Emit initial progress
    let _ = app_handle.emit(
        "download_progress",
        ProgressEvent::plain(job.id.clone(), 0, content_length, 0, 0),
    );

    // Expand ~ and guarantee the parent directory exists before any I/O.
    let save_dir    = crate::utils::expand_path(&job.save_path);
    let output_path = format!("{}/{}", save_dir, job.filename);
    crate::utils::ensure_parent_dir(&output_path)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // ── Step 2: Download ──────────────────────────────────────────────────
    let result = if supports_ranges && content_length > 0 {
        multi_segment_download(&mut job, &output_path, &app_handle, &db, &control).await
    } else {
        single_stream_download(&mut job, &output_path, client, &app_handle, &control).await
    };

    // ── Step 3: Finalize ──────────────────────────────────────────────────
    match result {
        Ok(DownloadOutcome::Finished(checksum)) => {
            job.status       = DownloadStatus::Completed;
            job.checksum     = Some(checksum.clone());
            job.completed_at = Some(Utc::now().to_rfc3339());
            // `job.downloaded` is left as the transfer set it. Assigning the
            // advertised Content-Length here would paint a truncated file as a
            // full one in the UI.

            let _ = app_handle.emit("download_complete", serde_json::json!({
                "id":        job.id,
                "save_path": output_path,
                "checksum":  checksum,
            }));

            info!("Download complete: {}", job.filename);

            let db = db.lock().await;
            db.upsert_download(&job)?;
            db.add_to_history(&job)?;
            control.clear(&job.id);
        }

        Ok(DownloadOutcome::Stopped(Interrupt::Paused)) => {
            info!("Download paused: {} ({} bytes retained)", job.filename, job.downloaded);
            job.status = DownloadStatus::Paused;

            let _ = app_handle.emit("download_paused", serde_json::json!({ "id": job.id }));
            db.lock().await.upsert_download(&job)?;
            // Deliberately does NOT clear the control flags: a scheduler-paused job
            // stays in `auto_paused` so the gate can re-queue it later.
        }

        Ok(DownloadOutcome::Stopped(Interrupt::Cancelled)) => {
            info!("Download cancelled: {}", job.filename);
            job.status = DownloadStatus::Cancelled;

            discard_part_file(&output_path);

            let _ = app_handle.emit("download_cancelled", serde_json::json!({ "id": job.id }));
            db.lock().await.upsert_download(&job)?;
            control.clear(&job.id);
        }

        Err(e) => {
            error!("Download failed for {}: {}", job.filename, e);
            job.status = DownloadStatus::Failed(e.to_string());

            let _ = app_handle.emit("download_error", serde_json::json!({
                "id":    job.id,
                "error": e.to_string(),
            }));

            db.lock().await.upsert_download(&job)?;
            control.clear(&job.id);
        }
    }

    Ok(())
}

/// Remove the partially written file for a cancelled download.
fn discard_part_file(output_path: &str) {
    let _ = std::fs::remove_file(part_path(output_path));
}

/// Whether a MIME type denotes a rendered page rather than file content.
fn is_markup(mime: &str) -> bool {
    let mime = mime.trim().to_ascii_lowercase();
    mime == "text/html" || mime == "application/xhtml+xml"
}

/// Whether a filename promises binary content.
///
/// Deliberately a whitelist of container/archive/media formats: an unknown or
/// absent extension stays permissive, so only a confident mismatch — `.zip`
/// answered with markup — is ever treated as an error.
fn expects_binary(filename: &str) -> bool {
    const BINARY_EXT: &[&str] = &[
        "zip", "rar", "7z", "gz", "bz2", "xz", "tar", "iso", "img", "bin", "cue",
        "exe", "msi", "dmg", "pkg", "deb", "rpm", "apk",
        "mp4", "mkv", "avi", "mov", "webm", "mp3", "flac", "wav", "m4a",
        "pdf", "epub", "jpg", "jpeg", "png", "gif", "webp",
    ];

    filename
        .rsplit_once('.')
        .map(|(_, ext)| {
            let ext = ext.to_ascii_lowercase();
            BINARY_EXT.contains(&ext.as_str())
        })
        .unwrap_or(false)
}

// ── Progress reporting ────────────────────────────────────────────────────────

/// How often the UI hears about a running download.
const PROGRESS_TICK: std::time::Duration = std::time::Duration::from_millis(250);

/// How often a speed sample is written to the analytics table.
const SPEED_SAMPLE_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// How often each segment's offset is checkpointed to the database.
///
/// A crash loses at most this much progress, because segments write in place and
/// leave no other record of where they got to.
const CHECKPOINT_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

/// Weight of the newest speed sample. At a 250 ms tick this averages over roughly
/// the last second — responsive, but immune to one slow chunk reading as a stall.
const SPEED_ALPHA: f64 = 0.25;

/// Fold every segment's byte count into one progress event stream.
///
/// Segments report their own absolute totals, which is what makes this safe: a
/// segment that restarts rewinds its entry instead of double-counting, and a
/// segment that never starts still contributes its seeded on-disk bytes.
async fn report_progress(
    app:              AppHandle,
    db:               Arc<Mutex<Database>>,
    job_id:           String,
    total_bytes:      u64,
    total_downloaded: Arc<AtomicU64>,
    mut seg_bytes:    HashMap<usize, u64>,
    mut rx:           UnboundedReceiver<(usize, u64)>,
) {
    let mut ema             = Ema::new(SPEED_ALPHA);
    let mut last_total      = seg_bytes.values().sum::<u64>();
    let mut last_tick       = Instant::now();
    let mut last_db_write   = Instant::now();
    let mut last_checkpoint = Instant::now();

    let mut ticker = tokio::time::interval(PROGRESS_TICK);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await; // interval yields immediately the first time

    loop {
        tokio::select! {
            msg = rx.recv() => match msg {
                Some((index, bytes)) => {
                    seg_bytes.insert(index, bytes);
                    // Drain whatever else is queued: a fast link produces updates
                    // far quicker than the tick, and we only want the newest.
                    while let Ok((index, bytes)) = rx.try_recv() {
                        seg_bytes.insert(index, bytes);
                    }
                }
                // All segment tasks are done.
                None => break,
            },

            _ = ticker.tick() => {
                let total: u64 = seg_bytes.values().sum();
                total_downloaded.store(total, Ordering::Relaxed);

                let dt     = last_tick.elapsed().as_secs_f64();
                last_tick  = Instant::now();
                let sample = if dt > 0.0 {
                    total.saturating_sub(last_total) as f64 / dt
                } else {
                    0.0
                };
                last_total = total;

                let speed = ema.update(sample);
                let eta   = if speed > 0 {
                    total_bytes.saturating_sub(total) / speed
                } else {
                    0
                };

                let _ = app.emit(
                    "download_progress",
                    ProgressEvent::plain(job_id.clone(), total, total_bytes, speed, eta),
                );

                if last_checkpoint.elapsed() >= CHECKPOINT_EVERY {
                    last_checkpoint = Instant::now();
                    checkpoint(&db, &job_id, &seg_bytes).await;
                }

                if last_db_write.elapsed() >= SPEED_SAMPLE_EVERY {
                    last_db_write = Instant::now();
                    if let Ok(db) = db.try_lock() {
                        let now_secs = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let _ = db.record_speed_point(now_secs, speed, &job_id);
                    }
                }
            }
        }
    }

    // One last event so the bar lands exactly on the final byte count rather than
    // wherever the previous tick left it.
    let total: u64 = seg_bytes.values().sum();
    total_downloaded.store(total, Ordering::Relaxed);
    let _ = app.emit(
        "download_progress",
        ProgressEvent::plain(job_id, total, total_bytes, 0, 0),
    );
}

/// Write each segment's current offset to its row.
///
/// Uses `try_lock`: a checkpoint is an optimisation, and waiting on the database
/// behind another writer would stall the progress ticks. The next tick retries.
async fn checkpoint(db: &Arc<Mutex<Database>>, job_id: &str, seg_bytes: &HashMap<usize, u64>) {
    if let Ok(db) = db.try_lock() {
        for (index, bytes) in seg_bytes {
            let _ = db.update_segment_progress(job_id, *index, *bytes);
        }
    }
}

// ── Multi-segment parallel download ──────────────────────────────────────────

async fn multi_segment_download(
    job:         &mut DownloadJob,
    output_path: &str,
    app_handle:  &AppHandle,
    db:          &Arc<Mutex<Database>>,
    control:     &Arc<DownloadControl>,
) -> Result<DownloadOutcome> {
    let part = part_path(output_path);

    // Check for resumable segments from a previous (interrupted) attempt.
    let resumable = {
        let db_lock = db.lock().await;
        crate::engine::resume::find_resumable_segments(&job.id, &*db_lock, &part)
    };

    let segments = if !resumable.is_empty() && resumable.len() as u8 == job.num_segments {
        let done_count = resumable.iter().filter(|s| s.status == SegmentStatus::Completed).count();
        info!(
            "Resuming download {} — {} segments ({} already complete)",
            job.id, resumable.len(), done_count
        );
        resumable
    } else {
        info!("Splitting {} bytes into {} segments", job.total_bytes, job.num_segments);
        split_into_segments(&job.id, job.total_bytes, job.num_segments)
    };

    // Every segment writes into its own slice of this one file, so it must exist
    // at full size before any of them seeks into it. On a resume the file is
    // already the right length and `set_len` is a no-op.
    {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&part)
            .with_context(|| format!("Cannot create {}", part.display()))?;
        file.set_len(job.total_bytes)
            .with_context(|| format!("Cannot pre-allocate {}", part.display()))?;
    }

    // Segments write in place, leaving no on-disk trace of how far they got, so
    // their rows must exist before they start — the reporter keeps them current
    // and a crash resumes from there.
    {
        let db_lock = db.lock().await;
        for segment in &segments {
            db_lock.upsert_segment(segment)?;
        }
    }

    // Bytes already present on disk across all segments (completed + partial).
    // Used to initialise the progress counter so the UI shows the correct starting position.
    let already_on_disk: u64 = segments.iter().map(|s| s.downloaded).sum();

    // Cumulative bytes for the progress bar. The reporter task owns the writes;
    // this is read once at the end to record where an interrupted job stopped.
    let total_downloaded = Arc::new(AtomicU64::new(already_on_disk));
    let total_bytes      = job.total_bytes;
    let job_id           = job.id.clone();
    let url              = job.url.clone();
    let headers          = job.headers.clone();
    let cookies          = job.cookies.clone();

    // Emit an initial progress event so the UI snaps to the correct position immediately.
    if already_on_disk > 0 {
        let _ = app_handle.emit(
            "download_progress",
            ProgressEvent::plain(job_id.clone(), already_on_disk, total_bytes, 0, 0),
        );
    }

    // Segments stream their running byte totals here; one reporter task turns the
    // whole set into a single smoothed progress event on a fixed tick.
    let (progress_tx, progress_rx) = tokio::sync::mpsc::unbounded_channel::<(usize, u64)>();
    let mut seg_bytes: HashMap<usize, u64> = HashMap::new();

    // Partition segments: fully done ones go straight to `completed`; the rest get a task.
    let mut handles:   Vec<tokio::task::JoinHandle<Result<Segment>>> = Vec::new();
    let mut completed: Vec<Segment> = Vec::new();

    for segment in segments {
        // Seed every segment's contribution, so the total is right before the
        // first chunk arrives and a skipped segment still counts.
        seg_bytes.insert(segment.index_num, segment.downloaded);

        if segment.status == SegmentStatus::Completed {
            completed.push(segment);
            continue;
        }

        let url         = url.clone();
        let headers     = headers.clone();
        let cookies     = cookies.clone();
        let db          = db.clone();
        let control     = control.clone();
        let progress_tx = progress_tx.clone();
        let part        = part.clone();

        handles.push(tokio::spawn(async move {
            let result = download_segment(
                segment,
                &url,
                headers.as_ref(),
                cookies.as_deref(),
                &control,
                &progress_tx,
                &part,
            )
            .await;

            match &result {
                Ok(seg) => {
                    if let Ok(db) = db.try_lock() {
                        let _ = db.upsert_segment(seg);
                    }
                }
                Err(e) => error!("Segment task error: {}", e),
            }

            result
        }));
    }

    // The reporter stops when the last segment task drops its sender, so this
    // copy must go too or it will wait forever.
    drop(progress_tx);

    let reporter = tokio::spawn(report_progress(
        app_handle.clone(),
        db.clone(),
        job_id.clone(),
        total_bytes,
        total_downloaded.clone(),
        seg_bytes,
        progress_rx,
    ));

    // Await all spawned segment tasks.
    let mut all_ok      = true;
    let mut interrupted = false;

    for handle in handles {
        match handle.await {
            Ok(Ok(seg)) => {
                if seg.status == SegmentStatus::Interrupted {
                    interrupted = true;
                }
                completed.push(seg);
            }
            Ok(Err(e)) => { error!("Segment failed: {}", e); all_ok = false; }
            Err(e)     => { error!("Task join error: {}", e); all_ok = false; }
        }
    }

    // Every sender is dropped now, so the reporter drains and exits. Waiting for it
    // guarantees `total_downloaded` holds the final count before we read it below.
    let _ = reporter.await;

    // A stop wins over a failure: if the user pulled the plug, a half-read socket
    // erroring out on the way down is expected, not a real failure to report.
    if interrupted {
        let reason = control
            .interrupt_reason(&job.id)
            .unwrap_or(Interrupt::Paused);

        // Persist exact byte counts so the next run resumes from the right offsets.
        // The part file keeps the bytes; these rows are what say where they end.
        if reason == Interrupt::Paused {
            let db_lock = db.lock().await;
            for seg in &completed {
                let _ = db_lock.upsert_segment(seg);
            }
        }

        job.downloaded = total_downloaded.load(Ordering::Relaxed);
        return Ok(DownloadOutcome::Stopped(reason));
    }

    if !all_ok {
        return Err(anyhow::anyhow!("One or more segments failed to download"));
    }

    // The bytes are already in place; finishing is a checksum and a rename.
    finalize_download(&part, output_path, total_bytes)
        .await
        .map(DownloadOutcome::Finished)
}

// ── Single-stream fallback (no Accept-Ranges) ─────────────────────────────────

async fn single_stream_download(
    job:         &mut DownloadJob,
    output_path: &str,
    client:      &reqwest::Client,
    app_handle:  &AppHandle,
    control:     &Arc<DownloadControl>,
) -> Result<DownloadOutcome> {
    info!("Server does not support range requests — single stream download");

    let response = http::apply_captured(
        client.get(&job.url),
        job.headers.as_ref(),
        job.cookies.as_deref(),
    )
    .send()
    .await?;

    // Checked before the file is created: an error response still carries a body,
    // and writing it would leave the error page sitting on disk under the real
    // filename.
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!(
            "server rejected the request for '{}': HTTP {}",
            job.filename,
            status
        );
    }

    let mut file        = tokio::fs::File::create(output_path).await?;
    let mut stream      = response.bytes_stream();
    let mut downloaded  = 0u64;
    let start_time      = Instant::now();
    let total_bytes     = job.total_bytes;
    let job_id          = job.id.clone();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        throttle::consume(chunk.len() as u64).await;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        if let Some(reason) = control.interrupt_reason(&job_id) {
            file.flush().await?;
            drop(file);

            // This branch runs only when the server refused range requests, so a
            // half-written file can never be resumed — discard it rather than
            // leave truncated bytes that look like a real download.
            let _ = tokio::fs::remove_file(output_path).await;
            job.downloaded = 0;

            info!("Single-stream download stopped ({:?}); partial file discarded", reason);
            return Ok(DownloadOutcome::Stopped(reason));
        }

        let elapsed = start_time.elapsed().as_secs_f64().max(0.001);
        let speed   = (downloaded as f64 / elapsed) as u64;
        let eta     = if speed > 0 {
            total_bytes.saturating_sub(downloaded) / speed
        } else {
            0
        };

        let _ = app_handle.emit(
            "download_progress",
            ProgressEvent::plain(job_id.clone(), downloaded, total_bytes, speed, eta),
        );
    }

    file.flush().await?;
    drop(file);
    job.downloaded = downloaded;

    // An empty body is never a real download. Discard it so a dead link cannot
    // masquerade as a completed file.
    if downloaded == 0 {
        let _ = tokio::fs::remove_file(output_path).await;
        anyhow::bail!(
            "server sent no data for '{}' — the link may have expired",
            job.filename
        );
    }

    calculate_sha256(output_path)
        .await
        .map(DownloadOutcome::Finished)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_types_are_recognized() {
        assert!(is_markup("text/html"));
        assert!(is_markup("TEXT/HTML"));
        assert!(is_markup("application/xhtml+xml"));
        assert!(!is_markup("application/zip"));
        assert!(!is_markup("application/octet-stream"));
    }

    #[test]
    fn binary_extensions_are_recognized() {
        assert!(expects_binary("God of War II (USA).zip"));
        assert!(expects_binary("disc.ISO"));
        assert!(expects_binary("movie.mkv"));
    }

    #[test]
    fn unknown_extensions_stay_permissive() {
        // The guard only fires on a confident mismatch, so anything it cannot
        // classify must fall through as non-binary.
        assert!(!expects_binary("index.html"));
        assert!(!expects_binary("README"));
        assert!(!expects_binary("data.somethingnew"));
    }
}
