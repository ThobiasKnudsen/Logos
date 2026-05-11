use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::lang::notebook_format::{parse_logos, serialize_logos};
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
        if !is_logos_path(path) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Logos only opens .logos files (got {:?})",
                    path.extension().and_then(|e| e.to_str()).unwrap_or("")
                ),
            ));
        }
        let contents = fs::read_to_string(path)?;
        let name = display_name_for_path(path);
        let mut notebook = build_notebook(reduce);
        let cells = parse_logos(&contents)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if cells.is_empty() {
            notebook.add_cell("");
        } else {
            for cell in cells {
                let seed = crate::notebook::alloc_color_seed();
                notebook.add_cell_with_color(&cell.content, seed, color_from_floats(cell.color));
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

    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .file_path
            .clone()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No file path set"))?;
        self.save_as(&path)
    }

    pub fn save_as(&mut self, path: &Path) -> io::Result<()> {
        let path = ensure_logos_extension(path);
        let cells = self.notebook.cells().iter().map(|c| {
            let [r, g, b, a] = c.plot_color.to_f32_array();
            (c.buffer.text().to_string(), [r, g, b, a])
        });
        let content = serialize_logos(cells);
        fs::write(&path, content)?;
        self.name = display_name_for_path(&path);
        self.file_path = Some(path);
        self.is_modified = false;
        Ok(())
    }

    pub fn mark_modified(&mut self) {
        self.is_modified = true;
    }
}

fn color_from_floats([r, g, b, a]: [f32; 4]) -> Rgba {
    Rgba {
        r: (r.clamp(0.0, 1.0) * 255.0).round() as u8,
        g: (g.clamp(0.0, 1.0) * 255.0).round() as u8,
        b: (b.clamp(0.0, 1.0) * 255.0).round() as u8,
        a: (a.clamp(0.0, 1.0) * 255.0).round() as u8,
    }
}

fn is_logos_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("logos"))
        .unwrap_or(false)
}

fn ensure_logos_extension(path: &Path) -> PathBuf {
    if is_logos_path(path) {
        path.to_path_buf()
    } else {
        path.with_extension("logos")
    }
}

/// Display name for a notebook file path: the file's stem when the extension
/// is `.logos` (the only on-disk format), otherwise the full file name. Keeps
/// tab labels and status text uncluttered without losing info for stray
/// non-`.logos` files.
pub(crate) fn display_name_for_path(path: &Path) -> String {
    let fallback = || {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Unknown".into())
    };
    if !is_logos_path(path) {
        return fallback();
    }
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(fallback)
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
    fn save_and_load_round_trip_preserves_text_and_color() {
        let dir = std::env::temp_dir().join(format!(
            "logos_test_roundtrip_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rt.logos");

        let mut view = NotebookView::new_untitled("rt".into(), None);
        let body = "r := sqrt(x*x + y*y)\nplot((r, r, r, 1.0))";
        view.cell_mut(0).buffer.set_text(body);
        view.cell_mut(0).plot_color = Rgba::new(245, 140, 168, 255);
        view.save_as(&path).expect("save succeeds");

        let loaded = NotebookView::from_file(&path, None).expect("load succeeds");
        assert_eq!(loaded.notebook.len(), 1);
        assert_eq!(loaded.cell(0).buffer.text(), body);
        let c = loaded.cell(0).plot_color;
        assert!(
            (c.r as i16 - 245).abs() <= 1
                && (c.g as i16 - 140).abs() <= 1
                && (c.b as i16 - 168).abs() <= 1
                && (c.a as i16 - 255).abs() <= 1,
            "color drift: {:?}",
            c
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_and_load_round_trip_multi_cell() {
        let dir = std::env::temp_dir().join(format!(
            "logos_test_multi_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("multi.logos");

        let mut view = NotebookView::new_untitled("multi".into(), None);
        view.cell_mut(0).buffer.set_text("plot(sin(x))");
        view.add_cell();
        view.cell_mut(1).buffer.set_text("plot(cos(x))");
        view.save_as(&path).expect("save succeeds");

        let loaded = NotebookView::from_file(&path, None).expect("load succeeds");
        assert_eq!(loaded.notebook.len(), 2);
        assert_eq!(loaded.cell(0).buffer.text(), "plot(sin(x))");
        assert_eq!(loaded.cell(1).buffer.text(), "plot(cos(x))");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_rejects_non_logos_extension() {
        let dir = std::env::temp_dir().join(format!(
            "logos_test_reject_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("foo.txt");
        std::fs::write(&path, "logos_version := 0.1\ncells := []\n").unwrap();
        let err = NotebookView::from_file(&path, None)
            .err()
            .expect("must reject .txt");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn shipped_examples_open_through_from_file() {
        // Hit the same code path the Examples menu uses — `from_file` on
        // every `.logos` file in `examples/` — and verify each ends up with
        // at least one cell of non-empty content. Iterates the directory so
        // newly-added examples are covered automatically.
        let examples_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        let entries = std::fs::read_dir(&examples_dir)
            .unwrap_or_else(|e| panic!("read_dir({}): {}", examples_dir.display(), e));
        let mut found = 0;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_none_or(|x| x != "logos") {
                continue;
            }
            found += 1;
            let view = NotebookView::from_file(&path, None)
                .unwrap_or_else(|e| panic!("from_file({}) failed: {}", path.display(), e));
            assert!(
                view.notebook.len() >= 1,
                "{}: expected at least one cell",
                path.display(),
            );
            assert!(
                !view.cell(0).buffer.text().is_empty(),
                "{}: cell 0 content is empty",
                path.display(),
            );
        }
        assert!(
            found > 0,
            "no `.logos` files found in {}",
            examples_dir.display(),
        );
    }

    #[test]
    fn save_as_appends_logos_extension_if_missing() {
        let dir = std::env::temp_dir().join(format!(
            "logos_test_ext_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("noext");
        let mut view = NotebookView::new_untitled("rt".into(), None);
        view.cell_mut(0).buffer.set_text("plot(x)");
        view.save_as(&target).expect("save succeeds");
        let actual = view.file_path.clone().unwrap();
        assert_eq!(actual, dir.join("noext.logos"));
        assert!(actual.exists(), "file with .logos extension should exist");
        std::fs::remove_dir_all(&dir).ok();
    }
}
