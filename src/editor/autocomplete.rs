use crate::lang::ast::AstNode;
use crate::lang::lexer;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CandidateKind {
    Keyword,
    Builtin,
    Type,
    AxisVar,
    UserBinding,
    UserFunc,
}

impl CandidateKind {
    pub fn badge(&self) -> &'static str {
        match self {
            CandidateKind::Keyword => "kw",
            CandidateKind::Builtin => "fn",
            CandidateKind::Type => "ty",
            CandidateKind::AxisVar => "ax",
            CandidateKind::UserBinding => "var",
            CandidateKind::UserFunc => "fn",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub label: String,
    pub kind: CandidateKind,
}

pub struct AutocompleteState {
    pub active: bool,
    pub prefix: String,
    pub prefix_start: usize,
    pub candidates: Vec<Candidate>,
    pub selected_index: usize,
}

impl AutocompleteState {
    pub fn new() -> Self {
        Self {
            active: false,
            prefix: String::new(),
            prefix_start: 0,
            candidates: Vec::new(),
            selected_index: 0,
        }
    }

    pub fn update(&mut self, prefix: &str, prefix_start: usize, all_candidates: &[Candidate]) {
        let lower = prefix.to_lowercase();
        self.prefix = prefix.to_string();
        self.prefix_start = prefix_start;

        self.candidates = all_candidates
            .iter()
            .filter(|c| {
                let label_lower = c.label.to_lowercase();
                label_lower.starts_with(&lower) && label_lower != lower
            })
            .cloned()
            .collect();

        // Sort: exact prefix-case match first, then alphabetical
        self.candidates.sort_by(|a, b| a.label.cmp(&b.label));

        self.candidates.truncate(10);
        self.active = !self.candidates.is_empty();
        self.selected_index = 0;
    }

    pub fn accept(&self) -> Option<(&str, usize)> {
        if !self.active {
            return None;
        }
        self.candidates
            .get(self.selected_index)
            .map(|c| (c.label.as_str(), self.prefix_start))
    }

    pub fn select_prev(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        if self.selected_index == 0 {
            self.selected_index = self.candidates.len() - 1;
        } else {
            self.selected_index -= 1;
        }
    }

    pub fn select_next(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        self.selected_index = (self.selected_index + 1) % self.candidates.len();
    }

