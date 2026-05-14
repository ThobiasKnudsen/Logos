use std::collections::HashSet;

use crate::lang::ir::Ir;
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
        let prefix_is_ascii = lower.is_ascii();
        self.prefix.clear();
        self.prefix.push_str(prefix);
        self.prefix_start = prefix_start;

        // Reuse the existing Vec; only clone candidates that match.
        self.candidates.clear();
        for c in all_candidates {
            let key = c.filter_key.as_deref().unwrap_or(&c.label);
            // Fast ASCII path: avoid building a temporary lowercase String.
            let matches = if prefix_is_ascii && key.is_ascii() {
                let kb = key.as_bytes();
                let pb = lower.as_bytes();
                kb.len() > pb.len()
                    && kb
                        .iter()
                        .zip(pb.iter())
                        .all(|(a, b)| a.eq_ignore_ascii_case(b))
            } else {
                let key_lower = key.to_lowercase();
                key_lower.starts_with(&lower) && key_lower != lower
            };
            if matches {
                self.candidates.push(c.clone());
            }
        }

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
///
/// Every entry must lex as a valid Logos token — the gate is the
/// `every_latex_symbol_substitution_lexes` test in `src/notebook/tests.rs`.
/// Codepoints in Unicode's "Sm" (Math Symbol) class are *not* `is_alphabetic`
/// and have no explicit case in `src/lang/lexer.rs`, so they would produce
/// "Unexpected character" the moment a user types the trigger. To add a new
/// substitution: either pick a codepoint the lexer already handles (Greek
/// letters lex as identifiers; π/ℯ/τ are numeric constants; ×/÷/−/≤/≥/≠/√/∫
/// have explicit cases), or first extend `src/lang/lexer.rs` to recognize it.
pub const LATEX_SYMBOLS: &[(&str, &str)] = &[
    // Lowercase Greek letters — lex as identifiers (`is_alphabetic`)
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
    ("\\pi", "\u{03C0}"),      // π — numeric constant (lexer: Number(PI))
    ("\\rho", "\u{03C1}"),     // ρ
    ("\\sigma", "\u{03C3}"),   // σ
    ("\\tau", "\u{03C4}"),     // τ — numeric constant (lexer: Number(TAU))
    ("\\upsilon", "\u{03C5}"), // υ
    ("\\phi", "\u{03C6}"),     // φ
    ("\\chi", "\u{03C7}"),     // χ
    ("\\psi", "\u{03C8}"),     // ψ
    ("\\omega", "\u{03C9}"),   // ω
    // Uppercase Greek letters
    ("\\Gamma", "\u{0393}"),  // Γ
    ("\\Delta", "\u{0394}"),  // Δ
    ("\\Theta", "\u{0398}"),  // Θ
    ("\\Lambda", "\u{039B}"), // Λ
    ("\\Xi", "\u{039E}"),     // Ξ
    ("\\Pi", "\u{03A0}"),     // Π
    ("\\Sigma", "\u{03A3}"),  // Σ
    ("\\Phi", "\u{03A6}"),    // Φ
    ("\\Psi", "\u{03A8}"),    // Ψ
    ("\\Omega", "\u{03A9}"),  // Ω
    // Math constants — lexer maps to specific identifiers or numbers
    ("\\euler", "\u{212F}"),      // ℯ (Number(E))
    ("\\derivative", "\u{2146}"), // ⅆ (Identifier "derivative" → REDUCE `df`)
    // CAS/math operators — explicit lexer cases in src/lang/lexer.rs
    ("\\integral", "\u{222B}"), // ∫ (Identifier "integral" → REDUCE `int`)
    ("\\sum", "\u{2211}"),     // ∑ (Identifier "sum")
    ("\\prod", "\u{220F}"),    // ∏ (Identifier "prod")
    ("\\partial", "\u{2202}"), // ∂ (Identifier "partial" → REDUCE `df`)
    ("\\nabla", "\u{2207}"),   // ∇ (Identifier "nabla")
    ("\\sqrt", "\u{221A}"),    // √ (Builtin "sqrt")
    ("\\infty", "\u{221E}"),   // ∞ (Identifier "infinity")
    ("\\times", "\u{00D7}"),   // × (Star — binary multiply)
    ("\\div", "\u{00F7}"),     // ÷ (Slash — binary divide)
    // Relations — explicit lexer cases (Lte/Gte/Neq)
    ("\\leq", "\u{2264}"), // ≤
    ("\\le", "\u{2264}"),  // ≤ (alias)
    ("\\geq", "\u{2265}"), // ≥
    ("\\ge", "\u{2265}"),  // ≥ (alias)
    ("\\neq", "\u{2260}"),    // ≠
    ("\\mapsto", "\u{21A6}"), // ↦ — same lexical token as `|->`
];

