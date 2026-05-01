//! A single notebook cell — the smallest unit of executable user input.
//!
//! Holds source text, a plot color, and the most recent run's outcome. All
//! editor concerns (cursor, selection, scroll, layout) live in the
//! `NotebookView` UI shell, not here — this type is pure backend state.

use std::cell::RefCell;

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

/// Everything the last play produced for this cell. None of these fields
/// reach for the GPU — the renderer reads `shader.wgsl` and the printer
/// reads `message`.
#[derive(Debug, Clone, Default)]
pub struct CellOutcome {
    pub message: Option<CellMessage>,
    pub shader: Option<ShaderSpec>,
    /// CPU-side program (currently the AST itself; future home for JIT IR).
    pub cpu_program: Option<AstNode>,
    /// Per-token color spans for syntax highlighting. Recomputed on every
    /// successful play and kept here so the UI doesn't need to re-lex.
    pub token_colors: Vec<HighlightSpan>,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct NotebookCell {
    pub id: usize,
    pub text: String,
    /// Color for non-RGBA plots produced by this cell. Mutable via
    /// `Notebook::set_plot_color`.
    pub plot_color: Rgba,
    pub state: CellState,
    pub outcome: CellOutcome,
    /// `Some(t)` after a successful play; `t` is the text snapshot at that
    /// moment. Used to detect "needs replay" — UI compares against `text`.
    pub last_played_text: Option<String>,
    pub(super) ast_cache: RefCell<Option<(String, AstNode)>>,
}

impl NotebookCell {
    pub(super) fn new(id: usize, text: &str, plot_color: Rgba) -> Self {
        Self {
            id,
            text: text.to_string(),
            plot_color,
            state: CellState::Idle,
            outcome: CellOutcome::default(),
            last_played_text: None,
            ast_cache: RefCell::new(None),
        }
    }

    /// True iff the cell has been played and its text has changed since.
    /// Idle cells are never stale; a stale cell shows the replay affordance.
    pub fn is_stale(&self) -> bool {
        match &self.last_played_text {
            Some(t) => t != &self.text,
            None => false,
        }
    }

    /// Parse and cache the AST for this cell's source. Reuses the cached AST
    /// when the text hasn't changed since the last call.
    pub(super) fn cached_ast(&self) -> Result<AstNode, String> {
        {
            let cache = self.ast_cache.borrow();
            if let Some((cached_text, ast)) = cache.as_ref() {
                if cached_text == &self.text {
                    return Ok(ast.clone());
                }
            }
        }
        let parsed = crate::lang::parse(&self.text)?;
        *self.ast_cache.borrow_mut() = Some((self.text.clone(), parsed.clone()));
        Ok(parsed)
    }

    pub(super) fn invalidate_ast(&mut self) {
        *self.ast_cache.borrow_mut() = None;
    }
}
