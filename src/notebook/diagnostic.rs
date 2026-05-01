//! User-facing diagnostics with row/col spans.
//!
//! Errors and warnings produced anywhere in the notebook pipeline (parse,
//! REDUCE substitution, WGSL gen, runtime) are collected as `Diagnostic`s on
//! the cell that produced them. Spans are 0-indexed, character-based (not
//! byte-based), to feed straight into UI rendering.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// 0-indexed character span within a single source string.
///
/// Both `start_col` and `end_col` count characters, not bytes — UI consumers
/// (red squiggles) can map directly to glyph positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl Span {
    /// Build a span from byte offsets `[start, end)` into `source`.
    pub fn from_byte_range(source: &str, start: usize, end: usize) -> Self {
        let (sl, sc) = byte_to_line_col(source, start);
        let (el, ec) = byte_to_line_col(source, end);
        Span {
            start_line: sl,
            start_col: sc,
            end_line: el,
            end_col: ec,
        }
    }

    /// Zero-width span at a single byte offset.
    pub fn point(source: &str, offset: usize) -> Self {
        Self::from_byte_range(source, offset, offset)
    }

    /// Span covering the whole source (line 0, col 0 → end).
    pub fn whole(source: &str) -> Self {
        Self::from_byte_range(source, 0, source.len())
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Span,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            span,
        }
    }

    pub fn warning(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            span,
        }
    }
}

/// Convert a byte offset into (line, col), both 0-indexed; col counts chars.
fn byte_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let mut line = 0;
    let mut line_start = 0;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = i + ch.len_utf8();
        }
    }
    let col = source[line_start..offset].chars().count();
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_at_zero() {
        let s = Span::point("hello", 0);
        assert_eq!(s.start_line, 0);
        assert_eq!(s.start_col, 0);
        assert_eq!(s.end_line, 0);
        assert_eq!(s.end_col, 0);
    }

    #[test]
    fn span_across_newline() {
        let src = "abc\ndef\nghi";
        let s = Span::from_byte_range(src, 1, 9); // 'b' through 'h'
        assert_eq!(s.start_line, 0);
        assert_eq!(s.start_col, 1);
        assert_eq!(s.end_line, 2);
        assert_eq!(s.end_col, 1);
    }

    #[test]
    fn span_counts_chars_not_bytes() {
        // 'π' is 2 bytes; column should be 1, not 2.
        let src = "πq";
        let s = Span::point(src, 'π'.len_utf8());
        assert_eq!(s.start_line, 0);
        assert_eq!(s.start_col, 1);
    }

    #[test]
    fn offset_past_end_clamps() {
        let src = "abc";
        let s = Span::point(src, 9999);
        assert_eq!(s.start_line, 0);
        assert_eq!(s.start_col, 3);
    }
}
