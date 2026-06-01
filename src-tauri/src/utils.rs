/// Cross-platform path helpers shared across the engine and server layers.

use std::path::{Path, PathBuf};
use anyhow::{Context, Result};

// ── Tilde expansion ───────────────────────────────────────────────────────────

/// Expand a leading `~` to the current user's home directory.
///
/// - `~/foo/bar` → `C:\Users\alice\foo\bar`  (Windows)
/// - `~/foo/bar` → `/home/alice/foo/bar`      (Linux/macOS)
/// - Any other path is returned unchanged.
pub fn expand_path(path: &str) -> String {
    if path == "~" {
        return home_dir().to_string_lossy().into_owned();
    }
    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        // Normalise separators so the tail always uses the OS separator
        let tail = rest.replace(['/', '\\'], std::path::MAIN_SEPARATOR_STR.chars().next().unwrap_or('/').to_string().as_str());
        return home_dir().join(tail).to_string_lossy().into_owned();
    }
    path.to_string()
}

fn home_dir() -> PathBuf {
    // USERPROFILE is the canonical home on Windows; HOME on Unix
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

// ── Directory creation ────────────────────────────────────────────────────────

/// Create all parent directories for `file_path`, returning an error with
/// a clear message if creation fails.
pub fn ensure_parent_dir(file_path: &str) -> Result<()> {
    let path = Path::new(file_path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }
    }
    Ok(())
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilde_expands_to_non_empty() {
        let result = expand_path("~/Downloads");
        assert!(!result.starts_with('~'), "tilde was not expanded: {}", result);
        assert!(result.contains("Downloads"), "path: {}", result);
    }

    #[test]
    fn plain_path_unchanged() {
        let p = "/tmp/test";
        assert_eq!(expand_path(p), p);
    }

    #[test]
    fn windows_absolute_unchanged() {
        let p = r"C:\Users\alice\Downloads";
        assert_eq!(expand_path(p), p);
    }
}
