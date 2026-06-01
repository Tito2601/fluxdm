/// Duplicate and existing-file detection.
///
/// Before queueing a download, FluxDM checks:
///   1. Was this URL already downloaded (via its SHA-256 URL hash)?
///   2. Does the output file already exist on disk?
///
/// Results are returned as `DuplicateCheck` and exposed via `cmd_check_duplicate`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::storage::db::Database;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateCheck {
    /// True if this exact URL appears in the completed history.
    pub is_url_duplicate: bool,
    /// Filename used in the previous download (if any).
    pub previous_filename: Option<String>,
    /// Save path of the previous download (if any).
    pub previous_save_path: Option<String>,
    /// When the previous download completed (ISO-8601 string, if any).
    pub previous_completed_at: Option<String>,
    /// True if the computed output file path already exists on disk.
    pub file_exists: bool,
    /// The resolved output path that was checked.
    pub output_path: String,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Check whether `url` was already downloaded and/or `output_path` exists on disk.
pub fn check_duplicate(url: &str, output_path: &str, db: &Database) -> DuplicateCheck {
    let url_hash = sha256_hex(url);

    let (is_url_duplicate, previous_filename, previous_save_path, previous_completed_at) =
        match db.find_history_by_url_hash(&url_hash) {
            Ok(Some((filename, save_path, completed_at))) => {
                (true, Some(filename), Some(save_path), Some(completed_at))
            }
            _ => (false, None, None, None),
        };

    let file_exists = std::path::Path::new(output_path).exists();

    DuplicateCheck {
        is_url_duplicate,
        previous_filename,
        previous_save_path,
        previous_completed_at,
        file_exists,
        output_path: output_path.to_string(),
    }
}

/// SHA-256 hex digest of a string (for URL hashing).
pub fn sha256_hex(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
