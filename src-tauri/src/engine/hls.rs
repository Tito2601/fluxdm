//! HLS (HTTP Live Streaming) M3U8 playlist parser.
//!
//! Handles both master playlists (`#EXT-X-STREAM-INF`) and
//! media playlists (`#EXTINF`).

use std::collections::HashMap;

/// A quality variant from an HLS master playlist.
#[derive(Debug, Clone)]
pub struct HlsVariant {
    pub bandwidth:  u64,
    pub resolution: Option<String>,
    pub codecs:     Option<String>,
    pub frame_rate: Option<f64>,
    pub url:        String,
}

/// A single media segment from an HLS media playlist.
#[derive(Debug, Clone)]
pub struct HlsSegment {
    pub url: String,
    /// Segment duration; kept for potential future use (e.g. accurate ETA).
    #[allow(dead_code)]
    pub duration_secs: f32,
}

/// Parsed M3U8 result — either a master or a media playlist.
pub enum M3u8 {
    Master(Vec<HlsVariant>),
    Media {
        segments:            Vec<HlsSegment>,
        total_duration_secs: f32,
    },
}

/// Parse raw M3U8 content. `base_url` resolves relative URIs.
pub fn parse_m3u8(content: &str, base_url: &str) -> Result<M3u8, String> {
    if !content.trim_start().starts_with("#EXTM3U") {
        return Err("Not a valid M3U8 playlist (missing #EXTM3U header)".into());
    }
    if content.contains("#EXT-X-STREAM-INF") {
        parse_master(content, base_url).map(M3u8::Master)
    } else {
        parse_media(content, base_url)
    }
}

fn parse_master(content: &str, base_url: &str) -> Result<Vec<HlsVariant>, String> {
    let mut variants = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with("#EXT-X-STREAM-INF:") {
            let attrs = parse_attrs(&line["#EXT-X-STREAM-INF:".len()..]);
            let bandwidth  = attrs.get("BANDWIDTH").and_then(|v| v.parse().ok()).unwrap_or(0);
            let resolution = attrs.get("RESOLUTION").cloned();
            let codecs     = attrs.get("CODECS").map(|s| s.trim_matches('"').to_string());
            let frame_rate = attrs.get("FRAME-RATE").and_then(|v| v.parse().ok());

            // The next non-empty, non-comment line is the variant URI
            let mut j = i + 1;
            while j < lines.len()
                && (lines[j].trim().is_empty() || lines[j].trim().starts_with('#'))
            {
                j += 1;
            }
            if j < lines.len() {
                let url = resolve_url(lines[j].trim(), base_url);
                variants.push(HlsVariant { bandwidth, resolution, codecs, frame_rate, url });
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }

    if variants.is_empty() {
        Err("No stream variants found in master playlist".into())
    } else {
        Ok(variants)
    }
}

fn parse_media(content: &str, base_url: &str) -> Result<M3u8, String> {
    let mut segments     = Vec::new();
    let mut total        = 0.0f32;
    let mut pending_dur  = 0.0f32;

    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("#EXTINF:") {
            pending_dur = t["#EXTINF:".len()..]
                .split(',')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
        } else if !t.is_empty() && !t.starts_with('#') {
            let url = resolve_url(t, base_url);
            total += pending_dur;
            segments.push(HlsSegment { url, duration_secs: pending_dur });
            pending_dur = 0.0;
        }
    }

    if segments.is_empty() {
        Err("No segments found in media playlist".into())
    } else {
        Ok(M3u8::Media { segments, total_duration_secs: total })
    }
}

// ── Attribute parser ──────────────────────────────────────────────────────────

/// Parse `KEY=VALUE,KEY2="VALUE2",...` attribute strings (HLS attribute-list format).
pub fn parse_attrs(s: &str) -> HashMap<String, String> {
    let mut map  = HashMap::new();
    let mut rest = s;

    while !rest.is_empty() {
        let eq = match rest.find('=') { Some(i) => i, None => break };
        let key = rest[..eq].trim().to_uppercase();
        rest = &rest[eq + 1..];

        let (value, next): (String, &str) = if rest.starts_with('"') {
            let inner = &rest[1..];
            let end   = inner.find('"').unwrap_or(inner.len());
            let v     = inner[..end].to_string();
            let after = if end + 1 < inner.len() {
                inner[end + 1..].trim_start_matches(',')
            } else {
                ""
            };
            (v, after)
        } else {
            let end   = rest.find(',').unwrap_or(rest.len());
            let v     = rest[..end].trim().to_string();
            let after = if end < rest.len() { &rest[end + 1..] } else { "" };
            (v, after)
        };

        map.insert(key, value);
        rest = next;
    }
    map
}

// ── URL resolver ──────────────────────────────────────────────────────────────

/// Resolve a possibly-relative URL against a base URL.
pub fn resolve_url(url: &str, base: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    if url.starts_with("//") {
        let scheme = if base.starts_with("https") { "https:" } else { "http:" };
        return format!("{}{}", scheme, url);
    }
    if url.starts_with('/') {
        // Absolute path on the same origin
        let scheme_end  = base.find("://").map(|i| i + 3).unwrap_or(0);
        let host_end    = base[scheme_end..].find('/').map(|i| i + scheme_end).unwrap_or(base.len());
        return format!("{}{}", &base[..host_end], url);
    }
    // Relative path — strip the filename from base
    let dir = base.rfind('/').map(|i| &base[..=i]).unwrap_or(base);
    format!("{}{}", dir, url)
}
