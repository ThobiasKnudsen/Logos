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
                DialogKind::Open => rfd::FileDialog::new().set_title("Open File").pick_file(),
                DialogKind::Save => rfd::FileDialog::new().set_title("Save File").save_file(),
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
