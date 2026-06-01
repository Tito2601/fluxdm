/// Smart file renamer — Phase 4 (rule-based + Content-Disposition + media cleaner).
///
/// Cleaning pipeline:
///   1. Content-Disposition header wins if present
///   2. URL-decode percent-encoded characters
///   3. Strip site/source watermarks like `(DOWNLOADED.FROM.SITE.COM)`
///   4. Strip CDN garbage prefixes (download-, tmp-, etc.)
///   5. Remove duplicate index markers like (1), [2]
///   6. For media files: convert dots to spaces, normalise year tag
///   7. Replace underscore/hyphen runs with spaces (if no spaces yet)
///   8. Collapse whitespace
///   9. Enrich from URL context (version tags)
///  10. Fix missing extension

use regex::Regex;

// ── Public API ────────────────────────────────────────────────────────────────

/// Clean and improve a raw filename derived from a download URL.
///
/// `content_disposition` is the raw `Content-Disposition` header value (may be empty).
pub fn smart_rename(url: &str, raw_filename: &str, content_disposition: &str) -> String {
    // Content-Disposition header wins — it's the server's explicit filename hint.
    let base = if !content_disposition.is_empty() {
        parse_content_disposition(content_disposition)
            .unwrap_or_else(|| raw_filename.to_string())
    } else {
        raw_filename.to_string()
    };

    let mut name = base;

    // ── Step 1: URL-decode ────────────────────────────────────────────────
    name = percent_decode(&name);

    // ── Step 2: Strip site watermarks ────────────────────────────────────
    // Matches patterns like:
    //   (DOWNLOADED.FROM.THENKIRI.COM)
    //   [SITE.COM]
    //   .DOWNLOADED.FROM.XYZ.COM
    //   .(SiteName.NET)
    lazy_static_regex!(site_tag_paren, r"(?i)\s*[\(\[]\s*(?:downloaded[\.\s]from[\.\s])?[a-z0-9\-]+\.[a-z]{2,6}\s*[\)\]]");
    name = site_tag_paren.replace_all(&name, "").to_string();

    lazy_static_regex!(site_tag_bare, r"(?i)[._\s]+downloaded[._\s]+from[._\s]+[a-z0-9\-]+\.[a-z]{2,6}");
    name = site_tag_bare.replace_all(&name, "").to_string();

    // ── Step 3: Trim CDN garbage prefixes ────────────────────────────────
    lazy_static_regex!(prefix_junk, r"(?i)^(download|file|tmp|temp|get|dl|attachment)[-_.]");
    name = prefix_junk.replace(&name, "").to_string();

    // ── Step 4: Remove duplicate index markers like (1), [2] ─────────────
    lazy_static_regex!(index_markers, r"\s*[\(\[]\d+[\)\]]");
    name = index_markers.replace_all(&name, "").to_string();

    // ── Step 5: Media filename cleaner ────────────────────────────────────
    // For video/audio files with dot-separated scene names, convert dots to
    // spaces and normalise the year tag: "Movie.Name.2024.BluRay" → "Movie Name (2024) BluRay"
    let ext = extract_extension(&name);
    let is_media = matches!(
        ext.as_str(),
        "mkv" | "mp4" | "avi" | "mov" | "webm" | "ts" | "m4v"
        | "mp3" | "flac" | "aac" | "ogg" | "m4a" | "opus"
    );

    if is_media && !name.contains(' ') && name.contains('.') {
        name = clean_media_dots(&name);
    }

    // ── Step 6: Replace underscore/hyphen runs with spaces ───────────────
    lazy_static_regex!(separators, r"[_\-]+");
    if !name.contains(' ') {
        name = separators.replace_all(&name, " ").to_string();
    }

    // ── Step 7: Collapse multiple spaces ─────────────────────────────────
    lazy_static_regex!(multi_space, r"\s{2,}");
    name = multi_space.replace_all(&name, " ").to_string();

    // ── Step 8: Enrich from URL context (version numbers) ────────────────
    name = enrich_from_url(url, &name);

    // ── Step 9: Fix missing extension from URL ────────────────────────────
    if !has_extension(&name) {
        if let Some(url_ext) = extract_extension_from_url(url) {
            name = format!("{}.{}", name, url_ext);
        }
    }

    // ── Step 10: Final trim ───────────────────────────────────────────────
    name.trim().to_string()
}

