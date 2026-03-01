use super::Buffer;

#[derive(Debug, Clone)]
pub enum CellOutput {
    None,
    Error(String),
}

pub struct CodeCell {
    pub id: usize,
    pub buffer: Buffer,
    pub is_playing: bool,
    pub output: CellOutput,
    pub color: Option<String>,
}

impl CodeCell {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            buffer: Buffer::new(),
            is_playing: false,
            output: CellOutput::None,
            color: None,
        }
    }
}
