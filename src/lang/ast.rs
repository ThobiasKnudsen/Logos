/// AST node for the Logos math language.
///
/// Following the Zig design: all operations are unified under `Apply`.
/// `a + b` → `Apply("add", [a, b])`
/// `sin(x)` → `Apply("sin", [x])`
/// `-x` → `Apply("neg", [x])`
#[derive(Debug, Clone)]
pub enum AstNode {
    /// Numeric literal
    Number(f64),

    /// Boolean literal
    BoolLit(bool),

    /// Variable reference
    Identifier(String),

    /// Unified operation node: name + arguments.
    /// Covers binary ops (add, sub, mul, div, pow, mod, eq, neq, lt, gt, lte, gte, and, or),
    /// unary ops (neg, not), and function calls (sin, cos, etc.)
    Apply {
        name: String,
        args: Vec<AstNode>,
    },

    /// Tuple literal: (a, b, c)
    Tuple(Vec<AstNode>),

    /// Variable binding: `name = expr` or `name: expr`
    Binding {
        name: String,
        value: Box<AstNode>,
    },

    /// Block: sequence of statements; last is the return value
    Block(Vec<AstNode>),

    /// If expression: `if cond then_branch else else_branch`
    IfExpr {
        condition: Box<AstNode>,
        then_branch: Box<AstNode>,
        else_branch: Option<Box<AstNode>>,
    },

    /// Function definition: `f(x, y) = body`
    FunctionDef {
        name: String,
        params: Vec<String>,
        body: Box<AstNode>,
    },

    /// For loop: `for(init, condition, update) body`
    ForLoop {
        init: Box<AstNode>,
        condition: Box<AstNode>,
        update: Box<AstNode>,
        body: Box<AstNode>,
    },

    /// While loop: `while (condition) body`
    WhileLoop {
        condition: Box<AstNode>,
        body: Box<AstNode>,
    },

    /// Property access: `x.min`, `x.max`, etc.
    PropertyAccess {
        object: Box<AstNode>,
        property: String,
    },

    /// Tuple destructuring binding: `(a, b): expr`
    TupleBinding {
        names: Vec<String>,
        value: Box<AstNode>,
    },
}