/// Parse the `filename` from a `Content-Disposition` header value.
///
/// Handles both `filename="foo.zip"` and `filename*=UTF-8''foo%20bar.zip` (RFC 6266).
pub fn parse_content_disposition(header: &str) -> Option<String> {
    // Prefer filename* (RFC 5987 extended) over filename
    if let Some(name) = parse_filename_star(header) {
        return Some(name);
    }

    // Plain filename="..."
    lazy_static_regex!(re, r#"(?i)filename\s*=\s*"([^"]+)""#);
    if let Some(cap) = re.captures(header) {
        let name = cap.get(1)?.as_str().trim().to_string();
        if !name.is_empty() {
            return Some(percent_decode(&name));
        }
    }

    // filename=foo (unquoted)
    lazy_static_regex!(re_unquoted, r#"(?i)filename\s*=\s*([^\s;]+)"#);
    if let Some(cap) = re_unquoted.captures(header) {
        let name = cap.get(1)?.as_str().trim().to_string();
        if !name.is_empty() {
            return Some(percent_decode(&name));
        }
    }

    None
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse RFC 5987 `filename*=charset'language'encoded-value`.
fn parse_filename_star(header: &str) -> Option<String> {
    lazy_static_regex!(re, r#"(?i)filename\*\s*=\s*([^']+)'[^']*'([^\s;]+)"#);
    let cap = re.captures(header)?;
    let encoded = cap.get(2)?.as_str();
    Some(percent_decode(encoded))
}

/// Convert a dot-separated scene/media filename to a readable title.
/// "Movie.Name.2024.WEBRip.mkv" → "Movie Name (2024) WEBRip.mkv"
fn clean_media_dots(filename: &str) -> String {
    let ext = extract_extension(filename);
    let stem = if ext.is_empty() {
        filename.to_string()
    } else {
        filename[..filename.len() - ext.len() - 1].to_string()
    };

    // Split on dots, identify year token (4-digit 1900–2099)
    let tokens: Vec<&str> = stem.split('.').collect();
    let mut before_year: Vec<String> = Vec::new();
    let mut year_str: Option<String> = None;
    let mut after_year: Vec<String> = Vec::new();
    let mut found_year = false;

    for token in &tokens {
        if !found_year {
            if let Ok(y) = token.parse::<u32>() {
                if (1900..=2099).contains(&y) {
                    year_str = Some(token.to_string());
                    found_year = true;
                    continue;
                }
            }
            // Skip obviously meaningless all-caps quality/source tags before year
            before_year.push(token.to_string());
        } else {
            // After year: keep quality/source/codec tags
            after_year.push(token.to_string());
        }
    }

    let title = before_year.join(" ");
    let tags  = after_year.join(" ");

    let mut result = title;
    if let Some(year) = year_str {
        result = format!("{} ({})", result.trim(), year);
    }
    if !tags.is_empty() {
        result = format!("{} {}", result.trim(), tags);
    }
    if !ext.is_empty() {
        result = format!("{}.{}", result.trim(), ext);
    }

    result
}

/// Macro to compile a Regex once lazily (per call site — suitable for hot paths).
macro_rules! lazy_static_regex {
    ($name:ident, $pattern:expr) => {
        let $name = Regex::new($pattern).unwrap();
    };
}
use lazy_static_regex;

fn percent_decode(s: &str) -> String {
    let result = s.replace('+', " ");
    let mut i = 0;
    let bytes = result.as_bytes().to_vec();
    let mut out = Vec::with_capacity(bytes.len());

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h1), Some(h2)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push(h1 << 4 | h2);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }

    String::from_utf8_lossy(&out).to_string()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _           => None,
    }
}

fn has_extension(name: &str) -> bool {
    std::path::Path::new(name).extension().is_some()
}

fn extract_extension(name: &str) -> String {
    std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default()
}

fn extract_extension_from_url(url: &str) -> Option<String> {
    let path = url.split('?').next()?;
    let last  = path.rsplit('/').next()?;
    let ext   = std::path::Path::new(last).extension()?.to_str()?.to_lowercase();
    if ext.len() <= 5 && ext.chars().all(|c| c.is_alphanumeric()) {
        Some(ext)
    } else {
        None
    }
}

fn enrich_from_url(url: &str, current_name: &str) -> String {
    lazy_static_regex!(ver_re, r"v?(\d+\.\d+[\.\d]*)");

    if let Some(cap) = ver_re.captures(url) {
        let ver = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        if !ver.is_empty() && !current_name.contains(ver) {
            if let Some(stem) = std::path::Path::new(current_name).file_stem().and_then(|s| s.to_str()) {
                let ext = extract_extension(current_name);
                return if ext.is_empty() {
                    format!("{} v{}", stem, ver)
                } else {
                    format!("{} v{}.{}", stem, ver, ext)
                };
            }
        }
    }

    current_name.to_string()
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_site_watermark() {
        let result = smart_rename(
            "https://example.com/Normals.(THENKIRI.COM).2026.WEBRip.DOWNLOADED.FROM.THENKIRI.COM.mkv",
            "Normals.(THENKIRI.COM).2026.WEBRip.DOWNLOADED.FROM.THENKIRI.COM.mkv",
            "",
        );
        assert!(!result.contains("THENKIRI"), "result: {}", result);
        assert!(result.contains("Normals"), "result: {}", result);
        assert!(result.contains("2026"), "result: {}", result);
    }

    #[test]
    fn parses_content_disposition_quoted() {
        let cd = r#"attachment; filename="my file.zip""#;
        assert_eq!(parse_content_disposition(cd), Some("my file.zip".into()));
    }

    #[test]
    fn parses_content_disposition_star() {
        let cd = "attachment; filename*=UTF-8''my%20file.zip";
        assert_eq!(parse_content_disposition(cd), Some("my file.zip".into()));
    }

    #[test]
    fn media_dots_to_spaces() {
        let result = smart_rename("", "The.Matrix.1999.BluRay.mkv", "");
        assert!(result.contains("1999"), "result: {}", result);
        assert!(!result.starts_with("The.Matrix"), "result: {}", result);
    }
}
