//! BitTorrent engine.
//!
//! Wraps a single `librqbit` session. Each torrent is keyed by the FluxDM job id
//! so the rest of the app never has to know about the library's own numeric ids.
//!
//! Torrents deliberately bypass the [`DownloadQueue`](crate::engine::queue::DownloadQueue):
//! a swarm is mostly idle waiting on peers rather than saturating a connection, so
//! counting it against `max_parallel_downloads` would starve HTTP transfers for no
//! benefit. The scheduler gate still applies — see [`poll_torrents`].

use std::collections::HashMap;
use std::path::{Component, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ByteBufOwned, ManagedTorrent, Session,
    TorrentMetaV1Info, TorrentStatsState,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::engine::control::DownloadControl;
use crate::engine::downloader::{DownloadStatus, ProgressEvent};
use crate::engine::rate::Ema;
use crate::storage::db::Database;

/// How often live torrent stats are recomputed and pushed to the UI.
const POLL: std::time::Duration = std::time::Duration::from_secs(1);

/// Persist to SQLite at most this often per torrent. The UI updates every [`POLL`],
/// but writing a row every second per torrent would thrash the disk for no gain.
const DB_WRITE_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

// ── Public data ───────────────────────────────────────────────────────────────

/// What the UI learns the moment a torrent is accepted.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentAdded {
    pub name:        String,
    pub total_bytes: u64,
    pub info_hash:   String,
    pub files:       Vec<TorrentFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentFile {
    pub name:   String,
    pub length: u64,
}

/// Is `source` something the torrent engine can take?
pub fn is_torrent_source(source: &str) -> bool {
    let s = source.trim();
    s.starts_with("magnet:") || s.to_lowercase().ends_with(".torrent")
}

/// A torrent name is attacker-controlled, so accept it as a folder only when it is
/// a single ordinary path component — no `..`, no root, no drive prefix.
fn safe_folder_name(name: &str) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    let pb = PathBuf::from(name);
    let mut components = pb.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(c)), None) => Some(PathBuf::from(c)),
        _ => None,
    }
}

/// Where a torrent's files should land under `base`.
///
/// A single-file torrent writes that file directly into `base`; anything with more
/// than one file gets a folder named after the torrent, matching what librqbit does
/// when it picks the folder itself. An unusable name falls back to `base`, which is
/// untidy but never writes outside the directory the user chose.
fn torrent_output_folder(base: PathBuf, info: &TorrentMetaV1Info<ByteBufOwned>) -> PathBuf {
    let file_count = info.iter_file_details().map(|f| f.count()).unwrap_or(0);
    if file_count < 2 {
        return base;
    }

    let name = info
        .name
        .as_ref()
        .map(|n| String::from_utf8_lossy(&n.0).into_owned())
        .and_then(|n| safe_folder_name(&n));

    match name {
        Some(sub) => base.join(sub),
        None => {
            warn!("Torrent has no usable name; writing files directly into {:?}", base);
            base
        }
    }
}

// ── Engine ────────────────────────────────────────────────────────────────────

pub struct TorrentEngine {
    session: Arc<Session>,
    /// job id → live handle.
    handles: RwLock<HashMap<String, Arc<ManagedTorrent>>>,
}

impl TorrentEngine {
    /// Start the session. `default_dir` is where torrents land unless overridden.
    pub async fn new(default_dir: PathBuf) -> Result<Arc<Self>> {
        std::fs::create_dir_all(&default_dir)
            .with_context(|| format!("Failed to create torrent dir {:?}", default_dir))?;

        let session = Session::new(default_dir)
            .await
            .context("Failed to start BitTorrent session")?;

        info!("BitTorrent session started");

        Ok(Arc::new(Self {
            session,
            handles: RwLock::new(HashMap::new()),
        }))
    }

    /// Apply the global download ceiling to the whole BitTorrent session.
    ///
    /// Torrents are rate-limited by librqbit rather than by
    /// [`crate::engine::throttle`], whose bucket only sees HTTP transfers.
    pub fn set_download_limit(&self, bytes_per_sec: u64) {
        let bps = u32::try_from(bytes_per_sec)
            .unwrap_or(u32::MAX)
            .try_into() // NonZeroU32; zero means "no limit"
            .ok();
        self.session.ratelimits.set_download_bps(bps);
        debug!("Torrent download limit set to {:?} B/s", bps);
    }

