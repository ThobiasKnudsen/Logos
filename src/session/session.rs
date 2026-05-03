use std::cell::RefCell;
use std::io;
use std::path::Path;
use std::rc::Rc;

use crate::lang::interpreter::GpuDispatch;
use crate::lang::reduce::service::ReduceService;

use super::NotebookView;

/// Default name used for every notebook that has not been saved to disk.
/// All untitled tabs share this name; the chrome distinguishes them by
/// rendering it in italic.
pub const UNTITLED_NAME: &str = "Untitled";

/// Factory that produces a fresh GPU dispatcher per notebook. The closure
/// captures whatever GPU handles the renderer exposes (typically
/// `Arc<Device>` + `Arc<Queue>`); each call clones them into a new
/// dispatcher owned by one notebook.
pub type GpuFactory = Box<dyn Fn() -> Box<dyn GpuDispatch>>;

/// Top-level state container: the set of open notebooks (one per UI tab) and
/// which one is active. Holds clones of the shared REDUCE service handle and
/// the GPU dispatcher factory so each new `NotebookView` is wired up the
/// same way.
pub struct Session {
    pub tabs: Vec<NotebookView>,
    pub active_index: usize,
    reduce: Option<Rc<RefCell<ReduceService>>>,
    gpu_factory: Option<GpuFactory>,
}

impl Session {
    /// Build a session. Pass `None` for `reduce` in offline contexts; pass
    /// `None` for `gpu_factory` to keep `parallel for`/array cells on the
    /// CPU fallback.
    pub fn new(
        reduce: Option<Rc<RefCell<ReduceService>>>,
        gpu_factory: Option<GpuFactory>,
    ) -> Self {
        let mut first = NotebookView::new_untitled(UNTITLED_NAME.into(), reduce.clone());
        if let Some(f) = &gpu_factory {
            first.notebook.set_gpu(f());
        }
        Self {
            tabs: vec![first],
            active_index: 0,
            reduce,
            gpu_factory,
        }
    }

    pub fn new_tab(&mut self) -> usize {
        let mut view = NotebookView::new_untitled(UNTITLED_NAME.into(), self.reduce.clone());
        if let Some(f) = &self.gpu_factory {
            view.notebook.set_gpu(f());
        }
        self.tabs.push(view);
        self.active_index = self.tabs.len() - 1;
        self.active_index
    }

    pub fn open_file(&mut self, path: &Path) -> io::Result<usize> {
        let mut view = NotebookView::from_file(path, self.reduce.clone())?;
        if let Some(f) = &self.gpu_factory {
            view.notebook.set_gpu(f());
        }
        self.tabs.push(view);
        self.active_index = self.tabs.len() - 1;
        Ok(self.active_index)
    }

    pub fn active_tab(&self) -> &NotebookView {
        &self.tabs[self.active_index]
    }

    pub fn active_tab_mut(&mut self) -> &mut NotebookView {
        &mut self.tabs[self.active_index]
    }

    pub fn set_active(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_index = index;
        }
    }

    pub fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            let mut view = NotebookView::new_untitled(UNTITLED_NAME.into(), self.reduce.clone());
            if let Some(f) = &self.gpu_factory {
                view.notebook.set_gpu(f());
            }
            self.tabs.push(view);
            self.active_index = 0;
        } else if self.active_index >= self.tabs.len() {
            self.active_index = self.tabs.len() - 1;
        } else if self.active_index > index {
            self.active_index -= 1;
        }
    }
}
