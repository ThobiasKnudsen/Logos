use crate::lang::lexer::Lexer;
use crate::lang::token::TokenType;
use crate::ui::theme::{self, Rgba};

/// A colored span of source text for syntax highlighting.
#[derive(Debug, Clone)]
pub struct HighlightSpan {
    /// Byte range [start, end) into the original source.
    pub start: usize,
    pub end: usize,
    /// Color to render this span.
    pub color: Rgba,
}

/// Map a token type to a color using the given syntax theme.
fn token_color(ty: &TokenType) -> Rgba {
    let theme = theme::theme();
    match ty {
        // Keywords
        TokenType::If
        | TokenType::Else
        | TokenType::For
        | TokenType::While
        | TokenType::And
        | TokenType::Or
        | TokenType::Not
        | TokenType::Gpu
        | TokenType::In => theme.keyword,

        // Literals
        TokenType::Number(_) => theme.number,
        TokenType::BoolLit(_) => theme.keyword, // true/false as keywords

        // Identifiers
        TokenType::Identifier(_) => theme.identifier,

        // Builtins (sin, cos, etc.)
        TokenType::Builtin(_) => theme.builtin,

        // Axis variables (x, y, z, time)
        TokenType::AxisVar(_) => theme.axis,

        // Type names
        TokenType::TypeF32
        | TokenType::TypeF64
        | TokenType::TypeI32
        | TokenType::TypeVec2
        | TokenType::TypeVec3
        | TokenType::TypeVec4
        | TokenType::TypeBool => theme.type_name,

        // Operators
        TokenType::Plus
        | TokenType::Minus
        | TokenType::Star
        | TokenType::Slash
        | TokenType::Percent
        | TokenType::Caret
        | TokenType::Eq
        | TokenType::Neq
        | TokenType::Lt
        | TokenType::Gt
        | TokenType::Lte
        | TokenType::Gte => theme.operator,

        // Binding operator
        TokenType::Colon => theme.operator,

        // Lambda introducer
        TokenType::Mapsto => theme.operator,

        // Punctuation
        TokenType::LParen
        | TokenType::RParen
        | TokenType::LBracket
        | TokenType::RBracket
        | TokenType::Comma
        | TokenType::Dot
        | TokenType::DotDot => theme.punctuation,

        // Whitespace / separators
        TokenType::Newline => theme.whitespace,

        // EOF — no visible content
        TokenType::Eof => theme.unknown,
    }
}

/// Produce syntax-highlighted spans for the given source text.
///
/// Gaps between tokens (whitespace, comments) are filled with the theme's
/// `comment` color for comment regions and `whitespace` color otherwise.
/// If the lexer fails, the entire text is returned as a single span with
/// the default text color (graceful degradation).
pub fn highlight(source: &str) -> Vec<HighlightSpan> {
    if source.is_empty() {
        return Vec::new();
    }

    let mut lexer = Lexer::new(source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(_) => {
            // Lexer error — return unhighlighted text so the editor still works.
            return vec![HighlightSpan {
                start: 0,
                end: source.len(),
                color: crate::ui::theme::theme().text_primary,
            }];
        }
    };

    let theme = theme::theme();
    let mut spans = Vec::with_capacity(tokens.len() * 2);
    let mut cursor = 0;

    for token in &tokens {
        let (tok_start, tok_end) = token.span;

        // Fill gap before this token (whitespace, comments, etc.)
        if tok_start > cursor {
            // Check if the gap contains a comment
            let gap = &source[cursor..tok_start];
            let color = if gap.contains("//") || gap.contains("/*") {
                theme.comment
            } else {
                theme.whitespace
            };
            spans.push(HighlightSpan {
                start: cursor,
                end: tok_start,
                color,
            });
        }

        // Skip zero-width tokens (EOF at end)
        if tok_start < tok_end {
            spans.push(HighlightSpan {
                start: tok_start,
                end: tok_end,
                color: token_color(&token.ty),
            });
        }

        cursor = tok_end;
    }

    // Trailing text after last token
    if cursor < source.len() {
        spans.push(HighlightSpan {
            start: cursor,
            end: source.len(),
            color: theme.whitespace,
        });
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_basic() {
        let spans = highlight("x + 3.14");
        // Should have spans for: "x", " ", "+", " ", "3.14"
        assert!(
            spans.len() >= 3,
            "Expected at least 3 spans, got {}",
            spans.len()
        );

        // Verify full coverage
        let total: usize = spans.iter().map(|s| s.end - s.start).sum();
        assert_eq!(total, "x + 3.14".len());
    }

    #[test]
    fn test_highlight_empty() {
        let spans = highlight("");
        assert!(spans.is_empty());
    }

    #[test]
    fn test_highlight_covers_all_bytes() {
        let source = "r: sqrt(x*x + y*y)\nsin(r * 10) / r";
        let spans = highlight(source);

        // Verify no gaps
        let mut cursor = 0;
        for span in &spans {
            assert_eq!(span.start, cursor, "Gap at byte {}", cursor);
            cursor = span.end;
        }
        assert_eq!(cursor, source.len(), "Didn't cover full source");
    }

    #[test]
    fn test_highlight_with_comments() {
        let source = "x // a comment\n+ y";
        let spans = highlight(source);
        let total: usize = spans.iter().map(|s| s.end - s.start).sum();
        assert_eq!(total, source.len());
    }
}
