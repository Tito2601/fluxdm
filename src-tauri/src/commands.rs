use std::sync::Arc;
use tauri::{Emitter, State};
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::engine::downloader::DownloadJob;
use crate::engine::queue::DownloadQueue;
use crate::storage::db::{AnalyticsData, Database};

// ── Type aliases for cleaner signatures ───────────────────────────────────────
type Db<'a> = State<'a, Arc<Mutex<Database>>>;
type Queue<'a> = State<'a, Arc<DownloadQueue>>;

// ── Add a new download ────────────────────────────────────────────────────────

/// Add a download URL to the queue. Returns the job ID.
#[tauri::command]
pub async fn cmd_add_download(
    url:       String,
    filename:  String,
    save_path: String,
    headers:   Option<serde_json::Value>,
    cookies:   Option<String>,
    db:        Db<'_>,
    queue:     Queue<'_>,
) -> Result<String, String> {
    info!("cmd_add_download: url={}", url);

    // AI: clean up the filename
    let clean_name = crate::ai::renamer::smart_rename(&url, &filename, "");

    let job = DownloadJob::new(url, clean_name, save_path, headers, cookies);
    let id  = job.id.clone();

    // Persist to DB before queuing (so UI reflects it immediately)
    db.lock().await
        .upsert_download(&job)
        .map_err(|e| { error!("DB error: {}", e); e.to_string() })?;

    queue.enqueue(job).await;

    Ok(id)
}

// ── Pause / Resume / Cancel / Delete ─────────────────────────────────────────

