use super::Buffer;

pub struct CodeCell {
    pub id: usize,
    pub buffer: Buffer,
}

impl CodeCell {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            buffer: Buffer::new(),
        }
    }
}
