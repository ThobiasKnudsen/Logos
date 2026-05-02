use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::lang::ast::AstNode;
use crate::lang::reduce::service::ReduceService;
use crate::notebook::{NoReduce, Notebook, NotebookCell, SharedReduce};
use crate::ui::theme::Rgba;

/// Default plot color for new cells until per-cell coloring is exposed via
/// the UI. Catppuccin Mocha "blue" — readable on both dark and light bgs.
const DEFAULT_PLOT_COLOR: u32 = 0x89b4fa;

/// One open notebook in the UI: file metadata, viewport state, and the
/// headless `Notebook` engine that owns the cells. The renderer reads cells
/// directly off the engine via `cell()` / `cells()` accessors.
pub struct NotebookView {
    pub name: String,
    pub file_path: Option<PathBuf>,
    pub active_cell_index: usize,
    pub is_modified: bool,
    /// Math-space viewport bounds (xmin, ymin, xmax, ymax). Saved/restored
    /// on tab switch.
    pub axis_bounds: Option<[f32; 4]>,
    pub notebook: Notebook,
}

impl NotebookView {
    /// Create an empty notebook view sharing `reduce` with the rest of the
    /// session. Pass `None` for `reduce` in offline contexts (tests, CLI)
    /// to fall back to a no-op REDUCE backend.
    pub fn new_untitled(name: String, reduce: Option<Rc<RefCell<ReduceService>>>) -> Self {
        let mut notebook = build_notebook(reduce);
        notebook.add_cell("", Rgba::hex(DEFAULT_PLOT_COLOR));
        Self {
            name,
            file_path: None,
            active_cell_index: 0,
            is_modified: false,
            axis_bounds: None,
            notebook,
        }
    }

    pub fn from_file(path: &Path, reduce: Option<Rc<RefCell<ReduceService>>>) -> io::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Unknown".into());
        let mut notebook = build_notebook(reduce);
        notebook.add_cell(&contents, Rgba::hex(DEFAULT_PLOT_COLOR));
        Ok(Self {
            name,
            file_path: Some(path.to_path_buf()),
            active_cell_index: 0,
            is_modified: false,
            axis_bounds: None,
            notebook,
        })
    }

    // ─── cell access (delegates to the notebook) ───────────────────────────

    pub fn cells(&self) -> &[NotebookCell] {
        self.notebook.cells()
    }

    pub fn cell(&self, idx: usize) -> &NotebookCell {
        self.notebook.cell(idx)
    }

    pub fn cell_mut(&mut self, idx: usize) -> &mut NotebookCell {
        self.notebook.cell_mut(idx)
    }

    pub fn active_cell(&self) -> &NotebookCell {
        self.notebook.cell(self.active_cell_index)
    }

    pub fn active_cell_mut(&mut self) -> &mut NotebookCell {
        self.notebook.cell_mut(self.active_cell_index)
    }

    pub fn add_cell(&mut self) -> usize {
        let new_index = self.notebook.add_cell("", Rgba::hex(DEFAULT_PLOT_COLOR));
        self.active_cell_index = new_index;
        self.is_modified = true;
        new_index
    }

    pub fn remove_cell(&mut self, index: usize) {
        if self.notebook.len() <= 1 || index >= self.notebook.len() {
            return;
        }
        self.notebook.remove_cell(index);
        let len = self.notebook.len();
        if self.active_cell_index >= len {
            self.active_cell_index = len - 1;
        } else if self.active_cell_index > index {
            self.active_cell_index -= 1;
        }
        self.is_modified = true;
    }

    pub fn set_active_cell(&mut self, index: usize) {
        if index < self.notebook.len() {
            self.active_cell_index = index;
        }
    }

    // ─── file I/O ──────────────────────────────────────────────────────────

    /// Save: concatenate all cells with double newlines.
    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .file_path
            .clone()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No file path set"))?;
        self.save_as(&path)
    }

    pub fn save_as(&mut self, path: &Path) -> io::Result<()> {
        let content = self.concatenated_text();
        fs::write(path, content)?;
        self.name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Unknown".into());
        self.file_path = Some(path.to_path_buf());
        self.is_modified = false;
        Ok(())
    }

    pub fn mark_modified(&mut self) {
        self.is_modified = true;
    }

    fn concatenated_text(&self) -> String {
        self.notebook
            .cells()
            .iter()
            .map(|c| c.buffer.text())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Combine the parsed ASTs of cells `[0..=cell_index]` into a single
    /// Block. Reuses each cell's cached AST when its source hasn't changed.
    /// Used by the renderer/lang service for autocomplete-time analysis;
    /// `Notebook::play` builds the same combined AST internally.
    pub fn combined_ast_up_to(&self, cell_index: usize) -> Result<AstNode, String> {
        let mut all_stmts = Vec::new();
        for (i, cell) in self.notebook.cells().iter().enumerate() {
            if i > cell_index {
                break;
            }
            let cell_ast = cell
                .cached_ast()
                .map_err(|e| format!("Cell {}: {}", i + 1, e))?;
            match cell_ast {
                AstNode::Block(stmts) => all_stmts.extend(stmts),
                other => all_stmts.push(other),
            }
        }
        Ok(AstNode::Block(all_stmts))
    }
}

/// Construct a `Notebook` wired to the shared REDUCE service if one is
/// provided, otherwise a `NoReduce` placeholder (for tests / CLI contexts
/// where no REDUCE worker is running).
fn build_notebook(reduce: Option<Rc<RefCell<ReduceService>>>) -> Notebook {
    let backend: Box<dyn crate::notebook::ReduceBackend> = match reduce {
        Some(rc) => Box::new(SharedReduce::new(rc)),
        None => Box::new(NoReduce),
    };
    Notebook::new(backend, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_untitled_creates_one_empty_cell() {
        let view = NotebookView::new_untitled("scratch".into(), None);
        assert_eq!(view.notebook.len(), 1);
        assert_eq!(view.cell(0).buffer.text(), "");
    }

    #[test]
    fn add_cell_grows_notebook_and_advances_active_index() {
        let mut view = NotebookView::new_untitled("scratch".into(), None);
        view.add_cell();
        assert_eq!(view.notebook.len(), 2);
        assert_eq!(view.active_cell_index, 1);
    }

    #[test]
    fn remove_cell_shrinks_notebook_and_clamps_active_index() {
        let mut view = NotebookView::new_untitled("scratch".into(), None);
        view.add_cell();
        view.add_cell();
        view.set_active_cell(2);
        view.remove_cell(2);
        assert_eq!(view.notebook.len(), 2);
        assert_eq!(view.active_cell_index, 1);
    }

    #[test]
    fn buffer_writes_are_visible_to_notebook_play() {
        let mut view = NotebookView::new_untitled("scratch".into(), None);
        view.cell_mut(0).buffer.set_text("plot(y = sin(x))");
        view.notebook.play(0);
        assert!(view.cell(0).outcome.shader.is_some());
    }
}
