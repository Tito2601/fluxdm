//! HLS/DASH stream downloader — Phase 5.
//!
//! `probe_stream(url)` detects whether a URL is an HLS or DASH stream and
//! returns the available quality variants for the UI to display.
//!
//! `download_stream(job, …)` fetches each media segment in order and appends
//! it to the output file, emitting the same Tauri events as normal downloads.

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use crate::engine::downloader::{DownloadJob, DownloadStatus, ProgressEvent};
use crate::engine::hls::{self, M3u8};
use crate::engine::http;
use crate::engine::throttle;
use crate::engine::dash;
use crate::storage::db::Database;
use crate::utils::{ensure_parent_dir, expand_path};

// ── Public types ──────────────────────────────────────────────────────────────

/// A single downloadable quality option for a stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamQuality {
    pub index:      usize,
    pub label:      String,
    pub bandwidth:  u64,
    pub resolution: Option<String>,
    pub codecs:     Option<String>,
    /// For HLS: the media playlist URL.
    /// For DASH: the MPD URL (same for all qualities).
    pub url:        String,
    /// DASH representation ID (None for HLS).
    pub repr_id:    Option<String>,
}

/// Result of probing a URL — tells the UI what stream type was found and
/// which quality options are available.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamInfo {
    /// `"hls"`, `"dash"`, or `"direct"` (not a stream).
    pub stream_type:      String,
    pub qualities:        Vec<StreamQuality>,
    pub duration_seconds: Option<f64>,
    pub title:            Option<String>,
}

// ── Probe ─────────────────────────────────────────────────────────────────────

/// Fetch `url` and determine whether it is an HLS or DASH stream.
/// Returns the quality list so the UI can show a picker.
pub async fn probe_stream(url: &str) -> Result<StreamInfo> {
    let client = http::client();

    let response = client
        .get(url)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?;

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let url_lower = url.to_lowercase();
    let is_hls = content_type.contains("mpegurl")
        || url_lower.ends_with(".m3u8")
        || url_lower.contains(".m3u8?");
    let is_dash = content_type.contains("dash+xml")
        || url_lower.ends_with(".mpd")
        || url_lower.contains(".mpd?");

    let body = response.text().await?;

    if is_hls {
        probe_hls(&body, url)
    } else if is_dash {
        probe_dash(&body, url)
    } else {
        // Return a "direct" descriptor — the caller falls back to a normal download
        Ok(StreamInfo {
            stream_type: "direct".into(),
            qualities: vec![StreamQuality {
                index:      0,
                label:      "Direct".into(),
                bandwidth:  0,
                resolution: None,
                codecs:     None,
                url:        url.to_string(),
                repr_id:    None,
            }],
            duration_seconds: None,
            title:            None,
        })
    }
}

fn probe_hls(content: &str, base_url: &str) -> Result<StreamInfo> {
    match hls::parse_m3u8(content, base_url).map_err(|e| anyhow::anyhow!(e))? {
        M3u8::Master(variants) => {
            let qualities = variants
                .into_iter()
                .enumerate()
                .map(|(i, v)| {
                    let label = match &v.resolution {
                        Some(res) => {
                            let height = res.split('x').nth(1).unwrap_or(res.as_str());
                            match v.frame_rate {
                                Some(fps) if fps > 50.0 => format!("{}p {:.0}fps", height, fps),
                                _ => format!("{}p", height),
                            }
                        }
                        None => format!("{:.0} Kbps", v.bandwidth as f64 / 1000.0),
                    };
                    StreamQuality {
                        index:      i,
                        label,
                        bandwidth:  v.bandwidth,
                        resolution: v.resolution,
                        codecs:     v.codecs,
                        url:        v.url,
                        repr_id:    None,
                    }
                })
                .collect();

            Ok(StreamInfo {
                stream_type:      "hls".into(),
                qualities,
                duration_seconds: None,
                title:            None,
            })
        }

        M3u8::Media { segments: _, total_duration_secs } => {
            // Directly a media playlist (no quality variants)
            Ok(StreamInfo {
                stream_type: "hls".into(),
                qualities: vec![StreamQuality {
                    index:      0,
                    label:      "Stream".into(),
                    bandwidth:  0,
                    resolution: None,
                    codecs:     None,
                    url:        base_url.to_string(),
                    repr_id:    None,
                }],
                duration_seconds: Some(total_duration_secs as f64),
                title:            None,
            })
        }
    }
}

