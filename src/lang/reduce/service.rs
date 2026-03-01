//! Background REDUCE service.
//!
//! Runs a single `ReduceSession` on a dedicated thread, serving
//! simplification requests from the main UI thread via channels.

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

use super::session::ReduceSession;

/// Maximum length of REDUCE output before truncation.
const MAX_OUTPUT_LEN: usize = 2000;

/// A request to simplify an expression.
pub struct ReduceRequest {
    pub cell_id: usize,
    pub expression: String,
    pub request_id: u64,
}

/// A response from the REDUCE worker.
pub struct ReduceResponse {
    pub cell_id: usize,
    pub request_id: u64,
    pub result: Result<String, String>,
}

/// Main-thread handle to the background REDUCE service.
pub struct ReduceService {
    request_tx: Option<mpsc::Sender<ReduceRequest>>,
    response_rx: mpsc::Receiver<ReduceResponse>,
    next_request_id: u64,
    /// Tracks the latest request_id sent for each cell, so stale responses
    /// can be discarded.
    latest_request: HashMap<usize, u64>,
    /// Whether the worker thread is alive. Set to false when send fails.
    worker_alive: bool,
}

impl ReduceService {
    /// Spawn the background REDUCE thread.
    pub fn new() -> Self {
        let (req_tx, resp_rx, alive) = Self::spawn_worker();

        ReduceService {
            request_tx: Some(req_tx),
            response_rx: resp_rx,
            next_request_id: 0,
            latest_request: HashMap::new(),
            worker_alive: alive,
        }
    }

    /// Spawn (or respawn) the worker thread. Returns (request_tx, response_rx, alive).
    fn spawn_worker() -> (
        mpsc::Sender<ReduceRequest>,
        mpsc::Receiver<ReduceResponse>,
        bool,
    ) {
        let (req_tx, req_rx) = mpsc::channel::<ReduceRequest>();
        let (resp_tx, resp_rx) = mpsc::channel::<ReduceResponse>();

        let spawned = thread::Builder::new()
            .name("reduce-worker".into())
            .spawn(move || {
                worker_loop(req_rx, resp_tx);
            });

        match spawned {
            Ok(_) => (req_tx, resp_rx, true),
            Err(e) => {
                log::error!("Failed to spawn REDUCE worker thread: {}", e);
                (req_tx, resp_rx, false)
            }
        }
    }

    /// Submit a simplification request for a cell.
    /// Returns the request_id assigned.
    pub fn submit(&mut self, cell_id: usize, expression: String) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        self.latest_request.insert(cell_id, request_id);

        // Try to send; if the worker is dead, attempt respawn once
        let req = ReduceRequest {
            cell_id,
            expression,
            request_id,
        };

        if let Some(tx) = &self.request_tx {
            if tx.send(req).is_err() {
                log::warn!("REDUCE worker channel disconnected, attempting respawn");
                self.respawn();
            }
        } else if self.worker_alive {
            // Should not happen, but handle gracefully
            self.respawn();
        }

        request_id
    }

    /// Attempt to respawn the worker thread after a crash.
    fn respawn(&mut self) {
        // Note: CSL has global state and can only be initialized once per process.
        // If the worker died mid-operation, respawning will likely fail since
        // CSL state is corrupted. We still try, but mark as dead if it fails.
        let (req_tx, resp_rx, alive) = Self::spawn_worker();
        self.request_tx = Some(req_tx);
        self.response_rx = resp_rx;
        self.worker_alive = alive;
        if alive {
            log::info!("REDUCE worker respawned successfully");
        } else {
            log::error!("REDUCE worker respawn failed — CAS unavailable");
        }
    }

    /// Poll for a completed response. Returns `None` if nothing is ready.
    /// Automatically discards stale responses (where request_id doesn't
    /// match the latest request for that cell).
    pub fn try_recv(&self) -> Option<ReduceResponse> {
        loop {
            match self.response_rx.try_recv() {
                Ok(resp) => {
                    // Check staleness
                    if let Some(&latest) = self.latest_request.get(&resp.cell_id) {
                        if resp.request_id == latest {
                            return Some(resp);
                        }
                        // Stale — discard and try the next one
                        continue;
                    }
                    // No record of this cell — discard
                    continue;
                }
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return None;
                }
            }
        }
    }

    /// Clear all pending requests (e.g. on tab switch).
    pub fn clear_pending(&mut self) {
        self.latest_request.clear();
    }

    /// Whether the worker is alive and accepting requests.
    pub fn is_alive(&self) -> bool {
        self.worker_alive
    }
}

/// Worker thread main loop.
fn worker_loop(
    req_rx: mpsc::Receiver<ReduceRequest>,
    resp_tx: mpsc::Sender<ReduceResponse>,
) {
    // Initialize REDUCE on this thread
    let session = match ReduceSession::new() {
        Ok(s) => s,
        Err(e) => {
            log::error!("Failed to initialize REDUCE session: {}", e);
            return;
        }
    };

    log::info!("REDUCE worker thread started");

    // Process requests until the channel is closed
    while let Ok(req) = req_rx.recv() {
        let result = session.simplify(&req.expression);

        // Truncate overly long output
        let result = result.map(|s| {
            if s.len() > MAX_OUTPUT_LEN {
                let mut truncated = s[..MAX_OUTPUT_LEN].to_string();
                truncated.push_str("...");
                truncated
            } else {
                s
            }
        });

        let _ = resp_tx.send(ReduceResponse {
            cell_id: req.cell_id,
            request_id: req.request_id,
            result,
        });
    }

    // Channel closed — clean shutdown
    drop(session);
    log::info!("REDUCE worker thread exiting");
}
