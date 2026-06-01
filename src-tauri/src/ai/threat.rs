/// Threat scorer for downloaded files.
///
/// Uses additive risk factors to produce a score 0–100.
/// Score > 60 → show warning badge in UI.
/// Score > 80 → high-risk alert.
///
/// Phase 4 adds `ThreatFactor` breakdown (for UI tooltips) and six new signals.

use serde::{Deserialize, Serialize};

// ── Static tables ─────────────────────────────────────────────────────────────

static TRUSTED_DOMAINS: &[&str] = &[
    // Source forges / package registries
    "github.com", "gitlab.com", "bitbucket.org",
    "npmjs.com", "pypi.org", "crates.io", "nuget.org",
    "rubygems.org", "packagist.org", "maven.apache.org",
    // OS vendors
    "microsoft.com", "apple.com", "canonical.com",
    "ubuntu.com", "debian.org", "fedoraproject.org",
    "archlinux.org", "kernel.org",
    // Browser vendors
    "mozilla.org", "google.com", "chromium.org",
    // Dev tools
    "rust-lang.org", "python.org", "nodejs.org", "openjdk.org",
    "golang.org", "dotnet.microsoft.com",
    // Common software
    "videolan.org", "ffmpeg.org", "7-zip.org",
    "winrar.com", "vim.org", "neovim.io", "vscode.dev",
    "apache.org", "eclipse.org", "jetbrains.com",
    "docker.com", "kubernetes.io",
];

static SUSPICIOUS_WORDS: &[&str] = &[
    // Piracy / cracking
    "crack", "keygen", "patch", "hack", "loader", "activator",
    "bypass", "serial", "warez", "pirate", "cheat", "repack",
    // Malware families
    "cryptolocker", "wannacry", "ransomware", "trojan",
    "rootkit", "spyware", "adware", "backdoor",
    // Social engineering
    "free-download", "no-virus", "100-safe", "clean-file",
];

static URL_SHORTENERS: &[&str] = &[
    "bit.ly", "tinyurl.com", "t.co", "goo.gl", "ow.ly",
    "buff.ly", "dlvr.it", "is.gd", "cli.gs", "tiny.cc",
    "shorturl.at", "cutt.ly",
];

static EXECUTABLE_EXTS: &[&str] = &[
    "exe", "bat", "cmd", "scr", "ps1", "vbs", "js", "jar",
    "com", "pif", "reg", "msi", "hta", "wsf", "lnk",
];

// ── Public types ──────────────────────────────────────────────────────────────

/// One contributing factor to the overall threat score.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreatFactor {
    /// Short human-readable name shown in the UI.
    pub name:   String,
    /// Score delta: positive = more dangerous, negative = safer.
    pub delta:  i32,
    /// One-sentence explanation for the user.
    pub reason: String,
}

/// Full threat analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreatAnalysis {
    pub score:   u8,
    pub factors: Vec<ThreatFactor>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Calculate a threat score 0–100 (thin wrapper around `explain_threat_score`).
pub fn calculate_threat_score(
    url:      &str,
    filename: &str,
    mime:     &str,
    referrer: &str,
) -> u8 {
    explain_threat_score(url, filename, mime, referrer, 0).score
}

