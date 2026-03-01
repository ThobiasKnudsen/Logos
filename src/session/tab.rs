use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::editor::CodeCell;

#[derive(Serialize, Deserialize)]
struct CellJson {
    code: String,
    color: Option<String>,
}

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

        let cell_entries: Vec<CellJson> = serde_json::from_str(&contents)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let mut cells = Vec::with_capacity(cell_entries.len().max(1));
        for (i, entry) in cell_entries.into_iter().enumerate() {
            let mut cell = CodeCell::new(i);
            cell.buffer.set_text(&entry.code);
            cell.color = entry.color;
            cells.push(cell);
        }

        // Ensure at least one cell exists
        if cells.is_empty() {
            cells.push(CodeCell::new(0));
        }

        let next_id = cells.len();

        Ok(Self {
            name,
            file_path: Some(path.to_path_buf()),
            cells,
            active_cell_index: 0,
            next_cell_id: next_id,
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
        } else if self.active_cell_index == index && self.active_cell_index >= self.cells.len() {
            self.active_cell_index = self.cells.len() - 1;
        }
        self.is_modified = true;
    }

    pub fn set_active_cell(&mut self, index: usize) {
        if index < self.cells.len() {
            self.active_cell_index = index;
        }
    }

    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .file_path
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No file path set"))?
            .clone();
        let json = self.to_json()?;
        fs::write(&path, json)?;
        self.is_modified = false;
        Ok(())
    }

    pub fn save_as(&mut self, path: &Path) -> io::Result<()> {
        let json = self.to_json()?;
        fs::write(path, json)?;
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

    fn to_json(&self) -> io::Result<String> {
        let entries: Vec<CellJson> = self
            .cells
            .iter()
            .map(|c| CellJson {
                code: c.buffer.text().to_string(),
                color: c.color.clone(),
            })
            .collect();
        serde_json::to_string_pretty(&entries)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}
