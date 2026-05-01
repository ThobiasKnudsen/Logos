use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::editor::CodeCell;
use crate::lang::ast::AstNode;
use crate::notebook::{NoReduce, Notebook};
use crate::ui::theme::Rgba;

/// Default plot color for new cells until per-cell coloring is exposed via
/// the UI. Catppuccin Mocha "blue" — readable on both dark and light bgs.
const DEFAULT_PLOT_COLOR: u32 = 0x89b4fa;

pub struct Tab {
    pub name: String,
    pub file_path: Option<PathBuf>,
    pub cells: Vec<CodeCell>,
    pub active_cell_index: usize,
    next_cell_id: usize,
    pub is_modified: bool,
    /// Per-tab axis bounds (saved/restored on tab switch). UI viewport state;
    /// will move to `NotebookView` in step 3.
    pub axis_bounds: Option<[f32; 4]>,
    /// Headless backend that mirrors `cells` for cell text and structure.
    /// Step 2 keeps this in sync via `add_cell` / `remove_cell` /
    /// `sync_texts_to_notebook`. Step 3 flips the source of truth so
    /// `cells` becomes a derived view (or goes away entirely) and the
    /// real `ReduceServiceBackend` from `App` is wired in.
    pub notebook: Notebook,
}

impl Tab {
    pub fn new_untitled(name: String) -> Self {
        let cell = CodeCell::new(0);
        let mut notebook = empty_notebook();
        notebook.add_cell("", Rgba::hex(DEFAULT_PLOT_COLOR));
        Self {
            name,
            file_path: None,
            cells: vec![cell],
            active_cell_index: 0,
            next_cell_id: 1,
            is_modified: false,
            axis_bounds: None,
            notebook,
        }
    }

    pub fn from_file(path: &Path) -> io::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Unknown".into());
        let mut cell = CodeCell::new(0);
        cell.buffer.set_text(&contents);
        let mut notebook = empty_notebook();
        notebook.add_cell(&contents, Rgba::hex(DEFAULT_PLOT_COLOR));
        Ok(Self {
            name,
            file_path: Some(path.to_path_buf()),
            cells: vec![cell],
            active_cell_index: 0,
            next_cell_id: 1,
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
        let id = self.next_cell_id;
        self.next_cell_id += 1;
        let cell = CodeCell::new(id);
        self.cells.push(cell);
        // Mirror in the notebook with the same default color.
        self.notebook.add_cell("", Rgba::hex(DEFAULT_PLOT_COLOR));
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

/// Construct a `Notebook` with no cells, no REDUCE wired, and the default
/// CPU dispatcher. Used by `Tab` constructors during the step-2 transition;
/// step 3 replaces this with a shared `ReduceServiceBackend` from `App`.
fn empty_notebook() -> Notebook {
    Notebook::new(Box::new(NoReduce), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_untitled_mirrors_one_empty_cell_into_notebook() {
        let tab = Tab::new_untitled("scratch".into());
        assert_eq!(tab.cells.len(), 1);
        assert_eq!(tab.notebook.len(), 1);
        assert_eq!(tab.notebook.cell(0).text, "");
    }

    #[test]
    fn add_cell_mirrors_into_notebook() {
        let mut tab = Tab::new_untitled("scratch".into());
        tab.add_cell();
        assert_eq!(tab.cells.len(), 2);
        assert_eq!(tab.notebook.len(), 2);
    }

    #[test]
    fn remove_cell_mirrors_into_notebook() {
        let mut tab = Tab::new_untitled("scratch".into());
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
        let mut tab = Tab::new_untitled("scratch".into());
        tab.cells[0].buffer.set_text("plot(y = sin(x))");
        // Before sync, the notebook's text is still empty.
        assert_eq!(tab.notebook.cell(0).text, "");
        tab.sync_texts_to_notebook();
        assert_eq!(tab.notebook.cell(0).text, "plot(y = sin(x))");
    }

    #[test]
    fn notebook_can_play_after_text_sync() {
        let mut tab = Tab::new_untitled("scratch".into());
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
