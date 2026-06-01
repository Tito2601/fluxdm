use std::collections::VecDeque;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::Mutex;
use tracing::info;

use crate::engine::downloader::{start_download, DownloadJob};
use crate::storage::db::Database;

// ── Queue ─────────────────────────────────────────────────────────────────────

/// Thread-safe download queue with pause/cancel control.
///
/// `active` is an `Arc<Mutex<...>>` so tasks spawned for each download
/// can hold their own clone and remove themselves when finished.
pub struct DownloadQueue {
    queue:     Mutex<VecDeque<DownloadJob>>,
    active:    Arc<Mutex<Vec<String>>>,  // Arc enables cloning into spawned tasks
    paused:    Mutex<Vec<String>>,
    cancelled: Mutex<Vec<String>>,
}

impl DownloadQueue {
    pub fn new() -> Self {
        Self {
            queue:     Mutex::new(VecDeque::new()),
            active:    Arc::new(Mutex::new(Vec::new())),
            paused:    Mutex::new(Vec::new()),
            cancelled: Mutex::new(Vec::new()),
        }
    }

    // ── Mutations ─────────────────────────────────────────────────────────

    /// Push a new job onto the back of the queue.
    pub async fn enqueue(&self, job: DownloadJob) {
        let mut q = self.queue.lock().await;
        info!("Enqueued '{}' (queue size now {})", job.filename, q.len() + 1);
        q.push_back(job);
    }

    /// Signal a job to pause; the download loop checks this flag.
    pub async fn pause(&self, id: &str) {
        let mut p = self.paused.lock().await;
        if !p.iter().any(|x| x == id) {
            p.push(id.to_string());
            info!("Paused job {}", id);
        }
    }

    /// Remove a job from the paused list (re-enables download).
    pub async fn resume(&self, id: &str) {
        let mut p = self.paused.lock().await;
        p.retain(|x| x != id);
        info!("Resumed job {}", id);
    }

    /// Cancel a queued (not yet started) or active job.
    pub async fn cancel(&self, id: &str) {
        {
            let mut c = self.cancelled.lock().await;
            if !c.iter().any(|x| x == id) {
                c.push(id.to_string());
            }
        }
        // Also remove from pending queue if it hasn't started yet
        self.queue.lock().await.retain(|j| j.id != id);
        info!("Cancelled job {}", id);
    }

    /// Move a job to a new position in the queue.
    pub async fn reorder(&self, id: &str, new_pos: usize) {
        let mut q = self.queue.lock().await;
        if let Some(i) = q.iter().position(|j| j.id == id) {
            let job = q.remove(i).unwrap();
            let pos = new_pos.min(q.len());
            q.insert(pos, job);
        }
    }

    // ── Queries ───────────────────────────────────────────────────────────

    pub async fn active_count(&self) -> usize {
        self.active.lock().await.len()
    }

    pub async fn is_cancelled(&self, id: &str) -> bool {
        self.cancelled.lock().await.iter().any(|x| x == id)
    }

    // ── Main loop ─────────────────────────────────────────────────────────

    /// Start the background queue processor. Runs indefinitely.
    /// `max_parallel` controls concurrency (from DB settings, default 3).
    pub async fn process_queue(
        &self,
        app_handle:   AppHandle,
        db:           Arc<Mutex<Database>>,
        max_parallel: usize,
    ) {
        info!("Queue processor started (max_parallel={})", max_parallel);

        loop {
            let active_count = self.active_count().await;

            if active_count < max_parallel {
                // Try to dequeue the next non-cancelled job
                let job = {
                    let mut q = self.queue.lock().await;
                    loop {
                        match q.pop_front() {
                            None => break None,
                            Some(j) if self.is_cancelled(&j.id).await => {
                                info!("Skipping cancelled job {}", j.id);
                                continue;
                            }
                            Some(j) => break Some(j),
                        }
                    }
                };

                if let Some(job) = job {
                    let job_id     = job.id.clone();
                    let app_handle = app_handle.clone();
                    let db_clone   = db.clone();

                    // Mark as active before spawning
                    self.active.lock().await.push(job_id.clone());

                    // Clone the Arc so the spawned task can remove itself when done
                    let active_arc     = Arc::clone(&self.active);
                    let id_for_cleanup = job_id.clone();

                    tokio::spawn(async move {
                        if let Err(e) = start_download(job, app_handle, db_clone).await {
                            tracing::error!("Download task error: {}", e);
                        }
                        // Remove from active list when done
                        active_arc.lock().await.retain(|x| x != &id_for_cleanup);
                        info!("Job {} finished, removed from active list", id_for_cleanup);
                    });
                }
            }

            // Poll every 200ms — light on CPU, responsive to new jobs
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
}
