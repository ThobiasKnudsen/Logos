use std::fmt;

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
    Parallel,
    In,

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
    Colon,      // := (binding/definition operator)

    // Punctuation
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Dot,
    DotDot,     // .. (range)
    Newline,    // Statement separator at top level

    // Special
    Eof,
}

impl fmt::Display for TokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenType::Number(n) => write!(f, "{n}"),
            TokenType::Identifier(s) => write!(f, "'{s}'"),
            TokenType::BoolLit(b) => write!(f, "{b}"),
            TokenType::If => write!(f, "'if'"),
            TokenType::Else => write!(f, "'else'"),
            TokenType::For => write!(f, "'for'"),
            TokenType::While => write!(f, "'while'"),
            TokenType::And => write!(f, "'and'"),
            TokenType::Or => write!(f, "'or'"),
            TokenType::Not => write!(f, "'not'"),
            TokenType::Parallel => write!(f, "'parallel'"),
            TokenType::In => write!(f, "'in'"),
            TokenType::TypeF32 => write!(f, "'f32'"),
            TokenType::TypeF64 => write!(f, "'f64'"),
            TokenType::TypeI32 => write!(f, "'i32'"),
            TokenType::TypeVec2 => write!(f, "'vec2'"),
            TokenType::TypeVec3 => write!(f, "'vec3'"),
            TokenType::TypeVec4 => write!(f, "'vec4'"),
            TokenType::TypeBool => write!(f, "'bool'"),
            TokenType::Builtin(s) => write!(f, "'{s}'"),
            TokenType::AxisVar(s) => write!(f, "'{s}'"),
            TokenType::Plus => write!(f, "'+'"),
            TokenType::Minus => write!(f, "'-'"),
            TokenType::Star => write!(f, "'*'"),
            TokenType::Slash => write!(f, "'/'"),
            TokenType::Percent => write!(f, "'%'"),
            TokenType::Caret => write!(f, "'^'"),
            TokenType::Eq => write!(f, "'='"),
            TokenType::Neq => write!(f, "'!='"),
            TokenType::Lt => write!(f, "'<'"),
            TokenType::Gt => write!(f, "'>'"),
            TokenType::Lte => write!(f, "'<='"),
            TokenType::Gte => write!(f, "'>='"),
            TokenType::Colon => write!(f, "':='"),
            TokenType::LParen => write!(f, "'('"),
            TokenType::RParen => write!(f, "')'"),
            TokenType::LBracket => write!(f, "'['"),
            TokenType::RBracket => write!(f, "']'"),
            TokenType::Comma => write!(f, "','"),
            TokenType::Dot => write!(f, "'.'"),
            TokenType::DotDot => write!(f, "'..'"),
            TokenType::Newline => write!(f, "newline"),
            TokenType::Eof => write!(f, "end of input"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub ty: TokenType,
    pub span: (usize, usize), // (start, end) byte offsets
}
