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
mod reduce_backend;
mod reduce_simplifier;
mod shader;

pub use cell::{CellMessage, CellOutcome, CellState, NotebookCell};
pub use diagnostic::{Diagnostic, Severity, Span};
pub use reduce_backend::{ReduceBackend, SharedReduce};
pub use reduce_simplifier::ReduceSimplifier;
pub use shader::ShaderSpec;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Process-global cell ID counter. Multiple notebooks may share a single
/// REDUCE service, so cell IDs must be unique across all notebooks for
/// response routing to work.
static NEXT_CELL_ID: AtomicUsize = AtomicUsize::new(0);

fn alloc_cell_id() -> usize {
    NEXT_CELL_ID.fetch_add(1, Ordering::Relaxed)
}

/// Process-global plot-color seed. Each `alloc_color_seed` call mixes a
/// monotonically-incrementing counter with the current wall-clock nanos so
/// successive cells get unrelated, "random-feeling" seeds (and a +1 step on
/// the color button still moves to a totally different color thanks to the
/// bit-mixing in `color_from_seed`).
static NEXT_COLOR_SEED: AtomicU32 = AtomicU32::new(0);

pub fn alloc_color_seed() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let counter = NEXT_COLOR_SEED.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u32)
        .unwrap_or(0);
    counter
        .wrapping_mul(0x9E3779B1)
        .wrapping_add(nanos)
}

/// Seed → color mapping. The seed is bit-mixed three different ways
/// (splitmix32) to derive hue, saturation, and lightness independently, so
/// adjacent seeds produce visually unrelated colors and the result spans
/// the full HSL space.
pub fn color_from_seed(seed: u32) -> crate::ui::theme::Rgba {
    fn splitmix32(mut x: u32) -> u32 {
        x ^= x >> 16;
        x = x.wrapping_mul(0x7feb352d);
        x ^= x >> 15;
        x = x.wrapping_mul(0x846ca68b);
        x ^= x >> 16;
        x
    }
    let h_bits = splitmix32(seed.wrapping_add(0x9E3779B1));
    let s_bits = splitmix32(seed.wrapping_add(0x517CC1B7));
    let l_bits = splitmix32(seed.wrapping_add(0x59E7E0A3));
    let unit = u32::MAX as f32;
    let hue = h_bits as f32 / unit;
    // Vibrant but not neon — keep saturation and lightness in plot-friendly
    // ranges so colors stay legible against both dark and light backgrounds.
    let sat = 0.55 + (s_bits as f32 / unit) * 0.40;
    let light = 0.45 + (l_bits as f32 / unit) * 0.25;
    let (r, g, b) = hsl_to_rgb(hue, sat, light);
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

use crate::lang::interpreter::{self, GpuDispatch};
use crate::lang::ir::{BuiltinOp, Callee, Ir};
use crate::lang::reduce::translate;
use crate::lang::symbolic::SymbolicSimplifier;
use crate::lang::{self, highlight};
use crate::ui::theme::Rgba;

/// Why a particular simplifier round-trip is in flight. Lets `tick()` route
/// the response to the right cell-update path.
enum SimplifyPurpose {
    /// One or more `print(...)` calls in the cell where at least the
    /// currently-in-flight one needed the simplifier. Carries the partial
    /// state of the cell's print batch so subsequent prints can be driven
    /// after this response lands.
    Print(PrintBatch),
    /// CAS function (`int`, `df`, etc.) embedded in source. On response,
    /// splice the simplified IR back into the cell's `effective_ir` at
    /// `path` and, if more CAS subtrees remain, resubmit; otherwise
    /// regenerate WGSL from the resolved IR.
    InlineCas { path: Vec<usize> },
}

/// Per-cell state for a multi-print run. Each `print(...)` call produces
/// one line of output; `completed` holds the lines already produced and
/// `remaining` holds the print indices yet to be evaluated. When the
/// in-flight simplifier response lands, we append its formatted result to
/// `completed`, then drive `remaining` until either we exhaust them
/// (publishing the joined output) or we hit another print that needs the
/// simplifier (re-parking on `Pending` with the updated batch).
struct PrintBatch {
    completed: Vec<String>,
    remaining: Vec<usize>,
    /// Whether the currently-in-flight (just-submitted) print expression
    /// was an equation, so we re-attach `= 0` to the response result.
    current_is_equation: bool,
    /// Snapshot of the combined IR captured when `run_cell` started. Used
    /// to look up bindings and rebuild print expressions for the queued
    /// prints as the batch advances.
    combined: Ir,
}

struct PendingSimplify {
    purpose: SimplifyPurpose,
    /// Wall-clock instant when the request was handed to the simplifier.
    /// `tick_with_now` compares against this to surface a timeout diagnostic
    /// when the symbolic engine never responds (worker crash, runaway
    /// REDUCE problem, etc.) — otherwise the cell would sit on `Pending`
    /// forever with no way for the user to tell `…` apart from a hung app.
    submitted_at: Instant,
}

/// Pull every top-level `Binding` statement out of `ir`, stopping at
/// `before_idx` if given (exclusive). Used to gather the in-scope binding
/// set when expanding identifier references in a CAS / print expression
/// before submitting it to the simplifier.
fn collect_top_bindings(ir: &Ir, before_idx: Option<usize>) -> Vec<(String, Ir)> {
    let stmts: &[Ir] = match ir {
        Ir::Block { items, .. } => items.as_slice(),
        single => std::slice::from_ref(single),
    };
    let limit = before_idx.unwrap_or(stmts.len()).min(stmts.len());
    let mut bindings = Vec::new();
    for stmt in stmts.iter().take(limit) {
        match stmt {
            // Plain `name := value` — `name` resolves to `value` everywhere.
            Ir::Binding { name, value, .. } => {
                bindings.push((name.clone(), value.as_ref().clone()));
            }
            // `f(x) := body` — parses as `FunctionDef`, not `Binding`. The
            // entire FunctionDef is the binding for `f`; `substitute_idents`
            // looks it up by name on `Apply { callee: User("f"), … }` and
            // beta-reduces the call. Without this entry, CAS subtrees that
            // reference user functions go to REDUCE with the call still
            // present (`int(f(x), x)`) — REDUCE then asks the user to
            // declare `f` as an operator.
            Ir::FunctionDef { name, .. } => {
                bindings.push((name.clone(), stmt.clone()));
            }
            _ => {}
        }
    }
    bindings
}

/// Walk the cell's IR for the top-level `print(arg)` or `plot(arg)` Apply
/// at `stmt_idx` and return its single argument. Returns `None` if the
/// statement isn't an Apply with one argument matching the requested op.
fn action_arg<'a>(combined: &'a Ir, stmt_idx: usize, op: BuiltinOp) -> Option<&'a Ir> {
    let stmts: &[Ir] = match combined {
        Ir::Block { items, .. } => items.as_slice(),
        single => std::slice::from_ref(single),
    };
    let stmt = stmts.get(stmt_idx)?;
    if let Ir::Apply {
        callee: Callee::Builtin(c),
        args,
        ..
    } = stmt
    {
        if *c == op && args.len() == 1 {
            return Some(&args[0]);
        }
    }
    None
}

