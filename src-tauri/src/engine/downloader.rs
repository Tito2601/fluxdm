use anyhow::Result;
use chrono::Utc;
use futures::StreamExt;
use reqwest::header::{ACCEPT_RANGES, CONTENT_LENGTH};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::engine::merger::{calculate_sha256, merge_segments};
use crate::engine::segment::{download_segment, Segment, SegmentStatus};
use crate::storage::db::Database;

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
) -> Result<()> {
    info!("Starting download: {} → {}/{}", job.url, job.save_path, job.filename);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    // ── Step 1: HEAD request ──────────────────────────────────────────────
    let head = client
        .head(&job.url)
        .header("User-Agent", "FluxDM/0.1")
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
    let _ = app_handle.emit("download_progress", ProgressEvent {
        id:               job.id.clone(),
        downloaded_bytes: 0,
        total_bytes:      content_length,
        speed_bps:        0,
        eta_seconds:      0,
    });

    // Expand ~ and guarantee the parent directory exists before any I/O.
    let save_dir    = crate::utils::expand_path(&job.save_path);
    let output_path = format!("{}/{}", save_dir, job.filename);
    crate::utils::ensure_parent_dir(&output_path)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // ── Step 2: Download ──────────────────────────────────────────────────
    let result = if supports_ranges && content_length > 0 {
        multi_segment_download(&mut job, &output_path, &client, &app_handle, &db).await
    } else {
        single_stream_download(&mut job, &output_path, &client, &app_handle).await
    };

    // ── Step 3: Finalize ──────────────────────────────────────────────────
    match result {
        Ok(checksum) => {
            job.status       = DownloadStatus::Completed;
            job.checksum     = Some(checksum.clone());
            job.downloaded   = content_length;
            job.completed_at = Some(Utc::now().to_rfc3339());

            let _ = app_handle.emit("download_complete", serde_json::json!({
                "id":        job.id,
                "save_path": output_path,
                "checksum":  checksum,
            }));

            info!("Download complete: {}", job.filename);

            let db = db.lock().await;
            db.upsert_download(&job)?;
            db.add_to_history(&job)?;
        }
        Err(e) => {
            error!("Download failed for {}: {}", job.filename, e);
            job.status = DownloadStatus::Failed(e.to_string());

            let _ = app_handle.emit("download_error", serde_json::json!({
                "id":    job.id,
                "error": e.to_string(),
            }));

            db.lock().await.upsert_download(&job)?;
        }
    }

    Ok(())
}

// ── Multi-segment parallel download ──────────────────────────────────────────

