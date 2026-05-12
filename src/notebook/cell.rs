//! A single notebook cell — the smallest unit of executable user input.
//!
//! Holds an editable text buffer, a plot color, the most recent run's
//! outcome, and a few pieces of cell-scoped UI state (output collapse,
//! contracted editor height) that the renderer reads back to decide layout.
//! Window-level UI state (active tab index, scroll offsets) lives on
//! `NotebookView` instead.

use std::cell::RefCell;
use std::time::Instant;

use crate::editor::Buffer;
use crate::lang::ir::Ir;
use crate::lang::highlight::HighlightSpan;
use crate::ui::theme::Rgba;

use super::diagnostic::Diagnostic;
use super::shader::ShaderSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellState {
    /// Never played in this session (or freshly cleared).
    Idle,
    /// Most recent play succeeded; the cell is "live" and the renderer is
    /// using its shader (if any).
    Playing,
    /// Was playing, then explicitly stopped.
    Stopped,
}

/// What to display at the bottom of the cell. A single field so the UI has
/// one obvious thing to render — no decision logic about which of three
/// fields to show.
#[derive(Debug, Clone)]
pub enum CellMessage {
    /// Awaiting an async backend (e.g. REDUCE). UI shows "…".
    Pending,
    /// Concrete value (interpreter result, formatted print value).
    Computed(String),
    /// REDUCE-simplified expression (substituted back into the working text).
    Simplified(String),
    /// User-facing error (single string; full structured diagnostics live in
    /// `CellOutcome::diagnostics`).
    Error(String),
}

impl CellMessage {
    /// Text to render at the bottom of the cell. `Pending` shows the
    /// ellipsis placeholder; the others return their stored string.
    pub fn display_text(&self) -> &str {
        match self {
            CellMessage::Pending => "\u{2026}",
            CellMessage::Computed(s)
            | CellMessage::Simplified(s)
            | CellMessage::Error(s) => s.as_str(),
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, CellMessage::Error(_))
    }
}

/// Everything the last play produced for this cell. None of these fields
/// reach for the GPU — the renderer reads `shaders[*].wgsl` and the printer
/// reads `message`.
#[derive(Debug, Clone, Default)]
pub struct CellOutcome {
    pub message: Option<CellMessage>,
    /// One shader per `plot(...)` call in the cell, in source order.
    /// Empty when the cell produced no plots.
    pub shaders: Vec<ShaderSpec>,
    /// Logos IR for the program this cell evaluates. Populated on every
    /// successful run (print path, plot path, interpreter path, pure-expr
    /// path, post-simplify path). Consumers: the planned cranelift CPU JIT;
    /// also useful for tooling that wants the structured form of the cell.
    pub program_ir: Option<Ir>,
    /// Per-token color spans for syntax highlighting. Recomputed on every
    /// successful play and kept here so the UI doesn't need to re-lex.
    pub token_colors: Vec<HighlightSpan>,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct NotebookCell {
    pub id: usize,
    /// Editable source text + cursor + selection. The renderer reads from
    /// here directly; mutations from keystrokes / paste go straight in
    /// (no separate mirror to keep in sync).
    pub buffer: Buffer,
    /// Color used by every plot this cell produces. The cell-color button
    /// cycles by stepping `color_seed`; `plot_color` is the cached result of
    /// `color_from_seed(color_seed)` so JSON serialization can save the
    /// concrete color directly.
    pub plot_color: Rgba,
    /// Source-of-truth for the plot color. Click forward → `+= 1`, click
    /// back → `-= 1`. New cells take a fresh seed from a process-global
    /// counter so back-to-back cells differ.
    pub color_seed: u32,
    pub state: CellState,
    pub outcome: CellOutcome,
    /// Whether the cell's output area is collapsed in the UI.
    pub output_collapsed: bool,
    /// Custom (smaller) editor height set by the user dragging the cell's
    /// bottom edge. `None` means "use the natural editor height for this
    /// cell's text".
    pub contracted_editor_h: Option<f32>,
    /// `Some(t)` after a successful play; `t` is the text snapshot at that
    /// moment. Used to detect "needs replay" — UI compares against current
    /// `buffer.text()`.
    pub last_played_text: Option<String>,
    /// Most recent edit timestamp, set by the app's keystroke handler and
    /// cleared by the auto-rerun pass once the cell has been re-played.
    /// Drives the "edit, then 200ms of quiet, then auto-replay" UX.
    pub last_edit_at: Option<Instant>,
    pub(super) ir_cache: RefCell<Option<(String, Ir)>>,
    /// Post-simplification IR for this cell. Set by the iterative-CAS path
    /// after the symbolic simplifier returns IR that's been spliced into
    /// the original cell IR. `Notebook::combined_ir_up_to` consults this
    /// before falling back to `cached_ir` so downstream cells see the
    /// resolved form, not the raw `int(...)`/`df(...)` calls. Cleared on
    /// the next `play()` of this cell (via the `outcome` reset).
    pub effective_ir: Option<Ir>,
}

impl NotebookCell {
    pub(super) fn new(id: usize, text: &str, color_seed: u32, plot_color: Rgba) -> Self {
        let mut buffer = Buffer::new();
        buffer.set_text(text);
        Self {
            id,
            buffer,
            plot_color,
            color_seed,
            state: CellState::Idle,
            outcome: CellOutcome::default(),
            output_collapsed: false,
            contracted_editor_h: None,
            last_played_text: None,
            last_edit_at: None,
            ir_cache: RefCell::new(None),
            effective_ir: None,
        }
    }

    /// Advance the color seed and recompute `plot_color`. Positive `delta`
    /// goes forward through the deterministic sequence; negative goes back.
    /// Wraps around `u32`.
    pub fn step_color(&mut self, delta: i32) {
        self.color_seed = self.color_seed.wrapping_add(delta as u32);
        self.plot_color = super::color_from_seed(self.color_seed);
    }

    /// True iff the cell has been played and its text has changed since.
    /// Idle cells are never stale; the auto-rerun pass watches this.
    pub fn is_stale(&self) -> bool {
        match &self.last_played_text {
            Some(t) => t.as_str() != self.buffer.text(),
            None => false,
        }
    }

    /// Parse and cache the IR for this cell's *raw* source. Reuses the
    /// cached IR when the buffer text hasn't changed since the last call.
    /// Note: when an iterative-CAS pass produces a `Simplified` message,
    /// `Notebook::combined_ir_up_to` parses that text directly instead of
    /// going through `cached_ir` — the cache here is for the editable form.
    pub fn cached_ir(&self) -> Result<Ir, String> {
        let text = self.buffer.text();
        {
            let cache = self.ir_cache.borrow();
            if let Some((cached_text, ir)) = cache.as_ref() {
                if cached_text.as_str() == text {
                    return Ok(ir.clone());
                }
            }
        }
        let parsed = crate::lang::parse(text)?;
        *self.ir_cache.borrow_mut() = Some((text.to_string(), parsed.clone()));
        Ok(parsed)
    }

    pub(super) fn invalidate_ir(&mut self) {
        *self.ir_cache.borrow_mut() = None;
        // Edits invalidate the post-simplification form too — the user is
        // typing on top of a simplified cell, so the splice no longer
        // applies to the new text.
        self.effective_ir = None;
    }
}
