//! Finalising a completed download.
//!
//! There is no merge step: segments write straight into their slices of the
//! `.fluxdm-part` file, so finishing a download is a checksum and a rename rather
//! than a second full copy of every byte.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::io::AsyncReadExt;
use tracing::info;

/// Verify the assembled part file, then move it into place under its real name.
///
/// Returns the SHA-256 of the finished file.
pub async fn finalize_download(part: &Path, output_path: &str, expected_len: u64) -> Result<String> {
    let actual = tokio::fs::metadata(part)
        .await
        .with_context(|| format!("Part file missing: {}", part.display()))?
        .len();

    // Every segment reported success, so a size mismatch means the pieces did not
    // cover the file. Better to fail loudly than to rename a file with a hole.
    if expected_len > 0 && actual != expected_len {
        return Err(anyhow::anyhow!(
            "Assembled file is {} bytes, expected {}",
            actual,
            expected_len
        ));
    }

    info!("Download assembled. Calculating checksum...");
    let checksum = calculate_sha256(&part.to_string_lossy()).await?;
    info!("SHA-256: {}", checksum);

    // `rename` refuses to clobber an existing file on Windows.
    if Path::new(output_path).exists() {
        tokio::fs::remove_file(output_path)
            .await
            .with_context(|| format!("Cannot replace existing file: {}", output_path))?;
    }
    tokio::fs::rename(part, output_path)
        .await
        .with_context(|| format!("Cannot move {} into place", part.display()))?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("fluxdm-merge-{}-{}", name, uuid::Uuid::new_v4()))
    }

    #[tokio::test]
    async fn moves_the_part_file_into_place_and_hashes_it() {
        let part = scratch("part");
        let out = scratch("out");
        std::fs::write(&part, b"hello").unwrap();

        let sum = finalize_download(&part, &out.to_string_lossy(), 5).await.unwrap();

        assert!(!part.exists(), "part file must be gone");
        assert_eq!(std::fs::read(&out).unwrap(), b"hello");
        // Known SHA-256 of "hello".
        assert_eq!(sum, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");

        let _ = std::fs::remove_file(out);
    }

    /// A short part file means the segments left a hole. Renaming it would hand
    /// the user a corrupt file that looks complete.
    #[tokio::test]
    async fn refuses_to_publish_a_file_of_the_wrong_size() {
        let part = scratch("short");
        let out = scratch("short-out");
        std::fs::write(&part, b"hi").unwrap();

        let err = finalize_download(&part, &out.to_string_lossy(), 99)
            .await
            .expect_err("must reject a size mismatch");

        assert!(err.to_string().contains("expected 99"), "got: {}", err);
        assert!(!out.exists(), "nothing may be published");
        let _ = std::fs::remove_file(part);
    }

    /// Re-downloading over an existing file must replace it; `rename` alone would
    /// fail on Windows.
    #[tokio::test]
    async fn replaces_an_existing_output_file() {
        let part = scratch("replace");
        let out = scratch("replace-out");
        std::fs::write(&part, b"new").unwrap();
        std::fs::write(&out, b"stale").unwrap();

        finalize_download(&part, &out.to_string_lossy(), 3).await.unwrap();

        assert_eq!(std::fs::read(&out).unwrap(), b"new");
        let _ = std::fs::remove_file(out);
    }
}
