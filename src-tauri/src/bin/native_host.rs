/// FluxDM Native Messaging Host (`fluxdm-host`)
///
/// Implements the Chrome / Firefox native messaging stdio protocol:
///   INPUT  (browser → host): [u32 LE length][JSON bytes]
///   OUTPUT (host → browser): [u32 LE length][JSON bytes]
///
/// This binary proxies messages to the running FluxDM app's HTTP server
/// (http://127.0.0.1:54321/add).  If the app is not running it returns
/// a structured error so the extension can show a helpful message.
///
/// Install (Windows — run as Administrator or the installing user):
///   1. Copy `fluxdm-host.exe` next to `FluxDM.exe`
///   2. Run `extension\install.ps1`
///
/// Install (macOS / Linux):
///   1. Copy `fluxdm-host` next to `FluxDM`
///   2. Run `extension/install.sh`

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const FLUXDM_HTTP: &str = "127.0.0.1:54321";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const READ_TIMEOUT:    Duration = Duration::from_secs(10);

fn main() {
    // Native hosts must not write anything to stderr (Chrome reads it as an
    // error and may terminate the connection), so we disable logging here.
    // Redirect stderr to a temp log file if you need to debug.

    let stdin  = io::stdin();
    let stdout = io::stdout();

    loop {
        // ── Read one message ──────────────────────────────────────────────
        let mut len_buf = [0u8; 4];
        match stdin.lock().read_exact(&mut len_buf) {
            Ok(_)  => {}
            Err(_) => break, // EOF — browser closed the port
        }

        let msg_len = u32::from_le_bytes(len_buf) as usize;

        // Guard: Chrome caps native messages at 1 MB
        if msg_len == 0 || msg_len > 1_048_576 {
            write_response(&stdout, r#"{"success":false,"error":"invalid message length"}"#);
            continue;
        }

        let mut msg_buf = vec![0u8; msg_len];
        if stdin.lock().read_exact(&mut msg_buf).is_err() {
            break;
        }

        let payload = String::from_utf8_lossy(&msg_buf);

        // ── Forward to running FluxDM app ─────────────────────────────────
        let response = match forward_to_app(&payload) {
            Ok(body) => body,
            Err(e)   => format!(
                r#"{{"success":false,"error":{},"hint":"Make sure FluxDM is running"}}"#,
                serde_json_str(&e.to_string())
            ),
        };

        write_response(&stdout, &response);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Forward the JSON payload to the FluxDM HTTP server and return the body.
fn forward_to_app(json: &str) -> Result<String, Box<dyn std::error::Error>> {
    let addr: SocketAddr = FLUXDM_HTTP.parse()?;
    let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
    stream.set_read_timeout(Some(READ_TIMEOUT))?;

    // Minimal HTTP/1.1 POST request
    let request = format!(
        "POST /add HTTP/1.1\r\n\
         Host: {}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        FLUXDM_HTTP,
        json.len(),
        json
    );

    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let mut raw = String::new();
    stream.read_to_string(&mut raw)?;

    // Split HTTP headers / body on the first blank line
    if let Some(idx) = raw.find("\r\n\r\n") {
        Ok(raw[idx + 4..].to_string())
    } else {
        Err("Malformed HTTP response".into())
    }
}

/// Write a native messaging length-prefixed response to stdout.
fn write_response(stdout: &io::Stdout, body: &str) {
    let bytes  = body.as_bytes();
    let length = (bytes.len() as u32).to_le_bytes();
    let mut out = stdout.lock();
    let _ = out.write_all(&length);
    let _ = out.write_all(bytes);
    let _ = out.flush();
}

/// Escape a Rust string as a JSON string literal (double-quoted, special chars escaped).
fn serde_json_str(s: &str) -> String {
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"',  "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{}\"", escaped)
}
