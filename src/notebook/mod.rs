//! Headless notebook engine — the backend half of the cell editor.
//!
//! Owns a list of cells (text, plot color, state), runs each through the
//! Logos pipeline (parse → REDUCE → wgsl_gen → interpreter), and produces
//! GPU-free outputs: shader source strings, print messages, syntax-coloring
//! spans, structured diagnostics with row/col positions.
//!
//! Methods never return errors — they store any user-facing error as a
//! `Diagnostic` on the cell and update the cell's outcome. Callers display
//! whatever's stored.
//!
//! No GPU, no winit, no UI framework. Reusable as-is in tests, in a CLI
//! frontend, or anywhere else. The UI shell that wraps this is `NotebookView`
//! (added in a later refactor step).

// Public types here form a deliberate API surface — fields and variants the
// renderer / future CLI / future JIT pipeline are expected to consume.
// Dead-code analysis can't see those callers yet.
#![allow(dead_code)]

mod cell;
mod diagnostic;
mod internals;
mod reduce_backend;
mod shader;

pub use cell::{CellMessage, CellOutcome, CellState, NotebookCell};
#[allow(unused_imports)]
pub use diagnostic::{Diagnostic, Severity, Span};
#[allow(unused_imports)]
pub use reduce_backend::{NoReduce, ReduceBackend, SharedReduce};
#[allow(unused_imports)]
pub use shader::{Access, BindingSpec, DispatchKind, ScalarType, ShaderSpec, SizeSpec};

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Process-global cell ID counter. Multiple notebooks may share a single
/// REDUCE service, so cell IDs must be unique across all notebooks for
/// response routing to work.
static NEXT_CELL_ID: AtomicUsize = AtomicUsize::new(0);

fn alloc_cell_id() -> usize {
    NEXT_CELL_ID.fetch_add(1, Ordering::Relaxed)
}

use crate::lang::ast::AstNode;
use crate::lang::interpreter::{self, GpuDispatch};
use crate::lang::reduce::translate;
use crate::lang::{self, highlight};
use crate::ui::theme::Rgba;

use internals::{
    collect_bindings, expand_bindings, extract_call_arg, extract_reduce_expr, find_cas_call,
    prepare_reduce_print, substitute_reduce_result,
};

/// Why a particular REDUCE round-trip is in flight. Lets `tick()` route the
/// response to the right cell-update path.
enum ReducePurpose {
    /// `print(f)` fell through because the interpreter couldn't evaluate.
    /// On response, format the simplified expression as the cell's print
    /// output (with optional `= 0` suffix for equations).
    Print { is_equation: bool },
    /// CAS function (`int`, `df`, etc.) embedded in source. On response,
    /// substitute the simplified form back into the cell's working text and
    /// either resubmit (if more CAS calls remain) or generate WGSL.
    InlineCas,
}

struct PendingReduce {
    cell_id: usize,
    purpose: ReducePurpose,
}

pub struct Notebook {
    cells: Vec<NotebookCell>,
    reduce: Box<dyn ReduceBackend>,
    /// In-flight REDUCE round-trips, keyed by `cell_id` (not index — indices
    /// shift on `remove_cell`).
    pending_reduce: HashMap<usize, PendingReduce>,
    /// CPU/GPU dispatcher for interpreter-side work (parallel for, arrays).
    /// Defaults to `CpuFallback`; the production app injects a wgpu-backed
    /// dispatcher when wired through `NotebookView`.
    gpu: Box<dyn GpuDispatch>,
}

impl Notebook {
    /// Construct an empty notebook. `reduce` is the REDUCE backend (production
    /// uses `ReduceServiceBackend::new()`); `gpu` defaults to a CPU fallback
    /// when `None` — fine for tests and CLI use.
    pub fn new(reduce: Box<dyn ReduceBackend>, gpu: Option<Box<dyn GpuDispatch>>) -> Self {
        Self {
            cells: Vec::new(),
            reduce,
            pending_reduce: HashMap::new(),
            gpu: gpu.unwrap_or_else(|| Box::new(interpreter::CpuFallback)),
        }
    }

    // ─── structure ─────────────────────────────────────────────────────────

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn cell(&self, idx: usize) -> &NotebookCell {
        &self.cells[idx]
    }

    pub fn cell_mut(&mut self, idx: usize) -> &mut NotebookCell {
        &mut self.cells[idx]
    }

    pub fn cells(&self) -> &[NotebookCell] {
        &self.cells
    }

    pub fn add_cell(&mut self, text: &str, plot_color: Rgba) -> usize {
        let id = alloc_cell_id();
        self.cells.push(NotebookCell::new(id, text, plot_color));
        self.cells.len() - 1
    }

