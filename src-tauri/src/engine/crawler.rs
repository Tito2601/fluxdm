//! Site grabber — discover downloadable files linked from a page.
//!
//! Breadth-first from a starting URL, collecting links that match a file-type
//! filter. Discovery only: the caller reviews the results and chooses what to
//! enqueue, because a crawl can easily surface hundreds of files and starting
//! them all unattended is rarely what anyone wants.
//!
//! This is the one part of FluxDM that generates traffic the user did not
//! individually ask for, so the defaults are deliberately polite:
//!
//! - **Same-origin by default.** Following off-site links turns "grab this
//!   gallery" into an unbounded walk of the open web.
//! - **Serial fetches with a delay.** Page fetches are paced rather than run
//!   concurrently; a grabber that opens dozens of sockets against one host is
//!   indistinguishable from an attack.
//! - **Hard caps on pages and depth**, so a crawl always terminates even on a
//!   site with circular links or generated URLs.

use std::collections::{HashSet, VecDeque};
use std::time::Duration;

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::engine::hls::resolve_url;
use crate::engine::http;

/// Pause between page fetches. Slow enough to stay a well-behaved client.
const FETCH_DELAY: Duration = Duration::from_millis(250);

/// Give up on a page that will not load promptly; one slow page must not stall
/// the whole crawl.
const PAGE_TIMEOUT: Duration = Duration::from_secs(20);

/// Absolute ceiling on pages fetched, whatever the requested depth.
const MAX_PAGES: usize = 200;

