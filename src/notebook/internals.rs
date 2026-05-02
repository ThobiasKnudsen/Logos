//! Helpers for the notebook pipeline — text-level binding extraction, CAS
//! call detection, and REDUCE expression preparation.
//!
//! These were moved here from `app.rs` during the Notebook migration;
//! the originals are gone, so this module is the sole owner.

use crate::lang::reduce::translate;

use super::cell::{CellMessage, NotebookCell};

const CAS_FUNCTIONS: &[&str] = &["int(", "df(", "limit(", "solve(", "diff("];

/// Locate a CAS call inside `text`, returning its `[start, end)` byte range.
/// Uses balanced-paren matching; respects word boundaries on the function name.
pub(super) fn find_cas_call(text: &str) -> Option<(usize, usize)> {
    for func in CAS_FUNCTIONS {
        let mut search_from = 0;
        while let Some(pos) = text[search_from..].find(func) {
            let start = search_from + pos;
            if start > 0 {
                let prev = text[..start].chars().last().unwrap();
                if prev.is_alphanumeric() || prev == '_' {
                    search_from = start + 1;
                    continue;
                }
            }
            let paren = start + func.len() - 1;
            let mut depth = 1usize;
            for (i, ch) in text[paren + 1..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some((start, paren + 1 + i + 1));
                        }
                    }
                    _ => {}
                }
            }
            break;
        }
    }
    None
}

/// Strip `name = ` assignment prefix, returning just the RHS.
fn strip_assignment_lhs(cell_text: &str) -> &str {
    let trimmed = cell_text.trim();
    if let Some(eq_pos) = trimmed.find('=') {
        if eq_pos > 0
            && trimmed.get(eq_pos + 1..eq_pos + 2) != Some("=")
            && !trimmed[..eq_pos].ends_with('!')
            && !trimmed[..eq_pos].ends_with('<')
            && !trimmed[..eq_pos].ends_with('>')
        {
            let lhs = trimmed[..eq_pos].trim();
            if !lhs.is_empty() && lhs.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return trimmed[eq_pos + 1..].trim();
            }
        }
    }
    trimmed
}

/// Extract the REDUCE-bound expression from a cell's text.
/// Returns `(expr, is_embedded_cas)`.
pub(super) fn extract_reduce_expr(cell_text: &str) -> (&str, bool) {
    let trimmed = cell_text.trim();
    if let Some((start, end)) = find_cas_call(trimmed) {
        let cas_call = &trimmed[start..end];
        let stripped = strip_assignment_lhs(trimmed);
        if stripped.trim() != cas_call {
            return (cas_call, true);
        }
    }
    (strip_assignment_lhs(trimmed), false)
}

/// Substitute a REDUCE result back into the original cell text.
pub(super) fn substitute_reduce_result(
    cell_text: &str,
    reduce_result: &str,
    embedded_cas: bool,
) -> String {
    let trimmed = cell_text.trim();
    if embedded_cas {
        if let Some((start, end)) = find_cas_call(trimmed) {
            let mut out = String::with_capacity(trimmed.len());
            out.push_str(&trimmed[..start]);
            out.push('(');
            out.push_str(reduce_result);
            out.push(')');
            out.push_str(&trimmed[end..]);
            return out;
        }
    }
    if let Some(eq_pos) = trimmed.find('=') {
        if eq_pos > 0
            && trimmed.get(eq_pos + 1..eq_pos + 2) != Some("=")
            && !trimmed[..eq_pos].ends_with('!')
            && !trimmed[..eq_pos].ends_with('<')
            && !trimmed[..eq_pos].ends_with('>')
        {
            let lhs = trimmed[..eq_pos].trim();
            if !lhs.is_empty() && lhs.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return format!("{} = {}", lhs, reduce_result);
            }
        }
    }
    reduce_result.to_string()
}

