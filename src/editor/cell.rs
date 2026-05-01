use std::borrow::Cow;
use std::cell::RefCell;

use crate::lang::ast::AstNode;

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
    pub contracted_editor_h: Option<f32>,
    pub print_pending: bool,
    pub print_is_equation: bool,
    ast_cache: RefCell<Option<(String, AstNode)>>,
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
            print_pending: false,
            print_is_equation: false,
            ast_cache: RefCell::new(None),
        }
    }

    /// Source text used for parsing — either the simplified output (after CAS
    /// substitution) or the raw buffer text.
    pub fn effective_source(&self) -> Cow<'_, str> {
        match &self.output {
            CellOutput::Simplified(s) => Cow::Borrowed(s.as_str()),
            _ => Cow::Borrowed(self.buffer.text()),
        }
    }

    /// Parsed AST for this cell. Reuses the cached AST when the effective
    /// source hasn't changed; otherwise re-parses and updates the cache.
    pub fn cached_ast(&self) -> Result<AstNode, String> {
        let text = self.effective_source();
        {
            let cache = self.ast_cache.borrow();
            if let Some((cached_text, ast)) = cache.as_ref() {
                if cached_text.as_str() == text.as_ref() {
                    return Ok(ast.clone());
                }
            }
        }
        let parsed = crate::lang::parse(text.as_ref())?;
        *self.ast_cache.borrow_mut() = Some((text.into_owned(), parsed.clone()));
        Ok(parsed)
    }
}
