use std::collections::VecDeque;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::Mutex;
use tracing::info;

use crate::engine::control::DownloadControl;
use crate::engine::downloader::{start_download, DownloadJob};
use crate::storage::db::Database;

// ── Queue ─────────────────────────────────────────────────────────────────────

/// Thread-safe download queue with pause/cancel control.
///
/// `active` is an `Arc<Mutex<...>>` so tasks spawned for each download
/// can hold their own clone and remove themselves when finished.
pub struct DownloadQueue {
    queue:   Mutex<VecDeque<DownloadJob>>,
    active:  Arc<Mutex<Vec<String>>>, // Arc enables cloning into spawned tasks
    control: Arc<DownloadControl>,
}

impl DownloadQueue {
    pub fn new() -> Self {
        Self {
            queue:   Mutex::new(VecDeque::new()),
            active:  Arc::new(Mutex::new(Vec::new())),
            control: Arc::new(DownloadControl::new()),
        }
    }

    /// Shared stop-signal handle, also held by the scheduler and by in-flight downloads.
    pub fn control(&self) -> Arc<DownloadControl> {
        Arc::clone(&self.control)
    }

    // ── Mutations ─────────────────────────────────────────────────────────

    /// Push a new job onto the back of the queue.
    pub async fn enqueue(&self, job: DownloadJob) {
        // A job that was cancelled or paused earlier must not inherit those
        // signals when it is deliberately started again.
        self.control.unpause(&job.id);

        let mut q = self.queue.lock().await;
        info!("Enqueued '{}' (queue size now {})", job.filename, q.len() + 1);
        q.push_back(job);
    }

    /// Signal a job to pause. A running transfer notices between chunks and stops;
    /// a queued one is dropped from the pending list (`cmd_resume_download`
    /// reloads it from the DB).
    pub async fn pause(&self, id: &str) {
        self.control.pause(id);
        self.queue.lock().await.retain(|j| j.id != id);
        info!("Paused job {}", id);
    }

    /// Clear the paused flag. The caller re-enqueues the job.
    pub async fn resume(&self, id: &str) {
        self.control.unpause(id);
        info!("Resumed job {}", id);
    }

    /// Cancel a queued (not yet started) or active job.
    pub async fn cancel(&self, id: &str) {
        self.control.cancel(id);
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

    /// Nothing running and nothing waiting to run.
    ///
    /// Both halves matter: a job between "dequeued" and "marked active" would
    /// otherwise read as idle for a tick, and anything watching for the queue to
    /// drain would fire in the middle of a working session.
    pub async fn is_idle(&self) -> bool {
        self.active.lock().await.is_empty() && self.queue.lock().await.is_empty()
    }

    /// IDs of the downloads currently transferring.
    pub async fn active_ids(&self) -> Vec<String> {
        self.active.lock().await.clone()
    }

    pub async fn is_cancelled(&self, id: &str) -> bool {
        self.control.is_cancelled(id)
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
            // The scheduler may forbid starting anything right now. Jobs stay
            // queued in order; nothing is lost.
            let gate_open    = self.control.gate_open();
            let active_count = self.active_count().await;

            if gate_open && active_count < max_parallel {
                // Try to dequeue the next non-cancelled job
                let job = {
                    let mut q = self.queue.lock().await;
                    loop {
                        match q.pop_front() {
                            None => break None,
                            Some(j) if self.control.is_cancelled(&j.id) => {
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
                    let control    = self.control();

                    // Mark as active before spawning
                    self.active.lock().await.push(job_id.clone());

                    // Clone the Arc so the spawned task can remove itself when done
                    let active_arc     = Arc::clone(&self.active);
                    let id_for_cleanup = job_id.clone();

                    tokio::spawn(async move {
                        if let Err(e) = start_download(job, app_handle, db_clone, control).await {
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

impl Default for DownloadQueue {
    fn default() -> Self {
        Self::new()
    }
}