/// Full breakdown: score + all contributing factors.
pub fn explain_threat_score(
    url:       &str,
    filename:  &str,
    mime:      &str,
    referrer:  &str,
    file_size: u64,
) -> ThreatAnalysis {
    let mut factors: Vec<ThreatFactor> = Vec::new();
    let mut score: i32 = 0;

    let url_lower      = url.to_lowercase();
    let filename_lower = filename.to_lowercase();
    let mime_lower     = mime.to_lowercase();
    let ext            = extract_extension(filename);
    let domain         = extract_domain(&url_lower);

    // ── Factor 1: Executable file extension (+30) ─────────────────────────
    if EXECUTABLE_EXTS.contains(&ext.as_str()) {
        score += 30;
        factors.push(ThreatFactor {
            name:   "Executable file".into(),
            delta:  30,
            reason: format!(".{} files can run arbitrary code on your system.", ext),
        });
    }

    // ── Factor 2: MIME type mismatch (+20) ────────────────────────────────
    if !mime_lower.is_empty()
        && !mime_lower.contains("octet-stream")
        && has_mime_extension_mismatch(&ext, &mime_lower)
    {
        score += 20;
        factors.push(ThreatFactor {
            name:   "MIME type mismatch".into(),
            delta:  20,
            reason: format!(
                "Server reports '{}' but filename suggests a .{} file — possible spoofing.",
                mime, ext
            ),
        });
    }

    // ── Factor 3: IP address or no TLD (+20) ─────────────────────────────
    if is_ip_address(&domain) {
        score += 20;
        factors.push(ThreatFactor {
            name:   "IP address host".into(),
            delta:  20,
            reason: "File served from a raw IP address instead of a domain name.".into(),
        });
    } else if domain.is_empty() || !domain.contains('.') {
        score += 10;
        factors.push(ThreatFactor {
            name:   "Unresolvable host".into(),
            delta:  10,
            reason: "Could not extract a valid domain from the URL.".into(),
        });
    }

    // ── Factor 4: High URL entropy (+15) ─────────────────────────────────
    if let Some(path) = extract_url_path(&url_lower) {
        if shannon_entropy(&path) > 4.5 {
            score += 15;
            factors.push(ThreatFactor {
                name:   "High-entropy URL".into(),
                delta:  15,
                reason: "The URL path looks obfuscated or auto-generated, which is common in malware distribution.".into(),
            });
        }
    }

    // ── Factor 5: Suspicious words in filename (+15) ──────────────────────
    if let Some(word) = SUSPICIOUS_WORDS.iter().find(|w| filename_lower.contains(*w)) {
        score += 15;
        factors.push(ThreatFactor {
            name:   "Suspicious keyword".into(),
            delta:  15,
            reason: format!("Filename contains '{}', associated with cracking tools or malware.", word),
        });
    }

    // ── Factor 6: Plain HTTP for executables (+15) ────────────────────────
    if EXECUTABLE_EXTS.contains(&ext.as_str()) && url_lower.starts_with("http://") {
        score += 15;
        factors.push(ThreatFactor {
            name:   "Unencrypted transfer".into(),
            delta:  15,
            reason: "Executable downloaded over plain HTTP — content can be tampered in transit.".into(),
        });
    }

    // ── Factor 7: URL shortener (+15) ────────────────────────────────────
    if URL_SHORTENERS.iter().any(|s| domain.contains(*s)) {
        score += 15;
        factors.push(ThreatFactor {
            name:   "Shortened URL".into(),
            delta:  15,
            reason: "URL passes through a shortener service, hiding the real destination.".into(),
        });
    }

    // ── Factor 8: File size anomaly for executables (+10) ────────────────
    if EXECUTABLE_EXTS.contains(&ext.as_str()) && file_size > 0 {
        if file_size < 50_000 {
            score += 10;
            factors.push(ThreatFactor {
                name:   "Unusually small executable".into(),
                delta:  10,
                reason: format!(
                    "File is only {} KB — many dropper/loader trojans are very small.",
                    file_size / 1024
                ),
            });
        }
    }

    // ── Factor 9: Cross-origin referrer (+5) ─────────────────────────────
    if !referrer.is_empty() {
        let ref_domain = extract_domain(&referrer.to_lowercase());
        if !ref_domain.is_empty() && ref_domain != domain
            && !TRUSTED_DOMAINS.iter().any(|t| ref_domain == *t || ref_domain.ends_with(&format!(".{}", t)))
        {
            score += 5;
            factors.push(ThreatFactor {
                name:   "Cross-origin referrer".into(),
                delta:  5,
                reason: format!(
                    "Download initiated from '{}' but file comes from '{}'.",
                    ref_domain, domain
                ),
            });
        }
    }

    // ── Factor 10: Trusted domain (-20) ──────────────────────────────────
    let effective_domain = if domain.is_empty() { extract_domain(&referrer.to_lowercase()) } else { domain.clone() };
    if TRUSTED_DOMAINS
        .iter()
        .any(|t| effective_domain == *t || effective_domain.ends_with(&format!(".{}", t)))
    {
        score -= 20;
        factors.push(ThreatFactor {
            name:   "Trusted publisher".into(),
            delta:  -20,
            reason: format!("'{}' is a well-known, trusted software publisher.", effective_domain),
        });
    }

    // ── Factor 11: HTTPS (-5) ─────────────────────────────────────────────
    if url_lower.starts_with("https://") {
        score -= 5;
        factors.push(ThreatFactor {
            name:   "Encrypted transfer".into(),
            delta:  -5,
            reason: "File is served over HTTPS, protecting against in-transit tampering.".into(),
        });
    }

    ThreatAnalysis {
        score: score.clamp(0, 100) as u8,
        factors,
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn extract_extension(filename: &str) -> String {
    std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default()
}

fn has_mime_extension_mismatch(ext: &str, mime: &str) -> bool {
    match ext {
        "jpg" | "jpeg" | "png" | "gif" | "webp" => !mime.starts_with("image/"),
        "mp4" | "mkv" | "avi" | "mov"           => !mime.starts_with("video/"),
        "mp3" | "flac" | "aac" | "ogg"          => !mime.starts_with("audio/"),
        "pdf"                                     => !mime.contains("pdf"),
        _                                         => false,
    }
}

fn extract_domain(url: &str) -> String {
    let url = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    // Strip port and path
    url.split('/').next()
        .unwrap_or("")
        .split(':').next()
        .unwrap_or("")
        .to_string()
}

fn extract_url_path(url: &str) -> Option<String> {
    let url = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let path = url.splitn(2, '/').nth(1)?;
    Some(path.to_string())
}

fn is_ip_address(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok()
}

/// Shannon entropy of character distribution — measures randomness.
pub fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut counts = [0u32; 256];
    for b in s.bytes() {
        counts[b as usize] += 1;
    }
    let len = s.len() as f64;
    counts.iter()
        .filter(|&&c| c > 0)
        .fold(0.0f64, |acc, &c| {
            let p = c as f64 / len;
            acc - p * p.log2()
        })
}