async fn multi_segment_download(
    job:         &mut DownloadJob,
    output_path: &str,
    _client:     &reqwest::Client,
    app_handle:  &AppHandle,
    db:          &Arc<Mutex<Database>>,
) -> Result<String> {
    // Check for resumable segments from a previous (interrupted) attempt.
    let resumable = {
        let db_lock = db.lock().await;
        crate::engine::resume::find_resumable_segments(&job.id, &*db_lock)
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

    // Bytes already present on disk across all segments (completed + partial).
    // Used to initialise the progress counter so the UI shows the correct starting position.
    let already_on_disk: u64 = segments.iter().map(|s| s.downloaded).sum();

    // total_downloaded: cumulative bytes for the progress bar (starts at already_on_disk).
    // session_bytes:    only bytes fetched in *this* session, used for accurate speed.
    let total_downloaded  = Arc::new(AtomicU64::new(already_on_disk));
    let session_bytes     = Arc::new(AtomicU64::new(0));
    let last_speed_record = Arc::new(AtomicU64::new(0));
    let start_time        = Instant::now();
    let total_bytes       = job.total_bytes;
    let job_id            = job.id.clone();
    let url               = job.url.clone();
    let cookies           = job.cookies.clone();

    // Emit an initial progress event so the UI snaps to the correct position immediately.
    if already_on_disk > 0 {
        let _ = app_handle.emit("download_progress", ProgressEvent {
            id:               job_id.clone(),
            downloaded_bytes: already_on_disk,
            total_bytes,
            speed_bps:        0,
            eta_seconds:      0,
        });
    }

    // Partition segments: fully done ones go straight to `completed`; the rest get a task.
    let mut handles:   Vec<tokio::task::JoinHandle<Result<Segment>>> = Vec::new();
    let mut completed: Vec<Segment> = Vec::new();

    for segment in segments {
        if segment.status == SegmentStatus::Completed && segment.temp_file_path.is_some() {
            completed.push(segment);
            continue;
        }

        // Capture initial on-disk bytes so we can derive newly_downloaded in the task.
        let initial_on_disk   = segment.downloaded;
        let url               = url.clone();
        let cookies           = cookies.clone();
        let app_handle        = app_handle.clone();
        let job_id            = job_id.clone();
        let total_downloaded  = total_downloaded.clone();
        let session_bytes     = session_bytes.clone();
        let last_speed_record = last_speed_record.clone();
        let db                = db.clone();

        handles.push(tokio::spawn(async move {
            let result = download_segment(segment, &url, cookies.as_deref()).await;

            match &result {
                Ok(seg) => {
                    // Only count bytes that were actually fetched in this session.
                    let newly = seg.downloaded.saturating_sub(initial_on_disk);

                    let total   = total_downloaded.fetch_add(newly, Ordering::Relaxed) + newly;
                    let session = session_bytes.fetch_add(newly, Ordering::Relaxed) + newly;

                    let elapsed = start_time.elapsed().as_secs_f64().max(0.001);
                    let speed   = (session as f64 / elapsed) as u64;
                    let eta     = if speed > 0 {
                        total_bytes.saturating_sub(total) / speed
                    } else {
                        0
                    };

                    let _ = app_handle.emit("download_progress", ProgressEvent {
                        id:               job_id.clone(),
                        downloaded_bytes: total,
                        total_bytes,
                        speed_bps:        speed,
                        eta_seconds:      eta,
                    });

                    // Persist segment state + throttled speed sample (at most 1 per 5s).
                    if let Ok(db) = db.try_lock() {
                        let _ = db.upsert_segment(seg);

                        let now_secs = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let prev = last_speed_record.load(Ordering::Relaxed);
                        if now_secs.saturating_sub(prev) >= 5 {
                            last_speed_record.store(now_secs, Ordering::Relaxed);
                            let _ = db.record_speed_point(now_secs, speed, &job_id);
                        }
                    }
                }
                Err(e) => error!("Segment task error: {}", e),
            }

            result
        }));
    }

    // Await all spawned segment tasks.
    let mut all_ok = true;

    for handle in handles {
        match handle.await {
            Ok(Ok(seg))  => completed.push(seg),
            Ok(Err(e))   => { error!("Segment failed: {}", e); all_ok = false; }
            Err(e)       => { error!("Task join error: {}", e); all_ok = false; }
        }
    }

    if !all_ok {
        return Err(anyhow::anyhow!("One or more segments failed to download"));
    }

    merge_segments(completed, output_path).await
}

// ── Single-stream fallback (no Accept-Ranges) ─────────────────────────────────

async fn single_stream_download(
    job:         &mut DownloadJob,
    output_path: &str,
    client:      &reqwest::Client,
    app_handle:  &AppHandle,
) -> Result<String> {
    info!("Server does not support range requests — single stream download");

    let response = client
        .get(&job.url)
        .header("User-Agent", "FluxDM/0.1")
        .send()
        .await?;

    let mut file        = tokio::fs::File::create(output_path).await?;
    let mut stream      = response.bytes_stream();
    let mut downloaded  = 0u64;
    let start_time      = Instant::now();
    let total_bytes     = job.total_bytes;
    let job_id          = job.id.clone();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        let elapsed = start_time.elapsed().as_secs_f64().max(0.001);
        let speed   = (downloaded as f64 / elapsed) as u64;
        let eta     = if speed > 0 {
            total_bytes.saturating_sub(downloaded) / speed
        } else {
            0
        };

        let _ = app_handle.emit("download_progress", ProgressEvent {
            id:               job_id.clone(),
            downloaded_bytes: downloaded,
            total_bytes,
            speed_bps:        speed,
            eta_seconds:      eta,
        });
    }

    file.flush().await?;
    job.downloaded = downloaded;

    calculate_sha256(output_path).await
}
