use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::lang::reduce::service::ReduceService;
use crate::lang::symbolic::{NoSimplifier, SymbolicSimplifier};
use crate::notebook::{Notebook, NotebookCell, ReduceSimplifier, SharedReduce};
use crate::ui::theme::Rgba;

/// Process-global tab id counter. Used as a stable key for the renderer's
/// stashed shader pipelines so a tab's GPU state survives index shifts when
/// other tabs are closed.
static NEXT_TAB_ID: AtomicU64 = AtomicU64::new(1);

pub fn alloc_tab_id() -> u64 {
    NEXT_TAB_ID.fetch_add(1, Ordering::Relaxed)
}

/// On-disk JSON representation of one notebook cell. The notebook itself is
/// saved as a `Vec<CellRecord>` (a JSON array at the document root).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CellRecord {
    color: Rgba,
    content: String,
}

/// One open notebook in the UI: file metadata, viewport state, and the
/// headless `Notebook` engine that owns the cells. The renderer reads cells
/// directly off the engine via `cell()` / `cells()` accessors.
pub struct NotebookView {
    /// Stable per-tab identity, independent of position in `Session::tabs`.
    /// Used as the key for stashed GPU pipelines so closing tab N doesn't
    /// orphan tab N+1's stashed shaders.
    pub tab_id: u64,
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
        notebook.add_cell("");
        Self {
            tab_id: alloc_tab_id(),
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
        if is_plain_text_path(path) {
            notebook.add_cell(&contents);
        } else {
            let records: Vec<CellRecord> = serde_json::from_str(&contents)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if records.is_empty() {
                notebook.add_cell("");
            } else {
                for rec in records {
                    let seed = crate::notebook::alloc_color_seed();
                    notebook.add_cell_with_color(&rec.content, seed, rec.color);
                }
            }
        }
        for i in 0..notebook.len() {
            notebook.cell_mut(i).buffer.set_cursor_byte(0);
        }
        Ok(Self {
            tab_id: alloc_tab_id(),
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
        let new_index = self.notebook.add_cell("");
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

    /// Save the notebook as a JSON array of `{color, content}` objects.
    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .file_path
            .clone()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No file path set"))?;
        self.save_as(&path)
    }

    pub fn save_as(&mut self, path: &Path) -> io::Result<()> {
        let content = if is_plain_text_path(path) {
            self.serialize_text()
        } else {
            self.serialize_json()?
        };
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

    fn serialize_json(&self) -> io::Result<String> {
        let records: Vec<CellRecord> = self
            .notebook
            .cells()
            .iter()
            .map(|c| CellRecord {
                color: c.plot_color,
                content: c.buffer.text().to_string(),
            })
            .collect();
        serde_json::to_string_pretty(&records)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Plain-text serialization for single-cell `.txt` files (notebook
    /// examples). Multi-cell notebooks join cells with a blank line so
    /// nothing is silently dropped, but the JSON format is the round-trip
    /// path for those — this branch exists so editing an example and
    /// hitting Save preserves it as readable source.
    fn serialize_text(&self) -> String {
        let parts: Vec<String> = self
            .notebook
            .cells()
            .iter()
            .map(|c| c.buffer.text().to_string())
            .collect();
        parts.join("\n\n")
    }
}

fn is_plain_text_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("txt"))
        .unwrap_or(false)
}

/// Construct a `Notebook` wired to the shared REDUCE service if one is
/// provided, otherwise a `NoSimplifier` placeholder (for tests / CLI
/// contexts where no REDUCE worker is running).
fn build_notebook(reduce: Option<Rc<RefCell<ReduceService>>>) -> Notebook {
    let simplifier: Box<dyn SymbolicSimplifier> = match reduce {
        Some(rc) => Box::new(ReduceSimplifier::new(Box::new(SharedReduce::new(rc)))),
        None => Box::new(NoSimplifier),
    };
    Notebook::new(simplifier, None)
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
        assert!(!view.cell(0).outcome.shaders.is_empty());
    }

    #[test]
    fn save_as_txt_writes_plain_text_not_json() {
        let dir = std::env::temp_dir().join(format!(
            "logos_test_save_txt_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gradient.txt");
        let body = "plot((x, y, 0.5, 1.0))";
        let mut view = NotebookView::new_untitled("Gradient".into(), None);
        view.cell_mut(0).buffer.set_text(body);
        view.save_as(&path).expect("save succeeds");
        let written = std::fs::read_to_string(&path).expect("file readable");
        assert_eq!(written, body, "txt save must round-trip cell text");
        assert!(!written.starts_with('['), "must not be JSON");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_file_loads_txt_as_single_cell() {
        let dir = std::env::temp_dir().join(format!(
            "logos_test_load_txt_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("example.txt");
        let body = "plot((x, y, 0.5, 1.0))\n";
        std::fs::write(&path, body).unwrap();
        let view = NotebookView::from_file(&path, None).expect("loads");
        assert_eq!(view.notebook.len(), 1, "txt loads as exactly one cell");
        assert_eq!(view.cell(0).buffer.text(), body);
        assert_eq!(view.file_path.as_deref(), Some(path.as_path()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_file_then_save_round_trips_txt() {
        let dir = std::env::temp_dir().join(format!(
            "logos_test_roundtrip_txt_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("example.txt");
        let body = "nx := (x - x.min) / (x.max - x.min)\nplot((nx, 0.0, 0.0, 1.0))";
        std::fs::write(&path, body).unwrap();
        let mut view = NotebookView::from_file(&path, None).expect("loads");
        view.save().expect("save uses stored path");
        let written = std::fs::read_to_string(&path).expect("file readable");
        assert_eq!(written, body, "txt must round-trip without reformatting");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_as_json_writes_json() {
        let dir = std::env::temp_dir().join(format!(
            "logos_test_save_json_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notebook.json");
        let mut view = NotebookView::new_untitled("Untitled".into(), None);
        view.cell_mut(0).buffer.set_text("plot(y = x)");
        view.save_as(&path).expect("save succeeds");
        let written = std::fs::read_to_string(&path).expect("file readable");
        assert!(
            written.trim_start().starts_with('['),
            "non-txt extension must serialize as JSON; got:\n{}",
            written
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