#[tauri::command]
pub async fn cmd_pause_download(id: String, db: Db<'_>, queue: Queue<'_>) -> Result<(), String> {
    queue.pause(&id).await;
    db.lock().await
        .update_download_status(&id, "paused")
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_resume_download(id: String, db: Db<'_>, queue: Queue<'_>) -> Result<(), String> {
    queue.resume(&id).await;

    let db_lock = db.lock().await;

    // Load the job and re-queue it so the queue processor picks it up.
    let jobs = db_lock.get_all_downloads().map_err(|e| e.to_string())?;

    if let Some(mut job) = jobs.into_iter().find(|j| j.id == id) {
        job.status = crate::engine::downloader::DownloadStatus::Queued;
        db_lock.update_download_status(&id, "queued").map_err(|e| e.to_string())?;
        drop(db_lock);
        queue.enqueue(job).await;
    }

    Ok(())
}

#[tauri::command]
pub async fn cmd_cancel_download(id: String, db: Db<'_>, queue: Queue<'_>) -> Result<(), String> {
    queue.cancel(&id).await;
    db.lock().await
        .update_download_status(&id, "cancelled")
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_delete_download(
    id:          String,
    delete_file: bool,
    db:          Db<'_>,
) -> Result<(), String> {
    // Optionally delete the actual file from disk
    if delete_file {
        let db_lock = db.lock().await;
        if let Ok(downloads) = db_lock.get_all_downloads() {
            if let Some(job) = downloads.iter().find(|d| d.id == id) {
                let path = format!("{}/{}", job.save_path, job.filename);
                if std::path::Path::new(&path).exists() {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        db_lock.delete_download(&id).map_err(|e| e.to_string())?;
    } else {
        db.lock().await
            .delete_download(&id)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

// ── Read downloads ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn cmd_get_downloads(db: Db<'_>) -> Result<Vec<DownloadJob>, String> {
    db.lock().await
        .get_all_downloads()
        .map_err(|e| e.to_string())
}

// ── Settings ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn cmd_get_settings(
    db: Db<'_>,
) -> Result<std::collections::HashMap<String, String>, String> {
    db.lock().await
        .get_all_settings()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_update_setting(
    key:   String,
    value: String,
    db:    Db<'_>,
) -> Result<(), String> {
    db.lock().await
        .update_setting(&key, &value)
        .map_err(|e| e.to_string())
}

// ── File system ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn cmd_open_file(path: String) -> Result<(), String> {
    open_path(&path)
}

#[tauri::command]
pub async fn cmd_open_folder(path: String) -> Result<(), String> {
    // Open the containing folder, not the file itself
    let folder = std::path::Path::new(&path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(path);
    open_path(&folder)
}

fn open_path(path: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ── History ───────────────────────────────────────────────────────────────────

/// Delete all history rows and remove completed / cancelled / failed downloads.
#[tauri::command]
pub async fn cmd_clear_history(db: Db<'_>) -> Result<(), String> {
    info!("cmd_clear_history");
    db.lock().await
        .clear_history()
        .map_err(|e| { error!("clear_history error: {}", e); e.to_string() })
}

// ── AI: Threat details ────────────────────────────────────────────────────────

/// Return the per-factor breakdown of the threat score for a given file.
/// Called by the UI when the user expands a ThreatBadge tooltip.
#[tauri::command]
pub async fn cmd_get_threat_details(
    url:       String,
    filename:  String,
    mime:      Option<String>,
    referrer:  Option<String>,
    file_size: Option<u64>,
) -> Result<crate::ai::threat::ThreatAnalysis, String> {
    Ok(crate::ai::threat::explain_threat_score(
        &url,
        &filename,
        mime.as_deref().unwrap_or(""),
        referrer.as_deref().unwrap_or(""),
        file_size.unwrap_or(0),
    ))
}

// ── AI: Duplicate detection ───────────────────────────────────────────────────

/// Check whether a URL was previously downloaded and/or the output file exists.
#[tauri::command]
pub async fn cmd_check_duplicate(
    url:         String,
    output_path: String,
    db:          Db<'_>,
) -> Result<crate::ai::duplicate::DuplicateCheck, String> {
    let expanded = crate::utils::expand_path(&output_path);
    Ok(crate::ai::duplicate::check_duplicate(&url, &expanded, &*db.lock().await))
}

// ── Analytics ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn cmd_get_analytics(db: Db<'_>) -> Result<AnalyticsData, String> {
    db.lock().await
        .get_analytics()
        .map_err(|e| e.to_string())
}

// ── AI: LLM filename suggestion ───────────────────────────────────────────────

/// Test whether the configured local LLM is reachable.
/// Returns the model's reply (usually "OK") or an error message.
#[tauri::command]
pub async fn cmd_test_llm(
    endpoint: String,
    model:    String,
) -> Result<String, String> {
    info!("cmd_test_llm: endpoint={}", endpoint);
    let config = crate::ai::llm::LlmConfig {
        endpoint,
        model,
        timeout: std::time::Duration::from_secs(20),
    };
    crate::ai::llm::test_connection(&config)
        .await
        .map_err(|e| e.to_string())
}

/// Ask the local LLM to suggest a cleaner filename.
/// Reads `llm_enabled`, `llm_endpoint`, `llm_model` from the settings table.
#[tauri::command]
pub async fn cmd_llm_suggest_name(
    url:      String,
    filename: String,
    mime:     Option<String>,
    db:       Db<'_>,
) -> Result<String, String> {
    info!("cmd_llm_suggest_name: {}", filename);

    let settings = db.lock().await
        .get_all_settings()
        .map_err(|e| e.to_string())?;

    if settings.get("llm_enabled").map(|v| v.as_str()) != Some("true") {
        return Err("LLM renaming is disabled in settings".into());
    }

    let endpoint = settings
        .get("llm_endpoint")
        .cloned()
        .unwrap_or_else(|| "http://localhost:11434/api/generate".into());
    let model = settings
        .get("llm_model")
        .cloned()
        .unwrap_or_else(|| "llama3.2:1b".into());

    let config = crate::ai::llm::LlmConfig {
        endpoint,
        model,
        timeout: std::time::Duration::from_secs(10),
    };

    crate::ai::llm::suggest_filename(
        &url,
        &filename,
        mime.as_deref().unwrap_or(""),
        &config,
    )
    .await
    .ok_or_else(|| "LLM did not return a usable suggestion (is it running?)".into())
}

// ── Stream: probe ─────────────────────────────────────────────────────────────

/// Fetch `url` and return stream type + available quality variants.
/// Returns `stream_type: "direct"` when the URL is a regular file (not HLS/DASH).
#[tauri::command]
pub async fn cmd_probe_stream(
    url: String,
) -> Result<crate::engine::stream::StreamInfo, String> {
    info!("cmd_probe_stream: url={}", url);
    crate::engine::stream::probe_stream(&url)
        .await
        .map_err(|e| e.to_string())
}

// ── Stream: start download ────────────────────────────────────────────────────

/// Start downloading an HLS or DASH stream.
///
/// - `manifest_url`  – HLS media playlist URL *or* DASH MPD URL.
/// - `repr_id`       – DASH representation ID (None for HLS).
/// - `stream_type`   – `"hls"` | `"dash"`.
/// - `filename`      – desired output filename (e.g. `"movie.ts"`, `"show.mp4"`).
/// - `save_path`     – destination directory (tilde is expanded).
///
/// Returns the new download job ID so the UI can track it immediately.
#[tauri::command]
pub async fn cmd_add_stream_download(
    manifest_url: String,
    repr_id:      Option<String>,
    stream_type:  String,
    filename:     String,
    save_path:    String,
    app:          tauri::AppHandle,
    db:           Db<'_>,
) -> Result<String, String> {
    info!(
        "cmd_add_stream_download: type={} url={}",
        stream_type, manifest_url
    );

    // Clean up the filename
    let clean_name = crate::ai::renamer::smart_rename(&manifest_url, &filename, "");

    // Build a DownloadJob that lives in the DB / UI like any other download
    let mut job = crate::engine::downloader::DownloadJob::new(
        manifest_url.clone(),
        clean_name,
        save_path,
        None,
        None,
    );
    job.category = "videos".to_string(); // streams are always video content

    let id = job.id.clone();

    // Persist and broadcast so the UI shows the row immediately
    db.lock().await
        .upsert_download(&job)
        .map_err(|e| { error!("DB error: {}", e); e.to_string() })?;

    app.emit("download_added", &job).map_err(|e| e.to_string())?;

    // Spawn the download task (runs independently of the normal queue)
    let db_arc      = db.inner().clone();
    let app_clone   = app.clone();
    let st_clone    = stream_type.clone();
    let ri_clone    = repr_id.clone();

    tokio::spawn(async move {
        if let Err(e) = crate::engine::stream::download_stream(
            job,
            st_clone,
            ri_clone,
            app_clone,
            db_arc,
        ).await {
            error!("Stream download task error: {}", e);
        }
    });

    Ok(id)
}