    pub fn dismiss(&mut self) {
        self.active = false;
        self.candidates.clear();
        self.selected_index = 0;
    }
}

/// Walk backwards from cursor to extract an identifier prefix.
/// Returns (prefix, start_byte_offset) or None if no prefix.
pub fn prefix_at_cursor(text: &str, cursor_byte: usize) -> Option<(&str, usize)> {
    if cursor_byte == 0 || cursor_byte > text.len() {
        return None;
    }
    let before = &text[..cursor_byte];
    let start = before
        .char_indices()
        .rev()
        .take_while(|&(_, c)| c.is_alphanumeric() || c == '_')
        .last()
        .map(|(i, _)| i)?;
    let prefix = &text[start..cursor_byte];
    if prefix.is_empty() {
        None
    } else {
        Some((prefix, start))
    }
}

/// Build the static candidate list from lexer constants + keywords.
pub fn static_candidates() -> Vec<Candidate> {
    let mut result = Vec::new();

    // Keywords
    for &kw in &["if", "else", "for", "while", "and", "or", "not", "true", "false"] {
        result.push(Candidate {
            label: kw.to_string(),
            kind: CandidateKind::Keyword,
        });
    }

    // Builtins
    for &name in lexer::BUILTINS {
        result.push(Candidate {
            label: name.to_string(),
            kind: CandidateKind::Builtin,
        });
    }

    // Type names
    for &(name, _) in lexer::TYPE_NAMES {
        result.push(Candidate {
            label: name.to_string(),
            kind: CandidateKind::Type,
        });
    }

    // Axis variables
    for &name in lexer::AXIS_VARS {
        result.push(Candidate {
            label: name.to_string(),
            kind: CandidateKind::AxisVar,
        });
    }

    result
}

/// Extract user-defined symbols from the AST of the current cell.
pub fn extract_user_symbols(ast: &AstNode) -> Vec<Candidate> {
    let mut result = Vec::new();
    walk_ast(ast, &mut result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefix_at_cursor() {
        assert_eq!(prefix_at_cursor("sin", 3), Some(("sin", 0)));
        assert_eq!(prefix_at_cursor("s", 1), Some(("s", 0)));
        assert_eq!(prefix_at_cursor("si", 2), Some(("si", 0)));
        assert_eq!(prefix_at_cursor("", 0), None);
        assert_eq!(prefix_at_cursor("a + s", 5), Some(("s", 4)));
        assert_eq!(prefix_at_cursor("(si", 3), Some(("si", 1)));
    }

    #[test]
    fn test_static_candidates_not_empty() {
        let c = static_candidates();
        assert!(!c.is_empty());
        assert!(c.iter().any(|c| c.label == "sin"));
        assert!(c.iter().any(|c| c.label == "cos"));
        assert!(c.iter().any(|c| c.label == "if"));
    }

    #[test]
    fn test_update_filters_by_prefix() {
        let mut state = AutocompleteState::new();
        let all = static_candidates();
        state.update("s", 0, &all);
        assert!(state.active);
        assert!(state.candidates.iter().any(|c| c.label == "sin"));
        assert!(state.candidates.iter().any(|c| c.label == "sqrt"));
        assert!(!state.candidates.iter().any(|c| c.label == "cos"));
    }

    #[test]
    fn test_exact_match_excluded() {
        let mut state = AutocompleteState::new();
        let all = static_candidates();
        state.update("sin", 0, &all);
        // "sin" exact match should be excluded, but "sinh" should still appear
        assert!(!state.candidates.iter().any(|c| c.label == "sin"));
        assert!(state.candidates.iter().any(|c| c.label == "sinh"));
    }

    #[test]
    fn test_accept() {
        let mut state = AutocompleteState::new();
        let all = static_candidates();
        state.update("sq", 0, &all);
        assert!(state.active);
        let (label, start) = state.accept().unwrap();
        assert_eq!(label, "sqrt");
        assert_eq!(start, 0);
    }

    #[test]
    fn test_full_flow_with_buffer() {
        use crate::editor::Buffer;

        let mut buf = Buffer::new();
        // Simulate typing 's'
        buf.insert('s');

        let text = buf.text();
        let cursor = buf.cursor_byte_offset();
        assert_eq!(text, "s");
        assert_eq!(cursor, 1);

        let (prefix, start) = prefix_at_cursor(text, cursor).unwrap();
        assert_eq!(prefix, "s");
        assert_eq!(start, 0);

        let all = static_candidates();
        let mut ac = AutocompleteState::new();
        ac.update(prefix, start, &all);
        assert!(ac.active, "autocomplete should be active after typing 's'");
        assert!(ac.candidates.len() > 0);

        // Accept completion
        let (label, prefix_start) = ac.accept().unwrap();
        buf.replace_range(prefix_start, cursor, label);
        assert!(buf.text().starts_with('s'), "should start with s: {}", buf.text());
    }
}

fn walk_ast(node: &AstNode, result: &mut Vec<Candidate>) {
    match node {
        AstNode::Binding { name, .. } => {
            if !result.iter().any(|c| c.label == *name) {
                result.push(Candidate {
                    label: name.clone(),
                    kind: CandidateKind::UserBinding,
                });
            }
        }
        AstNode::FunctionDef { name, .. } => {
            if !result.iter().any(|c| c.label == *name) {
                result.push(Candidate {
                    label: name.clone(),
                    kind: CandidateKind::UserFunc,
                });
            }
        }
        AstNode::TupleBinding { names, .. } => {
            for n in names {
                if !result.iter().any(|c| c.label == *n) {
                    result.push(Candidate {
                        label: n.clone(),
                        kind: CandidateKind::UserBinding,
                    });
                }
            }
        }
        AstNode::Block(stmts) => {
            for s in stmts {
                walk_ast(s, result);
            }
        }
        AstNode::IfExpr {
            then_branch,
            else_branch,
            ..
        } => {
            walk_ast(then_branch, result);
            if let Some(eb) = else_branch {
                walk_ast(eb, result);
            }
        }
        AstNode::ForLoop { body, .. } => {
            walk_ast(body, result);
        }
        AstNode::WhileLoop { body, .. } => {
            walk_ast(body, result);
        }
        _ => {}
    }
}
