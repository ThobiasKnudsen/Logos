mod notebook_view;
mod session;

pub use notebook_view::NotebookView;
pub(crate) use notebook_view::display_name_for_path;
pub use session::{GpuFactory, Session};
