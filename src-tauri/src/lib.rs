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
use engine::shutdown::ShutdownControl;
use engine::torrent::TorrentEngine;
use storage::db::Database;

/// Shared application state passed to all Tauri commands.
pub type DbState = Arc<Mutex<Database>>;
pub type QueueState = Arc<DownloadQueue>;
pub type TorrentState = Arc<TorrentEngine>;
pub type ShutdownState = Arc<ShutdownControl>;

/// Concurrency fallback when `max_parallel_downloads` is missing or unparseable.
const DEFAULT_MAX_PARALLEL: usize = 3;

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
        .plugin(tauri_plugin_dialog::init())
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

            let max_parallel = db
                .get_setting("max_parallel_downloads")
                .ok()
                .flatten()
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(DEFAULT_MAX_PARALLEL);

            let torrent_dir = db
                .get_setting("torrent_save_path")
                .ok()
                .flatten()
                .unwrap_or_else(|| "~/Downloads".to_string());

            let speed_limit = db.get_setting(engine::throttle::SETTING_KEY).ok().flatten();

            let db: DbState = Arc::new(Mutex::new(db));
            app.manage(db.clone());

            // ── Download Queue ────────────────────────────────────────────
            let queue: QueueState = Arc::new(DownloadQueue::new());
            app.manage(queue.clone());

            // The queue, the scheduler and every in-flight download share one
            // set of stop signals.
            let control = queue.control();

            // ── Auto-shutdown cancel flag ─────────────────────────────────
            // Managed so the UI's cancel button can reach an in-flight countdown.
            let shutdown_control: ShutdownState = Arc::new(ShutdownControl::new());
            app.manage(shutdown_control.clone());

            // ── BitTorrent session ────────────────────────────────────────
            // Built synchronously: the Tauri commands need the managed state to
            // exist before the first window can invoke them.
            let torrent_dir = utils::expand_path(&torrent_dir);
            let torrent: TorrentState = tauri::async_runtime::block_on(
                TorrentEngine::new(std::path::PathBuf::from(&torrent_dir)),
            )
            .map_err(|e| {
                tracing::error!("Failed to start BitTorrent session: {}", e);
                e
            })?;
            app.manage(torrent.clone());

            // Restore the saved rate limit before anything can start transferring.
            engine::throttle::apply_from_settings(speed_limit.as_deref(), Some(&torrent));

            // ── Background queue processor ────────────────────────────────
            {
                let app_handle = app.handle().clone();
                let db         = db.clone();
                let queue      = queue.clone();
                tauri::async_runtime::spawn(async move {
                    info!("Queue processor starting");
                    queue.process_queue(app_handle, db, max_parallel).await;
                });
            }

            // ── Scheduler ─────────────────────────────────────────────────
            {
                let app_handle = app.handle().clone();
                let db         = db.clone();
                let queue      = queue.clone();
                tauri::async_runtime::spawn(async move {
                    engine::scheduler::run_scheduler(app_handle, db, queue).await;
                });
            }

            // ── Auto-shutdown watcher ─────────────────────────────────────
            // Inert unless the `auto_shutdown` setting is on, and even then only
            // after it has seen the queue busy — see engine::shutdown.
            {
                let app_handle = app.handle().clone();
                let db         = db.clone();
                let queue      = queue.clone();
                let shutdown   = shutdown_control.clone();
                tauri::async_runtime::spawn(async move {
                    engine::shutdown::run_auto_shutdown(app_handle, db, queue, shutdown).await;
                });
            }

            // ── Torrent stats poller ──────────────────────────────────────
            {
                let app_handle = app.handle().clone();
                let db         = db.clone();
                let torrent    = torrent.clone();
                let control    = control.clone();
                tauri::async_runtime::spawn(async move {
                    engine::torrent::poll_torrents(app_handle, db, torrent, control).await;
                });
            }

            // ── Extension HTTP server ─────────────────────────────────────
            // Lets the browser extension (and native host) reach FluxDM at
            // http://127.0.0.1:54321  without needing native messaging.
            {
                let db         = db.clone();
                let queue      = queue.clone();
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    server::start_http_server(db, queue, app_handle).await;
                });
            }

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
            commands::cmd_is_torrent_source,
            commands::cmd_add_torrent,
            commands::cmd_cancel_shutdown,
            commands::cmd_crawl_site,
            commands::cmd_add_downloads,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FluxDM");
}
