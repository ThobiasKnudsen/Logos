use super::token::{Token, TokenType};

pub(crate) const BUILTINS: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh",
    "log", "log2", "log10", "exp", "exp2",
    "floor", "ceil", "round", "fract",
    "abs", "sign", "pow", "sqrt", "mod", "min", "max",
    "clamp", "mix", "step", "smoothstep",
    "length", "normalize", "dot", "cross",
];

pub(crate) const AXIS_VARS: &[&str] = &["x", "y", "z", "time"];

pub(crate) const TYPE_NAMES: &[(&str, TokenType)] = &[
    ("f32", TokenType::TypeF32),
    ("f64", TokenType::TypeF64),
    ("i32", TokenType::TypeI32),
    ("vec2", TokenType::TypeVec2),
    ("vec3", TokenType::TypeVec3),
    ("vec4", TokenType::TypeVec4),
    ("bool", TokenType::TypeBool),
];

pub struct Lexer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();
        loop {
            self.skip_spaces_and_comments();
            if self.pos >= self.input.len() {
                tokens.push(Token { ty: TokenType::Eof, span: (self.pos, self.pos) });
                break;
            }
            // Newlines are statement separators
            if self.peek() == Some('\n') {
                let start = self.pos;
                // Collapse consecutive newlines into one
                while self.peek() == Some('\n') {
                    self.advance();
                    self.skip_spaces_and_comments();
                }
                // Only emit newline if there are already tokens (skip leading newlines)
                // and we're not at EOF
                if !tokens.is_empty() && self.pos < self.input.len() {
                    tokens.push(Token { ty: TokenType::Newline, span: (start, self.pos) });
                }
                continue;
            }
            let tok = self.next_token()?;
            tokens.push(tok);
        }
        Ok(tokens)
    }

    /// Skip spaces, tabs, and comments — but NOT newlines.
    fn skip_spaces_and_comments(&mut self) {
        loop {
            // Skip non-newline whitespace
            while self.pos < self.input.len() {
                match self.peek() {
                    Some(c) if c.is_whitespace() && c != '\n' => {
                        self.pos += c.len_utf8();
                    }
                    _ => break,
                }
            }
            // Skip line comments (consume up to but not including newline)
            if self.starts_with("//") {
                while self.pos < self.input.len() && self.peek() != Some('\n') {
                    self.pos += self.peek().unwrap().len_utf8();
                }
                continue;
            }
            // Skip block comments
            if self.starts_with("/*") {
                self.pos += 2;
                while self.pos < self.input.len() && !self.starts_with("*/") {
                    self.pos += self.peek().unwrap().len_utf8();
                }
                if self.starts_with("*/") {
                    self.pos += 2;
                }
                continue;
            }
            break;
        }
    }

    fn next_token(&mut self) -> Result<Token, String> {
        let start = self.pos;
        let ch = self.peek().unwrap();

        // Two-character operators (must check before single-char)
        if let Some(ty) = self.try_two_char_op() {
            return Ok(Token { ty, span: (start, self.pos) });
        }

        // Single-character tokens
        match ch {
            '+' => { self.advance(); return Ok(Token { ty: TokenType::Plus, span: (start, self.pos) }); }
            '-' => { self.advance(); return Ok(Token { ty: TokenType::Minus, span: (start, self.pos) }); }
            '*' => { self.advance(); return Ok(Token { ty: TokenType::Star, span: (start, self.pos) }); }
            '/' => { self.advance(); return Ok(Token { ty: TokenType::Slash, span: (start, self.pos) }); }
            '%' => { self.advance(); return Ok(Token { ty: TokenType::Percent, span: (start, self.pos) }); }
            '^' => { self.advance(); return Ok(Token { ty: TokenType::Caret, span: (start, self.pos) }); }
            '<' => { self.advance(); return Ok(Token { ty: TokenType::Lt, span: (start, self.pos) }); }
            '>' => { self.advance(); return Ok(Token { ty: TokenType::Gt, span: (start, self.pos) }); }
            // `=` is equality in Logos (not assignment)
            '=' => { self.advance(); return Ok(Token { ty: TokenType::Eq, span: (start, self.pos) }); }
            ':' => { self.advance(); return Ok(Token { ty: TokenType::Colon, span: (start, self.pos) }); }
            '(' => { self.advance(); return Ok(Token { ty: TokenType::LParen, span: (start, self.pos) }); }
            ')' => { self.advance(); return Ok(Token { ty: TokenType::RParen, span: (start, self.pos) }); }
            '[' => { self.advance(); return Ok(Token { ty: TokenType::LBracket, span: (start, self.pos) }); }
            ']' => { self.advance(); return Ok(Token { ty: TokenType::RBracket, span: (start, self.pos) }); }
            ',' => { self.advance(); return Ok(Token { ty: TokenType::Comma, span: (start, self.pos) }); }
            '.' => { self.advance(); return Ok(Token { ty: TokenType::Dot, span: (start, self.pos) }); }
            _ => {}
        }

        // Numbers
        if ch.is_ascii_digit() || (ch == '.' && self.peek_at(1).map_or(false, |c| c.is_ascii_digit())) {
            return self.lex_number(start);
        }

        // Unicode math symbols — must check BEFORE identifiers since some
        // (like π, τ) are alphabetic in Unicode but should be their own tokens.
        if ch == '\u{00B2}' { // ² superscript
            self.advance();
            return Ok(Token { ty: TokenType::Builtin("square".to_string()), span: (start, self.pos) });
        }
        if ch == '\u{00B3}' { // ³ superscript
            self.advance();
            return Ok(Token { ty: TokenType::Builtin("cube".to_string()), span: (start, self.pos) });
        }
        if ch == '\u{03C0}' { // π
            self.advance();
            return Ok(Token { ty: TokenType::Number(std::f64::consts::PI), span: (start, self.pos) });
        }
        if ch == '\u{03C4}' { // τ (tau)
            self.advance();
            return Ok(Token { ty: TokenType::Number(std::f64::consts::TAU), span: (start, self.pos) });
        }
        if ch == '\u{2212}' { // − (unicode minus)
            self.advance();
            return Ok(Token { ty: TokenType::Minus, span: (start, self.pos) });
        }
        if ch == '\u{00D7}' { // × (multiplication sign)
            self.advance();
            return Ok(Token { ty: TokenType::Star, span: (start, self.pos) });
        }
        if ch == '\u{00F7}' { // ÷ (division sign)
            self.advance();
            return Ok(Token { ty: TokenType::Slash, span: (start, self.pos) });
        }
        if ch == '\u{222B}' { // ∫ (integral)
            self.advance();
            return Ok(Token { ty: TokenType::Builtin("int".to_string()), span: (start, self.pos) });
        }
        if ch == '\u{2202}' { // ∂ (partial derivative)
            self.advance();
            return Ok(Token { ty: TokenType::Builtin("df".to_string()), span: (start, self.pos) });
        }
        if ch == '\u{2207}' { // ∇ (nabla/gradient)
            self.advance();
            return Ok(Token { ty: TokenType::Identifier("nabla".to_string()), span: (start, self.pos) });
        }
        if ch == '\u{221A}' { // √ (square root)
            self.advance();
            return Ok(Token { ty: TokenType::Builtin("sqrt".to_string()), span: (start, self.pos) });
        }
        if ch == '\u{221E}' { // ∞ (infinity)
            self.advance();
            return Ok(Token { ty: TokenType::Identifier("infinity".to_string()), span: (start, self.pos) });
        }
        if ch == '\u{2211}' { // ∑ (summation)
            self.advance();
            return Ok(Token { ty: TokenType::Builtin("sum".to_string()), span: (start, self.pos) });
        }
        if ch == '\u{220F}' { // ∏ (product)
            self.advance();
            return Ok(Token { ty: TokenType::Builtin("prod".to_string()), span: (start, self.pos) });
        }
        if ch == '\u{2264}' { // ≤
            self.advance();
            return Ok(Token { ty: TokenType::Lte, span: (start, self.pos) });
        }
        if ch == '\u{2265}' { // ≥
            self.advance();
            return Ok(Token { ty: TokenType::Gte, span: (start, self.pos) });
        }
        if ch == '\u{2260}' { // ≠
            self.advance();
            return Ok(Token { ty: TokenType::Neq, span: (start, self.pos) });
        }
        // Superscript digits 0,1,4-9
        if let Some(exp) = match ch {
            '\u{2070}' => Some("0"), '\u{00B9}' => Some("1"),
            '\u{2074}' => Some("4"), '\u{2075}' => Some("5"),
            '\u{2076}' => Some("6"), '\u{2077}' => Some("7"),
            '\u{2078}' => Some("8"), '\u{2079}' => Some("9"),
            _ => None,
        } {
            self.advance();
            return Ok(Token {
                ty: TokenType::Builtin(format!("pow{}", exp)),
                span: (start, self.pos),
            });
        }

        // Identifiers / keywords / builtins
        if ch.is_alphabetic() || ch == '_' {
            return Ok(self.lex_identifier(start));
        }

        // Backslash: skip it gracefully (user typing LaTeX before autocomplete accepts)
        if ch == '\\' {
            self.advance();
            return Ok(Token { ty: TokenType::Identifier("\\".to_string()), span: (start, self.pos) });
        }

        Err(format!("Unexpected character '{}' at position {}", ch, start))
    }

    fn try_two_char_op(&mut self) -> Option<TokenType> {
        // `==` is also equality (same as `=` in Logos, for backward compat)
        if self.starts_with("==") { self.pos += 2; return Some(TokenType::Eq); }
        if self.starts_with("!=") { self.pos += 2; return Some(TokenType::Neq); }
        if self.starts_with("<=") { self.pos += 2; return Some(TokenType::Lte); }
        if self.starts_with(">=") { self.pos += 2; return Some(TokenType::Gte); }
        None
    }

    fn lex_number(&mut self, start: usize) -> Result<Token, String> {
        // Integer part
        while self.pos < self.input.len() && self.peek().map_or(false, |c| c.is_ascii_digit()) {
            self.advance();
        }
        // Decimal part
        if self.peek() == Some('.') && self.peek_at(1).map_or(false, |c| c.is_ascii_digit()) {
            self.advance(); // consume '.'
            while self.pos < self.input.len() && self.peek().map_or(false, |c| c.is_ascii_digit()) {
                self.advance();
            }
        }
        // Scientific notation
        if self.peek().map_or(false, |c| c == 'e' || c == 'E') {
            self.advance();
            if self.peek() == Some('+') || self.peek() == Some('-') {
                self.advance();
            }
            while self.pos < self.input.len() && self.peek().map_or(false, |c| c.is_ascii_digit()) {
                self.advance();
            }
        }

        let text = &self.input[start..self.pos];
        let value: f64 = text.parse().map_err(|e| format!("Invalid number '{}': {}", text, e))?;
        Ok(Token { ty: TokenType::Number(value), span: (start, self.pos) })
    }

    fn lex_identifier(&mut self, start: usize) -> Token {
        while self.pos < self.input.len() && self.peek().map_or(false, |c| {
            (c.is_alphanumeric() || c == '_') && !is_unicode_math_symbol(c)
        }) {
            self.advance();
        }
        let text = &self.input[start..self.pos];

        // Keywords
        let ty = match text {
            "if" => TokenType::If,
            "else" => TokenType::Else,
            "for" => TokenType::For,
            "while" => TokenType::While,
            "and" => TokenType::And,
            "or" => TokenType::Or,
            "not" => TokenType::Not,
            "true" => TokenType::BoolLit(true),
            "false" => TokenType::BoolLit(false),
            _ => {
                // Type names
                for &(name, ref tok) in TYPE_NAMES {
                    if text == name {
                        return Token { ty: tok.clone(), span: (start, self.pos) };
                    }
                }
                // Builtins
                if BUILTINS.contains(&text) {
                    TokenType::Builtin(text.to_string())
                }
                // Axis variables
                else if AXIS_VARS.contains(&text) {
                    TokenType::AxisVar(text.to_string())
                }
                // Regular identifier
                else {
                    TokenType::Identifier(text.to_string())
                }
            }
        };

        Token { ty, span: (start, self.pos) }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        let pos = self.pos + offset;
        if pos < self.input.len() {
            self.input[pos..].chars().next()
        } else {
            None
        }
    }

    fn advance(&mut self) {
        if let Some(ch) = self.peek() {
            self.pos += ch.len_utf8();
        }
    }

    fn starts_with(&self, s: &str) -> bool {
        self.input[self.pos..].starts_with(s)
    }
}