/// Rewrite an equation `lhs = rhs` as `lhs - rhs` so the simplifier
/// reduces the residual to a single value. Returns `(rewritten_ir,
/// is_equation)` where the flag tells the response handler whether to
/// re-attach the trailing `= 0` for display.
fn fold_equation(ir: Ir) -> (Ir, bool) {
    if let Ir::Apply {
        callee: Callee::Builtin(BuiltinOp::Eq),
        args,
        span,
        result_ty: None,
    } = ir
    {
        if args.len() == 2 {
            let mut iter = args.into_iter();
            let lhs = iter.next().unwrap();
            let rhs = iter.next().unwrap();
            return (
                Ir::Apply {
                    callee: Callee::Builtin(BuiltinOp::Sub),
                    args: vec![lhs, rhs],
                    span,
                    result_ty: None,
                },
                true,
            );
        }
        // Restore the original Apply if arity wasn't 2.
        return (
            Ir::Apply {
                callee: Callee::Builtin(BuiltinOp::Eq),
                args,
                span,
                result_ty: None,
            },
            false,
        );
    }
    (ir, false)
}

pub struct Notebook {
    cells: Vec<NotebookCell>,
    simplifier: Box<dyn SymbolicSimplifier>,
    /// In-flight symbolic-simplifier round-trips, keyed by `cell_id` (not
    /// index — indices shift on `remove_cell`).
    pending_simplify: HashMap<usize, PendingSimplify>,
    /// CPU/GPU dispatcher for interpreter-side work (parallel for, arrays).
    /// Defaults to `CpuFallback`; the production app injects a wgpu-backed
    /// dispatcher when wired through `NotebookView`.
    gpu: Box<dyn GpuDispatch>,
}

