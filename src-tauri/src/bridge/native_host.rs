#![allow(dead_code)]
/// Native Messaging Host — bridges the browser extension to FluxDM.
///
/// This module is the *in-process* companion to the standalone `fluxdm-host`
/// binary (see `src/bin/native_host.rs`).  It is not spawned directly by the
/// browser; instead it documents the protocol and provides shared helpers used
/// by the HTTP server layer (`src/server/mod.rs`).
///
/// Protocol (Chrome / Firefox native messaging):
///   Browser → host : [4-byte LE u32 length][JSON payload]
///   Host → browser : [4-byte LE u32 length][JSON response]
///
/// The standalone binary (`fluxdm-host`) handles actual stdin/stdout I/O and
/// proxies through to the HTTP server running inside the main Tauri process.
///
/// # Registration
///
/// | Platform | Chrome                                                                        |
/// |----------|-------------------------------------------------------------------------------|
/// | Windows  | `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.fluxdm.host` → JSON path |
/// | macOS    | `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/com.fluxdm.host.json` |
/// | Linux    | `~/.config/google-chrome/NativeMessagingHosts/com.fluxdm.host.json`          |
///
/// Firefox uses `allowed_extensions` instead of `allowed_origins` in the JSON.

use std::io::{Read, Write};

// ── Protocol helpers (shared with tests) ─────────────────────────────────────

/// Encode a message in native messaging format: [u32 LE length][bytes].
pub fn encode_message(json: &str) -> Vec<u8> {
    let bytes = json.as_bytes();
    let mut out = Vec::with_capacity(4 + bytes.len());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
    out
}

/// Decode one native messaging message from a reader.
/// Returns `None` on EOF.
pub fn decode_message<R: Read>(reader: &mut R) -> Option<String> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).ok()?;
    let len = u32::from_le_bytes(len_buf) as usize;

    if len == 0 || len > 1_048_576 {
        return None;
    }

    let mut msg = vec![0u8; len];
    reader.read_exact(&mut msg).ok()?;
    String::from_utf8(msg).ok()
}

/// Write one native messaging message to a writer.
pub fn write_message<W: Write>(writer: &mut W, json: &str) -> std::io::Result<()> {
    let encoded = encode_message(json);
    writer.write_all(&encoded)?;
    writer.flush()
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trip_encode_decode() {
        let msg = r#"{"action":"add_download","url":"https://example.com/file.zip"}"#;
        let encoded = encode_message(msg);

        let mut cursor = Cursor::new(encoded);
        let decoded = decode_message(&mut cursor).expect("decode should succeed");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn decode_rejects_empty_length() {
        let data = 0u32.to_le_bytes();
        let mut cursor = Cursor::new(data);
        assert!(decode_message(&mut cursor).is_none());
    }

    #[test]
    fn decode_rejects_oversized() {
        let data = (2_000_000u32).to_le_bytes();
        let mut cursor = Cursor::new(data);
        assert!(decode_message(&mut cursor).is_none());
    }
}
