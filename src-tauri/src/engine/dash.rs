//! DASH (Dynamic Adaptive Streaming over HTTP) MPD parser.
//!
//! Supports SegmentTemplate with `$Number$` / `$Time$` addressing,
//! and BaseURL per-Representation fallback.

use crate::engine::hls::resolve_url;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SegmentTemplate {
    pub init:          Option<String>, // initialization segment URL template
    pub media:         String,         // media segment URL template
    pub start_number:  u64,
    pub timescale:     u64,
    pub duration:      u64,            // segment duration in timescale units
    pub segment_count: Option<u64>,    // filled in once total duration is known
}

#[derive(Debug, Clone)]
pub struct DashStream {
    pub id:        String,
    pub bandwidth: u64,
    pub width:     Option<u32>,
    pub height:    Option<u32>,
    pub codecs:    Option<String>,
    pub template:  SegmentTemplate,
    pub base_url:  String,
}

impl DashStream {
    /// Build (init_url, [media_segment_urls]).
    pub fn segment_urls(&self) -> (Option<String>, Vec<String>) {
        let count  = self.template.segment_count.unwrap_or(0);
        let base   = &self.base_url;

        let init = self.template.init.as_ref().map(|t| {
            apply_template(t, &self.id, 0, base)
        });

        let segments = (self.template.start_number..self.template.start_number + count)
            .map(|n| apply_template(&self.template.media, &self.id, n, base))
            .collect();

        (init, segments)
    }
}

// ── Template expander ─────────────────────────────────────────────────────────

fn apply_template(template: &str, repr_id: &str, number: u64, base_url: &str) -> String {
    let s = template
        .replace("$RepresentationID$", repr_id)
        .replace("$Number$", &number.to_string())
        .replace("$Number%09d$", &format!("{:09}", number))
        .replace("$Number%08d$", &format!("{:08}", number))
        .replace("$Number%07d$", &format!("{:07}", number))
        .replace("$Number%06d$", &format!("{:06}", number))
        .replace("$Number%05d$", &format!("{:05}", number))
        .replace("$Number%04d$", &format!("{:04}", number))
        .replace("$Number%03d$", &format!("{:03}", number));

    // Handle any remaining $Number%0Nd$ patterns
    let s = replace_fmt_number(&s, number);

    resolve_url(&s, base_url)
}

/// Handle arbitrary `$Number%0Nd$` format specifiers with regex.
fn replace_fmt_number(s: &str, n: u64) -> String {
    let mut result = s.to_string();
    while let Some(start) = result.find("$Number%") {
        let rest = &result[start + 8..]; // skip "$Number%"
        if let Some(end_rel) = rest.find('$') {
            let fmt = &rest[..end_rel]; // e.g. "05d"
            if fmt.ends_with('d') {
                if let Ok(width) = fmt[..fmt.len() - 1].trim_start_matches('0').parse::<usize>() {
                    let formatted = format!("{:0>width$}", n);
                    let full_end  = start + 8 + end_rel + 1;
                    result = format!("{}{}{}", &result[..start], formatted, &result[full_end..]);
                    continue;
                }
            }
        }
        break;
    }
    result
}

// ── MPD parser ────────────────────────────────────────────────────────────────

/// Parse an MPD XML document and return all video streams sorted by bandwidth (best first).
pub fn parse_mpd(content: &str, base_url: &str) -> Result<Vec<DashStream>, String> {
    let mut streams = Vec::new();

    // Period-level SegmentTemplate (inherited by all AdaptationSets)
    let period_template = find_segment_template_in_block(content, base_url);

    // Walk each AdaptationSet block
    let mut pos = 0;
    while let Some(rel_start) = content[pos..].find("<AdaptationSet") {
        let start = pos + rel_start;
        let end   = find_element_end(content, start, "AdaptationSet")
            .unwrap_or(content.len());
        let block = &content[start..end];

        // Skip audio-only AdaptationSets
        let mime_type    = extract_attr(block, "mimeType").unwrap_or_default();
        let content_type = extract_attr(block, "contentType").unwrap_or_default();
        let is_video = mime_type.contains("video")
            || content_type == "video"
            || (!mime_type.contains("audio") && !content_type.contains("audio") && !mime_type.contains("text"));

        if is_video {
            let as_template = find_segment_template_in_block(block, base_url)
                .or_else(|| period_template.clone());
            let as_base = extract_element_text(block, "BaseURL")
                .map(|u| resolve_url(u.trim(), base_url))
                .unwrap_or_else(|| base_url.to_string());

            let mut rpos = 0;
            while let Some(rel_repr) = block[rpos..].find("<Representation") {
                let repr_start = rpos + rel_repr;
                let repr_end   = find_element_end(block, repr_start, "Representation")
                    .unwrap_or(block.len());
                let repr_block = &block[repr_start..repr_end];

                let id        = extract_attr(repr_block, "id")
                    .unwrap_or_else(|| streams.len().to_string());
                let bandwidth = extract_attr(repr_block, "bandwidth")
                    .and_then(|v| v.parse().ok()).unwrap_or(0);
                let width     = extract_attr(repr_block, "width")
                    .and_then(|v| v.parse().ok());
                let height    = extract_attr(repr_block, "height")
                    .and_then(|v| v.parse().ok());
                let codecs    = extract_attr(repr_block, "codecs")
                    .or_else(|| extract_attr(block, "codecs"));

                // Representation-level template overrides AdaptationSet-level
                let template = find_segment_template_in_block(repr_block, &as_base)
                    .or_else(|| as_template.clone());

                if let Some(mut tmpl) = template {
                    // Compute segment count from MPD total duration if available
                    if tmpl.segment_count.is_none() && tmpl.duration > 0 && tmpl.timescale > 0 {
                        if let Some(dur_secs) = extract_duration(content) {
                            let count = (dur_secs * tmpl.timescale as f64 / tmpl.duration as f64).ceil() as u64;
                            tmpl.segment_count = Some(count.max(1));
                        }
                    }

                    streams.push(DashStream {
                        id,
                        bandwidth,
                        width,
                        height,
                        codecs,
                        template: tmpl,
                        base_url: as_base.clone(),
                    });
                }

                rpos = repr_end.min(block.len());
            }
        }

        pos = end.min(content.len());
    }

    if streams.is_empty() {
        return Err("No video streams found in MPD".into());
    }

    // Best quality first
    streams.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));
    Ok(streams)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Find and parse a `<SegmentTemplate>` element within `block`.
