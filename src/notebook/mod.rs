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

mod cell;
mod diagnostic;
mod internals;
mod reduce_backend;
mod shader;

pub use cell::{CellMessage, CellOutcome, CellState, NotebookCell};
pub use diagnostic::{Diagnostic, Span};
pub use reduce_backend::{NoReduce, ReduceBackend, SharedReduce};
pub use shader::{DispatchKind, ShaderSpec};
// `Severity`, `Access`, `BindingSpec`, `ScalarType`, `SizeSpec` stay in
// their sub-modules. Re-export them at this level once a consumer (UI
// renderer for diagnostics, parallel/Monte-Carlo emission for bindings)
// actually wants them.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// Process-global cell ID counter. Multiple notebooks may share a single
/// REDUCE service, so cell IDs must be unique across all notebooks for
/// response routing to work.
static NEXT_CELL_ID: AtomicUsize = AtomicUsize::new(0);

fn alloc_cell_id() -> usize {
    NEXT_CELL_ID.fetch_add(1, Ordering::Relaxed)
}

/// Process-global plot-color seed. Each new cell takes one — the resulting
/// color is deterministic in the seed (see `color_from_seed`), so two cells
/// created back-to-back are always different.
static NEXT_COLOR_SEED: AtomicU32 = AtomicU32::new(0);

pub fn alloc_color_seed() -> u32 {
    NEXT_COLOR_SEED.fetch_add(1, Ordering::Relaxed)
}

