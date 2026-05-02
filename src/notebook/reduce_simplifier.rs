//! `SymbolicSimplifier` adapter over the existing `ReduceBackend`.
//!
//! Production wraps a `SharedReduce` (which talks to the process-global
//! `ReduceService`); tests can wrap any `ReduceBackend` (mocks, the real
//! service, `NoReduce` to test the no-CAS path).
//!
//! On the way out: `Ir → Logos source (Unicode) → REDUCE ASCII → REDUCE`.
//! On the way back: `REDUCE text → Logos source (Unicode) → parse → Ir`.
//! A future Lean simplifier would impl `SymbolicSimplifier` directly with
//! no text round-trip.

use crate::lang::ir::Ir;
use crate::lang::reduce::translate;
use crate::lang::symbolic::{SimplifyResponse, SymbolicSimplifier};

use super::reduce_backend::ReduceBackend;

pub struct ReduceSimplifier {
    backend: Box<dyn ReduceBackend>,
}

impl ReduceSimplifier {
    pub fn new(backend: Box<dyn ReduceBackend>) -> Self {
        Self { backend }
    }
}

impl SymbolicSimplifier for ReduceSimplifier {
    fn submit(&mut self, cell_id: usize, ir: &Ir) -> u64 {
        let reduce_expr = translate::ir_to_reduce(ir);
        self.backend.submit(cell_id, Vec::new(), reduce_expr)
    }

    fn try_recv(&mut self) -> Option<SimplifyResponse> {
        let resp = self.backend.try_recv()?;
        let result = match resp.result {
            Err(e) => Err(e),
            Ok(text) if text.trim().is_empty() => Ok(None),
            Ok(text) => {
                let logos_source = translate::from_reduce(&text);
                match crate::lang::parse(&logos_source) {
                    Ok(ir) => Ok(Some(ir)),
                    Err(parse_err) => Err(format!(
                        "Could not parse simplified expression {:?}: {}",
                        logos_source, parse_err
                    )),
                }
            }
        };
        Some(SimplifyResponse {
            cell_id: resp.cell_id,
            result,
        })
    }

    fn has_pending(&self) -> bool {
        self.backend.has_pending()
    }
}
