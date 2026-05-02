//! Background language service.
//!
//! Runs autocomplete symbol extraction (lex → parse → IR walk) on a
//! dedicated thread so the UI thread never blocks on parsing.

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

use crate::editor::autocomplete::{self, Candidate};

/// A request to parse a cell's source text for symbols.
pub struct LangRequest {
    pub cell_index: usize,
    pub source: String,
    pub request_id: u64,
}

/// The result of background symbol extraction.
pub struct LangResponse {
    pub cell_index: usize,
    pub request_id: u64,
    pub user_symbols: Vec<Candidate>,
}

/// Main-thread handle to the background language service.
pub struct LangService {
    request_tx: Option<mpsc::Sender<LangRequest>>,
    response_rx: mpsc::Receiver<LangResponse>,
    next_request_id: u64,
    /// Tracks the latest request_id sent for each cell, so stale responses
    /// can be discarded.
    latest_request: HashMap<usize, u64>,
}

impl LangService {
    /// Spawn the background language worker thread.
    pub fn new() -> Self {
        let (req_tx, req_rx) = mpsc::channel::<LangRequest>();
        let (resp_tx, resp_rx) = mpsc::channel::<LangResponse>();

        let spawned = thread::Builder::new()
            .name("lang-worker".into())
            .spawn(move || {
                worker_loop(req_rx, resp_tx);
            });

        match spawned {
            Ok(_) => {}
            Err(e) => {
                log::error!("Failed to spawn lang worker thread: {}", e);
            }
        }

        LangService {
            request_tx: Some(req_tx),
            response_rx: resp_rx,
            next_request_id: 0,
            latest_request: HashMap::new(),
        }
    }

    /// Submit a cell's source text for background processing.
    pub fn submit(&mut self, cell_index: usize, source: String) {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        self.latest_request.insert(cell_index, request_id);

        let req = LangRequest {
            cell_index,
            source,
            request_id,
        };

        if let Some(tx) = &self.request_tx {
            if tx.send(req).is_err() {
                log::error!("Lang worker channel disconnected");
                self.request_tx = None;
            }
        }
    }

    /// Poll for a completed response. Returns `None` if nothing is ready.
    /// Automatically discards stale responses.
    pub fn try_recv(&mut self) -> Option<LangResponse> {
        loop {
            match self.response_rx.try_recv() {
                Ok(resp) => {
                    if let Some(&latest) = self.latest_request.get(&resp.cell_index) {
                        if resp.request_id == latest {
                            self.latest_request.remove(&resp.cell_index);
                            return Some(resp);
                        }
                        // Stale — discard and try next
                        continue;
                    }
                    continue;
                }
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    log::error!("Lang worker channel disconnected");
                    self.latest_request.clear();
                    return None;
                }
            }
        }
    }

    /// Clear all pending requests (e.g. on tab switch or theme change).
    pub fn clear_pending(&mut self) {
        self.latest_request.clear();
    }
}

/// Worker thread main loop.
fn worker_loop(req_rx: mpsc::Receiver<LangRequest>, resp_tx: mpsc::Sender<LangResponse>) {
    loop {
        // Block until at least one request arrives
        let first = match req_rx.recv() {
            Ok(r) => r,
            Err(_) => return, // Main thread dropped sender — exit
        };

        // Drain all queued requests, keeping only the latest per cell_index
        let mut latest: HashMap<usize, LangRequest> = HashMap::new();
        latest.insert(first.cell_index, first);

        while let Ok(req) = req_rx.try_recv() {
            latest.insert(req.cell_index, req);
        }

        // Process each unique cell request
        for (_, req) in latest {
            // Catch panics so a malformed input that exposes a parser bug
            // doesn't kill the worker (which would silently freeze autocomplete).
            let user_symbols = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                extract_symbols(&req.source)
            }))
            .unwrap_or_else(|_| {
                log::error!(
                    "lang-worker: extract_symbols panicked on cell {}",
                    req.cell_index
                );
                Vec::new()
            });

            let resp = LangResponse {
                cell_index: req.cell_index,
                request_id: req.request_id,
                user_symbols,
            };

            if resp_tx.send(resp).is_err() {
                return; // Main thread dropped receiver — exit
            }
        }
    }
}

/// Lex + parse + extract user-defined symbols (best-effort).
fn extract_symbols(source: &str) -> Vec<Candidate> {
    let mut lex = crate::lang::lexer::Lexer::new(source);
    if let Ok(tokens) = lex.tokenize() {
        let mut parser = crate::lang::parser::Parser::new(tokens, source.to_string());
        if let Ok(ir) = parser.parse() {
            return autocomplete::extract_user_symbols(&ir);
        }
    }
    Vec::new()
}