/// Deterministic seed → color mapping. Uses golden-ratio hue rotation so
/// nearby seeds land on distinct, well-spread hues; saturation/lightness are
/// fixed for plot-friendly contrast against both dark and light backgrounds.
pub fn color_from_seed(seed: u32) -> crate::ui::theme::Rgba {
    // Golden-ratio conjugate (≈ 0.618) — classic technique for low-discrepancy
    // hue selection. Multiply the seed by it, take the fractional part as hue.
    const PHI: f32 = 0.618_034;
    let hue = (seed as f32 * PHI).fract();
    let s = 0.65;
    let l = 0.62;
    let (r, g, b) = hsl_to_rgb(hue, s, l);
    crate::ui::theme::Rgba::rgb(
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    )
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s <= f32::EPSILON {
        return (l, l, l);
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hue_to_rgb = |p: f32, q: f32, mut t: f32| -> f32 {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            return p + (q - p) * 6.0 * t;
        }
        if t < 0.5 {
            return q;
        }
        if t < 2.0 / 3.0 {
            return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
        }
        p
    };
    (
        hue_to_rgb(p, q, h + 1.0 / 3.0),
        hue_to_rgb(p, q, h),
        hue_to_rgb(p, q, h - 1.0 / 3.0),
    )
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

    /// Currently only used by tests; here as the obvious counterpart to
    /// `len()`.
    #[allow(dead_code)]
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

    /// Append a cell with a freshly-allocated color seed. Use this for
    /// new (untitled) cells; loaders that already know the color should
    /// call `add_cell_with_color` instead so the saved color round-trips.
    pub fn add_cell(&mut self, text: &str) -> usize {
        let seed = alloc_color_seed();
        let color = color_from_seed(seed);
        self.add_cell_with_color(text, seed, color)
    }

    /// Append a cell with an explicit color (and seed). Used by the JSON
    /// loader so saved colors round-trip exactly.
    pub fn add_cell_with_color(
        &mut self,
        text: &str,
        color_seed: u32,
        plot_color: Rgba,
    ) -> usize {
        let id = alloc_cell_id();
        self.cells
            .push(NotebookCell::new(id, text, color_seed, plot_color));
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

    /// Replace a cell's text. Currently only used by tests; the UI
    /// mutates the buffer directly via `cell_mut().buffer`. Kept as the
    /// programmatic equivalent so non-UI callers (CLI, scripts) don't
    /// have to know about `Buffer`.
    #[allow(dead_code)]
    pub fn set_text(&mut self, idx: usize, text: &str) {
        if idx >= self.cells.len() {
            return;
        }
        self.cells[idx].buffer.set_text(text);
        self.cells[idx].invalidate_ast();
    }

    /// Programmatic plot-color setter. Currently no UI binding (color is
    /// fixed at cell creation); kept as obvious public API for future
    /// theme/colorbar wiring.
    #[allow(dead_code)]
    pub fn set_plot_color(&mut self, idx: usize, color: Rgba) {
        if idx >= self.cells.len() {
            return;
        }
        self.cells[idx].plot_color = color;
    }

    /// Replace the GPU dispatcher used by the interpreter for
    /// `parallel for`/array cells. Production calls this once after the
    /// renderer is up — before that, the notebook uses `CpuFallback` and
    /// dispatch is sequential on the CPU.
    pub fn set_gpu(&mut self, gpu: Box<dyn GpuDispatch>) {
        self.gpu = gpu;
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
    /// (The UI doesn't bind a "replay" button yet; tests cover this.)
    #[allow(dead_code)]
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

    /// True if there's at least one REDUCE round-trip outstanding. Tests
    /// use this to drive their poll loops; production checks the shared
    /// `ReduceService::has_pending` directly.
    #[allow(dead_code)]
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
    /// *effective* source — the iterative-CAS-substituted text when one
    /// exists, otherwise the raw buffer text.
    ///
    /// Invariant on the per-cell `ast_cache`: it always reflects the raw
    /// buffer text (`cell.cached_ast()` parses `buffer.text()`). When a
    /// cell's outcome carries a `Simplified` message — produced by
    /// `compile_after_simplify` after a REDUCE round-trip rewrote the
    /// source — we bypass the cache and parse the simplified text directly,
    /// because (a) caching a key for "simplified" form would mean two cache
    /// entries per cell, and (b) the simplified form is short-lived and
    /// re-parsed at most once per REDUCE round-trip resolution.
    /// `(start, end)` half-open range of statement indices in
    /// `combined_ast_up_to(cell_index)` that come from cell `cell_index`
    /// itself. Used by `handle_plot` to filter plots to "this cell's plots
    /// only" — earlier cells' plots already had their shaders compiled when
    /// those cells ran.
    fn cell_stmt_range(&self, cell_index: usize) -> Result<(usize, usize), String> {
        let mut offset = 0;
        let stmt_count = |cell: &NotebookCell| -> Result<usize, String> {
            let parsed = match &cell.outcome.message {
                Some(CellMessage::Simplified(s)) => crate::lang::parse(s)?,
                _ => cell.cached_ast()?,
            };
            Ok(match parsed {
                AstNode::Block { items, .. } => items.len(),
                _ => 1,
            })
        };
        for (i, cell) in self.cells.iter().enumerate() {
            if i >= cell_index {
                break;
            }
            offset += stmt_count(cell).map_err(|e| format!("Cell {}: {}", i + 1, e))?;
        }
        let own = stmt_count(&self.cells[cell_index])
            .map_err(|e| format!("Cell {}: {}", cell_index + 1, e))?;
        Ok((offset, offset + own))
    }

    fn combined_ast_up_to(&self, cell_index: usize) -> Result<AstNode, String> {
        let mut all_stmts = Vec::new();
        for (i, cell) in self.cells.iter().enumerate() {
            if i > cell_index {
                break;
            }
            let parsed = match &cell.outcome.message {
                Some(CellMessage::Simplified(s)) => crate::lang::parse(s),
                _ => cell.cached_ast(),
            }
            .map_err(|e| format!("Cell {}: {}", i + 1, e))?;
            match parsed {
                AstNode::Block { items, .. } => all_stmts.extend(items),
                other => all_stmts.push(other),
            }
        }
        let span = if all_stmts.is_empty() {
            (0, 0)
        } else {
            (
                all_stmts.first().unwrap().span().0,
                all_stmts.last().unwrap().span().1,
            )
        };
        Ok(AstNode::Block {
            items: all_stmts,
            span,
        })
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

        // Restrict plots to those originating in *this* cell — earlier
        // cells' plots already produced shaders when those cells ran.
        let own_plots: Vec<usize> = match self.cell_stmt_range(idx) {
            Ok((start, end)) => actions
                .plots
                .iter()
                .copied()
                .filter(|i| (start..end).contains(i))
                .collect(),
            Err(_) => Vec::new(),
        };

        let cell_text = self.cells[idx].buffer.text().trim().to_string();

        if let Some(print_idx) = actions.first_print {
            self.handle_print(idx, print_idx, &combined, &cell_text);
            if own_plots.is_empty() {
                return;
            }
        }

        if !own_plots.is_empty() {
            self.handle_plots(idx, &own_plots, &combined, &snapshot);
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
                        purpose: ReducePurpose::Print { is_equation },
                    },
                );
                self.cells[idx].outcome.message = Some(CellMessage::Pending);
            }
        }
    }

    fn handle_plots(
        &mut self,
        idx: usize,
        plot_indices: &[usize],
        combined: &AstNode,
        snapshot: &str,
    ) {
        let mut shaders = Vec::with_capacity(plot_indices.len());
        let mut last_ast: Option<AstNode> = None;
        for &plot_idx in plot_indices {
            let plot_ast = lang::build_plot_ast(combined, plot_idx);
            match crate::lang::wgsl_gen::generate(&plot_ast) {
                Ok(wgsl) => {
                    shaders.push(ShaderSpec {
                        wgsl,
                        dispatch: DispatchKind::Fragment,
                        bindings: Vec::new(),
                    });
                    last_ast = Some(plot_ast);
                }
                Err(e) => {
                    self.set_runtime_error(idx, e, snapshot);
                    return;
                }
            }
        }
        self.cells[idx].outcome.shaders = shaders;
        self.cells[idx].outcome.cpu_program = last_ast;
        self.cells[idx].state = CellState::Playing;
        self.cells[idx].last_played_text = Some(snapshot.to_string());
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
        // Cell's effective text is already the Simplified message at this point
        // (the caller set it before calling us), so combined_ast_up_to and
        // cell_stmt_range reflect the substituted form.
        let combined_result = self.combined_ast_up_to(idx);
        if let Ok(combined) = combined_result {
            let actions = lang::detect_cell_actions(&combined);
            let own_plots: Vec<usize> = match self.cell_stmt_range(idx) {
                Ok((start, end)) => actions
                    .plots
                    .iter()
                    .copied()
                    .filter(|i| (start..end).contains(i))
                    .collect(),
                Err(_) => Vec::new(),
            };
            if !own_plots.is_empty() {
                let snapshot = self.cells[idx].buffer.text().to_string();
                self.handle_plots(idx, &own_plots, &combined, &snapshot);
                return;
            }
        }

        // No plot calls — fall back to whole-source compile so cells like
        // `int(x²,x)` that simplify to a raw expression still render as a
        // grayscale fragment shader.
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
                self.cells[idx].outcome.shaders = vec![ShaderSpec {
                    wgsl,
                    dispatch: DispatchKind::Fragment,
                    bindings: Vec::new(),
                }];
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

    /// Concatenate the effective source of cells `[0..=cell_index]` —
    /// using each cell's `Simplified` message as its source if present,
    /// otherwise the buffer text. Used by the renderer to build the
    /// "user-visible" source string for line/col error reporting on
    /// shader-compile failures.
    pub fn combined_source(&self, cell_index: usize) -> String {
        let mut s = String::new();
        for (i, cell) in self.cells.iter().enumerate() {
            if i > cell_index {
                break;
            }
            if !s.is_empty() {
                s.push('\n');
            }
            match &cell.outcome.message {
                Some(CellMessage::Simplified(out)) => s.push_str(out),
                _ => s.push_str(cell.buffer.text()),
            }
        }
        s
    }

    /// Flat list of `(cell_index, diagnostic)` across every cell. Cheap
    /// snapshot for a top-level "show all errors" view; the UI doesn't
    /// bind to this yet.
    #[allow(dead_code)]
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