/// Deepest link level that may be requested.
const MAX_DEPTH: u32 = 3;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlOptions {
    /// Where to start.
    pub url: String,
    /// How many link levels to follow. 0 = the starting page only.
    pub depth: u32,
    /// Restrict the crawl to the starting URL's host.
    pub same_host_only: bool,
    /// Lowercase extensions to collect, without dots. Empty = every known
    /// downloadable type.
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredFile {
    pub url:       String,
    pub filename:  String,
    pub extension: String,
    /// Link text or alt text, when the page offered one.
    pub label:     Option<String>,
    /// Page the link was found on.
    pub source:    String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlResult {
    pub files:         Vec<DiscoveredFile>,
    pub pages_visited: usize,
    /// Set when a cap cut the crawl short, so the UI can say so rather than
    /// implying the result is exhaustive.
    pub truncated:     bool,
}

/// Extensions collected when the caller does not narrow the filter.
const DEFAULT_EXTENSIONS: &[&str] = &[
    "zip", "rar", "7z", "gz", "bz2", "xz", "tar", "iso", "img", "bin",
    "exe", "msi", "dmg", "pkg", "deb", "rpm", "apk",
    "mp4", "mkv", "avi", "mov", "webm", "mp3", "flac", "wav", "m4a",
    "pdf", "epub", "mobi", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
    "jpg", "jpeg", "png", "gif", "webp", "svg", "bmp",
];

// ── Crawl ─────────────────────────────────────────────────────────────────────

/// Walk from `options.url` and collect matching file links.
pub async fn crawl(options: CrawlOptions) -> Result<CrawlResult> {
    let depth = options.depth.min(MAX_DEPTH);
    let start_host = host_of(&options.url)
        .ok_or_else(|| anyhow::anyhow!("'{}' is not a valid URL", options.url))?;

    let wanted: HashSet<String> = if options.extensions.is_empty() {
        DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect()
    } else {
        options.extensions.iter().map(|e| e.trim_start_matches('.').to_lowercase()).collect()
    };

    let client = http::client();

    let mut queue: VecDeque<(String, u32)> = VecDeque::new();
    let mut seen_pages: HashSet<String>    = HashSet::new();
    let mut seen_files: HashSet<String>    = HashSet::new();
    let mut files: Vec<DiscoveredFile>     = Vec::new();
    let mut truncated = false;

    queue.push_back((options.url.clone(), 0));
    seen_pages.insert(options.url.clone());

    while let Some((page_url, level)) = queue.pop_front() {
        if seen_pages.len() > MAX_PAGES {
            warn!("Crawl hit the {}-page ceiling; results are partial", MAX_PAGES);
            truncated = true;
            break;
        }

        info!("Crawling (depth {}): {}", level, page_url);

        let body = match fetch_page(client, &page_url).await {
            Ok(Some(b)) => b,
            // Not HTML, or unreadable — nothing to extract, but not fatal:
            // one dead link should not abort the whole crawl.
            Ok(None) => continue,
            Err(e) => {
                warn!("Skipping '{}': {}", page_url, e);
                continue;
            }
        };

        for link in extract_links(&body, &page_url) {
            if options.same_host_only && host_of(&link.url).as_deref() != Some(start_host.as_str()) {
                continue;
            }

            match extension_of(&link.url) {
                // A file we were asked to collect.
                Some(ext) if wanted.contains(&ext) => {
                    if seen_files.insert(link.url.clone()) {
                        files.push(DiscoveredFile {
                            filename:  filename_of(&link.url),
                            extension: ext,
                            url:       link.url,
                            label:     link.label,
                            source:    page_url.clone(),
                        });
                    }
                }
                // Anything else is a candidate page, if there is depth left.
                _ if level < depth => {
                    if looks_like_page(&link.url) && seen_pages.insert(link.url.clone()) {
                        queue.push_back((link.url, level + 1));
                    }
                }
                _ => {}
            }
        }

        // Paced rather than parallel — see the module comment.
        tokio::time::sleep(FETCH_DELAY).await;
    }

    info!(
        "Crawl finished: {} file(s) across {} page(s)",
        files.len(),
        seen_pages.len()
    );

    Ok(CrawlResult {
        files,
        pages_visited: seen_pages.len(),
        truncated,
    })
}

/// Fetch a page, returning its body only when it is HTML worth parsing.
async fn fetch_page(client: &reqwest::Client, url: &str) -> Result<Option<String>> {
    let response = client.get(url).timeout(PAGE_TIMEOUT).send().await?;

    if !response.status().is_success() {
        anyhow::bail!("HTTP {}", response.status());
    }

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    // Downloading a multi-gigabyte ISO into a String because it was linked as a
    // page would be catastrophic, so anything not announced as HTML is skipped.
    if !content_type.contains("text/html") && !content_type.contains("xhtml") {
        return Ok(None);
    }

    Ok(Some(response.text().await?))
}

// ── Link extraction ───────────────────────────────────────────────────────────

struct Link {
    url:   String,
    label: Option<String>,
}

/// Pull `href` and `src` targets out of an HTML document.
///
/// Regex rather than a real parser: FluxDM only needs attribute values, and a
/// full HTML5 tree-builder is a heavy dependency for that. The cost is that
/// links inside comments or scripts may be picked up — harmless, since every
/// candidate is filtered by extension and host afterwards.
fn extract_links(html: &str, base_url: &str) -> Vec<Link> {
    // Built once per call; the crawl is network-bound so this is not hot.
    let anchor = Regex::new(r#"(?is)<a\b[^>]*?href\s*=\s*["']([^"']+)["'][^>]*>(.*?)</a>"#).unwrap();
    let src    = Regex::new(r#"(?i)<(?:img|video|audio|source|embed|iframe)\b[^>]*?src\s*=\s*["']([^"']+)["']"#).unwrap();

    let mut out = Vec::new();

    for cap in anchor.captures_iter(html) {
        let raw = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        if let Some(url) = normalize(raw, base_url) {
            let label = cap
                .get(2)
                .map(|m| strip_tags(m.as_str()))
                .filter(|s| !s.is_empty());
            out.push(Link { url, label });
        }
    }

    for cap in src.captures_iter(html) {
        let raw = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        if let Some(url) = normalize(raw, base_url) {
            out.push(Link { url, label: None });
        }
    }

    out
}

/// Resolve a raw attribute value to an absolute http(s) URL, or discard it.
fn normalize(raw: &str, base_url: &str) -> Option<String> {
    let raw = raw.trim();

    // In-page anchors and non-navigable schemes are never downloads.
    if raw.is_empty() || raw.starts_with('#') {
        return None;
    }
    let lower = raw.to_lowercase();
    for scheme in ["javascript:", "mailto:", "tel:", "data:", "blob:", "about:"] {
        if lower.starts_with(scheme) {
            return None;
        }
    }

    // Drop the fragment: `page#a` and `page#b` are one page, and keeping both
    // would fetch it twice.
    let raw = raw.split('#').next().unwrap_or(raw);

    let resolved = resolve_url(raw, base_url);
    if resolved.starts_with("http://") || resolved.starts_with("https://") {
        Some(resolved)
    } else {
        None
    }
}

/// Collapse inner markup to plain text for use as a label.
fn strip_tags(fragment: &str) -> String {
    let tags = Regex::new(r"(?s)<[^>]*>").unwrap();
    tags.replace_all(fragment, " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect()
}

// ── URL helpers ───────────────────────────────────────────────────────────────

/// Host portion of an absolute URL, lowercased.
fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    // Strip credentials and port so `a.test:443` and `a.test` compare equal.
    let host = host.rsplit('@').next()?;
    let host = host.split(':').next()?;
    if host.is_empty() { None } else { Some(host.to_lowercase()) }
}

/// Path segment after the last slash, with query and fragment removed.
fn filename_of(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.rsplit('/').next().unwrap_or("download");
    if name.is_empty() { "download".to_string() } else { decode_percent(name) }
}

/// Lowercase extension of the URL's filename, if it has a plausible one.
fn extension_of(url: &str) -> Option<String> {
    let name = filename_of(url);
    let (_, ext) = name.rsplit_once('.')?;
    // A long or non-alphanumeric tail is a path artefact, not an extension.
    if ext.is_empty() || ext.len() > 5 || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ext.to_lowercase())
}

/// Would following this URL plausibly yield an HTML page?
fn looks_like_page(url: &str) -> bool {
    match extension_of(url) {
        None => true, // Extensionless paths are usually routes
        Some(ext) => matches!(ext.as_str(), "html" | "htm" | "php" | "asp" | "aspx" | "jsp"),
    }
}

/// Minimal percent-decoding so `My%20File.zip` is saved under its real name.
fn decode_percent(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }

    // Undecodable bytes mean this was not percent-encoded UTF-8 after all;
    // the original string is the better answer than replacement characters.
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_anchors_and_media_sources() {
        let html = r#"
            <a href="/files/game.zip">Get the game</a>
            <a href='https://other.test/doc.pdf'>Manual</a>
            <img src="/img/cover.png">
            <video><source src="clip.mp4"></video>
        "#;
        let links = extract_links(html, "https://site.test/page.html");
        let urls: Vec<_> = links.iter().map(|l| l.url.as_str()).collect();

        assert!(urls.contains(&"https://site.test/files/game.zip"));
        assert!(urls.contains(&"https://other.test/doc.pdf"));
        assert!(urls.contains(&"https://site.test/img/cover.png"));
        assert!(urls.contains(&"https://site.test/clip.mp4"));
    }

    #[test]
    fn captures_link_text_as_a_label() {
        let html = r#"<a href="/a.zip">  Download <b>now</b>  </a>"#;
        let links = extract_links(html, "https://site.test/");
        assert_eq!(links[0].label.as_deref(), Some("Download now"));
    }

    #[test]
    fn discards_non_navigable_schemes() {
        // r##"…"## because the `"#` in the fragment link would close an r#"…"#.
        let html = r##"
            <a href="javascript:void(0)">x</a>
            <a href="mailto:a@b.test">mail</a>
            <a href="#section">jump</a>
            <a href="data:text/plain,hi">data</a>
        "##;
        assert!(extract_links(html, "https://site.test/").is_empty());
    }

    #[test]
    fn a_fragment_does_not_make_a_second_page() {
        // Without stripping, `p#a` and `p#b` would both be fetched.
        let a = normalize("/p#a", "https://site.test/");
        let b = normalize("/p#b", "https://site.test/");
        assert_eq!(a, b);
    }

    #[test]
    fn host_comparison_ignores_port_and_credentials() {
        assert_eq!(host_of("https://a.test:443/x"), Some("a.test".into()));
        assert_eq!(host_of("https://user:pw@a.test/x"), Some("a.test".into()));
        assert_eq!(host_of("https://A.TEST/x"), Some("a.test".into()));
        assert_eq!(host_of("not a url"), None);
    }

    #[test]
    fn extensions_are_recognized_but_path_noise_is_not() {
        assert_eq!(extension_of("https://a.test/f.zip"), Some("zip".into()));
        assert_eq!(extension_of("https://a.test/f.ZIP?t=1"), Some("zip".into()));
        assert_eq!(extension_of("https://a.test/archive.tar.gz"), Some("gz".into()));
        assert_eq!(extension_of("https://a.test/path/"), None);
        assert_eq!(extension_of("https://a.test/v1.2.3-release"), None);
    }

    #[test]
    fn filenames_are_percent_decoded() {
        assert_eq!(filename_of("https://a.test/My%20File.zip"), "My File.zip");
        assert_eq!(filename_of("https://a.test/plain.zip?token=x"), "plain.zip");
        // A stray percent is not an encoding; leave the name alone.
        assert_eq!(filename_of("https://a.test/100%.zip"), "100%.zip");
    }

    #[test]
    fn only_page_like_urls_are_followed() {
        assert!(looks_like_page("https://a.test/section"));
        assert!(looks_like_page("https://a.test/index.html"));
        assert!(!looks_like_page("https://a.test/big.iso"));
        assert!(!looks_like_page("https://a.test/pic.jpg"));
    }

    // ── Integration ───────────────────────────────────────────────────────────

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Two-page site: the index links a zip and a second page, which links a PDF
    /// that is only reachable at depth 1.
    async fn serve_site() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                let (mut sock, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => return,
                };

                tokio::spawn(async move {
                    let mut buf = vec![0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/")
                        .to_string();

                    let body = match path.as_str() {
                        "/deep.html" => {
                            r#"<html><a href="/manual.pdf">Manual</a></html>"#.to_string()
                        }
                        _ => r#"<html>
                                <a href="/files/game.zip">Game</a>
                                <a href="/deep.html">More</a>
                                <a href="https://elsewhere.test/off.zip">Offsite</a>
                                <img src="/img/cover.png">
                               </html>"#
                            .to_string(),
                    };

                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = sock.write_all(response.as_bytes()).await;
                    let _ = sock.flush().await;
                });
            }
        });

        format!("http://{}/", addr)
    }

    #[tokio::test]
    async fn depth_zero_collects_only_the_starting_page() {
        let base = serve_site().await;
        let result = crawl(CrawlOptions {
            url:            base.clone(),
            depth:          0,
            same_host_only: true,
            extensions:     vec![],
        })
        .await
        .expect("crawl must succeed");

        let names: Vec<_> = result.files.iter().map(|f| f.filename.as_str()).collect();
        assert!(names.contains(&"game.zip"), "got {names:?}");
        assert!(names.contains(&"cover.png"), "img src must be collected");
        assert!(
            !names.contains(&"manual.pdf"),
            "depth 0 must not follow the link to deep.html"
        );
        assert_eq!(result.pages_visited, 1);
    }

    #[tokio::test]
    async fn depth_one_follows_links_one_level_down() {
        let base = serve_site().await;
        let result = crawl(CrawlOptions {
            url:            base,
            depth:          1,
            same_host_only: true,
            extensions:     vec![],
        })
        .await
        .expect("crawl must succeed");

        let names: Vec<_> = result.files.iter().map(|f| f.filename.as_str()).collect();
        assert!(names.contains(&"manual.pdf"), "depth 1 must reach deep.html: {names:?}");
    }

    #[tokio::test]
    async fn the_same_host_rule_excludes_offsite_links() {
        let base = serve_site().await;
        let result = crawl(CrawlOptions {
            url:            base,
            depth:          1,
            same_host_only: true,
            extensions:     vec![],
        })
        .await
        .expect("crawl must succeed");

        assert!(
            !result.files.iter().any(|f| f.url.contains("elsewhere.test")),
            "same_host_only must drop the offsite zip"
        );
    }

    #[tokio::test]
    async fn the_extension_filter_narrows_the_result() {
        let base = serve_site().await;
        let result = crawl(CrawlOptions {
            url:            base,
            depth:          1,
            same_host_only: true,
            extensions:     vec!["pdf".into()],
        })
        .await
        .expect("crawl must succeed");

        assert!(!result.files.is_empty(), "the pdf should still be found");
        assert!(
            result.files.iter().all(|f| f.extension == "pdf"),
            "only pdfs were requested"
        );
    }
}