    pub fn remove_cell(&mut self, idx: usize) {
        if idx < self.cells.len() {
            let id = self.cells[idx].id;
            self.cells.remove(idx);
            self.pending_reduce.remove(&id);
        }
    }

    // ─── writes ────────────────────────────────────────────────────────────

    pub fn set_text(&mut self, idx: usize, text: &str) {
        if idx >= self.cells.len() {
            return;
        }
        self.cells[idx].buffer.set_text(text);
        self.cells[idx].invalidate_ast();
    }

    pub fn set_plot_color(&mut self, idx: usize, color: Rgba) {
        if idx >= self.cells.len() {
            return;
        }
        self.cells[idx].plot_color = color;
    }

    // ─── execution ─────────────────────────────────────────────────────────

    /// Run cell `idx`, auto-running any earlier cells that aren't already
    /// playing. Mirrors Jupyter's "play this and what it depends on" model.
    pub fn play(&mut self, idx: usize) {
        if idx >= self.cells.len() {
            return;
        }
        for i in 0..idx {
            if !matches!(self.cells[i].state, CellState::Playing) {
                self.run_cell(i);
            }
        }
        self.run_cell(idx);
    }

    /// Re-run cell `idx`. Stops every later cell that's currently playing —
    /// the user is acknowledging that downstream results may now be invalid.
    pub fn replay(&mut self, idx: usize) {
        if idx >= self.cells.len() {
            return;
        }
        for i in (idx + 1)..self.cells.len() {
            if matches!(self.cells[i].state, CellState::Playing) {
                self.cells[i].state = CellState::Stopped;
            }
        }
        self.run_cell(idx);
    }

    pub fn stop(&mut self, idx: usize) {
        if idx >= self.cells.len() {
            return;
        }
        self.cells[idx].state = CellState::Stopped;
    }

    /// Drain ready REDUCE responses. Call once per frame (or until
    /// `has_pending() == false` for sync test loops). Returns the cell
    /// indices whose outcome changed.
    pub fn tick(&mut self) -> Vec<usize> {
        let mut updated = Vec::new();
        while let Some(resp) = self.reduce.try_recv() {
            if let Some(idx) = self.handle_reduce_response(resp) {
                if !updated.contains(&idx) {
                    updated.push(idx);
                }
            }
        }
        updated
    }

    /// True if there's at least one REDUCE round-trip outstanding.
    pub fn has_pending(&self) -> bool {
        self.reduce.has_pending() || !self.pending_reduce.is_empty()
    }

    /// Forget every in-flight REDUCE round-trip this notebook is tracking.
    /// The shared REDUCE service may still receive responses for these
    /// cells; the notebook will silently drop them since `pending_reduce`
    /// no longer maps the cell IDs.
    pub fn clear_pending(&mut self) {
        self.pending_reduce.clear();
    }

    // ─── internals ─────────────────────────────────────────────────────────

    /// Build the combined AST for cells `[0..=cell_index]` using each cell's
    /// effective source (the simplified output if any, else the buffer text).
    fn combined_ast_up_to(&self, cell_index: usize) -> Result<AstNode, String> {
        let mut all_stmts = Vec::new();
        for (i, cell) in self.cells.iter().enumerate() {
            if i > cell_index {
                break;
            }
            // For the simplified-output case, parse the simplified text
            // directly (it bypasses the cell's AST cache, which holds the
            // pre-substitution form).
            let parsed = match &cell.outcome.message {
                Some(CellMessage::Simplified(s)) => crate::lang::parse(s),
                _ => cell.cached_ast(),
            }
            .map_err(|e| format!("Cell {}: {}", i + 1, e))?;
            match parsed {
                AstNode::Block(stmts) => all_stmts.extend(stmts),
                other => all_stmts.push(other),
            }
        }
        Ok(AstNode::Block(all_stmts))
    }

