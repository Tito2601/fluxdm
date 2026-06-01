/// Rule-based file categorizer.
///
/// Uses MIME type → file extension → URL pattern in descending priority.
/// No ML model required — works fully offline.

/// Categorize a download into one of the standard categories.
/// Returns the category as a lowercase string.
pub fn categorize_download(url: &str, filename: &str, mime: &str) -> String {
    // ── 1. MIME type (most authoritative) ────────────────────────────────
    let mime_lower = mime.to_lowercase();
    if mime_lower.starts_with("video/") {
        return "videos".to_string();
    }
    if mime_lower.starts_with("audio/") {
        return "music".to_string();
    }
    if mime_lower.starts_with("image/") {
        return "images".to_string();
    }
    if mime_lower.contains("pdf")
        || mime_lower.starts_with("text/")
        || mime_lower.contains("word")
        || mime_lower.contains("spreadsheet")
        || mime_lower.contains("presentation")
    {
        return "documents".to_string();
    }
    if mime_lower.contains("zip")
        || mime_lower.contains("rar")
        || mime_lower.contains("x-7z")
        || mime_lower.contains("x-tar")
        || mime_lower.contains("gzip")
        || mime_lower.contains("bzip2")
        || mime_lower.contains("iso")
    {
        return "archives".to_string();
    }

    // ── 2. File extension ─────────────────────────────────────────────────
    let ext = extract_extension(filename);
    match ext.as_str() {
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "m4v" | "ts" | "vob" => {
            return "videos".to_string()
        }
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "opus" | "wma" | "alac" => {
            return "music".to_string()
        }
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "epub" | "mobi"
        | "odt" | "ods" | "odp" | "rtf" | "md" => return "documents".to_string(),
        "exe" | "msi" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" | "apk" | "ipa" => {
            return "software".to_string()
        }
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "ico" | "bmp" | "tiff" | "raw"
        | "heic" => return "images".to_string(),
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "iso" | "tar.gz" | "tgz"
        | "tar.bz2" | "tar.xz" => return "archives".to_string(),
        _ => {}
    }

    // ── 3. URL pattern heuristics ─────────────────────────────────────────
    let url_lower = url.to_lowercase();
    if url_lower.contains("/download")
        || url_lower.contains("/release")
        || url_lower.contains("/installer")
        || url_lower.contains("/setup")
        || url_lower.contains("/artifact")
    {
        return "software".to_string();
    }
    if url_lower.contains("/video")
        || url_lower.contains("/stream")
        || url_lower.contains("/watch")
        || url_lower.contains("youtube")
        || url_lower.contains("vimeo")
    {
        return "videos".to_string();
    }

    "other".to_string()
}

/// Extract the lowercase extension from a filename (without the dot).
fn extract_extension(filename: &str) -> String {
    std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default()
}

/// Suggest a subdirectory path based on category.
#[allow(dead_code)]
pub fn suggested_subfolder(category: &str) -> &'static str {
    match category {
        "videos"    => "Videos",
        "music"     => "Music",
        "documents" => "Documents",
        "software"  => "Software",
        "images"    => "Images",
        "archives"  => "Archives",
        _           => "Downloads",
    }
}
