//! Lowering pass: enrich a freshly-parsed `Ir` with the analysis information
//! every backend (interpreter, wgsl_gen, future cranelift JIT) needs.
//!
//! The IR coming out of the parser is essentially an AST — names are strings,
//! types are unknown, higher-order calls are not yet monomorphized, and
//! lambdas still appear as `Ir::Lambda` nodes. Backends used to each re-derive
//! all of this independently; this module is the single shared pipeline.
//!
//! The pass enriches the same `Ir` in place: every variant carries an
//! `IrAnnotations` field that's `Default::default()` (all-`None`) at parse
//! time and gets populated as the lowering pass runs. Backends consume the
//! same `Ir` type and read `.ann.result_ty.expect("lowered")` etc.
//!
//! ## Sub-passes (run in order)
//!
//! 1. `hoist_anonymous_blocks` — synthesize top-level bindings for imperative
//!    blocks that appear in expression position (e.g. `plot(y = (sum:=0; …))`).
//! 2. `lift_lambdas` — replace each `Ir::Lambda` with a synthetic
//!    `FunctionDef` plus an `Identifier` referring to it.
//! 3. `specialize_higher_order_calls` — monomorphize each HOF call so backends
//!    see concrete function references instead of function-typed parameters.
//! 4. `resolve_names` — populate `Resolution` on every `Identifier` so name
//!    lookups become field reads at the backend.
//! 5. `annotate_types` — populate `result_ty` on every expression so backends
//!    don't re-run type inference.
//!
//! The first three are currently inside `wgsl_gen` (`hoist_anonymous_blocks`,
//! `lift_lambdas`, `specialize_higher_order_calls`) and will move here in
//! later phases. For now this module provides the scaffold and identity
//! sub-passes so the pipeline can be wired up incrementally.

use super::ir::Ir;

/// Top-level entry point for the lowering pass.
///
/// Currently a stub: returns the input unchanged. Phases 2–6 will replace
/// the body with the full sub-pass sequence:
///
/// 1. `hoist_anonymous_blocks` (moves from `wgsl_gen`).
/// 2. `lift_lambdas` (moves from `wgsl_gen`).
/// 3. `specialize_higher_order_calls` (moves from `wgsl_gen`).
/// 4. `resolve_names` — populate `Resolution` on every `Identifier`.
/// 5. `annotate_types` — populate `result_ty` on every expression.
pub fn lower(ir: Ir) -> Result<Ir, String> {
    Ok(ir)
}
