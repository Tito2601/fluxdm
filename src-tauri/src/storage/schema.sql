-- ============================================================
-- FluxDM Database Schema v1
-- ============================================================

CREATE TABLE IF NOT EXISTS downloads (
    id           TEXT PRIMARY KEY,
    url          TEXT NOT NULL,
    filename     TEXT NOT NULL,
    save_path    TEXT NOT NULL,
    total_bytes  INTEGER DEFAULT 0,
    downloaded   INTEGER DEFAULT 0,
    status       TEXT DEFAULT 'queued',
    -- status values: queued | downloading | paused | completed | failed | cancelled
    speed_bps    INTEGER DEFAULT 0,
    segments     INTEGER DEFAULT 8,
    category     TEXT DEFAULT 'other',
    threat_score INTEGER DEFAULT 0,
    source_url   TEXT,
    referrer     TEXT,
    mime_type    TEXT,
    checksum     TEXT,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_downloads_status     ON downloads(status);
CREATE INDEX IF NOT EXISTS idx_downloads_created_at ON downloads(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_downloads_category   ON downloads(category);

CREATE TABLE IF NOT EXISTS segments (
    id          TEXT PRIMARY KEY,
    download_id TEXT NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    index_num   INTEGER NOT NULL,
    byte_start  INTEGER NOT NULL,
    byte_end    INTEGER NOT NULL,
    downloaded  INTEGER DEFAULT 0,
    status      TEXT DEFAULT 'pending',
    -- status values: pending | downloading | completed | failed
    UNIQUE(download_id, index_num)
);

CREATE INDEX IF NOT EXISTS idx_segments_download_id ON segments(download_id);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS history (
    id           TEXT PRIMARY KEY,
    url          TEXT NOT NULL,
    filename     TEXT NOT NULL,
    save_path    TEXT NOT NULL,
    total_bytes  INTEGER,
    completed_at TEXT NOT NULL,
    checksum     TEXT,
    url_hash     TEXT  -- for dedup and community layer
);

CREATE INDEX IF NOT EXISTS idx_history_completed_at ON history(completed_at DESC);
CREATE INDEX IF NOT EXISTS idx_history_url_hash     ON history(url_hash);

-- Speed samples recorded during active downloads (used by the analytics chart).
CREATE TABLE IF NOT EXISTS speed_history (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   INTEGER NOT NULL,   -- Unix epoch seconds (UTC)
    speed_bps   INTEGER NOT NULL,
    download_id TEXT REFERENCES downloads(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_speed_history_ts ON speed_history(timestamp DESC);

-- Default settings
INSERT OR IGNORE INTO settings (key, value) VALUES
    ('max_parallel_downloads',   '3'),
    ('max_segments_per_download','8'),
    ('default_save_path',        '~/Downloads'),
    ('speed_limit_kbps',         '0'),
    ('enable_scheduler',         'false'),
    ('scheduler_start',          '02:00'),
    ('scheduler_stop',           '07:00'),
    ('zero_log_mode',            'false'),
    ('theme',                    'system');