/// Pull `:=` bindings out of every prior cell (line-by-line) plus the lines
/// in the current cell that come before the first action call.
pub(super) fn collect_bindings(
    cells: &[NotebookCell],
    up_to: usize,
    cell_text: &str,
) -> Vec<(String, String)> {
    let mut bindings = Vec::new();

    for cell in cells.iter().take(up_to) {
        // Use the simplified text when the last play left one — that's the
        // post-REDUCE-substitution form.
        let src: &str = match &cell.outcome.message {
            Some(CellMessage::Simplified(s)) => s.as_str(),
            _ => cell.text.as_str(),
        };
        for line in src.trim().lines() {
            extract_binding(line.trim(), &mut bindings);
        }
    }

    let stop = [
        find_cas_call(cell_text).map(|(s, _)| s),
        cell_text.find("print("),
        cell_text.find("plot("),
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(cell_text.len());
    for line in cell_text[..stop].lines() {
        extract_binding(line.trim(), &mut bindings);
    }

    bindings
}

fn extract_binding(text: &str, bindings: &mut Vec<(String, String)>) {
    if text.is_empty() {
        return;
    }
    let Some(colon_pos) = text.find(":=") else {
        return;
    };
    let lhs = text[..colon_pos].trim();
    let rhs = text[colon_pos + 2..].trim();
    if lhs.is_empty() || rhs.is_empty() {
        return;
    }
    let name = if let Some(paren) = lhs.find('(') {
        lhs[..paren].trim()
    } else {
        lhs
    };
    if !name.is_empty() {
        bindings.push((name.to_string(), rhs.to_string()));
    }
}

/// Pull the inner argument of `fn_name(...)` from `text`, paren-balanced.
pub(super) fn extract_call_arg(text: &str, fn_name: &str) -> Option<String> {
    let prefix = format!("{}(", fn_name);
    let start = text.find(&prefix)?;
    let after = &text[start + prefix.len()..];
    let mut depth = 1;
    for (i, c) in after.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(after[..i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Substitute every binding name with its body, repeating to a fixed point.
pub(super) fn expand_bindings(expr: &str, bindings: &[(String, String)]) -> String {
    let mut result = expr.to_string();
    for _ in 0..20 {
        let mut changed = false;
        for (name, body) in bindings {
            let new = replace_at_word_boundaries(&result, name, &format!("({})", body));
            if new != result {
                result = new;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    result
}

fn replace_at_word_boundaries(input: &str, word: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut start = 0;
    while let Some(idx) = input[start..].find(word) {
        let abs = start + idx;
        let before_ok = abs == 0
            || !input[..abs]
                .chars()
                .last()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let after_pos = abs + word.len();
        let after_ok = after_pos >= input.len()
            || !input[after_pos..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if before_ok && after_ok {
            result.push_str(&input[start..abs]);
            result.push_str(replacement);
        } else {
            result.push_str(&input[start..abs + word.len()]);
        }
        start = abs + word.len();
    }
    result.push_str(&input[start..]);
    result
}

/// Build the REDUCE-side expression for a `print(...)` arg after binding
/// expansion. Returns `(reduce_expr, is_equation)`. Equations are converted
/// to `(lhs) - (rhs)` so REDUCE simplifies them as a single value.
pub(super) fn prepare_reduce_print(expanded: &str) -> (String, bool) {
    let reduce_expr = translate::to_reduce(expanded);
    if let Some(eq_pos) = reduce_expr.find('=') {
        if !reduce_expr[..eq_pos].ends_with(':')
            && !reduce_expr[..eq_pos].ends_with('<')
            && !reduce_expr[..eq_pos].ends_with('>')
            && !reduce_expr[..eq_pos].ends_with('!')
        {
            let lhs = &reduce_expr[..eq_pos];
            let rhs = &reduce_expr[eq_pos + 1..];
            let combined = format!("({}) - ({})", lhs.trim(), rhs.trim());
            return (combined, true);
        }
    }
    (reduce_expr, false)
}
