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
    Symbol,
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
            CandidateKind::Symbol => "sym",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub label: String,
    pub kind: CandidateKind,
    /// LaTeX command for prefix matching (e.g. `\pi`). If set, filtering
    /// uses this instead of `label`.
    pub filter_key: Option<String>,
    /// Display text for popup (e.g. `\pi  π`). If set, shown instead of `label`.
    pub display: Option<String>,
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
                // Use filter_key for matching if available (LaTeX symbols),
                // otherwise match on label.
                let key = c.filter_key.as_deref().unwrap_or(&c.label);
                let key_lower = key.to_lowercase();
                key_lower.starts_with(&lower) && key_lower != lower
            })
            .cloned()
            .collect();

        // Sort: exact prefix-case match first, then alphabetical
        self.candidates.sort_by(|a, b| {
            let ka = a.filter_key.as_deref().unwrap_or(&a.label);
            let kb = b.filter_key.as_deref().unwrap_or(&b.label);
            ka.cmp(kb)
        });

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
///
/// Handles LaTeX prefixes: walks back through alphanumeric/underscore chars,
/// then optionally includes a leading `\` so that `\pi` is a single prefix.
pub fn prefix_at_cursor(text: &str, cursor_byte: usize) -> Option<(&str, usize)> {
    if cursor_byte == 0 || cursor_byte > text.len() {
        return None;
    }
    let before = &text[..cursor_byte];

    // Walk back through alphanumeric / underscore characters
    let ident_start = before
        .char_indices()
        .rev()
        .take_while(|&(_, c)| c.is_alphanumeric() || c == '_')
        .last()
        .map(|(i, _)| i);

    let start = match ident_start {
        Some(s) => {
            // Check if there's a `\` immediately before the identifier
            if s > 0 {
                let prev = &text[..s];
                if prev.ends_with('\\') {
                    s - 1 // include the backslash
                } else {
                    s
                }
            } else {
                s
            }
        }
        None => {
            // No alphanumeric chars — check if cursor is right after a lone `\`
            if before.ends_with('\\') {
                cursor_byte - 1
            } else {
                return None;
            }
        }
    };

    let prefix = &text[start..cursor_byte];
    if prefix.is_empty() {
        None
    } else {
        Some((prefix, start))
    }
}

/// LaTeX command → Unicode symbol mapping.
/// Each entry: (latex_command, unicode_symbol)
pub const LATEX_SYMBOLS: &[(&str, &str)] = &[
    // Lowercase Greek letters
    ("\\alpha", "\u{03B1}"),   // α
    ("\\beta", "\u{03B2}"),    // β
    ("\\gamma", "\u{03B3}"),   // γ
    ("\\delta", "\u{03B4}"),   // δ
    ("\\epsilon", "\u{03B5}"), // ε
    ("\\zeta", "\u{03B6}"),    // ζ
    ("\\eta", "\u{03B7}"),     // η
    ("\\theta", "\u{03B8}"),   // θ
    ("\\iota", "\u{03B9}"),    // ι
    ("\\kappa", "\u{03BA}"),   // κ
    ("\\lambda", "\u{03BB}"),  // λ
    ("\\mu", "\u{03BC}"),      // μ
    ("\\nu", "\u{03BD}"),      // ν
    ("\\xi", "\u{03BE}"),      // ξ
    ("\\pi", "\u{03C0}"),      // π
    ("\\rho", "\u{03C1}"),     // ρ
    ("\\sigma", "\u{03C3}"),   // σ
    ("\\tau", "\u{03C4}"),     // τ
    ("\\upsilon", "\u{03C5}"), // υ
    ("\\phi", "\u{03C6}"),     // φ
    ("\\chi", "\u{03C7}"),     // χ
    ("\\psi", "\u{03C8}"),     // ψ
    ("\\omega", "\u{03C9}"),   // ω

    // Uppercase Greek letters
    ("\\Gamma", "\u{0393}"),   // Γ
    ("\\Delta", "\u{0394}"),   // Δ
    ("\\Theta", "\u{0398}"),   // Θ
    ("\\Lambda", "\u{039B}"),  // Λ
    ("\\Xi", "\u{039E}"),      // Ξ
    ("\\Pi", "\u{03A0}"),      // Π
    ("\\Sigma", "\u{03A3}"),   // Σ
    ("\\Phi", "\u{03A6}"),     // Φ
    ("\\Psi", "\u{03A8}"),     // Ψ
    ("\\Omega", "\u{03A9}"),   // Ω

    // Math operators
    ("\\int", "\u{222B}"),        // ∫
    ("\\sum", "\u{2211}"),        // ∑
    ("\\prod", "\u{220F}"),       // ∏
    ("\\partial", "\u{2202}"),    // ∂
    ("\\nabla", "\u{2207}"),      // ∇
    ("\\sqrt", "\u{221A}"),       // √
    ("\\infty", "\u{221E}"),      // ∞
    ("\\pm", "\u{00B1}"),         // ±
    ("\\mp", "\u{2213}"),         // ∓
    ("\\times", "\u{00D7}"),      // ×
    ("\\div", "\u{00F7}"),        // ÷
    ("\\cdot", "\u{22C5}"),       // ⋅

    // Arrows
    ("\\to", "\u{2192}"),         // →
    ("\\leftarrow", "\u{2190}"),  // ←
    ("\\rightarrow", "\u{2192}"), // →
    ("\\Leftarrow", "\u{21D0}"),  // ⇐
    ("\\Rightarrow", "\u{21D2}"), // ⇒
    ("\\leftrightarrow", "\u{2194}"), // ↔
    ("\\uparrow", "\u{2191}"),    // ↑
    ("\\downarrow", "\u{2193}"),  // ↓

    // Relations
    ("\\leq", "\u{2264}"),     // ≤
    ("\\geq", "\u{2265}"),     // ≥
    ("\\neq", "\u{2260}"),     // ≠
    ("\\approx", "\u{2248}"),  // ≈
    ("\\equiv", "\u{2261}"),   // ≡
    ("\\sim", "\u{223C}"),     // ∼
    ("\\propto", "\u{221D}"),  // ∝
    ("\\in", "\u{2208}"),      // ∈
    ("\\notin", "\u{2209}"),   // ∉
    ("\\subset", "\u{2282}"),  // ⊂
    ("\\supset", "\u{2283}"),  // ⊃

    // Misc
    ("\\forall", "\u{2200}"),  // ∀
    ("\\exists", "\u{2203}"),  // ∃
    ("\\emptyset", "\u{2205}"),// ∅
    ("\\degree", "\u{00B0}"),  // °
];

