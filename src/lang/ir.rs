//! Logos IR — the unified intermediate representation produced by the parser
//! and consumed by every downstream backend.
//!
//! This is the language's single shared currency:
//! - `wgsl_gen` lowers it to WGSL for the GPU fragment/compute paths.
//! - `interpreter` walks it on the CPU today; the planned cranelift JIT will
//!   lower it to machine code from this same shape.
//! - `SymbolicSimplifier` (REDUCE today, Lean planned) consumes IR and
//!   returns IR; the simplified subtree is spliced back into the cell's IR.
//!
//! It started life as a parser AST and is still mostly that shape — operators
//! and known builtins are unified under `Apply` with a typed `Callee`, but it
//! still lacks scope-resolved identifiers and per-expression type annotations.
//! Those enrichments land when a backend (cranelift) demands them. Spans are
//! present on every variant.

/// Source span: byte offsets `(start, end)` into the original source text.
///
/// Spans are inclusive on the start and exclusive on the end, matching
/// `Token::span`. They are propagated from tokens through every IR node so
/// downstream tooling (error reporting, go-to-definition, hover info,
/// formatter, refactorings) can map IR nodes back to their source range.
pub type Span = (usize, usize);

/// Known built-in operations and functions in the Logos language.
///
/// Anything the parser, type system, or any backend recognizes by name lives
/// here. Adding a variant forces every consumer's `match` to be updated —
/// that exhaustiveness is the whole point.
///
/// Names not in this set come through as `Callee::User(String)`: user-defined
/// functions, symbolic CAS calls, and anything not yet implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinOp {
    // Arithmetic
    Add, Sub, Mul, Div, Mod, Pow, Neg,

    // Comparison
    Eq, Neq, Lt, Gt, Lte, Gte,

    // Logical
    And, Or, Not,

    // Trig
    Sin, Cos, Tan, Asin, Acos, Atan, Sinh, Cosh, Tanh,

    // Exp/log
    Log, Log2, Log10, Exp, Exp2,

    // Misc unary math
    Sqrt, Abs, Sign, Floor, Ceil, Round, Fract,

    // Binary math
    Min, Max, Step,

    // Ternary math
    Clamp, Mix, Smoothstep,

    // Vector math
    Length, Normalize, Dot, Cross,

    // Constructors
    Vec2, Vec3, Vec4,

    // Casts
    F32, F64, I32,

    // Array
    Len,

    // Actions (have side effects in the cell pipeline)
    Print, Plot,
}

