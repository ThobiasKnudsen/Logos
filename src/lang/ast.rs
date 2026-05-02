/// Source span: byte offsets `(start, end)` into the original source text.
///
/// Spans are inclusive on the start and exclusive on the end, matching
/// `Token::span`. They are propagated from tokens through every AST node so
/// downstream tooling (error reporting, go-to-definition, hover info,
/// formatter, refactorings) can map AST nodes back to their source range.
pub type Span = (usize, usize);

/// AST node for the Logos math language.
///
/// Following the Zig design: all operations are unified under `Apply`.
/// `a + b` → `Apply("add", [a, b])`
/// `sin(x)` → `Apply("sin", [x])`
/// `-x` → `Apply("neg", [x])`
///
/// Every variant carries a `span` covering the source range it was parsed
/// from. Use `AstNode::span()` to read it without destructuring.
#[derive(Debug, Clone)]
pub enum AstNode {
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
        args: Vec<AstNode>,
        span: Span,
    },

    /// Tuple literal: (a, b, c)
    Tuple { items: Vec<AstNode>, span: Span },

    /// Variable binding: `name := expr`
    Binding {
        name: String,
        value: Box<AstNode>,
        span: Span,
    },

    /// Block: sequence of statements; last is the return value
    Block { items: Vec<AstNode>, span: Span },

    /// If expression: `if cond then_branch else else_branch`
    IfExpr {
        condition: Box<AstNode>,
        then_branch: Box<AstNode>,
        else_branch: Option<Box<AstNode>>,
        span: Span,
    },

    /// Function definition: `f(x, y) = body`
    FunctionDef {
        name: String,
        params: Vec<String>,
        body: Box<AstNode>,
        span: Span,
    },

    /// For loop: `for i in 0..n ( body )` — sequential CPU execution
    ForLoop {
        var: String,
        range: Box<AstNode>,
        body: Box<AstNode>,
        span: Span,
    },

    /// While loop: `while (condition) body`
    WhileLoop {
        condition: Box<AstNode>,
        body: Box<AstNode>,
        span: Span,
    },

    /// Property access: `x.min`, `x.max`, etc.
    PropertyAccess {
        object: Box<AstNode>,
        property: String,
        span: Span,
    },

    /// Tuple destructuring binding: `(a, b) := expr`
    TupleBinding {
        names: Vec<String>,
        value: Box<AstNode>,
        span: Span,
    },

    /// Array literal: `[1, 2, 3]`
    ArrayLiteral { items: Vec<AstNode>, span: Span },

    /// Array index access: `a[i]`
    IndexAccess {
        array: Box<AstNode>,
        index: Box<AstNode>,
        span: Span,
    },

    /// Range: `0..n`
    Range {
        start: Box<AstNode>,
        end: Box<AstNode>,
        span: Span,
    },

    /// Indexed assignment: `data[i]: data[i] * 2`
    IndexAssign {
        array: Box<AstNode>,
        index: Box<AstNode>,
        value: Box<AstNode>,
        span: Span,
    },

    /// Parallel for: `parallel for i in 0..n ( body )`
    /// Mutates arrays in-place. Body contains IndexAssign statements.
    ParallelFor {
        var: String,
        range: Box<AstNode>,
        body: Box<AstNode>,
        span: Span,
    },
}

impl AstNode {
    /// Source span this node was parsed from.
    pub fn span(&self) -> Span {
        match self {
            AstNode::Number { span, .. }
            | AstNode::BoolLit { span, .. }
            | AstNode::Identifier { span, .. }
            | AstNode::Apply { span, .. }
            | AstNode::Tuple { span, .. }
            | AstNode::Binding { span, .. }
            | AstNode::Block { span, .. }
            | AstNode::IfExpr { span, .. }
            | AstNode::FunctionDef { span, .. }
            | AstNode::ForLoop { span, .. }
            | AstNode::WhileLoop { span, .. }
            | AstNode::PropertyAccess { span, .. }
            | AstNode::TupleBinding { span, .. }
            | AstNode::ArrayLiteral { span, .. }
            | AstNode::IndexAccess { span, .. }
            | AstNode::Range { span, .. }
            | AstNode::IndexAssign { span, .. }
            | AstNode::ParallelFor { span, .. } => *span,
        }
    }
}
