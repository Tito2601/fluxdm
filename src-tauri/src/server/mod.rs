/// FluxDM HTTP server — local IPC for the browser extension.
///
/// Listens on `http://127.0.0.1:54321` so the Chrome/Firefox extension can:
///   GET  /status  → `{"running":true, "version":"x.y.z"}`
///   POST /add     → queue a new download, return `{"success":true, "id":"..."}`
///
/// The native messaging host binary (`fluxdm-host`) also proxies through here,
/// so all download routing goes through the same `DownloadQueue`.

use axum::{
    extract::State,
    http::{Method, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, warn};

use crate::ai;
use crate::engine::queue::DownloadQueue;
use crate::storage::db::Database;

// ── Shared state ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ServerState {
    /// Retained for future endpoints that may need DB access.
    #[allow(dead_code)]
    pub db:         Arc<Mutex<Database>>,
    /// Retained for future endpoints that may need queue access.
    #[allow(dead_code)]
    pub queue:      Arc<DownloadQueue>,
    pub app_handle: AppHandle,
}

// ── Request types ─────────────────────────────────────────────────────────────

/// Payload sent by the browser extension (background.js) or native host.
/// camelCase matches the JavaScript side directly.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddPayload {
    url:          String,
    filename:     Option<String>,
    save_path:    Option<String>,
    /// Reserved for future per-request header injection; not yet processed.
    #[allow(dead_code)]
    headers:      Option<serde_json::Value>,
    /// Reserved for future cookie forwarding; not yet processed.
    #[allow(dead_code)]
    cookies:      Option<String>,
    referrer:     Option<String>,
    page_url:     Option<String>,
    content_type: Option<String>,
    file_size:    Option<i64>,
}

// ── Server entry point ────────────────────────────────────────────────────────

/// Spawn the HTTP server as a tokio task. Errors binding the port are logged
/// but are not fatal — the app works without the HTTP bridge.
pub async fn start_http_server(
    db:         Arc<Mutex<Database>>,
    queue:      Arc<DownloadQueue>,
    app_handle: AppHandle,
) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    let state = ServerState { db, queue, app_handle };

    let app = Router::new()
        .route("/status", get(status_handler))
        .route("/add",    post(add_handler))
        .layer(cors)
        .with_state(state);

    match tokio::net::TcpListener::bind("127.0.0.1:54321").await {
        Ok(listener) => {
            info!("Extension HTTP server → http://127.0.0.1:54321");
            if let Err(e) = axum::serve(listener, app).await {
                error!("HTTP server stopped: {e}");
            }
        }
        Err(e) => {
            // Port already in use (second FluxDM instance?) — not fatal.
            warn!("Could not bind :54321 — extension HTTP fallback disabled ({e})");
        }
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn status_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "running":  true,
        "name":     "FluxDM",
        "version":  env!("CARGO_PKG_VERSION"),
    }))
}

async fn add_handler(
    State(state): State<ServerState>,
    Json(payload): Json<AddPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    info!("Extension /add  url={}", payload.url);

    // Derive a filename from the URL if the extension didn't send one.
    let raw_filename = payload
        .filename
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| {
            payload
                .url
                .split('/')
                .last()
                .and_then(|s| s.split('?').next())
                .filter(|s| !s.is_empty())
                .unwrap_or("download")
                .to_string()
        });

    let mime     = payload.content_type.clone().unwrap_or_default();
    let referrer = payload.referrer.clone().unwrap_or_default();

    // Run the AI pipeline to produce a clean filename and category suggestion.
    let filename     = ai::renamer::smart_rename(&payload.url, &raw_filename, "");
    let category     = ai::categorizer::categorize_download(&payload.url, &filename, &mime);
    let threat_score = ai::threat::calculate_threat_score(&payload.url, &filename, &mime, &referrer);
    let subfolder    = ai::categorizer::suggested_subfolder(&category);

    // Build a suggested save path (user can change this in the dialog).
    let raw_base  = payload.save_path.unwrap_or_else(|| "~/Downloads".to_string());
    let base_path = crate::utils::expand_path(&raw_base);
    let save_path = if subfolder.is_empty() {
        base_path
    } else {
        format!("{}/{}", base_path.trim_end_matches('/'), subfolder)
    };

    // ── Emit "download_requested" so the UI shows the save dialog ─────────────
    // We do NOT create a DB entry or enqueue here.
    // The user confirms the save location in the Add Download dialog,
    // then the normal cmd_add_download command does the actual work.
    let _ = state.app_handle.emit("download_requested", serde_json::json!({
        "url":         payload.url,
        "filename":    filename,
        "savePath":    save_path,
        "mimeType":    mime,
        "referrer":    referrer,
        "pageUrl":     payload.page_url,
        "threatScore": threat_score,
        "category":    category,
        "fileSize":    payload.file_size,
    }));

    Ok(Json(serde_json::json!({
        "success": true,
        "pending": true,   // tells the extension the user will confirm
    })))
}
