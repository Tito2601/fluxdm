use std::path::Path;

use tracing::debug;

use crate::engine::segment::{Segment, SegmentStatus};
use crate::storage::db::Database;

/// Load persisted segment state for a download that is being resumed.
///
/// Segments write in place into the shared `.fluxdm-part` file, so their byte
/// counts cannot be recovered from the filesystem — a half-written segment leaves
/// no trace in a pre-allocated file. The database is the record, and the reporter
/// keeps it current while a download runs.
///
/// Returns segments with accurate `status` and `downloaded` fields:
/// - `Completed`                      → slice is fully written; skip it
/// - `Pending` with `downloaded > 0`  → resume from that offset
/// - `Pending` with `downloaded == 0` → fresh download
///
/// Returns an empty Vec when there is nothing to resume — no stored segments, or
/// a part file that is missing or the wrong size. The caller then starts over.
pub fn find_resumable_segments(download_id: &str, db: &Database, part: &Path) -> Vec<Segment> {
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

    // The recorded offsets only mean anything against the file they were written
    // into. A part file of the wrong length is not that file.
    let expected_len = db_segments.iter().map(|s| s.byte_end + 1).max().unwrap_or(0);
    let part_len = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    if part_len != expected_len {
        debug!(
            "Part file for {} is {} bytes, expected {} — restarting",
            download_id, part_len, expected_len
        );
        return Vec::new();
    }

    db_segments
        .into_iter()
        .map(|mut seg| {
            let expected = seg.expected_bytes();

            // A stored count above the slice size would seek past the boundary.
            seg.downloaded = seg.downloaded.min(expected);
            seg.status = if seg.downloaded >= expected {
                SegmentStatus::Completed
            } else {
                SegmentStatus::Pending
            };

            debug!(
                "Resume check segment {}: downloaded={} expected={} status={:?}",
                seg.index_num, seg.downloaded, expected, seg.status
            );

            seg
        })
        .collect()
}
