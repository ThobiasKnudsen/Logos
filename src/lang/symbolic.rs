//! Symbolic-simplification interface.
//!
//! `SymbolicSimplifier` is the IR↔IR contract for any symbolic-algebra
//! engine that simplifies Logos expressions: REDUCE today (via
//! `notebook::reduce_simplifier::ReduceSimplifier`), Lean planned. The
//! trait is async at the API boundary because REDUCE runs out-of-process
//! and the GUI must keep ticking; submit returns a request id and the
//! response is pulled later via `try_recv`.
//!
//! The contract is IR in, IR out. Implementations that bridge to
//! string-based engines (REDUCE) hide the text round-trip internally;
//! a future native-IR engine (Lean) skips it entirely.
//!
//! An empty `Ok(None)` signals "the engine returned nothing meaningful"
//! (e.g. REDUCE returned an empty string for `solve()` with no roots).
//! `Err(_)` is a hard failure (worker crashed, parse failure on the
//! response, etc.).

use super::ir::Ir;

pub trait SymbolicSimplifier {
    /// Submit a simplification request. Returns a request id that the
    /// implementation guarantees is unique across in-flight requests; the
    /// caller is free to discard it (cell ids on responses are how we
    /// route back).
    fn submit(&mut self, cell_id: usize, ir: &Ir) -> u64;

    /// Drain one ready response. Returns `None` if nothing's ready or
    /// the underlying transport is empty.
    fn try_recv(&mut self) -> Option<SimplifyResponse>;

    fn has_pending(&self) -> bool;
}

#[derive(Debug)]
pub struct SimplifyResponse {
    pub cell_id: usize,
    /// `Ok(Some(ir))` is a successful simplification.
    /// `Ok(None)` is "engine ran but produced nothing" (callers typically
    /// clear the cell's pending state without changing the displayed text).
    /// `Err(msg)` is a hard failure.
    pub result: Result<Option<Ir>, String>,
}

/// Placeholder simplifier that drops every submission and never produces
/// a response. Use as a default in contexts where REDUCE isn't wired —
/// pure interpreter and WGSL paths still work, but symbolic prints and
/// inline-CAS calls park indefinitely on `Pending`.
pub struct NoSimplifier;

impl SymbolicSimplifier for NoSimplifier {
    fn submit(&mut self, _cell_id: usize, _ir: &Ir) -> u64 {
        0
    }
    fn try_recv(&mut self) -> Option<SimplifyResponse> {
        None
    }
    fn has_pending(&self) -> bool {
        false
    }
}
