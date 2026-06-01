use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;
use tracing::info;

mod ai;
mod bridge;
mod commands;
mod engine;
mod server;
mod storage;
mod tray;
pub mod utils;

use engine::queue::DownloadQueue;
use storage::db::Database;

/// Shared application state passed to all Tauri commands.
pub type DbState = Arc<Mutex<Database>>;
pub type QueueState = Arc<DownloadQueue>;

pub fn run() {
    // Initialize tracing (logging)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "fluxdm=debug,warn".into()),
        )
        .init();

    info!("Starting FluxDM v{}", env!("CARGO_PKG_VERSION"));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // ── System tray ───────────────────────────────────────────────
            tray::setup_tray(app)?;

            // ── Database ──────────────────────────────────────────────────
            let data_dir = app.path().app_data_dir()?;
            let db = Database::new(data_dir).map_err(|e| {
                tracing::error!("Failed to open database: {}", e);
                e
            })?;

            // Reset any downloads that were mid-flight when the app last closed.
            match db.reset_interrupted_downloads() {
                Ok(0) => {}
                Ok(n) => info!("{} interrupted download(s) reset to paused — resume to continue", n),
                Err(e) => tracing::warn!("Could not reset interrupted downloads: {}", e),
            }

            let db: DbState = Arc::new(Mutex::new(db));
            app.manage(db.clone());

            // ── Download Queue ────────────────────────────────────────────
            let queue: QueueState = Arc::new(DownloadQueue::new());
            app.manage(queue.clone());

            // ── Background queue processor ────────────────────────────────
            let app_handle = app.handle().clone();
            let db_clone = db.clone();
            let queue_clone = queue.clone();

            tauri::async_runtime::spawn(async move {
                info!("Queue processor starting");
                queue_clone
                    .process_queue(app_handle, db_clone, 3)
                    .await;
            });

            // ── Extension HTTP server ─────────────────────────────────────
            // Lets the browser extension (and native host) reach FluxDM at
            // http://127.0.0.1:54321  without needing native messaging.
            let db_http     = db.clone();
            let queue_http  = queue.clone();
            let handle_http = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                server::start_http_server(db_http, queue_http, handle_http).await;
            });

            info!("FluxDM setup complete");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::cmd_add_download,
            commands::cmd_pause_download,
            commands::cmd_resume_download,
            commands::cmd_cancel_download,
            commands::cmd_delete_download,
            commands::cmd_get_downloads,
            commands::cmd_get_settings,
            commands::cmd_update_setting,
            commands::cmd_open_file,
            commands::cmd_open_folder,
            commands::cmd_clear_history,
            commands::cmd_get_analytics,
            commands::cmd_get_threat_details,
            commands::cmd_check_duplicate,
            commands::cmd_probe_stream,
            commands::cmd_add_stream_download,
            commands::cmd_test_llm,
            commands::cmd_llm_suggest_name,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FluxDM");
}
