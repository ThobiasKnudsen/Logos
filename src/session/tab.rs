use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::editor::CodeCell;

pub struct Tab {
    pub name: String,
    pub file_path: Option<PathBuf>,
    pub cells: Vec<CodeCell>,
    pub active_cell_index: usize,
    next_cell_id: usize,
    pub is_modified: bool,
}

impl Tab {
    pub fn new_untitled(name: String) -> Self {
        let cell = CodeCell::new(0);
        Self {
            name,
            file_path: None,
            cells: vec![cell],
            active_cell_index: 0,
            next_cell_id: 1,
            is_modified: false,
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
        Ok(Self {
            name,
            file_path: Some(path.to_path_buf()),
            cells: vec![cell],
            active_cell_index: 0,
            next_cell_id: 1,
            is_modified: false,
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
        if self.active_cell_index >= self.cells.len() {
            self.active_cell_index = self.cells.len() - 1;
        } else if self.active_cell_index > index {
            self.active_cell_index -= 1;
            }
        self.is_modified = true;
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
}