fn find_segment_template_in_block(block: &str, base_url: &str) -> Option<SegmentTemplate> {
    let start = block.find("<SegmentTemplate")?;
    // Find the end of the opening tag (may be self-closing />)
    let tag_end = block[start..].find('>').map(|i| i + start)?;
    let tag     = &block[start..=tag_end];

    let timescale    = extract_attr(tag, "timescale").and_then(|v| v.parse().ok()).unwrap_or(1);
    let duration     = extract_attr(tag, "duration").and_then(|v| v.parse::<f64>().ok())
        .map(|d| d as u64).unwrap_or(0);
    let start_number = extract_attr(tag, "startNumber").and_then(|v| v.parse().ok()).unwrap_or(1);

    let init  = extract_attr(tag, "initialization");
    let media = extract_attr(tag, "media")?;

    // Resolve init/media templates relative to base_url
    let init = init.map(|t| {
        if t.starts_with("http://") || t.starts_with("https://") { t }
        else { resolve_url(&t, base_url) }
    });
    let media = if media.starts_with("http://") || media.starts_with("https://") {
        media
    } else {
        // Keep template as-is; apply_template will resolve after substitution
        media
    };

    Some(SegmentTemplate { init, media, start_number, timescale, duration, segment_count: None })
}

/// Extract `mediaPresentationDuration` from the root `<MPD>` element (seconds).
pub fn extract_duration(content: &str) -> Option<f64> {
    let mpd_start   = content.find("<MPD")?;
    let mpd_tag_end = content[mpd_start..].find('>').map(|i| i + mpd_start)?;
    let mpd_tag     = &content[mpd_start..=mpd_tag_end];
    let dur_str     = extract_attr(mpd_tag, "mediaPresentationDuration")?;
    parse_iso8601_duration(&dur_str)
}

/// Parse ISO 8601 duration `PTxHxMxS` → seconds.
fn parse_iso8601_duration(s: &str) -> Option<f64> {
    let s = s.trim_start_matches('P');
    let (date_part, time_part) = if let Some(t) = s.find('T') {
        (&s[..t], &s[t + 1..])
    } else {
        ("", s)
    };

    let mut total = 0.0f64;

    if let Some(d) = date_part.find('D') {
        if let Ok(v) = date_part[..d].parse::<f64>() { total += v * 86400.0; }
    }

    let mut rem = time_part;
    if let Some(h) = rem.find('H') {
        if let Ok(v) = rem[..h].parse::<f64>() { total += v * 3600.0; }
        rem = &rem[h + 1..];
    }
    if let Some(m) = rem.find('M') {
        if let Ok(v) = rem[..m].parse::<f64>() { total += v * 60.0; }
        rem = &rem[m + 1..];
    }
    if let Some(s_pos) = rem.find('S') {
        if let Ok(v) = rem[..s_pos].parse::<f64>() { total += v; }
    }

    if total > 0.0 { Some(total) } else { None }
}

/// Find the end position of an XML element, handling self-closing and nested tags.
fn find_element_end(content: &str, start: usize, element_name: &str) -> Option<usize> {
    let tag_end = content[start..].find('>').map(|i| i + start)?;
    // Self-closing: <Element ... />
    if tag_end > 0 && content.as_bytes().get(tag_end - 1) == Some(&b'/') {
        return Some(tag_end + 1);
    }
    // Find the matching close tag
    let close = format!("</{}>", element_name);
    content[start..].find(&close).map(|i| i + start + close.len())
}

/// Extract XML attribute value from an element's opening tag text.
fn extract_attr(element: &str, attr: &str) -> Option<String> {
    for &quote in &['"', '\''] {
        let needle = format!("{}={}", attr, quote);
        if let Some(pos) = element.find(&needle) {
            let rest = &element[pos + needle.len()..];
            if let Some(end) = rest.find(quote) {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

/// Extract text content of `<tag>text</tag>`.
fn extract_element_text(content: &str, tag: &str) -> Option<String> {
    let open  = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = content.find(&open).map(|i| i + open.len())?;
    let end   = content[start..].find(&close).map(|i| i + start)?;
    let text  = content[start..end].trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_iso8601_pt1h30m20s() {
        let dur = parse_iso8601_duration("PT1H30M20S").unwrap();
        assert!((dur - 5420.0).abs() < 0.1);
    }

    #[test]
    fn parse_iso8601_pt9m() {
        let dur = parse_iso8601_duration("PT9M").unwrap();
        assert!((dur - 540.0).abs() < 0.1);
    }

    #[test]
    fn fmt_number_template() {
        let result = apply_template("seg_$Number%05d$.m4s", "1", 42, "https://cdn.example.com/");
        assert_eq!(result, "https://cdn.example.com/seg_00042.m4s");
    }
}
