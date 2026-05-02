use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::editor::CodeCell;
use crate::lang::ast::AstNode;
use crate::lang::reduce::service::ReduceService;
use crate::notebook::{NoReduce, Notebook, SharedReduce};
use crate::ui::theme::Rgba;

/// Default plot color for new cells until per-cell coloring is exposed via
/// the UI. Catppuccin Mocha "blue" — readable on both dark and light bgs.
const DEFAULT_PLOT_COLOR: u32 = 0x89b4fa;

pub struct NotebookView {
    pub name: String,
    pub file_path: Option<PathBuf>,
    pub cells: Vec<CodeCell>,
    pub active_cell_index: usize,
    pub is_modified: bool,
    /// Math-space viewport bounds (xmin, ymin, xmax, ymax). Saved/restored
    /// on tab switch.
    pub axis_bounds: Option<[f32; 4]>,
    /// Headless engine that runs cells through parse → REDUCE → wgsl_gen →
    /// interpreter. The source of truth for cell IDs and outcomes; `cells`
    /// is a UI/buffer view kept in sync per text edit.
    pub notebook: Notebook,
}

impl NotebookView {
    /// Create an empty notebook view sharing `reduce` with the rest of the
    /// session. Pass `None` for `reduce` in offline contexts (tests, CLI)
    /// to fall back to a no-op REDUCE backend.
    pub fn new_untitled(name: String, reduce: Option<Rc<RefCell<ReduceService>>>) -> Self {
        let mut notebook = build_notebook(reduce);
        let nb_idx = notebook.add_cell("", Rgba::hex(DEFAULT_PLOT_COLOR));
        let id = notebook.cell(nb_idx).id;
        let cell = CodeCell::new(id);
        Self {
            name,
            file_path: None,
            cells: vec![cell],
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
        let nb_idx = notebook.add_cell(&contents, Rgba::hex(DEFAULT_PLOT_COLOR));
        let id = notebook.cell(nb_idx).id;
        let mut cell = CodeCell::new(id);
        cell.buffer.set_text(&contents);
        Ok(Self {
            name,
            file_path: Some(path.to_path_buf()),
            cells: vec![cell],
            active_cell_index: 0,
            is_modified: false,
            axis_bounds: None,
            notebook,
        })
    }

    pub fn active_cell(&self) -> &CodeCell {
        &self.cells[self.active_cell_index]
    }

    pub fn active_cell_mut(&mut self) -> &mut CodeCell {
        &mut self.cells[self.active_cell_index]
    }

    pub fn add_cell(&mut self) -> usize {
        let nb_idx = self.notebook.add_cell("", Rgba::hex(DEFAULT_PLOT_COLOR));
        let id = self.notebook.cell(nb_idx).id;
        let cell = CodeCell::new(id);
        self.cells.push(cell);
        let new_index = self.cells.len() - 1;
        self.active_cell_index = new_index;
        self.is_modified = true;
        new_index
    }

    pub fn remove_cell(&mut self, index: usize) {
        if self.cells.len() <= 1 || index >= self.cells.len() {
            return;
        }
        self.cells.remove(index);
        self.notebook.remove_cell(index);
        if self.active_cell_index >= self.cells.len() {
            self.active_cell_index = self.cells.len() - 1;
        } else if self.active_cell_index > index {
            self.active_cell_index -= 1;
        }
        self.is_modified = true;
    }

    /// Push every cell's current buffer text into the notebook. App writes
    /// directly to `cell.buffer` per keystroke; the notebook isn't told
    /// until something needs it (typically right before `notebook.play`).
    /// Step 3 will replace this with a reactive flow.
    pub fn sync_texts_to_notebook(&mut self) {
        for (i, cell) in self.cells.iter().enumerate() {
            self.notebook.set_text(i, cell.buffer.text());
        }
    }

    pub fn set_active_cell(&mut self, index: usize) {
        if index < self.cells.len() {
            self.active_cell_index = index;
        }
    }

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
        self.cells
            .iter()
            .map(|c| c.buffer.text())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Combine the parsed ASTs of cells `[0..=cell_index]` into a single Block.
    /// Reuses each cell's cached AST when its source hasn't changed.
    pub fn combined_ast_up_to(&self, cell_index: usize) -> Result<AstNode, String> {
        let mut all_stmts = Vec::new();
        for (i, cell) in self.cells.iter().enumerate() {
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
    fn new_untitled_mirrors_one_empty_cell_into_notebook() {
        let tab = NotebookView::new_untitled("scratch".into(), None);
        assert_eq!(tab.cells.len(), 1);
        assert_eq!(tab.notebook.len(), 1);
        assert_eq!(tab.notebook.cell(0).text, "");
    }

    #[test]
    fn add_cell_mirrors_into_notebook() {
        let mut tab = NotebookView::new_untitled("scratch".into(), None);
        tab.add_cell();
        assert_eq!(tab.cells.len(), 2);
        assert_eq!(tab.notebook.len(), 2);
    }

    #[test]
    fn remove_cell_mirrors_into_notebook() {
        let mut tab = NotebookView::new_untitled("scratch".into(), None);
        tab.add_cell();
        tab.add_cell();
        assert_eq!(tab.cells.len(), 3);
        assert_eq!(tab.notebook.len(), 3);
        tab.remove_cell(1);
        assert_eq!(tab.cells.len(), 2);
        assert_eq!(tab.notebook.len(), 2);
    }

    #[test]
    fn sync_texts_pushes_buffer_state_into_notebook() {
        let mut tab = NotebookView::new_untitled("scratch".into(), None);
        tab.cells[0].buffer.set_text("plot(y = sin(x))");
        // Before sync, the notebook's text is still empty.
        assert_eq!(tab.notebook.cell(0).text, "");
        tab.sync_texts_to_notebook();
        assert_eq!(tab.notebook.cell(0).text, "plot(y = sin(x))");
    }

    #[test]
    fn notebook_can_play_after_text_sync() {
        let mut tab = NotebookView::new_untitled("scratch".into(), None);
        tab.cells[0].buffer.set_text("plot(y = sin(x))");
        tab.sync_texts_to_notebook();
        tab.notebook.play(0);
        let cell = tab.notebook.cell(0);
        assert!(
            cell.outcome.shader.is_some(),
            "notebook play should produce a shader"
        );
    }
}
