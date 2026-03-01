use super::token::{Token, TokenType};

const BUILTINS: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh",
    "log", "log2", "log10", "exp", "exp2",
    "floor", "ceil", "round", "fract",
    "abs", "sign", "pow", "sqrt", "mod", "min", "max",
    "clamp", "mix", "step", "smoothstep",
    "length", "normalize", "dot", "cross",
];

const AXIS_VARS: &[&str] = &["x", "y", "z", "time"];

const TYPE_NAMES: &[(&str, TokenType)] = &[
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

        // Identifiers / keywords / builtins
        if ch.is_alphabetic() || ch == '_' {
            return Ok(self.lex_identifier(start));
        }

        // Unicode math symbols
        if ch == '\u{00B2}' { // ² superscript
            self.advance();
            return Ok(Token { ty: TokenType::Builtin("square".to_string()), span: (start, self.pos) });
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
        while self.pos < self.input.len() && self.peek().map_or(false, |c| c.is_alphanumeric() || c == '_') {
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
}