/// Multi-character substitutions where the entire sequence (no `\` prefix,
/// the *final* keystroke is what triggers the swap, and that keystroke is
/// itself part of the pattern). Unlike `LATEX_SYMBOLS`, the trigger is
/// consumed by the substitution rather than preserved after it.
///
/// Each entry is `(source, replacement)`. The lexer must accept both spellings
/// — `|->` and the chosen Unicode — as equivalent tokens; this table only
/// shapes what ends up in the buffer.
pub const TEXT_SUBSTITUTIONS: &[(&str, &str)] = &[
    ("|->", "\u{21A6}"), // ↦
    ("<=", "\u{2264}"),  // ≤   (lexer also accepts `<=`)
    (">=", "\u{2265}"),  // ≥   (lexer also accepts `>=`)
    ("!=", "\u{2260}"),  // ≠   (lexer also accepts `!=`)
];

/// Result of an auto-substitution check: replace `text[start..end]` with
/// `replacement`. Returned by `latex_auto_substitute` so callers don't need
/// to know whether the trigger character was consumed by the pattern (as in
/// `|->`) or preserved after it (as in `\int<space>`).
#[derive(Debug, Clone, PartialEq)]
pub struct AutoSubstitute {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

/// If the user just typed a character that completes a known substitution
/// pattern, return the range to replace and what to put there. Two kinds
/// of patterns are recognized:
///
///   1. `\command<delimiter>` from `LATEX_SYMBOLS` — the trigger delimiter
///      is preserved after the replacement (so `\int(` → `∫(`).
///   2. Self-contained sequences from `TEXT_SUBSTITUTIONS` like `|->` —
///      the final character is part of the pattern and is consumed by the
///      replacement (so `|->` → `↦`).
///
/// Returns `None` if neither kind of pattern matches at the cursor.
pub fn latex_auto_substitute(text: &str, cursor: usize) -> Option<AutoSubstitute> {
    if cursor == 0 || cursor > text.len() {
        return None;
    }

    // Self-contained text substitutions first — the trigger keystroke is
    // the *last* char of the pattern, so we just look at the suffix of the
    // text ending at the cursor.
    for &(pattern, sym) in TEXT_SUBSTITUTIONS {
        if text[..cursor].ends_with(pattern) {
            return Some(AutoSubstitute {
                start: cursor - pattern.len(),
                end: cursor,
                replacement: sym.to_string(),
            });
        }
    }

    // `\command<delimiter>` patterns. The trigger has to be a non-identifier
    // delimiter; if it's a letter, the user is still typing.
    let trigger = text[..cursor].chars().next_back()?;
    if trigger.is_alphanumeric() || trigger == '_' || trigger == '\\' {
        return None;
    }
    let before_trigger = cursor - trigger.len_utf8();
    let (prefix, prefix_start) = prefix_at_cursor(text, before_trigger)?;
    if !prefix.starts_with('\\') || prefix.len() < 2 {
        return None;
    }
    let sym = LATEX_SYMBOLS
        .iter()
        .find(|&&(cmd, _)| cmd == prefix)
        .map(|&(_, sym)| sym)?;
    let trigger_str = &text[before_trigger..cursor];
    Some(AutoSubstitute {
        start: prefix_start,
        end: cursor,
        replacement: format!("{sym}{trigger_str}"),
    })
}

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
    for &kw in &[
        "if", "else", "for", "while", "and", "or", "not", "true", "false", "gpu", "in",
    ] {
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

/// Extract user-defined symbols from the IR of the current cell.
pub fn extract_user_symbols(ir: &Ir) -> Vec<Candidate> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    walk_ir(ir, &mut result, &mut seen);
    result
}

fn walk_ir(node: &Ir, result: &mut Vec<Candidate>, seen: &mut HashSet<String>) {
    match node {
        Ir::Binding { name, .. } => {
            if seen.insert(name.clone()) {
                result.push(Candidate {
                    label: name.clone(),
                    kind: CandidateKind::UserBinding,
                    filter_key: None,
                    display: None,
                });
            }
        }
        Ir::FunctionDef { name, .. } => {
            if seen.insert(name.clone()) {
                result.push(Candidate {
                    label: name.clone(),
                    kind: CandidateKind::UserFunc,
                    filter_key: None,
                    display: None,
                });
            }
        }
        Ir::TupleBinding { names, .. } => {
            for n in names {
                if seen.insert(n.clone()) {
                    result.push(Candidate {
                        label: n.clone(),
                        kind: CandidateKind::UserBinding,
                        filter_key: None,
                        display: None,
                    });
                }
            }
        }
        Ir::Block { items: stmts, .. } => {
            for s in stmts {
                walk_ir(s, result, seen);
            }
        }
        Ir::IfExpr {
            then_branch,
            else_branch,
            ..
        } => {
            walk_ir(then_branch, result, seen);
            if let Some(eb) = else_branch {
                walk_ir(eb, result, seen);
            }
        }
        Ir::WhileLoop { body, .. }
        | Ir::ForLoop { body, .. }
        | Ir::ParallelFor { body, .. } => {
            walk_ir(body, result, seen);
        }
        _ => {}
    }
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
        assert!(syms
            .iter()
            .any(|c| c.label == "\u{03C0}" && c.filter_key.as_deref() == Some("\\pi")));
        assert!(syms
            .iter()
            .any(|c| c.label == "\u{222B}" && c.filter_key.as_deref() == Some("\\integral")));
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

    // ── latex_auto_substitute ────────────────────────────────────────────
    //
    // Pure-function gate: given the buffer state right after the user typed
    // a character, decide whether a `\command<delimiter>` just completed and
    // what to substitute. Caller (event handler) does the actual edit.

    fn assert_substitute(text: &str, want_prefix_start: usize, want_symbol: &str) {
        let cursor = text.len();
        let got = latex_auto_substitute(text, cursor);
        // The pre-existing `\command<trigger>` callers expect the trigger
        // preserved after the symbol; build that comparison value here so
        // existing tests don't have to spell it out.
        let trigger = &text[(cursor - text[..cursor].chars().next_back().map_or(0, char::len_utf8))..cursor];
        let expected = AutoSubstitute {
            start: want_prefix_start,
            end: cursor,
            replacement: format!("{want_symbol}{trigger}"),
        };
        assert_eq!(
            got,
            Some(expected),
            "expected substitution at {} → {:?} for {:?}; got {:?}",
            want_prefix_start, want_symbol, text, got,
        );
    }

    fn assert_no_substitute(text: &str) {
        let cursor = text.len();
        let got = latex_auto_substitute(text, cursor);
        assert!(
            got.is_none(),
            "expected no substitution for {:?}; got {:?}",
            text, got,
        );
    }

    #[test]
    fn auto_substitute_fires_on_space_after_complete_command() {
        // The user-reported case: `\integral ` (with the renamed command).
        assert_substitute("\\integral ", 0, "\u{222B}"); // ∫
    }

    #[test]
    fn auto_substitute_fires_on_paren_after_complete_command() {
        // `\integral(...)` — the trigger is `(`.
        assert_substitute("\\integral(", 0, "\u{222B}");
    }

    #[test]
    fn auto_substitute_fires_mid_text() {
        // Substitution must work in the middle of a line, not just at start.
        assert_substitute("x + \\pi ", 4, "\u{03C0}"); // π
    }

    #[test]
    fn auto_substitute_does_not_fire_on_letter() {
        // Letter continues the command; the user is still typing.
        assert_no_substitute("\\integ");
        assert_no_substitute("\\integra");
    }

    #[test]
    fn auto_substitute_does_not_fire_without_backslash() {
        // `pi ` (no backslash) is just an identifier followed by space.
        assert_no_substitute("pi ");
    }

    #[test]
    fn auto_substitute_does_not_fire_on_bare_backslash() {
        // Single `\` followed by space — not a real command.
        assert_no_substitute("\\ ");
    }

    #[test]
    fn auto_substitute_rewrites_pipe_arrow_to_mapsto() {
        // `|->` is a self-contained pattern: the final `>` keystroke is part
        // of the match and gets consumed by the substitution.
        let text = "x |->";
        let cursor = text.len();
        let got = latex_auto_substitute(text, cursor).expect("should substitute");
        assert_eq!(got.start, 2); // start of `|`
        assert_eq!(got.end, cursor);
        assert_eq!(got.replacement, "\u{21A6}"); // ↦
    }

    #[test]
    fn auto_substitute_pipe_arrow_does_not_fire_on_partial_pattern() {
        // `|-` alone is not a match — substitution waits for `>`.
        assert_no_substitute("x |-");
    }

    #[test]
    fn auto_substitute_rewrites_comparison_operators() {
        // Each completes on its final ASCII keystroke and is consumed entirely.
        for (ascii, unicode) in [("<=", "\u{2264}"), (">=", "\u{2265}"), ("!=", "\u{2260}")] {
            let text = format!("a {}", ascii);
            let cursor = text.len();
            let got = latex_auto_substitute(&text, cursor)
                .unwrap_or_else(|| panic!("should substitute {:?}", ascii));
            assert_eq!(got.start, cursor - ascii.len());
            assert_eq!(got.end, cursor);
            assert_eq!(got.replacement, unicode);
        }
    }

    #[test]
    fn auto_substitute_does_not_fire_for_unknown_command() {
        // `\nope` isn't in LATEX_SYMBOLS, so the trigger should not match.
        assert_no_substitute("\\nope ");
    }

    #[test]
    fn auto_substitute_le_alias_still_fires() {
        // `\le` is an alias for ≤. After my rename of `\int` → `\integral`,
        // `\le` and `\leq` are the only remaining prefix collision: typing
        // `\le<space>` substitutes to ≤. Users who want `\leq` must type it
        // in one go (the next char would still need to be the q, then a
        // delimiter — the system handles `\leq<space>` correctly because
        // `\le<q>` does not auto-substitute, q is alphanumeric).
        assert_substitute("\\le ", 0, "\u{2264}"); // ≤
        assert_substitute("\\leq ", 0, "\u{2264}"); // ≤
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
        assert!(!ac.candidates.is_empty());

        // Accept completion
        let (label, prefix_start) = ac.accept().unwrap();
        buf.replace_range(prefix_start, cursor, label);
        assert!(
            buf.text().starts_with('s'),
            "should start with s: {}",
            buf.text()
        );
    }
}
