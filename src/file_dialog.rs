use std::path::PathBuf;
use std::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    Open,
    Save,
}

pub enum DialogResult {
    Pending,
    Selected(PathBuf),
    Cancelled,
}

pub struct FileDialog {
    pub kind: DialogKind,
    receiver: mpsc::Receiver<Option<PathBuf>>,
}

impl FileDialog {
    pub fn spawn(kind: DialogKind) -> Self {
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let result = match kind {
                DialogKind::Open => rfd::FileDialog::new()
                    .set_title("Open File")
                    .add_filter("Logos notebook", &["logos"])
                    .pick_file(),
                DialogKind::Save => rfd::FileDialog::new()
                    .set_title("Save File")
                    .add_filter("Logos notebook", &["logos"])
                    .set_file_name("untitled.logos")
                    .save_file(),
            };
            let _ = tx.send(result);
        });

        Self { kind, receiver: rx }
    }

    pub fn poll(&self) -> DialogResult {
        match self.receiver.try_recv() {
            Ok(Some(path)) => DialogResult::Selected(path),
            Ok(None) => DialogResult::Cancelled,
            Err(mpsc::TryRecvError::Empty) => DialogResult::Pending,
            Err(mpsc::TryRecvError::Disconnected) => DialogResult::Cancelled,
        }
    }
}

/// Show a modal error dialog. Spawned on a background thread so it doesn't
/// block the winit event loop — the user can dismiss it whenever, the app
/// keeps responding in the meantime.
pub fn show_error(title: &str, message: &str) {
    let title = title.to_string();
    let message = message.to_string();
    std::thread::spawn(move || {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title(&title)
            .set_description(&message)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    });
}