/// Build candidate list for LaTeX symbol completion.
pub fn symbol_candidates() -> Vec<Candidate> {
    LATEX_SYMBOLS
        .iter()
        .map(|&(cmd, sym)| Candidate {
            label: sym.to_string(),
            kind: CandidateKind::Symbol,
            filter_key: Some(cmd.to_string()),
            display: Some(format!("{}  {}", cmd, sym)),
        })
        .collect()
}

/// Build the static candidate list from lexer constants + keywords.
pub fn static_candidates() -> Vec<Candidate> {
    let mut result = Vec::new();

    // Keywords
    for &kw in &["if", "else", "for", "while", "and", "or", "not", "true", "false"] {
        result.push(Candidate {
            label: kw.to_string(),
            kind: CandidateKind::Keyword,
            filter_key: None,
            display: None,
        });
    }

    // Builtins
    for &name in lexer::BUILTINS {
        result.push(Candidate {
            label: name.to_string(),
            kind: CandidateKind::Builtin,
            filter_key: None,
            display: None,
        });
    }

    // Type names
    for &(name, _) in lexer::TYPE_NAMES {
        result.push(Candidate {
            label: name.to_string(),
            kind: CandidateKind::Type,
            filter_key: None,
            display: None,
        });
    }

    // Axis variables
    for &name in lexer::AXIS_VARS {
        result.push(Candidate {
            label: name.to_string(),
            kind: CandidateKind::AxisVar,
            filter_key: None,
            display: None,
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
        // LaTeX backslash prefixes
        assert_eq!(prefix_at_cursor("\\pi", 3), Some(("\\pi", 0)));
        assert_eq!(prefix_at_cursor("a + \\al", 7), Some(("\\al", 4)));
        assert_eq!(prefix_at_cursor("\\", 1), Some(("\\", 0)));
        // Backslash mid-text should NOT merge into previous identifier
        assert_eq!(prefix_at_cursor("foo\\bar", 7), Some(("\\bar", 3)));
        // Backslash after space
        assert_eq!(prefix_at_cursor("a \\pi", 5), Some(("\\pi", 2)));
        // No backslash — normal identifier
        assert_eq!(prefix_at_cursor("abc", 3), Some(("abc", 0)));
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
    fn test_symbol_candidates_latex() {
        let syms = symbol_candidates();
        assert!(!syms.is_empty());
        assert!(syms.iter().any(|c| c.label == "\u{03C0}" && c.filter_key.as_deref() == Some("\\pi")));
        assert!(syms.iter().any(|c| c.label == "\u{222B}" && c.filter_key.as_deref() == Some("\\int")));
    }

    #[test]
    fn test_update_filters_symbol_by_latex_prefix() {
        let mut state = AutocompleteState::new();
        let syms = symbol_candidates();
        state.update("\\p", 0, &syms);
        assert!(state.active);
        assert!(state.candidates.iter().any(|c| c.label == "\u{03C0}")); // π via \pi
        assert!(!state.candidates.iter().any(|c| c.label == "\u{03B1}")); // α (\alpha) should not match
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
                    filter_key: None,
                    display: None,
                });
            }
        }
        AstNode::FunctionDef { name, .. } => {
            if !result.iter().any(|c| c.label == *name) {
                result.push(Candidate {
                    label: name.clone(),
                    kind: CandidateKind::UserFunc,
                    filter_key: None,
                    display: None,
                });
            }
        }
        AstNode::TupleBinding { names, .. } => {
            for n in names {
                if !result.iter().any(|c| c.label == *n) {
                    result.push(Candidate {
                        label: n.clone(),
                        kind: CandidateKind::UserBinding,
                        filter_key: None,
                        display: None,
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
