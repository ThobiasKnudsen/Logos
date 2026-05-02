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
//! are already unified under `Apply`, but it lacks type annotations and
//! scope-resolved identifiers. Those enrichments land when a backend
//! (cranelift) demands them. Spans are present on every variant.

/// Source span: byte offsets `(start, end)` into the original source text.
///
/// Spans are inclusive on the start and exclusive on the end, matching
/// `Token::span`. They are propagated from tokens through every IR node so
/// downstream tooling (error reporting, go-to-definition, hover info,
/// formatter, refactorings) can map IR nodes back to their source range.
pub type Span = (usize, usize);

/// IR node for the Logos math language.
///
/// Following the Zig design: all operations are unified under `Apply`.
/// `a + b` → `Apply("add", [a, b])`
/// `sin(x)` → `Apply("sin", [x])`
/// `-x` → `Apply("neg", [x])`
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

    /// Unified operation node: name + arguments.
    /// Covers binary ops (add, sub, mul, div, pow, mod, eq, neq, lt, gt, lte, gte, and, or),
    /// unary ops (neg, not), and function calls (sin, cos, etc.)
    Apply {
        name: String,
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
