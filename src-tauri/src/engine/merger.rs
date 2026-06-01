use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::io::{Seek, SeekFrom, Write};
use tokio::io::AsyncReadExt;
use tracing::{debug, info};

use crate::engine::segment::Segment;

/// Merge all completed segments into the final output file.
///
/// Segments are written at their exact byte offsets so they can arrive in any order.
/// After merging, temp files are deleted and the SHA-256 checksum is returned.
pub async fn merge_segments(mut segments: Vec<Segment>, output_path: &str) -> Result<String> {
    // Sort by index to write in order (more cache-friendly for sequential reads)
    segments.sort_by_key(|s| s.index_num);

    info!(
        "Merging {} segments into '{}'",
        segments.len(),
        output_path
    );

    // Pre-allocate output file to the expected total size
    let total_size = segments
        .last()
        .map(|s| s.byte_end + 1)
        .unwrap_or(0);

    let mut output = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(output_path)
        .with_context(|| format!("Cannot create output file: {}", output_path))?;

    if total_size > 0 {
        output
            .set_len(total_size)
            .context("Failed to pre-allocate output file")?;
    }

    // Write each segment at its correct byte offset
    for segment in &segments {
        let temp_path = match &segment.temp_file_path {
            Some(p) => p.clone(),
            None => {
                return Err(anyhow::anyhow!(
                    "Segment {} has no temp file path",
                    segment.index_num
                ))
            }
        };

        let data = std::fs::read(&temp_path).with_context(|| {
            format!("Failed to read segment temp file: {}", temp_path)
        })?;

        output.seek(SeekFrom::Start(segment.byte_start))?;
        output.write_all(&data)?;

        debug!(
            "Written segment {} ({} bytes at offset {})",
            segment.index_num,
            data.len(),
            segment.byte_start
        );

        // Clean up temp file
        let _ = std::fs::remove_file(&temp_path);
    }

    output.flush()?;
    drop(output);

    info!("All segments merged. Calculating checksum...");
    let checksum = calculate_sha256(output_path).await?;
    info!("SHA-256: {}", checksum);

    Ok(checksum)
}

/// Calculate the SHA-256 checksum of a file using async I/O with 64 KB chunks.
pub async fn calculate_sha256(file_path: &str) -> Result<String> {
    let mut file = tokio::fs::File::open(file_path)
        .await
        .with_context(|| format!("Cannot open file for checksum: {}", file_path))?;

    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 65_536]; // 64 KB

    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        // sha2::Digest::update is sync; we alternate async reads with sync hashing
        sha2::Digest::update(&mut hasher, &buffer[..n]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}