impl BuiltinOp {
    /// Canonical lowercase name as the parser/IR has always emitted it.
    /// This is the surface form users write in source.
    pub fn name(self) -> &'static str {
        match self {
            Self::Add => "add", Self::Sub => "sub", Self::Mul => "mul",
            Self::Div => "div", Self::Mod => "mod", Self::Pow => "pow",
            Self::Neg => "neg",
            Self::Eq => "eq", Self::Neq => "neq",
            Self::Lt => "lt", Self::Gt => "gt",
            Self::Lte => "lte", Self::Gte => "gte",
            Self::And => "and", Self::Or => "or", Self::Not => "not",
            Self::Sin => "sin", Self::Cos => "cos", Self::Tan => "tan",
            Self::Asin => "asin", Self::Acos => "acos", Self::Atan => "atan",
            Self::Sinh => "sinh", Self::Cosh => "cosh", Self::Tanh => "tanh",
            Self::Log => "log", Self::Log2 => "log2", Self::Log10 => "log10",
            Self::Exp => "exp", Self::Exp2 => "exp2",
            Self::Sqrt => "sqrt", Self::Abs => "abs", Self::Sign => "sign",
            Self::Floor => "floor", Self::Ceil => "ceil",
            Self::Round => "round", Self::Fract => "fract",
            Self::Min => "min", Self::Max => "max", Self::Step => "step",
            Self::Clamp => "clamp", Self::Mix => "mix",
            Self::Smoothstep => "smoothstep",
            Self::Length => "length", Self::Normalize => "normalize",
            Self::Dot => "dot", Self::Cross => "cross",
            Self::Vec2 => "vec2", Self::Vec3 => "vec3", Self::Vec4 => "vec4",
            Self::F32 => "f32", Self::F64 => "f64", Self::I32 => "i32",
            Self::Len => "len",
            Self::Print => "print", Self::Plot => "plot",
        }
    }

    /// Map a surface name back to the builtin variant, if known.
    /// The parser uses this to classify identifier-call targets.
    pub fn from_name(s: &str) -> Option<Self> {
        let op = match s {
            "add" => Self::Add, "sub" => Self::Sub, "mul" => Self::Mul,
            "div" => Self::Div, "mod" => Self::Mod, "pow" => Self::Pow,
            "neg" => Self::Neg,
            "eq" => Self::Eq, "neq" => Self::Neq,
            "lt" => Self::Lt, "gt" => Self::Gt,
            "lte" => Self::Lte, "gte" => Self::Gte,
            "and" => Self::And, "or" => Self::Or, "not" => Self::Not,
            "sin" => Self::Sin, "cos" => Self::Cos, "tan" => Self::Tan,
            "asin" => Self::Asin, "acos" => Self::Acos, "atan" => Self::Atan,
            "sinh" => Self::Sinh, "cosh" => Self::Cosh, "tanh" => Self::Tanh,
            "log" => Self::Log, "log2" => Self::Log2, "log10" => Self::Log10,
            "exp" => Self::Exp, "exp2" => Self::Exp2,
            "sqrt" => Self::Sqrt, "abs" => Self::Abs, "sign" => Self::Sign,
            "floor" => Self::Floor, "ceil" => Self::Ceil,
            "round" => Self::Round, "fract" => Self::Fract,
            "min" => Self::Min, "max" => Self::Max, "step" => Self::Step,
            "clamp" => Self::Clamp, "mix" => Self::Mix,
            "smoothstep" => Self::Smoothstep,
            "length" => Self::Length, "normalize" => Self::Normalize,
            "dot" => Self::Dot, "cross" => Self::Cross,
            "vec2" => Self::Vec2, "vec3" => Self::Vec3, "vec4" => Self::Vec4,
            "f32" => Self::F32, "f64" => Self::F64, "i32" => Self::I32,
            "len" => Self::Len,
            "print" => Self::Print, "plot" => Self::Plot,
            _ => return None,
        };
        Some(op)
    }
}

/// Resolved callee of an `Apply` node.
///
/// Either a known builtin (matched exhaustively by consumers) or a
/// user-defined or unrecognized name (carried as a string until name
/// resolution lands).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Callee {
    Builtin(BuiltinOp),
    User(String),
}

impl Callee {
    /// Build a callee from a surface name: known builtins become `Builtin`,
    /// everything else becomes `User`.
    pub fn from_name(name: String) -> Self {
        match BuiltinOp::from_name(&name) {
            Some(op) => Self::Builtin(op),
            None => Self::User(name),
        }
    }

    /// Surface name for this callee — `BuiltinOp::name()` for builtins,
    /// the stored string for user names. Bridge for consumers that don't
    /// yet match on the enum directly.
    pub fn name(&self) -> &str {
        match self {
            Self::Builtin(op) => op.name(),
            Self::User(name) => name.as_str(),
        }
    }
}

/// IR node for the Logos math language.
///
/// Following the Zig design: all operations are unified under `Apply`.
/// `a + b` → `Apply(Builtin(Add), [a, b])`
/// `sin(x)` → `Apply(Builtin(Sin), [x])`
/// `-x` → `Apply(Builtin(Neg), [x])`
/// `f(x)` → `Apply(User("f"), [x])`
///
/// Every variant carries a `span` covering the source range it was parsed
/// from. Use `Ir::span()` to read it without destructuring.
#[derive(Debug, Clone)]
pub enum Ir {
    /// Numeric literal
    Number { value: f64, span: Span },