    /// Add a magnet link, a `.torrent` URL, or a local `.torrent` file.
    ///
    /// Blocks until the metadata is known — for a magnet link that means waiting
    /// on the DHT, so this can take a few seconds. The caller gets a fully
    /// populated [`TorrentAdded`] rather than a name-less placeholder row.
    ///
    /// A multi-file torrent is placed in a folder of its own under `output_dir`,
    /// so its contents never spill loose into the save directory.
    pub async fn add(&self, job_id: &str, source: &str, output_dir: &str) -> Result<TorrentAdded> {
        let source = source.trim();

        let add = if source.starts_with("magnet:") || source.starts_with("http") {
            AddTorrent::from_url(source)
        } else {
            AddTorrent::from_local_filename(source)
                .with_context(|| format!("Could not read torrent file '{}'", source))?
        };

        // Resolve the metadata without starting the transfer. librqbit nests a
        // multi-file torrent under its own name only when `output_folder` is unset
        // (see its `add_torrent_internal`), and we always set it — so we have to
        // apply that rule ourselves, which means knowing the name and the file
        // count up front. The probe returns the raw torrent bytes, so a magnet is
        // resolved from the swarm once rather than twice.
        let probe = match self
            .session
            .add_torrent(
                add,
                Some(AddTorrentOptions {
                    list_only: true,
                    ..Default::default()
                }),
            )
            .await?
        {
            AddTorrentResponse::ListOnly(r) => r,
            AddTorrentResponse::AlreadyManaged(_, h) => {
                warn!("Torrent already in session, reusing handle");
                return self.finish_add(job_id, h).await;
            }
            AddTorrentResponse::Added(_, _) => {
                return Err(anyhow!("Torrent started despite a list-only request"));
            }
        };

        let output_folder = torrent_output_folder(PathBuf::from(output_dir), &probe.info);
        debug!("Torrent will be written to {:?}", output_folder);

        let opts = AddTorrentOptions {
            output_folder: Some(output_folder.to_string_lossy().into_owned()),
            overwrite: true,
            // The probe already found these; skip rediscovering them.
            initial_peers: Some(probe.seen_peers),
            ..Default::default()
        };

        let handle = match self
            .session
            .add_torrent(AddTorrent::from_bytes(probe.torrent_bytes), Some(opts))
            .await?
        {
            AddTorrentResponse::Added(_, h) => h,
            AddTorrentResponse::AlreadyManaged(_, h) => {
                warn!("Torrent already in session, reusing handle");
                h
            }
            AddTorrentResponse::ListOnly(_) => {
                return Err(anyhow!("Torrent was listed but not added"));
            }
        };

        self.finish_add(job_id, handle).await
    }

