//! The one HTTP client every transfer shares.
//!
//! Building a `reqwest::Client` per request throws away its connection pool, so
//! each segment and each retry pays a fresh TCP and TLS handshake. One process-wide
//! client lets an eight-segment download reuse keep-alive connections to the origin.

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::{Client, RequestBuilder};

pub const USER_AGENT: &str = "FluxDM/0.1";

/// Give up on a connection that stalls for this long. Deliberately *not*
/// `ClientBuilder::timeout`, which bounds the whole request including the body —
/// that would kill a healthy but slow multi-gigabyte segment partway through.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

static CLIENT: OnceLock<Client> = OnceLock::new();

/// The shared client. Cheap to call: `Client` is an `Arc` internally.
///
/// Callers that need a deadline on a short request (a HEAD probe, a manifest
/// fetch) should add `.timeout(..)` to that individual request.
pub fn client() -> &'static Client {
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .pool_max_idle_per_host(16)
            .build()
            .expect("HTTP client construction cannot fail with these options")
    })
}

// ── Captured request headers ──────────────────────────────────────────────────

/// Headers captured from the browser that must never reach the origin.
///
/// Two distinct reasons, both of which corrupt a transfer if ignored:
///
/// - `range` is owned by the segmenter. A captured value describes whatever the
///   browser happened to ask for, so forwarding it would silently overwrite the
///   slice a segment is trying to fetch.
/// - The rest are connection-scoped or recomputed by `reqwest` for this specific
///   request. Replaying the browser's values describes the wrong connection.
const SKIPPED_HEADERS: &[&str] = &[
    "range",
    "host",
    "content-length",
    "connection",
    "transfer-encoding",
    "keep-alive",
    "upgrade",
    "accept-encoding", // reqwest negotiates this to match its own decompression
];

/// Replay browser-captured headers and cookies onto an outgoing request.
///
/// Signed URLs, hotlink guards, and session-scoped media servers routinely reject
/// a bare request that succeeds in the browser, because the grant lives in the
/// `Referer` or `Cookie` header rather than the URL. The extension already
/// collects those; this puts them back on the wire.
///
/// Applied before the caller's own `.header()` calls would be, so anything the
/// transfer sets explicitly still wins.
pub fn apply_captured(
    mut req: RequestBuilder,
    headers: Option<&serde_json::Value>,
    cookies: Option<&str>,
) -> RequestBuilder {
    if let Some(serde_json::Value::Object(map)) = headers {
        for (name, value) in map {
            let lower = name.to_ascii_lowercase();
            if SKIPPED_HEADERS.contains(&lower.as_str()) {
                continue;
            }
            // Non-string JSON values are not headers; skip rather than stringify
            // them into something the origin will not recognize.
            if let Some(v) = value.as_str() {
                req = req.header(name.as_str(), v);
            }
        }
    }

    // Applied after the header map so an explicit cookie jar wins over a stale
    // `Cookie` entry that happened to be captured alongside it.
    if let Some(cookie) = cookies {
        req = req.header("Cookie", cookie);
    }

    req
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_is_shared() {
        assert!(std::ptr::eq(client(), client()));
    }

    #[test]
    fn range_is_never_replayed() {
        // The segmenter owns Range; a captured value would redirect the slice.
        assert!(SKIPPED_HEADERS.contains(&"range"));
    }

    #[test]
    fn connection_scoped_headers_are_skipped() {
        for h in ["host", "content-length", "connection", "accept-encoding"] {
            assert!(SKIPPED_HEADERS.contains(&h), "{h} should be skipped");
        }
    }

    #[test]
    fn ordinary_headers_are_not_skipped() {
        for h in ["referer", "cookie", "user-agent", "origin", "authorization"] {
            assert!(!SKIPPED_HEADERS.contains(&h), "{h} should be forwarded");
        }
    }
}
