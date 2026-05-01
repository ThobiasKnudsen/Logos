//! Trait abstraction over the REDUCE subprocess.
//!
//! Production wraps the real `lang::reduce::service::ReduceService`. Tests
//! either share the same real service (REDUCE has process-global state, so
//! only one worker per process) or supply their own implementation.

use crate::lang::reduce::service::{ReduceResponse, ReduceService};

pub trait ReduceBackend {
    /// Submit a simplification request and return its `request_id`.
    fn submit(&mut self, cell_id: usize, context: Vec<String>, expression: String) -> u64;

    /// Drain one ready response, or `None` if nothing's ready. Implementors
    /// should silently discard stale responses (where the request_id no
    /// longer matches the latest for that cell).
    fn try_recv(&mut self) -> Option<ReduceResponse>;

    fn has_pending(&self) -> bool;

    fn clear_pending(&mut self);
}

/// Production backend: thin wrapper over `ReduceService`.
pub struct ReduceServiceBackend {
    inner: ReduceService,
}

impl ReduceServiceBackend {
    pub fn new() -> Self {
        Self {
            inner: ReduceService::new(),
        }
    }

    pub fn from_service(service: ReduceService) -> Self {
        Self { inner: service }
    }
}

impl ReduceBackend for ReduceServiceBackend {
    fn submit(&mut self, cell_id: usize, context: Vec<String>, expression: String) -> u64 {
        self.inner.submit(cell_id, context, expression)
    }

    fn try_recv(&mut self) -> Option<ReduceResponse> {
        self.inner.try_recv()
    }

    fn has_pending(&self) -> bool {
        self.inner.has_pending()
    }

    fn clear_pending(&mut self) {
        self.inner.clear_pending()
    }
}