/// Unicode math symbols that look alphanumeric but should be separate tokens.
/// These must NOT be consumed as part of identifiers.
fn is_unicode_math_symbol(c: char) -> bool {
    matches!(c,
        '\u{00B2}'   // ² superscript 2
        | '\u{00B3}' // ³ superscript 3
        | '\u{03C0}' // π pi
        | '\u{03C4}' // τ tau
        | '\u{2212}' // − unicode minus
        | '\u{00D7}' // × multiplication sign
        | '\u{00F7}' // ÷ division sign
        | '\u{222B}' // ∫ integral
        | '\u{2202}' // ∂ partial
        | '\u{2207}' // ∇ nabla
        | '\u{221A}' // √ sqrt
        | '\u{221E}' // ∞ infinity
        | '\u{2211}' // ∑ sum
        | '\u{220F}' // ∏ product
        | '\u{2264}' // ≤
        | '\u{2265}' // ≥
        | '\u{2260}' // ≠
        | '\u{2070}' // ⁰
        | '\u{00B9}' // ¹
        | '\u{2074}' // ⁴
        | '\u{2075}' // ⁵
        | '\u{2076}' // ⁶
        | '\u{2077}' // ⁷
        | '\u{2078}' // ⁸
        | '\u{2079}' // ⁹
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let mut lexer = Lexer::new("x + 3.14 * sin(y)");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].ty, TokenType::AxisVar(ref s) if s == "x"));
        assert_eq!(tokens[1].ty, TokenType::Plus);
        assert!(matches!(tokens[2].ty, TokenType::Number(n) if (n - 3.14).abs() < 1e-10));
        assert_eq!(tokens[3].ty, TokenType::Star);
        assert!(matches!(tokens[4].ty, TokenType::Builtin(ref s) if s == "sin"));
        assert_eq!(tokens[5].ty, TokenType::LParen);
        assert!(matches!(tokens[6].ty, TokenType::AxisVar(ref s) if s == "y"));
        assert_eq!(tokens[7].ty, TokenType::RParen);
        assert_eq!(tokens[8].ty, TokenType::Eof);
    }

    #[test]
    fn test_equality_is_eq() {
        // In Logos, `=` is equality, not assignment
        let mut lexer = Lexer::new("a = b");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[1].ty, TokenType::Eq);
    }

    #[test]
    fn test_double_equals_also_eq() {
        let mut lexer = Lexer::new("a == b");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[1].ty, TokenType::Eq);
    }

    #[test]
    fn test_colon_binding() {
        let mut lexer = Lexer::new("r: 5");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].ty, TokenType::Identifier(ref s) if s == "r"));
        assert_eq!(tokens[1].ty, TokenType::Colon);
        assert!(matches!(tokens[2].ty, TokenType::Number(n) if (n - 5.0).abs() < 1e-10));
    }

    #[test]
    fn test_comparison_ops() {
        let mut lexer = Lexer::new("a != c <= d >= e");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[1].ty, TokenType::Neq);
        assert_eq!(tokens[3].ty, TokenType::Lte);
        assert_eq!(tokens[5].ty, TokenType::Gte);
    }

    #[test]
    fn test_keywords() {
        let mut lexer = Lexer::new("if true else false and or not");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].ty, TokenType::If);
        assert_eq!(tokens[1].ty, TokenType::BoolLit(true));
        assert_eq!(tokens[2].ty, TokenType::Else);
        assert_eq!(tokens[3].ty, TokenType::BoolLit(false));
        assert_eq!(tokens[4].ty, TokenType::And);
        assert_eq!(tokens[5].ty, TokenType::Or);
        assert_eq!(tokens[6].ty, TokenType::Not);
    }

    #[test]
    fn test_scientific_notation() {
        let mut lexer = Lexer::new("1e5 2.5e-3");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].ty, TokenType::Number(n) if (n - 1e5).abs() < 1.0));
        assert!(matches!(tokens[1].ty, TokenType::Number(n) if (n - 2.5e-3).abs() < 1e-10));
    }

    #[test]
    fn test_comments() {
        let mut lexer = Lexer::new("x // comment\n+ y /* block */ * z");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].ty, TokenType::AxisVar(ref s) if s == "x"));
        // After comment, there's a newline token, then + y * z
        assert_eq!(tokens[1].ty, TokenType::Newline);
        assert_eq!(tokens[2].ty, TokenType::Plus);
        assert!(matches!(tokens[3].ty, TokenType::AxisVar(ref s) if s == "y"));
        assert_eq!(tokens[4].ty, TokenType::Star);
        assert!(matches!(tokens[5].ty, TokenType::AxisVar(ref s) if s == "z"));
    }

    #[test]
    fn test_newlines_as_separators() {
        let mut lexer = Lexer::new("a: 5\nb: 10\na + b");
        let tokens = lexer.tokenize().unwrap();
        // a : 5 \n b : 10 \n a + b EOF
        assert!(matches!(tokens[0].ty, TokenType::Identifier(ref s) if s == "a"));
        assert_eq!(tokens[1].ty, TokenType::Colon);
        assert!(matches!(tokens[2].ty, TokenType::Number(_)));
        assert_eq!(tokens[3].ty, TokenType::Newline);
        assert!(matches!(tokens[4].ty, TokenType::Identifier(ref s) if s == "b"));
    }

    #[test]
    fn test_empty_input() {
        let mut lexer = Lexer::new("");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].ty, TokenType::Eof);
    }

    #[test]
    fn test_whitespace_only() {
        let mut lexer = Lexer::new("   \t\t  ");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].ty, TokenType::Eof);
    }

    #[test]
    fn test_unicode_superscript_not_part_of_identifier() {
        // ² is alphanumeric in Unicode but must be a separate token, not part of "x"
        let mut lexer = Lexer::new("x\u{00B2}");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].ty, TokenType::AxisVar(ref s) if s == "x"),
            "x should be AxisVar, got {:?}", tokens[0].ty);
        assert!(matches!(tokens[1].ty, TokenType::Builtin(ref s) if s == "square"),
            "² should be Builtin(square), got {:?}", tokens[1].ty);
    }

    #[test]
    fn test_unicode_pi_not_part_of_identifier() {
        // π after an identifier should be a separate Number token
        let mut lexer = Lexer::new("r\u{03C0}");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].ty, TokenType::Identifier(ref s) if s == "r"));
        assert!(matches!(tokens[1].ty, TokenType::Number(_)));
    }

    #[test]
    fn test_complex_unicode_expression() {
        // x²*y²+sin(x)³-sin(y²)²=9
        let input = "x\u{00B2}*y\u{00B2}+sin(x)\u{00B3}-sin(y\u{00B2})\u{00B2}=9";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();

        // Expected: x ² * y ² + sin ( x ) ³ - sin ( y ² ) ² = 9 EOF
        let expected: Vec<&str> = vec![
            "AxisVar(x)", "Builtin(square)", "Star",
            "AxisVar(y)", "Builtin(square)", "Plus",
            "Builtin(sin)", "LParen", "AxisVar(x)", "RParen", "Builtin(cube)", "Minus",
            "Builtin(sin)", "LParen", "AxisVar(y)", "Builtin(square)", "RParen", "Builtin(square)",
            "Eq", "Number(9)", "Eof",
        ];

        assert_eq!(
            tokens.len(), expected.len(),
            "Expected {} tokens, got {}: {:?}", expected.len(), tokens.len(),
            tokens.iter().map(|t| format!("{:?}", t.ty)).collect::<Vec<_>>()
        );

        // Verify key tokens
        assert!(matches!(tokens[0].ty, TokenType::AxisVar(ref s) if s == "x"));
        assert!(matches!(tokens[1].ty, TokenType::Builtin(ref s) if s == "square"));
        assert_eq!(tokens[2].ty, TokenType::Star);
        assert!(matches!(tokens[3].ty, TokenType::AxisVar(ref s) if s == "y"));
        assert!(matches!(tokens[4].ty, TokenType::Builtin(ref s) if s == "square"));
        assert_eq!(tokens[5].ty, TokenType::Plus);
        assert!(matches!(tokens[6].ty, TokenType::Builtin(ref s) if s == "sin"));
        assert_eq!(tokens[7].ty, TokenType::LParen);
        assert!(matches!(tokens[8].ty, TokenType::AxisVar(ref s) if s == "x"));
        assert_eq!(tokens[9].ty, TokenType::RParen);
        assert!(matches!(tokens[10].ty, TokenType::Builtin(ref s) if s == "cube"));
        assert_eq!(tokens[11].ty, TokenType::Minus);
        assert!(matches!(tokens[12].ty, TokenType::Builtin(ref s) if s == "sin"));
        assert_eq!(tokens[13].ty, TokenType::LParen);
        assert!(matches!(tokens[14].ty, TokenType::AxisVar(ref s) if s == "y"));
        assert!(matches!(tokens[15].ty, TokenType::Builtin(ref s) if s == "square"));
        assert_eq!(tokens[16].ty, TokenType::RParen);
        assert!(matches!(tokens[17].ty, TokenType::Builtin(ref s) if s == "square"));
        assert_eq!(tokens[18].ty, TokenType::Eq);
        assert!(matches!(tokens[19].ty, TokenType::Number(n) if (n - 9.0).abs() < 1e-10));
    }
}