    fn run_cell(&mut self, idx: usize) {
        let snapshot = self.cells[idx].buffer.text().to_string();

        // Reset outcome but keep token colors fresh regardless of success.
        self.cells[idx].outcome = CellOutcome::default();
        self.cells[idx].outcome.token_colors = highlight::highlight(&snapshot);

        // Drop any pending REDUCE for this cell — it would only confuse the
        // response handler if it arrives now.
        let cell_id = self.cells[idx].id;
        self.pending_reduce.remove(&cell_id);

        let combined = match self.combined_ast_up_to(idx) {
            Ok(a) => a,
            Err(msg) => {
                self.set_parse_error(idx, msg, &snapshot);
                return;
            }
        };

        let actions = lang::detect_cell_actions(&combined);

        let cell_text = self.cells[idx].buffer.text().trim().to_string();

        if let Some(print_idx) = actions.first_print {
            self.handle_print(idx, print_idx, &combined, &cell_text);
            if actions.last_plot.is_none() {
                return;
            }
        }

        if let Some(plot_idx) = actions.last_plot {
            self.handle_plot(idx, plot_idx, &combined, &snapshot);
            return;
        }

        if actions.has_action() {
            return;
        }

        // No print/plot. Try the interpreter for parallel/array cells.
        if lang::needs_interpreter(&combined) {
            match interpreter::eval(&combined, self.gpu.as_ref()) {
                Ok(val) => {
                    self.cells[idx].outcome.message =
                        Some(CellMessage::Computed(format!("{}", val)));
                    self.cells[idx].state = CellState::Playing;
                    self.cells[idx].last_played_text = Some(snapshot);
                }
                Err(e) => {
                    self.set_runtime_error(idx, e, &snapshot);
                }
            }
            return;
        }

        // CAS-only cell with no print/plot (e.g. raw `int(x²,x)` pasted as
        // a cell). Submit to REDUCE for simplification.
        if find_cas_call(&cell_text).is_some() && !cell_text.is_empty() {
            let (reduce_input, _) = extract_reduce_expr(&cell_text);
            let bindings = collect_bindings(&self.cells, idx, &cell_text);
            let expanded = expand_bindings(reduce_input, &bindings);
            let reduce_expr = translate::to_reduce(&expanded);
            self.reduce
                .submit(cell_id, Vec::new(), reduce_expr.clone());
            self.pending_reduce.insert(
                cell_id,
                PendingReduce {
                    cell_id,
                    purpose: ReducePurpose::InlineCas,
                },
            );
            self.cells[idx].outcome.message = Some(CellMessage::Pending);
            return;
        }

        // Pure expression cell with no action — treat as Playing (its bindings
        // are still in scope for later cells via combined_ast_up_to).
        self.cells[idx].state = CellState::Playing;
        self.cells[idx].last_played_text = Some(snapshot);
    }

    fn handle_print(
        &mut self,
        idx: usize,
        print_idx: usize,
        combined: &AstNode,
        cell_text: &str,
    ) {
        let eval_ast = lang::build_print_ast(combined, print_idx);
        match interpreter::eval(&eval_ast, self.gpu.as_ref()) {
            Ok(val) => {
                self.cells[idx].outcome.message =
                    Some(CellMessage::Computed(format!("{}", val)));
            }
            Err(_) => {
                // Interpreter couldn't evaluate (e.g. expression references
                // axis variable `x`). Fall through to REDUCE for symbolic
                // simplification.
                let inner =
                    extract_call_arg(cell_text, "print").unwrap_or_else(|| cell_text.to_string());
                let bindings = collect_bindings(&self.cells, idx, cell_text);
                let expanded = expand_bindings(&inner, &bindings);
                let (reduce_expr, is_equation) = prepare_reduce_print(&expanded);
                let cell_id = self.cells[idx].id;
                self.reduce.submit(cell_id, Vec::new(), reduce_expr);
                self.pending_reduce.insert(
                    cell_id,
                    PendingReduce {
                        cell_id,
                        purpose: ReducePurpose::Print { is_equation },
                    },
                );
                self.cells[idx].outcome.message = Some(CellMessage::Pending);
            }
        }
    }

    fn handle_plot(&mut self, idx: usize, plot_idx: usize, combined: &AstNode, snapshot: &str) {
        let plot_ast = lang::build_plot_ast(combined, plot_idx);
        match crate::lang::wgsl_gen::generate(&plot_ast) {
            Ok(wgsl) => {
                self.cells[idx].outcome.shader = Some(ShaderSpec {
                    wgsl,
                    dispatch: DispatchKind::Fragment,
                    bindings: Vec::new(),
                });
                self.cells[idx].outcome.cpu_program = Some(plot_ast);
                self.cells[idx].state = CellState::Playing;
                self.cells[idx].last_played_text = Some(snapshot.to_string());
            }
            Err(e) => {
                self.set_runtime_error(idx, e, snapshot);
            }
        }
    }

