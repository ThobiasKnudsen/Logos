#[derive(Debug, Clone, PartialEq)]
pub enum TokenType {
    // Literals
    Number(f64),
    Identifier(String),
    BoolLit(bool),

    // Keywords
    If,
    Else,
    For,
    While,
    And,
    Or,
    Not,

    // Types
    TypeF32,
    TypeF64,
    TypeI32,
    TypeVec2,
    TypeVec3,
    TypeVec4,
    TypeBool,

    // Builtins
    Builtin(String),

    // Axis / special variables
    AxisVar(String), // x, y, z, time

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,      // ^ (power)
    Eq,         // = (equality in Logos, not assignment)
    Neq,        // !=
    Lt,         // <
    Gt,         // >
    Lte,        // <=
    Gte,        // >=

    // Binding
    Colon,      // : (binding/definition operator)

    // Punctuation
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Dot,
    Newline,    // Statement separator at top level

    // Special
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub ty: TokenType,
    pub span: (usize, usize), // (start, end) byte offsets
}
