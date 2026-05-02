//! Trait abstraction over the REDUCE subprocess.
//!
//! Production wraps the real `lang::reduce::service::ReduceService`. Tests
//! either share the same real service (REDUCE has process-global state, so
//! only one worker per process) or supply their own implementation.
//! `NoReduce` is a safe default for contexts that don't need symbolic
//! simplification — symbolic prints and inline-CAS calls park indefinitely.

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

/// Placeholder backend that drops every submission and never returns a
/// response. Use as a default when a notebook is constructed in a context
/// where REDUCE isn't wired (yet) — pure interpreter and WGSL paths still
/// work, but symbolic prints and inline-CAS calls park indefinitely on
/// `Pending`.
pub struct NoReduce;

impl ReduceBackend for NoReduce {
    fn submit(&mut self, _cell_id: usize, _context: Vec<String>, _expression: String) -> u64 {
        0
    }
    fn try_recv(&mut self) -> Option<ReduceResponse> {
        None
    }
    fn has_pending(&self) -> bool {
        false
    }
    fn clear_pending(&mut self) {}
}

/// Backend that funnels into a single shared `ReduceService` via
/// `Rc<RefCell<…>>`. Production wires one of these into every
/// `NotebookView`'s `Notebook` so all open notebooks share the single
/// process-wide REDUCE worker (CSL has process-global state — only one
/// worker is allowed per process).
///
/// Cell IDs are assumed unique across notebooks (via the global cell-id
/// counter on `Notebook`), so responses route back to the right cell
/// regardless of which notebook submitted them.
pub struct SharedReduce {
    inner: std::rc::Rc<std::cell::RefCell<ReduceService>>,
}

impl SharedReduce {
    pub fn new(inner: std::rc::Rc<std::cell::RefCell<ReduceService>>) -> Self {
        Self { inner }
    }
}

impl ReduceBackend for SharedReduce {
    fn submit(&mut self, cell_id: usize, context: Vec<String>, expression: String) -> u64 {
        self.inner.borrow_mut().submit(cell_id, context, expression)
    }
    fn try_recv(&mut self) -> Option<ReduceResponse> {
        self.inner.borrow_mut().try_recv()
    }
    fn has_pending(&self) -> bool {
        self.inner.borrow().has_pending()
    }
    fn clear_pending(&mut self) {
        self.inner.borrow_mut().clear_pending()
    }
}
