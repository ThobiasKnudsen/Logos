use super::Buffer;

#[derive(Debug, Clone)]
pub enum CellOutput {
    None,
    Simplified(String),
    Computed(String),
    Error(String),
}

pub struct CodeCell {
    pub id: usize,
    pub buffer: Buffer,
    pub is_playing: bool,
    pub output: CellOutput,
    pub output_collapsed: bool,
    /// User-set contracted height for the editor area (None = auto-fit to content).
    pub contracted_editor_h: Option<f32>,
}

impl CodeCell {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            buffer: Buffer::new(),
            is_playing: false,
            output: CellOutput::None,
            output_collapsed: false,
            contracted_editor_h: None,
        }
    }
}
