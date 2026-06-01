use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info};
use uuid::Uuid;

use crate::engine::downloader::{DownloadJob, DownloadStatus};
use crate::engine::segment::Segment;

// ── Analytics ────────────────────────────────────────────────────────────────

/// Analytics response — camelCase so it maps 1:1 to the TypeScript `AnalyticsData` interface.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsData {
    pub total_downloaded_bytes: u64,
    pub downloads_today: u64,
    pub avg_speed_bps: u64,
    pub downloads_by_category: HashMap<String, u64>,
    pub speed_history: Vec<SpeedDataPoint>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedDataPoint {
    pub timestamp: u64,
    pub speed_bps: u64,
}

// ── Database ─────────────────────────────────────────────────────────────────

pub struct Database {
    conn: Connection,
}

// Safety: rusqlite::Connection is Send (but not Sync).
// Wrapping in Mutex gives us Send + Sync for the whole Database.
unsafe impl Send for Database {}

impl Database {
    /// Open (or create) the FluxDM SQLite database in the app data directory.
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("Failed to create data dir: {:?}", data_dir))?;

        let db_path = data_dir.join("fluxdm.db");
        info!("Opening database at {:?}", db_path);

        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open SQLite at {:?}", db_path))?;

        // Performance pragmas
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous   = NORMAL;
             PRAGMA foreign_keys  = ON;
             PRAGMA cache_size    = -8000;",
        )?;

        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        let schema = include_str!("schema.sql");
        self.conn
            .execute_batch(schema)
            .context("Failed to initialize database schema")?;
        debug!("Database schema initialized");
        Ok(())
    }

    // ── Downloads ─────────────────────────────────────────────────────────

    /// Insert or replace a download record.
    pub fn upsert_download(&self, job: &DownloadJob) -> Result<()> {
        let status = job.status.as_str();
        let now = chrono::Utc::now().to_rfc3339();

        self.conn.execute(
            "INSERT OR REPLACE INTO downloads (
                id, url, filename, save_path, total_bytes, downloaded,
                status, speed_bps, segments, category, threat_score,
                source_url, referrer, mime_type, checksum,
                created_at, updated_at, completed_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9, ?10, ?11,
                ?12, ?13, ?14, ?15,
                ?16, ?17, ?18
            )",
            params![
                job.id,
                job.url,
                job.filename,
                job.save_path,
                job.total_bytes as i64,
                job.downloaded as i64,
                status,
                job.speed_bps as i64,
                job.num_segments as i64,
                job.category,
                job.threat_score as i64,
                job.source_url,
                job.referrer,
                job.mime_type,
                job.checksum,
                job.created_at,
                now,
                job.completed_at,
            ],
        )?;
        Ok(())
    }

    /// Fetch all downloads ordered by creation date (newest first).
    pub fn get_all_downloads(&self) -> Result<Vec<DownloadJob>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, url, filename, save_path, total_bytes, downloaded,
                    status, speed_bps, segments, category, threat_score,
                    source_url, referrer, mime_type, checksum,
                    created_at, updated_at, completed_at
             FROM downloads
             ORDER BY created_at DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            let status_str: String = row.get(6)?;
            let status = DownloadStatus::from_str(&status_str);

            Ok(DownloadJob {
                id:           row.get(0)?,
                url:          row.get(1)?,
                filename:     row.get(2)?,
                save_path:    row.get(3)?,
                total_bytes:  row.get::<_, i64>(4)? as u64,
                downloaded:   row.get::<_, i64>(5)? as u64,
                status,
                speed_bps:    row.get::<_, i64>(7)? as u64,
                num_segments: row.get::<_, i64>(8)? as u8,
                category:     row.get(9)?,
                threat_score: row.get::<_, i64>(10)? as u8,
                source_url:   row.get(11)?,
                referrer:     row.get(12)?,
                mime_type:    row.get(13)?,
                checksum:     row.get(14)?,
                created_at:   row.get(15)?,
                updated_at:   row.get(16)?,
                completed_at: row.get(17)?,
                segments:     Vec::new(), // loaded separately if needed
                headers:      None,
                cookies:      None,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .context("Failed to read downloads from DB")
    }

    /// Update just the status field.
    pub fn update_download_status(&self, id: &str, status: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE downloads SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, now, id],
        )?;
        Ok(())
    }

    /// Delete a download record (cascades to its segments).
    pub fn delete_download(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM downloads WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ── Segments ──────────────────────────────────────────────────────────

    pub fn get_segments_for_download(&self, download_id: &str) -> Result<Vec<Segment>> {
        use crate::engine::segment::SegmentStatus;

        let mut stmt = self.conn.prepare(
            "SELECT id, download_id, index_num, byte_start, byte_end, downloaded, status
             FROM segments WHERE download_id = ?1 ORDER BY index_num ASC",
        )?;
        let rows = stmt.query_map(params![download_id], |row| {
            let status_str: String = row.get(6)?;
            let status = match status_str.as_str() {
                "completed"   => SegmentStatus::Completed,
                "downloading" => SegmentStatus::Downloading,
                "failed"      => SegmentStatus::Failed,
                _             => SegmentStatus::Pending,
            };
            Ok(Segment {
                id:             row.get(0)?,
                download_id:    row.get(1)?,
                index_num:      row.get::<_, i64>(2)? as usize,
                byte_start:     row.get::<_, i64>(3)? as u64,
                byte_end:       row.get::<_, i64>(4)? as u64,
                downloaded:     row.get::<_, i64>(5)? as u64,
                status,
                temp_file_path: None,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .context("Failed to read segments for download")
    }

    /// Reset downloads stuck in "downloading" (interrupted by crash/force-quit) to "paused"
    /// so the user can see and resume them. Called once at app startup.
    pub fn reset_interrupted_downloads(&self) -> Result<usize> {
        let count = self.conn.execute(
            "UPDATE downloads SET status = 'paused', updated_at = ?1 WHERE status = 'downloading'",
            params![chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(count)
    }

    pub fn upsert_segment(&self, segment: &Segment) -> Result<()> {
        let status = segment.status.as_str();
        self.conn.execute(
            "INSERT OR REPLACE INTO segments
             (id, download_id, index_num, byte_start, byte_end, downloaded, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                segment.id,
                segment.download_id,
                segment.index_num as i64,
                segment.byte_start as i64,
                segment.byte_end as i64,
                segment.downloaded as i64,
                status,
            ],
        )?;
        Ok(())
    }

    // ── Settings ──────────────────────────────────────────────────────────

    pub fn get_all_settings(&self) -> Result<HashMap<String, String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM settings")?;
        let map = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<HashMap<_, _>, _>>()?;
        Ok(map)
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM settings WHERE key = ?1")?;
        match stmt.query_row(params![key], |r| r.get::<_, String>(0)) {
            Ok(v)                                             => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e)                                          => Err(anyhow::anyhow!(e)),
        }
    }

    pub fn update_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    // ── History ───────────────────────────────────────────────────────────

    pub fn add_to_history(&self, job: &DownloadJob) -> Result<()> {
        let url_hash = format!("{:x}", Sha256::digest(job.url.as_bytes()));
        let completed_at = job
            .completed_at
            .clone()
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

        self.conn.execute(
            "INSERT OR IGNORE INTO history
             (id, url, filename, save_path, total_bytes, completed_at, checksum, url_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                Uuid::new_v4().to_string(),
                job.url,
                job.filename,
                job.save_path,
                job.total_bytes as i64,
                completed_at,
                job.checksum,
                url_hash,
            ],
        )?;
        Ok(())
    }

    /// Wipe all history rows and remove completed / cancelled / failed download records.
    pub fn clear_history(&self) -> Result<()> {
        self.conn.execute("DELETE FROM history", [])?;
        self.conn.execute(
            "DELETE FROM downloads WHERE status IN ('completed', 'cancelled', 'failed')",
            [],
        )?;
        Ok(())
    }

    // ── Speed history ─────────────────────────────────────────────────────

    /// Record one speed sample. Call at most once every few seconds per download.
    pub fn record_speed_point(&self, timestamp: u64, speed_bps: u64, download_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO speed_history (timestamp, speed_bps, download_id) VALUES (?1, ?2, ?3)",
            params![timestamp as i64, speed_bps as i64, download_id],
        )?;
        Ok(())
    }

    /// Fetch the most recent `limit` speed samples, oldest first (for charting).
    pub fn get_speed_history(&self, limit: usize) -> Result<Vec<SpeedDataPoint>> {
        let mut stmt = self.conn.prepare(
            "SELECT timestamp, speed_bps FROM (
                 SELECT timestamp, speed_bps FROM speed_history
                 ORDER BY timestamp DESC LIMIT ?1
             ) ORDER BY timestamp ASC",
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok(SpeedDataPoint {
                timestamp: r.get::<_, i64>(0)? as u64,
                speed_bps: r.get::<_, i64>(1)? as u64,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .context("Failed to read speed history")
    }

    /// Check whether a URL (by its SHA-256 hash) was previously downloaded.
    pub fn find_history_by_url_hash(&self, url_hash: &str) -> Result<Option<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT filename, save_path, completed_at FROM history WHERE url_hash = ?1 LIMIT 1",
        )?;
        match stmt.query_row(params![url_hash], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        }) {
            Ok(row)                                          => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e)                                          => Err(anyhow::anyhow!(e)),
        }
    }

    // ── Analytics ─────────────────────────────────────────────────────────

    pub fn get_analytics(&self) -> Result<AnalyticsData> {
        // Total bytes downloaded from history
        let total_bytes: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(total_bytes), 0) FROM history",
            [],
            |r| r.get(0),
        )?;

        // Downloads today (UTC date prefix)
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let downloads_today: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM history WHERE completed_at LIKE ?1",
            params![format!("{}%", today)],
            |r| r.get(0),
        )?;

        // Downloads by category
        let mut cat_stmt = self.conn.prepare(
            "SELECT category, COUNT(*) FROM downloads
             WHERE status = 'completed' GROUP BY category",
        )?;
        let by_category = cat_stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64)))?
            .collect::<Result<HashMap<_, _>, _>>()?;

        // Speed history — last 120 samples (~10 min at one sample/5s)
        let speed_history = self.get_speed_history(120).unwrap_or_default();

        // Average speed from recent samples (last 20)
        let avg_speed_bps = if speed_history.is_empty() {
            0
        } else {
            let recent: Vec<_> = speed_history.iter().rev().take(20).collect();
            recent.iter().map(|p| p.speed_bps).sum::<u64>() / recent.len() as u64
        };

        Ok(AnalyticsData {
            total_downloaded_bytes: total_bytes as u64,
            downloads_today: downloads_today as u64,
            avg_speed_bps,
            downloads_by_category: by_category,
            speed_history,
        })
    }
}
