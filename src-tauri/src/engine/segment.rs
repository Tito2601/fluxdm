use anyhow::Result;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};
use uuid::Uuid;

/// Deterministic temp-file path for a segment — shared by downloader and resume logic.
pub fn seg_temp_path(download_id: &str, index_num: usize) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("fluxdm_{}_{}.seg", download_id, index_num))
}

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SegmentStatus {
    Pending,
    Downloading,
    Completed,
    Failed,
}

impl SegmentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SegmentStatus::Pending     => "pending",
            SegmentStatus::Downloading => "downloading",
            SegmentStatus::Completed   => "completed",
            SegmentStatus::Failed      => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub id:             String,
    pub download_id:    String,
    pub index_num:      usize,
    pub byte_start:     u64,
    pub byte_end:       u64,
    pub downloaded:     u64,
    pub status:         SegmentStatus,
    pub temp_file_path: Option<String>,
}

impl Segment {
    pub fn new(
        download_id: &str,
        index_num: usize,
        byte_start: u64,
        byte_end: u64,
    ) -> Self {
        Self {
            id:             Uuid::new_v4().to_string(),
            download_id:    download_id.to_string(),
            index_num,
            byte_start,
            byte_end,
            downloaded:     0,
            status:         SegmentStatus::Pending,
            temp_file_path: None,
        }
    }

    /// Total number of bytes this segment should download.
    pub fn expected_bytes(&self) -> u64 {
        self.byte_end - self.byte_start + 1
    }
}

// ── Download worker ───────────────────────────────────────────────────────────

/// Download a single byte-range segment to a temp file.
/// Resumes automatically if a partial temp file already exists on disk.
/// Retries up to 3 times on network failure.
pub async fn download_segment(
    mut segment: Segment,
    url: &str,
    cookies: Option<&str>,
) -> Result<Segment> {
    use reqwest::header::RANGE;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let temp_path = seg_temp_path(&segment.download_id, segment.index_num);
    let expected  = segment.expected_bytes();
    let mut last_error = String::new();

    for attempt in 1..=3_u8 {
        // How many bytes are already written for this segment on disk?
        let on_disk: u64 = if temp_path.exists() {
            std::fs::metadata(&temp_path)
                .map(|m| m.len())
                .unwrap_or(0)
                .min(expected)
        } else {
            0
        };

        // Segment is already fully on disk — no network request needed.
        if on_disk >= expected {
            info!("Segment {} already complete on disk ({} bytes)", segment.index_num, on_disk);
            segment.downloaded     = on_disk;
            segment.status         = SegmentStatus::Completed;
            segment.temp_file_path = Some(temp_path.to_string_lossy().to_string());
            return Ok(segment);
        }

        let range_start  = segment.byte_start + on_disk;
        let range_header = format!("bytes={}-{}", range_start, segment.byte_end);

        if on_disk > 0 {
            info!(
                "Segment {}: resuming from offset {} ({} bytes already on disk)",
                segment.index_num, range_start, on_disk
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

                // We sent a partial-range request but got a full 200 response —
                // the server ignored our Range header. Discard the partial file and retry.
                if on_disk > 0 && http_status == 200 {
                    warn!(
                        "Segment {}: server returned 200 for partial range, restarting segment",
                        segment.index_num
                    );
                    let _ = tokio::fs::remove_file(&temp_path).await;
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

                // Append to a partial file; create fresh otherwise.
                let mut file = if on_disk > 0 {
                    tokio::fs::OpenOptions::new()
                        .append(true)
                        .open(&temp_path)
                        .await?
                } else {
                    tokio::fs::File::create(&temp_path).await?
                };

                let mut stream     = response.bytes_stream();
                let mut downloaded = on_disk; // includes bytes already on disk
                let mut stream_ok  = true;

                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => {
                            file.write_all(&chunk).await?;
                            downloaded += chunk.len() as u64;
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

                file.flush().await?;

                if downloaded > on_disk && stream_ok {
                    info!(
                        "Segment {} complete: {} bytes (resumed from {} on disk)",
                        segment.index_num, downloaded, on_disk
                    );
                    segment.downloaded     = downloaded;
                    segment.status         = SegmentStatus::Completed;
                    segment.temp_file_path = Some(temp_path.to_string_lossy().to_string());
                    return Ok(segment);
                }

                last_error = "Zero new bytes received".to_string();
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

    segment.status = SegmentStatus::Failed;
    Err(anyhow::anyhow!(
        "Segment {} failed after 3 attempts: {}",
        segment.index_num,
        last_error
    ))
}