    /// Boolean literal
    BoolLit { value: bool, span: Span },

    /// Variable reference
    Identifier { name: String, span: Span },

    /// Unified operation node: callee + arguments.
    /// Covers binary ops (add, sub, mul, div, pow, mod, eq, neq, lt, gt, lte, gte, and, or),
    /// unary ops (neg, not), and function calls (sin, cos, etc.)
    Apply {
        callee: Callee,
        args: Vec<Ir>,
        span: Span,
    },

    /// Tuple literal: (a, b, c)
    Tuple { items: Vec<Ir>, span: Span },

    /// Variable binding: `name := expr`
    Binding {
        name: String,
        value: Box<Ir>,
        span: Span,
    },

    /// Block: sequence of statements; last is the return value
    Block { items: Vec<Ir>, span: Span },

    /// If expression: `if cond then_branch else else_branch`
    IfExpr {
        condition: Box<Ir>,
        then_branch: Box<Ir>,
        else_branch: Option<Box<Ir>>,
        span: Span,
    },

    /// Function definition: `f(x, y) = body`
    FunctionDef {
        name: String,
        params: Vec<String>,
        body: Box<Ir>,
        span: Span,
    },

    /// For loop: `for i in 0..n ( body )` — sequential CPU execution
    ForLoop {
        var: String,
        range: Box<Ir>,
        body: Box<Ir>,
        span: Span,
    },

    /// While loop: `while (condition) body`
    WhileLoop {
        condition: Box<Ir>,
        body: Box<Ir>,
        span: Span,
    },

    /// Property access: `x.min`, `x.max`, etc.
    PropertyAccess {
        object: Box<Ir>,
        property: String,
        span: Span,
    },

    /// Tuple destructuring binding: `(a, b) := expr`
    TupleBinding {
        names: Vec<String>,
        value: Box<Ir>,
        span: Span,
    },

    /// Array literal: `[1, 2, 3]`
    ArrayLiteral { items: Vec<Ir>, span: Span },

    /// Array index access: `a[i]`
    IndexAccess {
        array: Box<Ir>,
        index: Box<Ir>,
        span: Span,
    },

    /// Range: `0..n`
    Range {
        start: Box<Ir>,
        end: Box<Ir>,
        span: Span,
    },

    /// Indexed assignment: `data[i]: data[i] * 2`
    IndexAssign {
        array: Box<Ir>,
        index: Box<Ir>,
        value: Box<Ir>,
        span: Span,
    },

    /// Parallel for: `parallel for i in 0..n ( body )`
    /// Mutates arrays in-place. Body contains IndexAssign statements.
    ParallelFor {
        var: String,
        range: Box<Ir>,
        body: Box<Ir>,
        span: Span,
    },
}

impl Ir {
    /// Source span this node was parsed from.
    pub fn span(&self) -> Span {
        match self {
            Ir::Number { span, .. }
            | Ir::BoolLit { span, .. }
            | Ir::Identifier { span, .. }
            | Ir::Apply { span, .. }
            | Ir::Tuple { span, .. }
            | Ir::Binding { span, .. }
            | Ir::Block { span, .. }
            | Ir::IfExpr { span, .. }
            | Ir::FunctionDef { span, .. }
            | Ir::ForLoop { span, .. }
            | Ir::WhileLoop { span, .. }
            | Ir::PropertyAccess { span, .. }
            | Ir::TupleBinding { span, .. }
            | Ir::ArrayLiteral { span, .. }
            | Ir::IndexAccess { span, .. }
            | Ir::Range { span, .. }
            | Ir::IndexAssign { span, .. }
            | Ir::ParallelFor { span, .. } => *span,
        }
    }
}
