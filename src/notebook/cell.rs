//! A single notebook cell — the smallest unit of executable user input.
//!
//! Holds an editable text buffer, a plot color, the most recent run's
//! outcome, and a few pieces of cell-scoped UI state (output collapse,
//! contracted editor height) that the renderer reads back to decide layout.
//! Window-level UI state (active tab index, scroll offsets) lives on
//! `NotebookView` instead.

use std::cell::RefCell;

use crate::editor::Buffer;
use crate::lang::ast::AstNode;
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
    /// CPU-side program (currently the AST itself; future home for JIT IR).
    pub cpu_program: Option<AstNode>,
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
    pub(super) ast_cache: RefCell<Option<(String, AstNode)>>,
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
            ast_cache: RefCell::new(None),
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
    /// Idle cells are never stale; a stale cell shows the replay affordance.
    /// (UI replay-button binding is future work; tests cover this today.)
    #[allow(dead_code)]
    pub fn is_stale(&self) -> bool {
        match &self.last_played_text {
            Some(t) => t.as_str() != self.buffer.text(),
            None => false,
        }
    }

    /// Parse and cache the AST for this cell's *raw* source. Reuses the
    /// cached AST when the buffer text hasn't changed since the last call.
    /// Note: when an iterative-CAS pass produces a `Simplified` message,
    /// `Notebook::combined_ast_up_to` parses that text directly instead of
    /// going through `cached_ast` — the cache here is for the editable form.
    pub fn cached_ast(&self) -> Result<AstNode, String> {
        let text = self.buffer.text();
        {
            let cache = self.ast_cache.borrow();
            if let Some((cached_text, ast)) = cache.as_ref() {
                if cached_text.as_str() == text {
                    return Ok(ast.clone());
                }
            }
        }
        let parsed = crate::lang::parse(text)?;
        *self.ast_cache.borrow_mut() = Some((text.to_string(), parsed.clone()));
        Ok(parsed)
    }

    pub(super) fn invalidate_ast(&mut self) {
        *self.ast_cache.borrow_mut() = None;
    }
}