fn probe_dash(content: &str, base_url: &str) -> Result<StreamInfo> {
    let streams  = dash::parse_mpd(content, base_url).map_err(|e| anyhow::anyhow!(e))?;
    let duration = dash::extract_duration(content);

    let qualities = streams
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            let label = match s.height {
                Some(h) => format!("{}p", h),
                None    => format!("{:.0} Kbps", s.bandwidth as f64 / 1000.0),
            };
            StreamQuality {
                index:      i,
                label,
                bandwidth:  s.bandwidth,
                resolution: s.width.zip(s.height).map(|(w, h)| format!("{}x{}", w, h)),
                codecs:     s.codecs.clone(),
                // MPD URL is the same for all representations;
                // repr_id disambiguates which one to download.
                url:        base_url.to_string(),
                repr_id:    Some(s.id.clone()),
            }
        })
        .collect();

    Ok(StreamInfo {
        stream_type:      "dash".into(),
        qualities,
        duration_seconds: duration,
        title:            None,
    })
}

// ── Download ──────────────────────────────────────────────────────────────────

/// Download a stream (HLS or DASH), emitting the same Tauri events as
/// normal parallel downloads so the UI reacts identically.
pub async fn download_stream(
    mut job:         DownloadJob,
    stream_type:     String,
    repr_id:         Option<String>,
    app_handle:      AppHandle,
    db:              Arc<Mutex<Database>>,
) -> Result<()> {
    info!(
        "Stream download start: {} ({}) → {}/{}",
        job.url, stream_type, job.save_path, job.filename
    );

    let save_path = expand_path(&job.save_path);
    let out_path  = format!("{}/{}", save_path, job.filename);
    ensure_parent_dir(&out_path)?;

    job.status = DownloadStatus::Downloading;
    db.lock().await.upsert_download(&job).map_err(|e| anyhow::anyhow!(e))?;

    let result = match stream_type.as_str() {
        "dash" => download_dash(&mut job, repr_id.as_deref(), &out_path, &app_handle).await,
        _      => download_hls(&mut job, &out_path, &app_handle).await,
    };

    match result {
        Ok(written) => {
            job.status       = DownloadStatus::Completed;
            job.downloaded   = written;
            job.completed_at = Some(Utc::now().to_rfc3339());
            db.lock().await.upsert_download(&job).map_err(|e| anyhow::anyhow!(e))?;

            app_handle.emit("download_complete", serde_json::json!({
                "id":       job.id,
                "savePath": out_path,
                "checksum": "",          // checksum skipped for large video files
            })).ok();
            info!("Stream download complete: {}", job.filename);
        }
        Err(e) => {
            error!("Stream download failed ({}): {}", job.filename, e);
            job.status = DownloadStatus::Failed(e.to_string());
            db.lock().await.upsert_download(&job).map_err(|e2| anyhow::anyhow!(e2))?;

            app_handle.emit("download_error", serde_json::json!({
                "id":    job.id,
                "error": e.to_string(),
            })).ok();
        }
    }

    Ok(())
}

// ── HLS downloader ────────────────────────────────────────────────────────────