    /// Wait for metadata, register the handle, and describe what was added.
    async fn finish_add(
        &self,
        job_id: &str,
        handle: Arc<ManagedTorrent>,
    ) -> Result<TorrentAdded> {
        // A magnet has no file list until metadata arrives from the swarm.
        handle
            .wait_until_initialized()
            .await
            .context("Timed out waiting for torrent metadata")?;

        let stats = handle.stats();
        let name  = handle.name().unwrap_or_else(|| "torrent".to_string());
        let files = handle
            .with_metadata(|m| {
                m.info
                    .iter_file_details()
                    .map(|details| {
                        details
                            .filter_map(|f| {
                                let name = f.filename.to_vec().ok()?.join("/");
                                Some(TorrentFile { name, length: f.len })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();

        let added = TorrentAdded {
            name,
            total_bytes: stats.total_bytes,
            info_hash:   handle.info_hash().as_string(),
            files,
        };

        info!(
            "Torrent added: '{}' ({} bytes, {} files)",
            added.name,
            added.total_bytes,
            added.files.len()
        );

        self.handles.write().await.insert(job_id.to_string(), handle);
        Ok(added)
    }

    pub async fn pause(&self, job_id: &str) -> Result<()> {
        let handle = self.handle(job_id).await?;
        self.session.pause(&handle).await?;
        Ok(())
    }

    pub async fn resume(&self, job_id: &str) -> Result<()> {
        let handle = self.handle(job_id).await?;
        self.session.unpause(&handle).await?;
        Ok(())
    }

    /// Remove from the session, optionally deleting the downloaded data.
    pub async fn remove(&self, job_id: &str, delete_files: bool) -> Result<()> {
        if let Some(handle) = self.handles.write().await.remove(job_id) {
            self.session.delete(handle.id().into(), delete_files).await?;
        }
        Ok(())
    }

    /// Whether this job id is a torrent we currently manage.
    pub async fn manages(&self, job_id: &str) -> bool {
        self.handles.read().await.contains_key(job_id)
    }

    async fn handle(&self, job_id: &str) -> Result<Arc<ManagedTorrent>> {
        self.handles
            .read()
            .await
            .get(job_id)
            .cloned()
            .ok_or_else(|| anyhow!("No active torrent for job {}", job_id))
    }

    async fn snapshot(&self) -> Vec<(String, Arc<ManagedTorrent>)> {
        self.handles
            .read()
            .await
            .iter()
            .map(|(id, h)| (id.clone(), Arc::clone(h)))
            .collect()
    }
}

// ── Speed tracking ────────────────────────────────────────────────────────────

/// Per-torrent state the poller keeps between ticks so it can turn librqbit's
/// cumulative byte counters into rates.
///
/// Speeds are derived here rather than read from librqbit's own estimator so that
/// torrents and HTTP downloads report speed the same way, in bytes per second.
///
/// `progress_bytes` advances only when a piece finishes verifying, so the raw
/// per-tick delta swings between zero and a burst. The rates are smoothed before
/// they leave this struct — see [`crate::engine::rate`].
struct Rates {
    last_downloaded: u64,
    last_uploaded:   u64,
    last_tick:       Instant,
    last_db_write:   Instant,
    /// Set once the completion event has been emitted, so it fires exactly once.
    completed:       bool,
    down:            Ema,
    up:              Ema,
}

/// One second between ticks, so this averages over roughly the last three.
const RATE_ALPHA: f64 = 0.3;

impl Rates {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            last_downloaded: 0,
            last_uploaded:   0,
            last_tick:       now,
            // Backdate so the first tick writes immediately.
            last_db_write:   now - DB_WRITE_EVERY,
            completed:       false,
            down:            Ema::new(RATE_ALPHA),
            up:              Ema::new(RATE_ALPHA),
        }
    }

    /// Smoothed bytes/sec since the previous tick, for both directions.
    fn rates(&mut self, downloaded: u64, uploaded: u64) -> (u64, u64) {
        let elapsed = self.last_tick.elapsed().as_secs_f64();
        self.last_tick = Instant::now();

        if elapsed < 0.001 {
            return (0, 0);
        }

        // `saturating_sub` guards against a counter that resets when a torrent is
        // re-checked or re-added, which would otherwise underflow.
        let down = downloaded.saturating_sub(self.last_downloaded);
        let up   = uploaded.saturating_sub(self.last_uploaded);

        self.last_downloaded = downloaded;
        self.last_uploaded   = uploaded;

        (
            self.down.update(down as f64 / elapsed),
            self.up.update(up as f64 / elapsed),
        )
    }

    /// Move the window forward without reporting a rate, for a paused torrent.
    ///
    /// The averages are dropped too: on resume the first tick should reflect the
    /// new speed, not the one from before the pause.
    fn skip(&mut self, downloaded: u64, uploaded: u64) {
        self.last_tick       = Instant::now();
        self.last_downloaded = downloaded;
        self.last_uploaded   = uploaded;
        self.down.reset();
        self.up.reset();
    }
}

// ── Poller ────────────────────────────────────────────────────────────────────

/// Push live torrent stats to the UI and periodically to the database.
///
/// Also enforces the scheduler gate: when the gate closes, managed torrents are
/// paused; when it reopens, the ones this task paused are resumed. Torrents the
/// user paused by hand are left alone.
pub async fn poll_torrents(
    app:     AppHandle,
    db:      Arc<Mutex<Database>>,
    engine:  Arc<TorrentEngine>,
    control: Arc<DownloadControl>,
) {
    let mut rates: HashMap<String, Rates> = HashMap::new();
    let mut gate_paused: Vec<String>      = Vec::new();
    let mut gate_was_open                 = control.gate_open();

    info!("Torrent poller started");

    loop {
        tokio::time::sleep(POLL).await;

        // ── Scheduler gate transitions ────────────────────────────────────
        let gate_open = control.gate_open();
        if gate_open != gate_was_open {
            if gate_open {
                for id in gate_paused.drain(..) {
                    if let Err(e) = engine.resume(&id).await {
                        warn!("Could not resume torrent {}: {}", id, e);
                    }
                }
                info!("Torrent poller: gate reopened");
            } else {
                for (id, handle) in engine.snapshot().await {
                    // Don't touch what the user already paused; we'd wrongly
                    // resume it when the gate reopens.
                    if handle.is_paused() || control.is_paused(&id) {
                        continue;
                    }
                    if let Err(e) = engine.pause(&id).await {
                        warn!("Could not pause torrent {}: {}", id, e);
                    } else {
                        gate_paused.push(id);
                    }
                }
                info!("Torrent poller: gate closed, {} torrent(s) held", gate_paused.len());
            }
            gate_was_open = gate_open;
        }

        // ── Per-torrent stats ─────────────────────────────────────────────
        let torrents = engine.snapshot().await;
        rates.retain(|id, _| torrents.iter().any(|(t, _)| t == id));

        for (job_id, handle) in torrents {
            let stats = handle.stats();
            let entry = rates.entry(job_id.clone()).or_insert_with(Rates::new);

            // A paused torrent still answers `stats()`. Emitting for it would tell
            // the UI it is transferring, so stay quiet — but keep the rate window
            // aligned to now, or the first tick after resuming would divide the
            // bytes of one second by the whole length of the pause.
            if matches!(stats.state, TorrentStatsState::Paused) {
                entry.skip(stats.progress_bytes, stats.uploaded_bytes);
                continue;
            }

            let uploaded = stats.uploaded_bytes;
            let (down_speed, up_speed) = entry.rates(stats.progress_bytes, uploaded);

            // `live` is absent while initializing, paused, or errored.
            let (peers_connected, peers_total) = stats
                .live
                .as_ref()
                .map(|l| (l.snapshot.peer_stats.live as u32, l.snapshot.peer_stats.seen as u32))
                .unwrap_or((0, 0));

            let eta = if down_speed > 0 {
                stats.total_bytes.saturating_sub(stats.progress_bytes) / down_speed
            } else {
                0
            };

            let _ = app.emit(
                "download_progress",
                ProgressEvent {
                    id:               job_id.clone(),
                    downloaded_bytes: stats.progress_bytes,
                    total_bytes:      stats.total_bytes,
                    speed_bps:        down_speed,
                    eta_seconds:      eta,
                    uploaded_bytes:   Some(uploaded),
                    upload_speed_bps: Some(up_speed),
                    peers_connected:  Some(peers_connected),
                    peers_total:      Some(peers_total),
                },
            );

            // Surface a torrent that has gone into an error state.
            if let (TorrentStatsState::Error, Some(err)) = (stats.state, stats.error.as_ref()) {
                error!("Torrent {} error: {}", job_id, err);
                let _ = app.emit(
                    "download_error",
                    serde_json::json!({ "id": job_id, "error": err }),
                );
                if let Ok(db) = db.try_lock() {
                    let _ = db.update_download_status(&job_id, DownloadStatus::Failed(err.clone()).as_str());
                }
                continue;
            }

            // Fire the completion event once, when the torrent finishes downloading.
            // It keeps seeding afterwards, so `finished` stays true from here on.
            if stats.finished && !entry.completed {
                entry.completed = true;
                info!("Torrent complete: {}", job_id);

                let _ = app.emit(
                    "download_complete",
                    serde_json::json!({ "id": job_id, "save_path": "", "checksum": "" }),
                );

                if let Ok(db) = db.try_lock() {
                    let _ = db.update_download_status(&job_id, "completed");
                }
            }

            // ── Throttled persistence ─────────────────────────────────────
            if entry.last_db_write.elapsed() >= DB_WRITE_EVERY {
                entry.last_db_write = Instant::now();
                if let Ok(db) = db.try_lock() {
                    if let Err(e) = db.update_torrent_progress(
                        &job_id,
                        stats.progress_bytes,
                        stats.total_bytes,
                        down_speed,
                        uploaded,
                        up_speed,
                        peers_connected,
                        peers_total,
                    ) {
                        debug!("Could not persist torrent stats for {}: {}", job_id, e);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_magnet_links() {
        assert!(is_torrent_source("magnet:?xt=urn:btih:abc123"));
    }

    #[test]
    fn recognises_torrent_files() {
        assert!(is_torrent_source("C:\\downloads\\ubuntu.torrent"));
        assert!(is_torrent_source("/home/u/x.TORRENT"), "extension is case-insensitive");
        assert!(is_torrent_source("https://example.com/linux.torrent"));
    }

    #[test]
    fn rejects_plain_downloads() {
        assert!(!is_torrent_source("https://example.com/file.zip"));
        assert!(!is_torrent_source("https://example.com/stream.m3u8"));
        assert!(!is_torrent_source(""));
        assert!(
            !is_torrent_source("https://example.com/torrent-guide.html"),
            "'torrent' in the path is not a torrent file"
        );
    }

    #[test]
    fn rates_are_bytes_per_second() {
        let mut r = Rates::new();
        r.last_tick = Instant::now() - std::time::Duration::from_secs(2);
        // The first sample passes through the average untouched.
        let (down, up) = r.rates(2000, 400);
        // 2000 bytes over ~2s ≈ 1000 B/s; allow slack for the clock.
        assert!((900..=1100).contains(&down), "down was {}", down);
        assert!((150..=250).contains(&up), "up was {}", up);
    }

    #[test]
    fn rates_survive_a_counter_reset() {
        let mut r = Rates::new();
        r.last_tick = Instant::now() - std::time::Duration::from_secs(1);
        r.rates(5000, 5000);

        // A re-check can rewind the counters. The delta saturates to zero rather
        // than underflowing into a nonsense rate; the average then decays toward it.
        r.last_tick = Instant::now() - std::time::Duration::from_secs(1);
        let (down, up) = r.rates(10, 10);
        assert!(down < 5000, "rate must fall, not underflow; was {}", down);
        assert!(up < 5000, "rate must fall, not underflow; was {}", up);
    }

    #[test]
    fn accepts_an_ordinary_torrent_name() {
        assert_eq!(
            safe_folder_name("ubuntu-24.04-desktop"),
            Some(PathBuf::from("ubuntu-24.04-desktop"))
        );
        assert_eq!(safe_folder_name("Some Show S01"), Some(PathBuf::from("Some Show S01")));
    }

    /// A torrent's `name` comes from whoever made the torrent.
    #[test]
    fn rejects_names_that_escape_the_save_directory() {
        assert_eq!(safe_folder_name(""), None);
        assert_eq!(safe_folder_name(".."), None);
        assert_eq!(safe_folder_name("../../etc"), None);
        assert_eq!(safe_folder_name("/etc/passwd"), None);
        assert_eq!(safe_folder_name("nested/path"), None);
        assert_eq!(safe_folder_name("."), None);
        #[cfg(windows)]
        {
            assert_eq!(safe_folder_name(r"C:\Windows"), None);
            assert_eq!(safe_folder_name(r"..\..\Windows"), None);
        }
    }

    /// The flicker this smoothing exists to prevent: a torrent completing a piece
    /// every other tick must never report a dead zero mid-transfer.
    #[test]
    fn a_stalled_tick_does_not_zero_the_rate() {
        let mut r = Rates::new();
        let mut cumulative = 0u64;

        for i in 0..8 {
            if i % 2 == 0 {
                cumulative += 1_000_000; // a piece landed
            }
            r.last_tick = Instant::now() - std::time::Duration::from_secs(1);
            let (down, _) = r.rates(cumulative, 0);
            // Skip the very first tick, which has no history to average against.
            if i > 0 {
                assert!(down > 0, "tick {} reported a zero rate", i);
            }
        }
    }

    #[test]
    fn skip_clears_the_average_across_a_pause() {
        let mut r = Rates::new();
        r.last_tick = Instant::now() - std::time::Duration::from_secs(1);
        r.rates(9_000_000, 0);

        r.skip(9_000_000, 0);

        // After resuming, the first tick reflects the new speed, not the old one.
        r.last_tick = Instant::now() - std::time::Duration::from_secs(1);
        let (down, _) = r.rates(9_001_000, 0);
        assert!((900..=1100).contains(&down), "down was {}", down);
    }
}