    fn handle_reduce_response(
        &mut self,
        resp: crate::lang::reduce::service::ReduceResponse,
    ) -> Option<usize> {
        let pending = self.pending_reduce.remove(&resp.cell_id)?;
        let idx = self.cells.iter().position(|c| c.id == resp.cell_id)?;

        let simplified = match resp.result {
            Ok(text) if !text.is_empty() => Some(translate::from_reduce(&text)),
            Ok(_) => {
                self.cells[idx].outcome.message = None;
                return Some(idx);
            }
            Err(e) => {
                self.set_runtime_error(idx, e, &self.cells[idx].buffer.text().to_string());
                return Some(idx);
            }
        };

        let result = simplified?;

        match pending.purpose {
            ReducePurpose::Print { is_equation } => {
                let display = if is_equation {
                    format!("{} = 0", result)
                } else {
                    result
                };
                self.cells[idx].outcome.message = Some(CellMessage::Computed(display));
            }
            ReducePurpose::InlineCas => {
                let working_text = match &self.cells[idx].outcome.message {
                    Some(CellMessage::Simplified(s)) => s.clone(),
                    _ => self.cells[idx].buffer.text().to_string(),
                };
                let (_, embedded) = extract_reduce_expr(&working_text);
                let substituted = substitute_reduce_result(&working_text, &result, embedded);
                self.cells[idx].outcome.message =
                    Some(CellMessage::Simplified(substituted.clone()));

                if find_cas_call(&substituted).is_some() {
                    // Iterative CAS resolution — resubmit with bindings.
                    let (next_input, _) = extract_reduce_expr(&substituted);
                    let bindings = collect_bindings(&self.cells, idx, &substituted);
                    let expanded = expand_bindings(next_input, &bindings);
                    let reduce_expr = translate::to_reduce(&expanded);
                    let cell_id = self.cells[idx].id;
                    self.reduce.submit(cell_id, Vec::new(), reduce_expr);
                    self.pending_reduce.insert(
                        cell_id,
                        PendingReduce {
                            cell_id,
                            purpose: ReducePurpose::InlineCas,
                        },
                    );
                    return Some(idx);
                }

                if let Some(op) = translate::detect_unevaluated_cas(&substituted) {
                    let msg = format!(
                        "This {} cannot be computed symbolically. \
                         Consider using a numerical method instead.",
                        op
                    );
                    self.set_runtime_error(idx, msg, &self.cells[idx].buffer.text().to_string());
                    return Some(idx);
                }
                let special = translate::detect_special_functions(&substituted);
                if !special.is_empty() {
                    let names: Vec<&str> = special.iter().map(|(_, d)| *d).collect();
                    let msg = format!(
                        "No closed-form solution \u{2014} result requires {}. \
                         Consider using a numerical method instead.",
                        names.join(", ")
                    );
                    self.set_runtime_error(idx, msg, &self.cells[idx].buffer.text().to_string());
                    return Some(idx);
                }

                // All clean — generate WGSL from the combined source with
                // this cell's text replaced by the simplified form.
                self.compile_after_simplify(idx, &substituted);
            }
        }
        Some(idx)
    }

    fn compile_after_simplify(&mut self, idx: usize, substituted: &str) {
        let mut source = String::new();
        for (i, cell) in self.cells.iter().enumerate() {
            if i > idx {
                break;
            }
            if !source.is_empty() {
                source.push('\n');
            }
            if i == idx {
                source.push_str(substituted);
            } else if let Some(CellMessage::Simplified(s)) = &cell.outcome.message {
                source.push_str(s);
            } else {
                source.push_str(cell.buffer.text());
            }
        }
        match crate::lang::compile(&source) {
            Ok(wgsl) => {
                self.cells[idx].outcome.shader = Some(ShaderSpec {
                    wgsl,
                    dispatch: DispatchKind::Fragment,
                    bindings: Vec::new(),
                });
                self.cells[idx].state = CellState::Playing;
                self.cells[idx].last_played_text = Some(self.cells[idx].buffer.text().to_string());
            }
            Err(e) => {
                if !e.contains("No result expression") {
                    self.set_runtime_error(idx, e, &source);
                }
            }
        }
    }

    // ─── error helpers ─────────────────────────────────────────────────────

    fn set_parse_error(&mut self, idx: usize, msg: String, snapshot: &str) {
        let span = Span::whole(snapshot);
        self.cells[idx]
            .outcome
            .diagnostics
            .push(Diagnostic::error(msg.clone(), span));
        self.cells[idx].outcome.message = Some(CellMessage::Error(msg));
    }

    fn set_runtime_error(&mut self, idx: usize, msg: String, snapshot: &str) {
        let span = Span::whole(snapshot);
        self.cells[idx]
            .outcome
            .diagnostics
            .push(Diagnostic::error(msg.clone(), span));
        self.cells[idx].outcome.message = Some(CellMessage::Error(msg));
    }

    /// Flat list of `(cell_index, diagnostic)` across every cell. Cheap
    /// snapshot for a top-level "show all errors" view.
    pub fn diagnostics(&self) -> Vec<(usize, &Diagnostic)> {
        let mut out = Vec::new();
        for (i, cell) in self.cells.iter().enumerate() {
            for d in &cell.outcome.diagnostics {
                out.push((i, d));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
