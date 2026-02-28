use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::editor::Buffer;

pub struct Tab {
    pub name: String,
    pub file_path: Option<PathBuf>,
    pub buffer: Buffer,
    pub is_modified: bool,
}

impl Tab {
    pub fn new_untitled(name: String) -> Self {
        Self {
            name,
            file_path: None,
            buffer: Buffer::new(),
            is_modified: false,
        }
    }

    pub fn from_file(path: &Path) -> io::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Unknown".into());
        let mut buffer = Buffer::new();
        buffer.set_text(&contents);
        Ok(Self {
            name,
            file_path: Some(path.to_path_buf()),
            buffer,
            is_modified: false,
        })
    }

    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .file_path
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No file path set"))?;
        fs::write(path, self.buffer.text())?;
        self.is_modified = false;
        Ok(())
    }

    pub fn save_as(&mut self, path: &Path) -> io::Result<()> {
        fs::write(path, self.buffer.text())?;
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
}