async fn download_hls(
    job:        &mut DownloadJob,
    out_path:   &str,
    app_handle: &AppHandle,
) -> Result<u64> {
    let client = http::client();

    // Fetch the media playlist
    let response = client
        .get(&job.url)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await?;
    let content  = response.text().await?;

    let segments = match hls::parse_m3u8(&content, &job.url).map_err(|e| anyhow::anyhow!(e))? {
        M3u8::Media { segments, total_duration_secs } => {
            // Estimate total bytes from bitrate-based heuristic (streams rarely have Content-Length)
            job.total_bytes = (total_duration_secs * 500_000.0) as u64; // ~500 KB/s rough estimate
            segments
        }
        M3u8::Master(_) => {
            anyhow::bail!("Expected an HLS media playlist but got a master playlist. Pick a quality first.");
        }
    };

    let total_segs = segments.len();
    info!("HLS: {} segments to download", total_segs);

    let mut out = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(out_path)
        .await?;

    let mut downloaded  = 0u64;
    let start_time      = std::time::Instant::now();

    for (i, seg) in segments.iter().enumerate() {
        let mut resp = client.get(&seg.url).send().await?;

        while let Some(chunk) = resp.chunk().await? {
            throttle::consume(chunk.len() as u64).await;
            out.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
        }

        let elapsed = start_time.elapsed().as_secs_f64().max(0.01);
        let speed   = (downloaded as f64 / elapsed) as u64;
        let done    = (i + 1) as f64 / total_segs as f64;
        let eta     = if done > 0.0 { ((1.0 - done) / done * elapsed) as u64 } else { 0 };

        job.downloaded = downloaded;
        job.speed_bps  = speed;
        job.total_bytes = job.total_bytes.max(downloaded); // update as we go

        app_handle.emit(
            "download_progress",
            ProgressEvent::plain(job.id.clone(), downloaded, job.total_bytes, speed, eta),
        ).ok();
    }

    out.flush().await?;
    Ok(downloaded)
}

// ── DASH downloader ───────────────────────────────────────────────────────────

async fn download_dash(
    job:        &mut DownloadJob,
    repr_id:    Option<&str>,
    out_path:   &str,
    app_handle: &AppHandle,
) -> Result<u64> {
    let client = http::client();

    // Fetch the MPD manifest
    let response = client
        .get(&job.url)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await?;
    let content  = response.text().await?;
    let streams  = dash::parse_mpd(&content, &job.url).map_err(|e| anyhow::anyhow!(e))?;

    // Find the requested representation (or best quality)
    let stream = if let Some(id) = repr_id {
        streams.iter().find(|s| s.id == id)
            .ok_or_else(|| anyhow::anyhow!("DASH representation '{}' not found in MPD", id))?
    } else {
        streams.first().ok_or_else(|| anyhow::anyhow!("No video streams in MPD"))?
    };

    let (init_url, seg_urls) = stream.segment_urls();
    if seg_urls.is_empty() {
        warn!("DASH: no segment URLs generated — possibly missing segment count in MPD");
    }

    let total_count = seg_urls.len() + if init_url.is_some() { 1 } else { 0 };
    info!("DASH: {} segments (+ {} init)", seg_urls.len(), if init_url.is_some() { 1 } else { 0 });

    let mut out = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(out_path)
        .await?;

    let mut downloaded  = 0u64;
    let mut done_count  = 0usize;
    let start_time      = std::time::Instant::now();

    // Download initialization segment first
    if let Some(url) = &init_url {
        let mut resp = client.get(url).send().await?;
        while let Some(chunk) = resp.chunk().await? {
            throttle::consume(chunk.len() as u64).await;
            out.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
        }
        done_count += 1;
    }

    // Download media segments
    for url in &seg_urls {
        let resp = client.get(url).send().await?;

        // A 404 signals end of stream for live-to-VOD manifests with open-ended count
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            info!("DASH: got 404 — treating as end of stream at segment {}", done_count);
            break;
        }

        let mut resp = resp;
        while let Some(chunk) = resp.chunk().await? {
            throttle::consume(chunk.len() as u64).await;
            out.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
        }

        done_count += 1;
        let elapsed = start_time.elapsed().as_secs_f64().max(0.01);
        let speed   = (downloaded as f64 / elapsed) as u64;
        let done    = done_count as f64 / total_count.max(1) as f64;
        let eta     = if done > 0.0 { ((1.0 - done) / done * elapsed) as u64 } else { 0 };

        job.downloaded  = downloaded;
        job.speed_bps   = speed;
        job.total_bytes = job.total_bytes.max(downloaded);

        app_handle.emit(
            "download_progress",
            ProgressEvent::plain(job.id.clone(), downloaded, job.total_bytes, speed, eta),
        ).ok();
    }

    out.flush().await?;
    Ok(downloaded)
}
