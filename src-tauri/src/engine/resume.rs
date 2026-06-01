use tracing::debug;

use crate::engine::segment::{seg_temp_path, Segment, SegmentStatus};
use crate::storage::db::Database;

/// Load persisted segment state from DB and verify temp files on disk.
///
/// Returns segments with accurate `status` and `downloaded` fields:
/// - `Completed` + `temp_file_path` set  → temp file is present and full; skip re-download
/// - `Pending` with `downloaded > 0`     → partial file on disk; `download_segment` will resume
/// - `Pending` with `downloaded == 0`    → nothing on disk; fresh download
///
/// Returns an empty Vec when no segments are stored (brand-new download).
pub fn find_resumable_segments(download_id: &str, db: &Database) -> Vec<Segment> {
    let db_segments = match db.get_segments_for_download(download_id) {
        Ok(s) => s,
        Err(e) => {
            debug!("Could not load segments for {}: {}", download_id, e);
            return Vec::new();
        }
    };

    if db_segments.is_empty() {
        return Vec::new();
    }

    db_segments
        .into_iter()
        .map(|mut seg| {
            let path     = seg_temp_path(&seg.download_id, seg.index_num);
            let expected = seg.expected_bytes();

            let on_disk: u64 = if path.exists() {
                std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
            } else {
                0
            };

            if on_disk >= expected {
                // Temp file fully present — skip re-download.
                seg.downloaded     = on_disk;
                seg.status         = SegmentStatus::Completed;
                seg.temp_file_path = Some(path.to_string_lossy().to_string());
            } else if on_disk > 0 {
                // Partial temp file — download_segment will append from here.
                // temp_file_path stays None; download_segment reconstructs it via seg_temp_path.
                seg.downloaded = on_disk;
                seg.status     = SegmentStatus::Pending;
            } else {
                // Nothing on disk — start fresh.
                seg.downloaded = 0;
                seg.status     = SegmentStatus::Pending;
            }

            debug!(
                "Resume check segment {}: on_disk={} expected={} status={:?}",
                seg.index_num, on_disk, expected, seg.status
            );

            seg
        })
        .collect()
}