impl Notebook {
    /// Construct an empty notebook. `simplifier` is the symbolic-algebra
    /// backend (production uses `ReduceSimplifier::new(SharedReduce::…)`);
    /// `gpu` defaults to a CPU fallback when `None` — fine for tests and
    /// CLI use.
    pub fn new(
        simplifier: Box<dyn SymbolicSimplifier>,
        gpu: Option<Box<dyn GpuDispatch>>,
    ) -> Self {
        Self {
            cells: Vec::new(),
            simplifier,
            pending_simplify: HashMap::new(),
            gpu: gpu.unwrap_or_else(|| Box::new(interpreter::CpuFallback)),
        }
    }

    // ─── structure ─────────────────────────────────────────────────────────

    pub fn len(&self) -> usize {
        self.cells.len()
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
            self.pending_simplify.remove(&id);
        }
    }

    // ─── writes ────────────────────────────────────────────────────────────

    /// Replace a cell's text. The UI mutates the buffer directly via
    /// `cell_mut().buffer`; this is the programmatic equivalent for tests
    /// that drive the notebook without going through `Buffer`.
    #[allow(dead_code)]
    pub fn set_text(&mut self, idx: usize, text: &str) {
        if idx >= self.cells.len() {
            return;
        }
        self.cells[idx].buffer.set_text(text);
        self.cells[idx].invalidate_ir();
        // Drop any pending simplifier round-trip — the path that the
        // response was going to splice into refers to the old IR, which
        // no longer matches the buffer text. Without this, a late
        // response can either splice unrelated bits into the new text or
        // raise a spurious "splice path no longer valid" diagnostic.
        let cell_id = self.cells[idx].id;
        self.pending_simplify.remove(&cell_id);
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
    /// The UI doesn't bind a replay button yet; tests drive this directly.
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
        // Forget any in-flight simplifier round-trip for this cell —
        // otherwise a late REDUCE response would land on a stopped cell
        // and re-publish a Computed/Simplified message, contradicting
        // the user's "stop" action.
        let cell_id = self.cells[idx].id;
        self.pending_simplify.remove(&cell_id);
    }

    /// Drain ready REDUCE responses. Call once per frame (or until
    /// `has_pending() == false` for sync test loops). Returns the cell
    /// indices whose outcome changed.
    pub fn tick(&mut self) -> Vec<usize> {
        self.tick_with_now(Instant::now())
    }

    /// `tick` with an injectable clock. Production calls `tick`; tests use
    /// this entry point so they can advance simulated time past
    /// `PENDING_TIMEOUT` and assert the resulting timeout diagnostic
    /// without sleeping for 30 real seconds.
    pub fn tick_with_now(&mut self, now: Instant) -> Vec<usize> {
        let mut updated = Vec::new();
        while let Some(resp) = self.simplifier.try_recv() {
            if let Some(idx) = self.handle_simplify_response(resp) {
                if !updated.contains(&idx) {
                    updated.push(idx);
                }
            }
        }

        // Surface any pending simplifier round-trip that the symbolic engine
        // hasn't answered within `PENDING_TIMEOUT` as an error diagnostic on
        // the cell. Without this safety net, a crashed REDUCE worker or a
        // runaway symbolic problem leaves the cell on `…` indefinitely —
        // indistinguishable to the user from "the app froze".
        let timed_out: Vec<usize> = self
            .pending_simplify
            .iter()
            .filter(|(_, p)| now.saturating_duration_since(p.submitted_at) >= Self::PENDING_TIMEOUT)
            .map(|(&cell_id, _)| cell_id)
            .collect();
        for cell_id in timed_out {
            self.pending_simplify.remove(&cell_id);
            if let Some(idx) = self.cells.iter().position(|c| c.id == cell_id) {
                let snapshot = self.cells[idx].buffer.text().to_string();
                let msg = format!(
                    "Symbolic engine did not respond within {}s. The service may have \
                     crashed, or the symbolic problem is too hard to solve in reasonable \
                     time.",
                    Self::PENDING_TIMEOUT.as_secs()
                );
                self.set_runtime_error(idx, msg, &snapshot);
                if !updated.contains(&idx) {
                    updated.push(idx);
                }
            }
        }

        updated
    }

    /// Maximum time a pending symbolic-simplifier round-trip is allowed to
    /// take before `tick_with_now` surfaces it as a user-visible error.
    pub const PENDING_TIMEOUT: Duration = Duration::from_secs(30);

    /// Mark a cell as just-edited. Sets `last_edit_at = now`, which the
    /// auto-rerun pass watches to decide when 200ms of quiet has elapsed.
    /// Called by the app's keystroke handler after each text-changing input.
    ///
    /// Also clears any pending-simplifier round-trip on the cell — the
    /// user's keystroke has invalidated whatever the prior submission's
    /// splice path was pointing at, so a late response from REDUCE must
    /// not be allowed to land. `set_text` (the programmatic equivalent)
    /// does the same; we mirror it here for the live-typing path.
    pub fn mark_edited(&mut self, idx: usize, now: std::time::Instant) {
        if let Some(cell) = self.cells.get_mut(idx) {
            cell.last_edit_at = Some(now);
            cell.effective_ir = None;
        }
        if let Some(cell) = self.cells.get(idx) {
            let cell_id = cell.id;
            self.pending_simplify.remove(&cell_id);
        }
    }

    /// Time the cell must have been idle (no further edits) before the
    /// auto-rerun pass replays it. Long enough that a normal burst of typing
    /// doesn't trigger redundant compiles, short enough that the user
    /// perceives the plot as "live."
    pub const AUTO_RERUN_QUIET_PERIOD: std::time::Duration =
        std::time::Duration::from_millis(200);

    /// Replay every cell that has been edited and then idle for at least
    /// `AUTO_RERUN_QUIET_PERIOD`. Returns the cell indices that were replayed
    /// so the caller can resync UI state for them.
    pub fn tick_auto_rerun(&mut self, now: std::time::Instant) -> Vec<usize> {
        let ready: Vec<usize> = self
            .cells
            .iter()
            .enumerate()
            .filter_map(|(idx, cell)| {
                let edited_at = cell.last_edit_at?;
                let elapsed = now.saturating_duration_since(edited_at);
                if elapsed >= Self::AUTO_RERUN_QUIET_PERIOD && cell.is_stale() {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect();
        for &idx in &ready {
            self.cells[idx].last_edit_at = None;
            self.play(idx);
        }
        ready
    }

    /// Earliest wake-up time the app loop should wait until for the next
    /// pending auto-rerun. Returns `None` when no cell is awaiting replay.
    /// The app uses this to set `ControlFlow::WaitUntil` so 200ms of editor
    /// silence actually fires the rerun without the user having to nudge
    /// another event.
    pub fn next_auto_rerun_deadline(
        &self,
        now: std::time::Instant,
    ) -> Option<std::time::Instant> {
        let _ = now;
        self.cells
            .iter()
            .filter_map(|cell| {
                let edited_at = cell.last_edit_at?;
                if cell.is_stale() {
                    Some(edited_at + Self::AUTO_RERUN_QUIET_PERIOD)
                } else {
                    None
                }
            })
            .min()
    }

    /// True if there's at least one REDUCE round-trip outstanding. Tests
    /// use this to drive their poll loops; production checks the shared
    /// `ReduceService::has_pending` directly.
    #[allow(dead_code)]
    pub fn has_pending(&self) -> bool {
        self.simplifier.has_pending() || !self.pending_simplify.is_empty()
    }

    /// Forget every in-flight REDUCE round-trip this notebook is tracking.
    /// The shared REDUCE service may still receive responses for these
    /// cells; the notebook will silently drop them since `pending_simplify`
    /// no longer maps the cell IDs.
    pub fn clear_pending(&mut self) {
        self.pending_simplify.clear();
    }

    // ─── internals ─────────────────────────────────────────────────────────

    /// `cell.effective_ir` is set by the iterative-CAS path after each
    /// `int(...)` / `df(...)` is replaced with the simplified subtree.
    /// Downstream cells should see the resolved form so their bindings
    /// reference fully-evaluated values, not the raw CAS calls.
    fn combined_ir_up_to(&self, cell_index: usize) -> Result<Ir, String> {
        let mut all_stmts = Vec::new();
        for (i, cell) in self.cells.iter().enumerate() {
            if i > cell_index {
                break;
            }
            let parsed = if let Some(ir) = &cell.effective_ir {
                ir.clone()
            } else {
                cell.cached_ir()
                    .map_err(|e| format!("Cell {}: {}", i + 1, e))?
            };
            match parsed {
                Ir::Block { items, .. } => all_stmts.extend(items),
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
        Ok(Ir::Block {
            items: all_stmts,
            span,
        })
    }

    /// `(start, end)` half-open range of statement indices in
    /// `combined_ir_up_to(cell_index)` that come from cell `cell_index`
    /// itself. Used by `run_cell` to filter `actions.plots` to "this
    /// cell's plots only" — earlier cells' plots already had shaders
    /// emitted when those cells ran.
    fn cell_stmt_range(&self, cell_index: usize) -> Result<(usize, usize), String> {
        let stmt_count = |cell: &NotebookCell| -> Result<usize, String> {
            let parsed = if let Some(ir) = &cell.effective_ir {
                ir.clone()
            } else {
                cell.cached_ir()?
            };
            Ok(match parsed {
                Ir::Block { items, .. } => items.len(),
                _ => 1,
            })
        };
        let mut offset = 0;
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

    fn run_cell(&mut self, idx: usize) {
        let snapshot = self.cells[idx].buffer.text().to_string();

        // Reset outcome but keep token colors fresh regardless of success.
        // Also clear any prior post-simplification splice — a fresh play
        // starts from the raw buffer IR.
        self.cells[idx].outcome = CellOutcome::default();
        self.cells[idx].outcome.token_colors = highlight::highlight(&snapshot);
        self.cells[idx].effective_ir = None;

        // Drop any pending simplifier round-trip for this cell — it would
        // only confuse the response handler if it arrives now.
        let cell_id = self.cells[idx].id;
        self.pending_simplify.remove(&cell_id);

        // Surface parse failures (this cell and any earlier cell whose
        // bindings we'd need to splice in) up front — every downstream step
        // needs a well-formed IR.
        let combined = match self.combined_ir_up_to(idx) {
            Ok(ir) => ir,
            Err(msg) => {
                self.set_parse_error(idx, msg, &snapshot);
                return;
            }
        };
        let cell_ir = match self.cells[idx].cached_ir() {
            Ok(ir) => ir,
            Err(msg) => {
                self.set_parse_error(idx, msg, &snapshot);
                return;
            }
        };

        // CAS-fixpoint precondition: any unresolved `int(...)`/`df(...)`
        // subtree in this cell's IR — including those buried in user
        // function bodies — gets shipped to the symbolic simplifier *before*
        // we dispatch to an action handler. The response handler splices
        // the result back, then re-enters `dispatch` (or this same loop if
        // more CAS subtrees remain). Making CAS resolution a precondition
        // means new action types (parallel/array/etc.) can't accidentally
        // leak unresolved CAS calls downstream — they're guaranteed gone
        // before dispatch.
        //
        // One carve-out: cells whose only action is `print(...)` skip the
        // fixpoint. `handle_prints` already routes interpreter-failing
        // expressions through the symbolic simplifier per-print — for
        // `print(∫(sin(x), x))` that's one round-trip with the CAS subtree
        // intact, vs. two for "fixpoint, then re-submit the spliced text".
        // Plot/interpreter/bare-expression branches have no such fallback,
        // so for them the fixpoint is mandatory.
        if self.cell_needs_eager_cas(idx, &combined) {
            if let Some(cas_path) = translate::find_cas_path(&cell_ir) {
                self.submit_cas_at(idx, &cell_ir, cas_path);
                return;
            }
        }

        self.dispatch(idx, &snapshot);
    }

    /// True iff the cell at `idx` has a downstream consumer that can't
    /// handle unresolved CAS subtrees on its own — i.e. a plot, an
    /// interpreter-requiring expression, or a pure-expression cell.
    fn cell_needs_eager_cas(&self, idx: usize, combined: &Ir) -> bool {
        let actions = lang::detect_cell_actions(combined);
        let own_plots: Vec<usize> = match self.cell_stmt_range(idx) {
            Ok((start, end)) => actions
                .plots
                .iter()
                .copied()
                .filter(|i| (start..end).contains(i))
                .collect(),
            Err(_) => actions.plots.clone(),
        };
        !own_plots.is_empty() || lang::needs_interpreter(combined) || !actions.has_action()
    }

    /// Action-routing for a cell whose CAS subtrees (if any) are already
    /// resolved. Entered from `run_cell` when the cell has no CAS subtree,
    /// and re-entered from `handle_simplify_response` once the iterative
    /// CAS fixpoint has fully expanded the cell's IR. Never submits to the
    /// symbolic simplifier itself — the only path back to `Pending` is
    /// `handle_prints` (interpreter fails on a bare-axis print and falls
    /// back to symbolic per-print).
    fn dispatch(&mut self, idx: usize, snapshot: &str) {
        let combined = match self.combined_ir_up_to(idx) {
            Ok(ir) => ir,
            Err(msg) => {
                self.set_parse_error(idx, msg, snapshot);
                return;
            }
        };

        let actions = lang::detect_cell_actions(&combined);

        // Restrict plots/prints to those originating in *this* cell —
        // earlier cells' plots already produced shaders when those cells
        // ran, and re-running their prints would clobber this cell's
        // output with output from a different cell.
        let (own_prints, own_plots): (Vec<usize>, Vec<usize>) = match self.cell_stmt_range(idx) {
            Ok((start, end)) => {
                let in_range = |i: &usize| (start..end).contains(i);
                (
                    actions.prints.iter().copied().filter(in_range).collect(),
                    actions.plots.iter().copied().filter(in_range).collect(),
                )
            }
            Err(_) => (Vec::new(), Vec::new()),
        };

        if !own_prints.is_empty() {
            self.handle_prints(idx, &own_prints, &combined);
            if own_plots.is_empty() {
                return;
            }
        }

        if !own_plots.is_empty() {
            self.handle_plots(idx, &own_plots, &combined, snapshot);
            return;
        }

        if actions.has_action() {
            return;
        }

        // No print/plot — array/parallel cells need the interpreter.
        if lang::needs_interpreter(&combined) {
            match interpreter::eval(&combined, self.gpu.as_ref()) {
                Ok(val) => {
                    self.cells[idx].outcome.message =
                        Some(CellMessage::Computed(format!("{}", val)));
                    self.cells[idx].outcome.program_ir = Some(combined);
                    self.cells[idx].state = CellState::Playing;
                    self.cells[idx].last_played_text = Some(snapshot.to_string());
                }
                Err(e) => self.set_runtime_error(idx, e, snapshot),
            }
            return;
        }

        // Pure expression / bare-CAS-resolved cell. Try WGSL gen so cells
        // like `int(x²,x)` (which simplifies to a raw expression) render as
        // a grayscale fragment shader. Pure-binding cells like `f := x²`
        // have no result expression — `wgsl_gen` flags that with a known
        // sentinel error and we mark them Playing with no shader so
        // downstream cells can still reference the binding.
        if let Err(e) = crate::lang::type_check(&combined, snapshot) {
            self.set_runtime_error(idx, e, snapshot);
            return;
        }
        match crate::lang::wgsl_gen::generate(&combined) {
            Ok(wgsl) => {
                self.cells[idx].outcome.shaders = vec![ShaderSpec { wgsl }];
                self.cells[idx].outcome.program_ir = Some(combined);
                self.cells[idx].state = CellState::Playing;
                self.cells[idx].last_played_text = Some(snapshot.to_string());
            }
            Err(e) => {
                if e.contains("No result expression") {
                    self.cells[idx].outcome.program_ir = Some(combined);
                    self.cells[idx].state = CellState::Playing;
                    self.cells[idx].last_played_text = Some(snapshot.to_string());
                } else {
                    self.set_runtime_error(idx, e, snapshot);
                }
            }
        }
    }

    /// Gather the in-scope bindings for a CAS-or-print expression at
    /// `current_path` in `current_cell_ir`: every top-level `Binding` from
    /// prior cells (using their post-simplification IR when available) plus
    /// the bindings in the current cell that come before the addressed
    /// statement.
    fn in_scope_bindings(
        &self,
        idx: usize,
        current_cell_ir: &Ir,
        current_path: &[usize],
    ) -> Vec<(String, Ir)> {
        let mut bindings = Vec::new();
        for cell in self.cells.iter().take(idx) {
            let prior_ir = cell.effective_ir.clone().or_else(|| cell.cached_ir().ok());
            if let Some(ir) = prior_ir {
                bindings.extend(collect_top_bindings(&ir, None));
            }
        }
        bindings.extend(collect_top_bindings(
            current_cell_ir,
            current_path.first().copied(),
        ));
        bindings
    }

    /// Submit the CAS subtree at `cas_path` in `cell_ir` to the simplifier,
    /// after substituting in-scope bindings. Stores `cas_path` on the
    /// pending entry so the response can splice into the cell's working IR
    /// at the same spot.
    fn submit_cas_at(&mut self, idx: usize, cell_ir: &Ir, cas_path: Vec<usize>) {
        let bindings = self.in_scope_bindings(idx, cell_ir, &cas_path);
        let cas_subtree = match cell_ir.get_at_path(&cas_path) {
            Some(node) => node.clone(),
            None => {
                self.set_runtime_error(
                    idx,
                    "internal error: CAS path didn't resolve in cell IR".to_string(),
                    &self.cells[idx].buffer.text().to_string(),
                );
                return;
            }
        };
        let submit_ir = cas_subtree.substitute_idents(&bindings);

        let cell_id = self.cells[idx].id;
        self.simplifier.submit(cell_id, &submit_ir);
        self.pending_simplify.insert(
            cell_id,
            PendingSimplify {
                purpose: SimplifyPurpose::InlineCas { path: cas_path },
                submitted_at: Instant::now(),
            },
        );
        self.cells[idx].outcome.message = Some(CellMessage::Pending);
    }

    fn handle_prints(&mut self, idx: usize, prints: &[usize], combined: &Ir) {
        let batch = PrintBatch {
            completed: Vec::new(),
            remaining: prints.to_vec(),
            current_is_equation: false,
            combined: combined.clone(),
        };
        self.advance_print_batch(idx, batch);
    }

    /// Drive a print batch as far as the synchronous interpreter can take
    /// it. For each remaining print: try the interpreter, and on success
    /// append the formatted value to `completed`. The first print the
    /// interpreter can't evaluate gets submitted to the symbolic
    /// simplifier; the batch (with the survivors of the queue) is parked
    /// on `pending_simplify` and the cell shows `Pending`. If the queue
    /// drains entirely, publish the joined output as `Computed`.
    fn advance_print_batch(&mut self, idx: usize, mut batch: PrintBatch) {
        while let Some(print_idx) = batch.remaining.first().copied() {
            batch.remaining.remove(0);
            let eval_ir = lang::build_print_ir(&batch.combined, print_idx);
            match interpreter::eval(&eval_ir, self.gpu.as_ref()) {
                Ok(val) => {
                    batch.completed.push(format!("{}", val));
                    // Track the most recent IR; on the all-sync path this
                    // ends up being the last print's IR.
                    self.cells[idx].outcome.program_ir = Some(eval_ir);
                }
                Err(_) => {
                    // Interpreter couldn't evaluate (e.g. expression
                    // references axis variable `x`). Fall through to
                    // symbolic simplification for this print, parking the
                    // remaining queue on the pending entry so the response
                    // handler can resume.
                    let arg = match action_arg(&batch.combined, print_idx, BuiltinOp::Print) {
                        Some(a) => a.clone(),
                        None => {
                            self.set_runtime_error(
                                idx,
                                "internal error: print arg not found in IR".to_string(),
                                "",
                            );
                            return;
                        }
                    };
                    let bindings = collect_top_bindings(&batch.combined, Some(print_idx));
                    let expanded = arg.substitute_idents(&bindings);
                    let (submit_ir, is_equation) = fold_equation(expanded);
                    batch.current_is_equation = is_equation;
                    let cell_id = self.cells[idx].id;
                    self.simplifier.submit(cell_id, &submit_ir);
                    self.pending_simplify.insert(
                        cell_id,
                        PendingSimplify {
                            purpose: SimplifyPurpose::Print(batch),
                            submitted_at: Instant::now(),
                        },
                    );
                    self.cells[idx].outcome.message = Some(CellMessage::Pending);
                    return;
                }
            }
        }

        // Queue drained — publish the joined results.
        let display = batch.completed.join("\n");
        self.cells[idx].outcome.message = Some(CellMessage::Computed(display));
    }

    fn handle_plots(
        &mut self,
        idx: usize,
        plot_indices: &[usize],
        combined: &Ir,
        snapshot: &str,
    ) {
        let mut shaders = Vec::with_capacity(plot_indices.len());
        let mut last_ir: Option<Ir> = None;
        for &plot_idx in plot_indices {
            let plot_ir = lang::build_plot_ir(combined, plot_idx);
            if let Err(e) = crate::lang::type_check(&plot_ir, snapshot) {
                self.set_runtime_error(idx, e, snapshot);
                return;
            }
            match crate::lang::wgsl_gen::generate(&plot_ir) {
                Ok(wgsl) => {
                    shaders.push(ShaderSpec { wgsl });
                    last_ir = Some(plot_ir);
                }
                Err(e) => {
                    self.set_runtime_error(idx, e, snapshot);
                    return;
                }
            }
        }
        self.cells[idx].outcome.shaders = shaders;
        self.cells[idx].outcome.program_ir = last_ir;
        self.cells[idx].state = CellState::Playing;
        self.cells[idx].last_played_text = Some(snapshot.to_string());
    }

    fn handle_simplify_response(
        &mut self,
        resp: crate::lang::symbolic::SimplifyResponse,
    ) -> Option<usize> {
        let pending = self.pending_simplify.remove(&resp.cell_id)?;
        let idx = self.cells.iter().position(|c| c.id == resp.cell_id)?;

        let result_ir = match resp.result {
            Ok(Some(ir)) => ir,
            Ok(None) => {
                self.cells[idx].outcome.message = None;
                return Some(idx);
            }
            Err(e) => {
                self.set_runtime_error(idx, e, &self.cells[idx].buffer.text().to_string());
                return Some(idx);
            }
        };

        match pending.purpose {
            SimplifyPurpose::Print(mut batch) => {
                let result = result_ir.to_source();
                let formatted = if batch.current_is_equation {
                    format!("{} = 0", result)
                } else {
                    result
                };
                batch.completed.push(formatted);
                batch.current_is_equation = false;
                // Continue with the remaining prints — any that the
                // interpreter can handle finish synchronously; the next
                // symbolic one re-parks the cell on `Pending`.
                self.advance_print_batch(idx, batch);
            }
            SimplifyPurpose::InlineCas { path } => {
                // Splice the simplified IR into the cell's working IR at
                // `path`. The working IR is the previous splice result if
                // present, else the parsed buffer text.
                let working_ir = self.cells[idx]
                    .effective_ir
                    .clone()
                    .or_else(|| self.cells[idx].cached_ir().ok());
                let working_ir = match working_ir {
                    Some(ir) => ir,
                    None => return Some(idx),
                };
                let new_ir = match working_ir.replace_at_path(&path, result_ir) {
                    Some(spliced) => spliced,
                    None => {
                        self.set_runtime_error(
                            idx,
                            "internal error: simplifier splice path no longer valid".to_string(),
                            &self.cells[idx].buffer.text().to_string(),
                        );
                        return Some(idx);
                    }
                };

                // Persist the spliced form. The `Simplified` message is a
                // pretty-printed display string derived from the IR; the IR
                // itself is the source of truth for downstream cells.
                self.cells[idx].outcome.message =
                    Some(CellMessage::Simplified(new_ir.to_source()));
                self.cells[idx].effective_ir = Some(new_ir.clone());

                // Surface unsolvable / special-function residuals before
                // attempting another iteration: a returned `int(...)` /
                // `df(...)` would otherwise drive an infinite resubmit loop.
                if let Some(op) = translate::detect_unevaluated_cas_ir(&new_ir) {
                    let msg = format!(
                        "This {} cannot be computed symbolically. \
                         Consider using a numerical method instead.",
                        op
                    );
                    self.set_runtime_error(idx, msg, &self.cells[idx].buffer.text().to_string());
                    return Some(idx);
                }
                let special = translate::detect_special_functions_ir(&new_ir);
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

                // Another CAS subtree to resolve?
                if let Some(next_path) = translate::find_cas_path(&new_ir) {
                    self.submit_cas_at(idx, &new_ir, next_path);
                    return Some(idx);
                }

                // All CAS resolved — re-enter the same dispatcher `run_cell`
                // uses so prints, plots, and the interpreter path all share
                // one resume point.
                let snapshot = self.cells[idx].buffer.text().to_string();
                self.dispatch(idx, &snapshot);
            }
        }
        Some(idx)
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
    /// pretty-printing the post-simplification IR when present, otherwise
    /// the buffer text. Used by the renderer to build the "user-visible"
    /// source string for line/col error reporting on shader-compile
    /// failures.
    pub fn combined_source(&self, cell_index: usize) -> String {
        let mut s = String::new();
        for (i, cell) in self.cells.iter().enumerate() {
            if i > cell_index {
                break;
            }
            if !s.is_empty() {
                s.push('\n');
            }
            if let Some(ir) = &cell.effective_ir {
                s.push_str(&ir.to_source());
            } else {
                s.push_str(cell.buffer.text());
            }
        }
        s
    }

    /// Flat list of `(cell_index, diagnostic)` across every cell.
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

