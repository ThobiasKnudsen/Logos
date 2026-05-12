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

/// How a builtin maps to WGSL source. Drives the generic dispatch in
/// `wgsl_gen::emit_apply_builtin`; ops whose lowering doesn't fit one of
/// the shape variants get `Custom` and stay in the per-op branch.
#[derive(Debug, Clone, Copy)]
pub enum WgslLowering {
    /// `(a OP b)` — binary infix. Used for `+`, `==`, `&&`, etc.
    Infix(&'static str),
    /// `OP(a)` — unary prefix. Used for `!`, `-`.
    Prefix(&'static str),
    /// `name(arg, arg, ...)` — direct WGSL call. Covers ordinary math
    /// functions, casts (`f32(x)`), and vector constructors (`vec3<f32>(...)`)
    /// since they all share the call syntax.
    Call(&'static str),
    /// Per-op logic in `wgsl_gen` (e.g. `pow` with int specialization,
    /// `atan`'s 1-or-2-arg overloads, builtins with no fragment semantics).
    Custom,
}

/// How a builtin evaluates on the CPU interpreter. Drives the generic
/// dispatch in `interpreter::eval_builtin`; ops with non-`f64` arguments,
/// arity > 2, special error paths, or no interpreter support get `Custom`.
#[derive(Debug, Clone, Copy)]
pub enum EvalImpl {
    /// `fn(x: f64) -> f64`. Unary numeric — `sin`, `sqrt`, `abs`, `neg`, …
    UnaryF(fn(f64) -> f64),
    /// `fn(a: f64, b: f64) -> f64`. Binary numeric — `+`, `*`, `pow`,
    /// `min`, `max`, `step`.
    BinaryF(fn(f64, f64) -> f64),
    /// `fn(a: f64, b: f64) -> bool`. Binary numeric comparison — `<`, `>=`,
    /// `==`, `!=`, etc. (`Eq`/`Neq` use an epsilon tolerance.)
    CmpF(fn(f64, f64) -> bool),
    /// Per-op logic in `interpreter` (bool args, ternary math, casts, array
    /// ops, action ops, and vector ops which are not yet supported on CPU).
    Custom,
}

/// Generate `BuiltinOp` and its lookup tables from a single table.
///
/// Each row is `Variant => "surface_name", <WgslLowering>, <EvalImpl>;`.
/// The expansion produces the enum plus `name()` / `from_name()` /
/// `wgsl_lowering()` / `eval_impl()` using `match`, so every mapping is
/// exhaustive at compile time and adding a new builtin is a one-line
/// change with no risk of forgetting one direction.
macro_rules! define_builtins {
    ( $( $variant:ident => $name:literal, $wgsl:expr, $eval:expr );* $(;)? ) => {
        /// Known built-in operations and functions in the Logos language.
        ///
        /// Anything the parser, type system, or any backend recognizes by
        /// name lives here. Names not in this set come through as
        /// `Callee::User(String)`: user-defined functions, symbolic CAS
        /// calls, and anything not yet implemented.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum BuiltinOp {
            $( $variant, )*
        }

        impl BuiltinOp {
            /// Canonical lowercase name as the parser/IR has always emitted
            /// it. This is the surface form users write in source.
            pub fn name(self) -> &'static str {
                match self {
                    $( Self::$variant => $name, )*
                }
            }

            /// Map a surface name back to the builtin variant, if known.
            /// The parser uses this to classify identifier-call targets.
            pub fn from_name(s: &str) -> Option<Self> {
                match s {
                    $( $name => Some(Self::$variant), )*
                    _ => None,
                }
            }

            /// How this op lowers to WGSL. The wgsl_gen Apply dispatcher
            /// handles `Infix`/`Prefix`/`Call` generically; `Custom` ops
            /// fall through to per-op branches.
            pub fn wgsl_lowering(self) -> WgslLowering {
                match self {
                    $( Self::$variant => $wgsl, )*
                }
            }

            /// How this op evaluates on the CPU interpreter. The interpreter
            /// dispatcher handles `UnaryF`/`BinaryF`/`CmpF` generically;
            /// `Custom` ops fall through to per-op branches.
            pub fn eval_impl(self) -> EvalImpl {
                match self {
                    $( Self::$variant => $eval, )*
                }
            }
        }
    }
}

define_builtins! {
    // Arithmetic — `mod` uses a custom modulo formula, `pow` has an int
    // specialization for cheap repeated multiplication of `x²` etc. `div`
    // and `mod` need runtime divide-by-zero checks on CPU.
    Add  => "add", WgslLowering::Infix("+"),  EvalImpl::BinaryF(|a, b| a + b);
    Sub  => "sub", WgslLowering::Infix("-"),  EvalImpl::BinaryF(|a, b| a - b);
    Mul  => "mul", WgslLowering::Infix("*"),  EvalImpl::BinaryF(|a, b| a * b);
    Div  => "div", WgslLowering::Infix("/"),  EvalImpl::Custom;
    Mod  => "mod", WgslLowering::Custom,      EvalImpl::Custom;
    Pow  => "pow", WgslLowering::Custom,      EvalImpl::BinaryF(f64::powf);
    Neg  => "neg", WgslLowering::Prefix("-"), EvalImpl::UnaryF(|x| -x);

    // Comparison — `Eq`/`Neq` use an epsilon tolerance to keep float plots
    // robust to ULP-level noise.
    Eq   => "eq",  WgslLowering::Infix("=="), EvalImpl::CmpF(|a, b| (a - b).abs() < 1e-10);
    Neq  => "neq", WgslLowering::Infix("!="), EvalImpl::CmpF(|a, b| (a - b).abs() >= 1e-10);
    Lt   => "lt",  WgslLowering::Infix("<"),  EvalImpl::CmpF(|a, b| a < b);
    Gt   => "gt",  WgslLowering::Infix(">"),  EvalImpl::CmpF(|a, b| a > b);
    Lte  => "lte", WgslLowering::Infix("<="), EvalImpl::CmpF(|a, b| a <= b);
    Gte  => "gte", WgslLowering::Infix(">="), EvalImpl::CmpF(|a, b| a >= b);

    // Logical — bool args, can't reuse the f64 dispatchers.
    And  => "and", WgslLowering::Infix("&&"), EvalImpl::Custom;
    Or   => "or",  WgslLowering::Infix("||"), EvalImpl::Custom;
    Not  => "not", WgslLowering::Prefix("!"), EvalImpl::Custom;

    // Trig — WGSL's `atan` is overloaded (1 or 2 args); the CPU interpreter
    // handles only the 1-arg form.
    Sin  => "sin",  WgslLowering::Call("sin"),  EvalImpl::UnaryF(f64::sin);
    Cos  => "cos",  WgslLowering::Call("cos"),  EvalImpl::UnaryF(f64::cos);
    Tan  => "tan",  WgslLowering::Call("tan"),  EvalImpl::UnaryF(f64::tan);
    Asin => "asin", WgslLowering::Call("asin"), EvalImpl::UnaryF(f64::asin);
    Acos => "acos", WgslLowering::Call("acos"), EvalImpl::UnaryF(f64::acos);
    Atan => "atan", WgslLowering::Custom,       EvalImpl::UnaryF(f64::atan);
    Sinh => "sinh", WgslLowering::Call("sinh"), EvalImpl::UnaryF(f64::sinh);
    Cosh => "cosh", WgslLowering::Call("cosh"), EvalImpl::UnaryF(f64::cosh);
    Tanh => "tanh", WgslLowering::Call("tanh"), EvalImpl::UnaryF(f64::tanh);

    // Exp/log — WGSL's `log10` is expanded to `log2(x) / log2(10)`; the
    // CPU interpreter uses the native `f64::log10`.
    Log   => "log",   WgslLowering::Call("log"),  EvalImpl::UnaryF(f64::ln);
    Log2  => "log2",  WgslLowering::Call("log2"), EvalImpl::UnaryF(f64::log2);
    Log10 => "log10", WgslLowering::Custom,       EvalImpl::UnaryF(f64::log10);
    Exp   => "exp",   WgslLowering::Call("exp"),  EvalImpl::UnaryF(f64::exp);
    Exp2  => "exp2",  WgslLowering::Call("exp2"), EvalImpl::UnaryF(f64::exp2);

    // Misc unary math
    Sqrt  => "sqrt",  WgslLowering::Call("sqrt"),  EvalImpl::UnaryF(f64::sqrt);
    Abs   => "abs",   WgslLowering::Call("abs"),   EvalImpl::UnaryF(f64::abs);
    Sign  => "sign",  WgslLowering::Call("sign"),  EvalImpl::UnaryF(f64::signum);
    Floor => "floor", WgslLowering::Call("floor"), EvalImpl::UnaryF(f64::floor);
    Ceil  => "ceil",  WgslLowering::Call("ceil"),  EvalImpl::UnaryF(f64::ceil);
    Round => "round", WgslLowering::Call("round"), EvalImpl::UnaryF(f64::round);
    Fract => "fract", WgslLowering::Call("fract"), EvalImpl::UnaryF(f64::fract);

    // Binary math
    Min  => "min",  WgslLowering::Call("min"),  EvalImpl::BinaryF(f64::min);
    Max  => "max",  WgslLowering::Call("max"),  EvalImpl::BinaryF(f64::max);
    Step => "step", WgslLowering::Call("step"), EvalImpl::BinaryF(|edge, x| if x < edge { 0.0 } else { 1.0 });

    // Ternary math — arity 3, doesn't fit the binary dispatchers.
    Clamp      => "clamp",      WgslLowering::Call("clamp"),      EvalImpl::Custom;
    Mix        => "mix",        WgslLowering::Call("mix"),        EvalImpl::Custom;
    Smoothstep => "smoothstep", WgslLowering::Call("smoothstep"), EvalImpl::Custom;

    // Vector math — currently no CPU interpreter support.
    Length    => "length",    WgslLowering::Call("length"),    EvalImpl::Custom;
    Normalize => "normalize", WgslLowering::Call("normalize"), EvalImpl::Custom;
    Dot       => "dot",       WgslLowering::Call("dot"),       EvalImpl::Custom;
    Cross     => "cross",     WgslLowering::Call("cross"),     EvalImpl::Custom;

    // Constructors — no CPU representation today.
    Vec2 => "vec2", WgslLowering::Call("vec2<f32>"), EvalImpl::Custom;
    Vec3 => "vec3", WgslLowering::Call("vec3<f32>"), EvalImpl::Custom;
    Vec4 => "vec4", WgslLowering::Call("vec4<f32>"), EvalImpl::Custom;

    // Casts — `f64` lowers to `f32` since WGSL has no f64. CPU keeps
    // everything as f64 internally; `i32` truncates.
    F32 => "f32", WgslLowering::Call("f32"), EvalImpl::Custom;
    F64 => "f64", WgslLowering::Call("f32"), EvalImpl::Custom;
    I32 => "i32", WgslLowering::Call("i32"), EvalImpl::Custom;

    // Array
    Len => "len", WgslLowering::Custom, EvalImpl::Custom;

    // Actions (have side effects in the cell pipeline; not fragment-shader-emittable)
    Print => "print", WgslLowering::Custom, EvalImpl::Custom;
    Plot  => "plot",  WgslLowering::Custom, EvalImpl::Custom;
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

/// Stable identity of a `Binding` after `lower::resolve_names` has run.
/// Backends use this to look up the corresponding declaration without
/// re-running scope resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindingId(pub u32);

/// Stable identity of a `FunctionDef` (including lifted lambdas and HOF
/// specializations), assigned by `lower::resolve_names`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncId(pub u32);

/// What an `Ir::Identifier` actually refers to, populated by
/// `lower::resolve_names`. The parser leaves `Identifier.resolved = None`;
/// after lowering, every identifier reaching a backend has a `Resolution`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Resolution {
    /// A `let`-style binding in some enclosing block.
    LocalBinding(BindingId),
    /// A user-defined function (`f(x) := …`).
    FunctionDef(FuncId),
    /// A function synthesized by `lower::lift_lambdas` from an
    /// `Ir::Lambda` (or by HOF specialization).
    LiftedLambda(FuncId),
    /// A binding synthesized by `lower::hoist_anonymous_blocks` for an
    /// inline imperative block in expression position.
    LiftedBlock(BindingId),
    /// A function parameter, by zero-based position within its enclosing
    /// `FunctionDef.params`.
    Param(u32),
    /// The implicit axis variables `x` / `y` available in plot contexts.
    AxisVar,
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
///
/// Some variants carry additional analysis annotations (e.g.
/// `Identifier.resolved`) that the parser leaves `None` and the lowering
/// pass populates. Backends consume these with `.expect(...)`-style reads.
#[derive(Debug, Clone)]
pub enum Ir {
    /// Numeric literal
    Number { value: f64, span: Span },

    /// Boolean literal
    BoolLit { value: bool, span: Span },

    /// Variable reference. `resolved` is `None` at parse time and gets
    /// populated by `lower::resolve_names`; backends read it with
    /// `.expect("must be lowered")`.
    Identifier {
        name: String,
        span: Span,
        resolved: Option<Resolution>,
    },

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

    /// Anonymous function: `x |-> body` or `(a, b) |-> body`.
    /// Only meaningful as an argument to a higher-order user function — the
    /// codegen's specialization pass lifts it into a synthetic FunctionDef.
    Lambda {
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
            | Ir::Lambda { span, .. }
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

    /// Pretty-print this IR back to Logos source code.
    ///
    /// Output should be syntactically valid Logos that, re-parsed, yields
    /// equivalent IR. Used by `Notebook` to splice REDUCE's simplified IR
    /// back into the cell text and to format `print` results for display.
    ///
    /// Top-level binary operators are emitted bare (no enclosing parens) so
    /// the result reads naturally; sub-expressions get parenthesized so
    /// substitution into a larger context doesn't change precedence. The
    /// rule is "over-parenthesize when nested, never at the root."
    ///
    /// Variants that the parser produces but `SymbolicSimplifier` results
    /// never contain (block, function def, loops, indexed assigns, parallel
    /// for, …) are still handled: future callers may pretty-print arbitrary
    /// IR, and an incomplete printer would silently corrupt their output.
    pub fn to_source(&self) -> String {
        let mut out = String::new();
        write_ir(&mut out, self, false);
        out
    }

    /// Direct children of this node, in a stable left-to-right order. Used
    /// by `find_path` / `get_at_path` / `replace_at_path` to address sub-
    /// expressions by their child index. Leaves return an empty slice.
    ///
    /// Order is field-declaration order for struct-shaped variants
    /// (e.g. IfExpr → condition, then_branch, else_branch?). For
    /// vec-shaped variants (Apply.args, Block.items, Tuple.items,
    /// ArrayLiteral.items) the order is the vec order.
    pub fn children(&self) -> Vec<&Ir> {
        match self {
            Ir::Number { .. } | Ir::BoolLit { .. } | Ir::Identifier { .. } => Vec::new(),
            Ir::Apply { args, .. } => args.iter().collect(),
            Ir::Tuple { items, .. } | Ir::ArrayLiteral { items, .. } | Ir::Block { items, .. } => {
                items.iter().collect()
            }
            Ir::Binding { value, .. } | Ir::TupleBinding { value, .. } => vec![value],
            Ir::IfExpr {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                let mut v = vec![condition.as_ref(), then_branch.as_ref()];
                if let Some(eb) = else_branch {
                    v.push(eb);
                }
                v
            }
            Ir::FunctionDef { body, .. } => vec![body],
            Ir::Lambda { body, .. } => vec![body],
            Ir::ForLoop { range, body, .. } => vec![range, body],
            Ir::WhileLoop { condition, body, .. } => vec![condition, body],
            Ir::PropertyAccess { object, .. } => vec![object],
            Ir::IndexAccess { array, index, .. } => vec![array, index],
            Ir::Range { start, end, .. } => vec![start, end],
            Ir::IndexAssign {
                array, index, value, ..
            } => vec![array, index, value],
            Ir::ParallelFor { range, body, .. } => vec![range, body],
        }
    }

    /// Find the path to the first node (in left-most pre-order traversal)
    /// for which `pred` returns true. The empty path matches `self`. Returns
    /// `None` if no node matches.
    pub fn find_path(&self, pred: &mut dyn FnMut(&Ir) -> bool) -> Option<Vec<usize>> {
        if pred(self) {
            return Some(Vec::new());
        }
        for (i, child) in self.children().into_iter().enumerate() {
            if let Some(mut p) = child.find_path(pred) {
                p.insert(0, i);
                return Some(p);
            }
        }
        None
    }

    /// Resolve a path produced by `find_path`. Empty path returns `self`.
    pub fn get_at_path(&self, path: &[usize]) -> Option<&Ir> {
        let mut cur = self;
        for &i in path {
            cur = *cur.children().get(i)?;
        }
        Some(cur)
    }

    /// Return a copy of `self` with the node at `path` replaced by
    /// `replacement`. Empty path replaces the whole tree. Returns `None` if
    /// the path doesn't resolve.
    pub fn replace_at_path(&self, path: &[usize], replacement: Ir) -> Option<Ir> {
        replace_at_path_inner(self.clone(), path, replacement)
    }

    /// Substitute every `Identifier` whose name appears in `bindings` with
    /// the corresponding IR value (cloned). Used for binding expansion
    /// before submitting a sub-expression to the symbolic simplifier — the
    /// IR-level analogue of the old text-level `expand_bindings`.
    pub fn substitute_idents(&self, bindings: &[(String, Ir)]) -> Ir {
        substitute_idents_inner(self, bindings)
    }
}

fn replace_at_path_inner(node: Ir, path: &[usize], replacement: Ir) -> Option<Ir> {
    if path.is_empty() {
        return Some(replacement);
    }
    let head = path[0];
    let tail = &path[1..];
    match node {
        Ir::Apply { callee, mut args, span } => {
            let child = args.get(head)?.clone();
            let new_child = replace_at_path_inner(child, tail, replacement)?;
            args[head] = new_child;
            Some(Ir::Apply { callee, args, span })
        }
        Ir::Tuple { mut items, span } => {
            let child = items.get(head)?.clone();
            items[head] = replace_at_path_inner(child, tail, replacement)?;
            Some(Ir::Tuple { items, span })
        }
        Ir::ArrayLiteral { mut items, span } => {
            let child = items.get(head)?.clone();
            items[head] = replace_at_path_inner(child, tail, replacement)?;
            Some(Ir::ArrayLiteral { items, span })
        }
        Ir::Block { mut items, span } => {
            let child = items.get(head)?.clone();
            items[head] = replace_at_path_inner(child, tail, replacement)?;
            Some(Ir::Block { items, span })
        }
        Ir::Binding { name, value, span } if head == 0 => {
            let new_value = replace_at_path_inner(*value, tail, replacement)?;
            Some(Ir::Binding {
                name,
                value: Box::new(new_value),
                span,
            })
        }
        Ir::TupleBinding { names, value, span } if head == 0 => {
            let new_value = replace_at_path_inner(*value, tail, replacement)?;
            Some(Ir::TupleBinding {
                names,
                value: Box::new(new_value),
                span,
            })
        }
        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            span,
        } => match head {
            0 => Some(Ir::IfExpr {
                condition: Box::new(replace_at_path_inner(*condition, tail, replacement)?),
                then_branch,
                else_branch,
                span,
            }),
            1 => Some(Ir::IfExpr {
                condition,
                then_branch: Box::new(replace_at_path_inner(*then_branch, tail, replacement)?),
                else_branch,
                span,
            }),
            2 => {
                let eb = else_branch?;
                Some(Ir::IfExpr {
                    condition,
                    then_branch,
                    else_branch: Some(Box::new(replace_at_path_inner(*eb, tail, replacement)?)),
                    span,
                })
            }
            _ => None,
        },
        Ir::FunctionDef {
            name,
            params,
            body,
            span,
        } if head == 0 => Some(Ir::FunctionDef {
            name,
            params,
            body: Box::new(replace_at_path_inner(*body, tail, replacement)?),
            span,
        }),
        Ir::ForLoop {
            var, range, body, span,
        } => match head {
            0 => Some(Ir::ForLoop {
                var,
                range: Box::new(replace_at_path_inner(*range, tail, replacement)?),
                body,
                span,
            }),
            1 => Some(Ir::ForLoop {
                var,
                range,
                body: Box::new(replace_at_path_inner(*body, tail, replacement)?),
                span,
            }),
            _ => None,
        },
        Ir::WhileLoop {
            condition, body, span,
        } => match head {
            0 => Some(Ir::WhileLoop {
                condition: Box::new(replace_at_path_inner(*condition, tail, replacement)?),
                body,
                span,
            }),
            1 => Some(Ir::WhileLoop {
                condition,
                body: Box::new(replace_at_path_inner(*body, tail, replacement)?),
                span,
            }),
            _ => None,
        },
        Ir::PropertyAccess {
            object, property, span,
        } if head == 0 => Some(Ir::PropertyAccess {
            object: Box::new(replace_at_path_inner(*object, tail, replacement)?),
            property,
            span,
        }),
        Ir::IndexAccess { array, index, span } => match head {
            0 => Some(Ir::IndexAccess {
                array: Box::new(replace_at_path_inner(*array, tail, replacement)?),
                index,
                span,
            }),
            1 => Some(Ir::IndexAccess {
                array,
                index: Box::new(replace_at_path_inner(*index, tail, replacement)?),
                span,
            }),
            _ => None,
        },
        Ir::Range { start, end, span } => match head {
            0 => Some(Ir::Range {
                start: Box::new(replace_at_path_inner(*start, tail, replacement)?),
                end,
                span,
            }),
            1 => Some(Ir::Range {
                start,
                end: Box::new(replace_at_path_inner(*end, tail, replacement)?),
                span,
            }),
            _ => None,
        },
        Ir::IndexAssign {
            array, index, value, span,
        } => match head {
            0 => Some(Ir::IndexAssign {
                array: Box::new(replace_at_path_inner(*array, tail, replacement)?),
                index,
                value,
                span,
            }),
            1 => Some(Ir::IndexAssign {
                array,
                index: Box::new(replace_at_path_inner(*index, tail, replacement)?),
                value,
                span,
            }),
            2 => Some(Ir::IndexAssign {
                array,
                index,
                value: Box::new(replace_at_path_inner(*value, tail, replacement)?),
                span,
            }),
            _ => None,
        },
        Ir::ParallelFor {
            var, range, body, span,
        } => match head {
            0 => Some(Ir::ParallelFor {
                var,
                range: Box::new(replace_at_path_inner(*range, tail, replacement)?),
                body,
                span,
            }),
            1 => Some(Ir::ParallelFor {
                var,
                range,
                body: Box::new(replace_at_path_inner(*body, tail, replacement)?),
                span,
            }),
            _ => None,
        },
        _ => None,
    }
}

fn substitute_idents_inner(node: &Ir, bindings: &[(String, Ir)]) -> Ir {
    match node {
        Ir::Identifier { name, .. } => {
            for (b_name, b_value) in bindings {
                if b_name == name {
                    // Function bindings (`f(x) := …` → FunctionDef in
                    // `collect_top_bindings`) are expanded on `Apply`, not
                    // on bare identifier references. Skip them here so
                    // `f` (as a value) doesn't get rewritten to its
                    // FunctionDef.
                    if matches!(b_value, Ir::FunctionDef { .. }) {
                        continue;
                    }
                    return b_value.clone();
                }
            }
            node.clone()
        }
        Ir::Number { .. } | Ir::BoolLit { .. } => node.clone(),
        Ir::Apply { callee, args, span } => {
            // First substitute identifiers inside each argument expression
            // (in the *call-site* scope — these are not yet inside the
            // function body).
            let new_args: Vec<Ir> = args
                .iter()
                .map(|a| substitute_idents_inner(a, bindings))
                .collect();
            // If the callee names a user-defined function we can see, inline
            // its body with parameters bound to the (already-substituted)
            // arguments. Required so CAS subtrees like `int(f(x), x)` reach
            // REDUCE as a closed-form expression instead of `int(f(x), x)`
            // with `f` left undeclared. Self-recursion is broken by skipping
            // the function's own name in the recursive binding list; mutual
            // recursion is not handled and would infinite-loop here (today
            // no Logos surface lets a user write it).
            if let Callee::User(name) = callee {
                if let Some(binding_value) =
                    bindings.iter().find(|(n, _)| n == name).map(|(_, v)| v)
                {
                    if let Ir::FunctionDef { params, body, .. } = binding_value {
                        if params.len() == new_args.len() {
                            let mut inlined: Vec<(String, Ir)> = params
                                .iter()
                                .zip(new_args.iter())
                                .map(|(p, a)| (p.clone(), a.clone()))
                                .collect();
                            for (n, v) in bindings {
                                if n != name && !params.contains(n) {
                                    inlined.push((n.clone(), v.clone()));
                                }
                            }
                            return substitute_idents_inner(body, &inlined);
                        }
                    }
                }
            }
            Ir::Apply {
                callee: callee.clone(),
                args: new_args,
                span: *span,
            }
        }
        Ir::Tuple { items, span } => Ir::Tuple {
            items: items.iter().map(|a| substitute_idents_inner(a, bindings)).collect(),
            span: *span,
        },
        Ir::ArrayLiteral { items, span } => Ir::ArrayLiteral {
            items: items.iter().map(|a| substitute_idents_inner(a, bindings)).collect(),
            span: *span,
        },
        Ir::Block { items, span } => Ir::Block {
            items: items.iter().map(|a| substitute_idents_inner(a, bindings)).collect(),
            span: *span,
        },
        Ir::Binding { name, value, span } => Ir::Binding {
            name: name.clone(),
            value: Box::new(substitute_idents_inner(value, bindings)),
            span: *span,
        },
        Ir::TupleBinding { names, value, span } => Ir::TupleBinding {
            names: names.clone(),
            value: Box::new(substitute_idents_inner(value, bindings)),
            span: *span,
        },
        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            span,
        } => Ir::IfExpr {
            condition: Box::new(substitute_idents_inner(condition, bindings)),
            then_branch: Box::new(substitute_idents_inner(then_branch, bindings)),
            else_branch: else_branch
                .as_ref()
                .map(|eb| Box::new(substitute_idents_inner(eb, bindings))),
            span: *span,
        },
        Ir::FunctionDef {
            name,
            params,
            body,
            span,
        } => Ir::FunctionDef {
            name: name.clone(),
            params: params.clone(),
            body: Box::new(substitute_idents_inner(body, bindings)),
            span: *span,
        },
        Ir::Lambda { params, body, span } => Ir::Lambda {
            params: params.clone(),
            body: Box::new(substitute_idents_inner(body, bindings)),
            span: *span,
        },
        Ir::ForLoop {
            var, range, body, span,
        } => Ir::ForLoop {
            var: var.clone(),
            range: Box::new(substitute_idents_inner(range, bindings)),
            body: Box::new(substitute_idents_inner(body, bindings)),
            span: *span,
        },
        Ir::WhileLoop {
            condition, body, span,
        } => Ir::WhileLoop {
            condition: Box::new(substitute_idents_inner(condition, bindings)),
            body: Box::new(substitute_idents_inner(body, bindings)),
            span: *span,
        },
        Ir::PropertyAccess {
            object, property, span,
        } => Ir::PropertyAccess {
            object: Box::new(substitute_idents_inner(object, bindings)),
            property: property.clone(),
            span: *span,
        },
        Ir::IndexAccess { array, index, span } => Ir::IndexAccess {
            array: Box::new(substitute_idents_inner(array, bindings)),
            index: Box::new(substitute_idents_inner(index, bindings)),
            span: *span,
        },
        Ir::Range { start, end, span } => Ir::Range {
            start: Box::new(substitute_idents_inner(start, bindings)),
            end: Box::new(substitute_idents_inner(end, bindings)),
            span: *span,
        },
        Ir::IndexAssign {
            array, index, value, span,
        } => Ir::IndexAssign {
            array: Box::new(substitute_idents_inner(array, bindings)),
            index: Box::new(substitute_idents_inner(index, bindings)),
            value: Box::new(substitute_idents_inner(value, bindings)),
            span: *span,
        },
        Ir::ParallelFor {
            var, range, body, span,
        } => Ir::ParallelFor {
            var: var.clone(),
            range: Box::new(substitute_idents_inner(range, bindings)),
            body: Box::new(substitute_idents_inner(body, bindings)),
            span: *span,
        },
    }
}

fn write_ir(out: &mut String, ir: &Ir, wrap_binary: bool) {
    match ir {
        Ir::Number { value, .. } => {
            if value.is_finite() && *value == value.trunc() && value.abs() < 1e15 {
                out.push_str(&format!("{}", *value as i64));
            } else {
                out.push_str(&format!("{}", value));
            }
        }
        Ir::BoolLit { value, .. } => out.push_str(if *value { "true" } else { "false" }),
        Ir::Identifier { name, .. } => out.push_str(name),
        Ir::Apply { callee, args, .. } => write_apply(out, callee, args, wrap_binary),
        Ir::Tuple { items, .. } => {
            out.push('(');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_ir(out, item, false);
            }
            out.push(')');
        }
        Ir::Binding { name, value, .. } => {
            out.push_str(name);
            out.push_str(" := ");
            write_ir(out, value, false);
        }
        Ir::TupleBinding { names, value, .. } => {
            out.push('(');
            for (i, n) in names.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(n);
            }
            out.push_str(") := ");
            write_ir(out, value, false);
        }
        Ir::Block { items, .. } => {
            out.push('(');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                write_ir(out, item, false);
            }
            out.push(')');
        }
        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            out.push_str("if (");
            write_ir(out, condition, false);
            out.push_str(") ");
            write_ir(out, then_branch, true);
            if let Some(eb) = else_branch {
                out.push_str(" else ");
                write_ir(out, eb, true);
            }
        }
        Ir::FunctionDef {
            name, params, body, ..
        } => {
            out.push_str(name);
            out.push('(');
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(p);
            }
            out.push_str(") := ");
            write_ir(out, body, false);
        }
        Ir::Lambda { params, body, .. } => {
            if params.len() == 1 {
                out.push_str(&params[0]);
            } else {
                out.push('(');
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(p);
                }
                out.push(')');
            }
            out.push_str(" |-> ");
            write_ir(out, body, false);
        }
        Ir::ForLoop {
            var, range, body, ..
        } => {
            out.push_str("for ");
            out.push_str(var);
            out.push_str(" in ");
            write_ir(out, range, true);
            out.push(' ');
            write_ir(out, body, true);
        }
        Ir::WhileLoop {
            condition, body, ..
        } => {
            out.push_str("while (");
            write_ir(out, condition, false);
            out.push_str(") ");
            write_ir(out, body, true);
        }
        Ir::PropertyAccess { object, property, .. } => {
            write_ir(out, object, true);
            out.push('.');
            out.push_str(property);
        }
        Ir::ArrayLiteral { items, .. } => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_ir(out, item, false);
            }
            out.push(']');
        }
        Ir::IndexAccess { array, index, .. } => {
            write_ir(out, array, true);
            out.push('[');
            write_ir(out, index, false);
            out.push(']');
        }
        Ir::Range { start, end, .. } => {
            write_ir(out, start, true);
            out.push_str("..");
            write_ir(out, end, true);
        }
        Ir::IndexAssign {
            array, index, value, ..
        } => {
            write_ir(out, array, true);
            out.push('[');
            write_ir(out, index, false);
            out.push_str("] := ");
            write_ir(out, value, false);
        }
        Ir::ParallelFor {
            var, range, body, ..
        } => {
            out.push_str("parallel for ");
            out.push_str(var);
            out.push_str(" in ");
            write_ir(out, range, true);
            out.push(' ');
            write_ir(out, body, true);
        }
    }
}

fn write_apply(out: &mut String, callee: &Callee, args: &[Ir], wrap_binary: bool) {
    let infix: Option<&str> = match callee {
        Callee::Builtin(BuiltinOp::Add) => Some(" + "),
        Callee::Builtin(BuiltinOp::Sub) => Some(" - "),
        Callee::Builtin(BuiltinOp::Mul) => Some("*"),
        Callee::Builtin(BuiltinOp::Div) => Some("/"),
        Callee::Builtin(BuiltinOp::Mod) => Some(" % "),
        Callee::Builtin(BuiltinOp::Pow) => Some("^"),
        Callee::Builtin(BuiltinOp::Eq) => Some(" = "),
        Callee::Builtin(BuiltinOp::Neq) => Some(" \u{2260} "),
        Callee::Builtin(BuiltinOp::Lt) => Some(" < "),
        Callee::Builtin(BuiltinOp::Gt) => Some(" > "),
        Callee::Builtin(BuiltinOp::Lte) => Some(" \u{2264} "),
        Callee::Builtin(BuiltinOp::Gte) => Some(" \u{2265} "),
        Callee::Builtin(BuiltinOp::And) => Some(" and "),
        Callee::Builtin(BuiltinOp::Or) => Some(" or "),
        _ => None,
    };
    if let Some(sep) = infix {
        if args.len() == 2 {
            if wrap_binary {
                out.push('(');
            }
            write_ir(out, &args[0], true);
            out.push_str(sep);
            write_ir(out, &args[1], true);
            if wrap_binary {
                out.push(')');
            }
            return;
        }
    }

    // Unary prefix operators. Always wrap the operand to keep precedence
    // unambiguous (`-x*y` would otherwise mean `-(x*y)` instead of `(-x)*y`).
    if let Callee::Builtin(BuiltinOp::Neg) = callee {
        if args.len() == 1 {
            out.push_str("-");
            write_ir(out, &args[0], true);
            return;
        }
    }
    if let Callee::Builtin(BuiltinOp::Not) = callee {
        if args.len() == 1 {
            out.push_str("not ");
            write_ir(out, &args[0], true);
            return;
        }
    }

    // Function call form for everything else (math builtins, user fns, CAS).
    out.push_str(callee.name());
    out.push('(');
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_ir(out, arg, false);
    }
    out.push(')');
}

#[cfg(test)]
mod to_source_tests {
    use super::*;

    fn parse_unwrap(s: &str) -> Ir {
        crate::lang::parse(s).expect("parse")
    }

    /// Round-trip: source → parse → to_source → parse should produce
    /// equivalent IR shape (we compare the discriminant string of the
    /// parser output, which is sufficient for the cases we care about).
    fn assert_roundtrip(s: &str) {
        let ir1 = parse_unwrap(s);
        let printed = ir1.to_source();
        let ir2 = parse_unwrap(&printed);
        assert_eq!(
            format!("{:?}", strip_spans(&ir1)),
            format!("{:?}", strip_spans(&ir2)),
            "round-trip differs:\n  input: {:?}\n  printed: {:?}",
            s,
            printed
        );
    }

    /// Replace every span with (0, 0) so structural comparison ignores
    /// position differences between original and re-parsed IR.
    fn strip_spans(ir: &Ir) -> Ir {
        match ir {
            Ir::Number { value, .. } => Ir::Number {
                value: *value,
                span: (0, 0),
            },
            Ir::BoolLit { value, .. } => Ir::BoolLit {
                value: *value,
                span: (0, 0),
            },
            Ir::Identifier { name, .. } => Ir::Identifier {
                name: name.clone(),
                span: (0, 0),
                resolved: None,
            },
            Ir::Apply { callee, args, .. } => Ir::Apply {
                callee: callee.clone(),
                args: args.iter().map(strip_spans).collect(),
                span: (0, 0),
            },
            Ir::Tuple { items, .. } => Ir::Tuple {
                items: items.iter().map(strip_spans).collect(),
                span: (0, 0),
            },
            Ir::Binding { name, value, .. } => Ir::Binding {
                name: name.clone(),
                value: Box::new(strip_spans(value)),
                span: (0, 0),
            },
            Ir::Block { items, .. } => Ir::Block {
                items: items.iter().map(strip_spans).collect(),
                span: (0, 0),
            },
            other => other.clone(),
        }
    }

    #[test]
    fn number() {
        assert_eq!(
            Ir::Number {
                value: 5.0,
                span: (0, 0)
            }
            .to_source(),
            "5"
        );
    }

    #[test]
    fn identifier() {
        assert_eq!(
            Ir::Identifier {
                name: "x".to_string(),
                span: (0, 0),
                resolved: None,
            }
            .to_source(),
            "x"
        );
    }

    #[test]
    fn binary_add() {
        assert_roundtrip("x + y");
    }

    #[test]
    fn nested_arith() {
        assert_roundtrip("(x + y) * (a - b)");
    }

    #[test]
    fn function_call() {
        assert_roundtrip("sin(x)");
    }

    #[test]
    fn unary_negate() {
        assert_roundtrip("-x");
    }

    #[test]
    fn power() {
        assert_roundtrip("x^2");
    }

    #[test]
    fn equation() {
        assert_roundtrip("x = 0");
    }
}

/// A Logos type. The universe is intentionally small for now and grows by
/// adding variants — every consumer that cares about types will then have to
/// handle the new case (exhaustiveness as a forcing function).
///
/// Future variants this enum is expected to host: `Symbolic` (CAS-tracked
/// unevaluated expressions), `Matrix(usize, usize)`, `Complex`, units, and
/// user-defined types.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// Numeric scalar (f32 / f64 — the type system stays precision-agnostic
    /// for now; backends pick their own representation).
    Num,

    /// Boolean.
    Bool,

    /// Fixed-size float vectors. Width is encoded in the variant so consumers
    /// can match on it directly.
    Vec2,
    Vec3,
    Vec4,

    /// `start..end` half-open integer range; produced by `Range` nodes,
    /// consumed by `for` / `parallel for`.
    Range,

    /// Heterogeneous tuple. Element types are tracked so destructuring
    /// bindings can give each name the right type.
    Tuple(Vec<Type>),

    /// Homogeneous array. Today the parser only emits arrays of `Num`, but
    /// the variant carries the element type so future arrays of vectors or
    /// other shapes are representable without a breaking change.
    Array(Box<Type>),

    /// Statement-level "no value" — `plot(...)`, `IndexAssign`, side-effecting
    /// `print` in some contexts.
    Void,

    /// User-defined function. Stored separately from value types because
    /// functions in Logos aren't first-class values yet.
    Function(Vec<Type>, Box<Type>),

    /// Type information not yet known. Used as a placeholder during checking
    /// and as the type of identifiers that haven't been bound yet (so that
    /// downstream errors don't cascade from a single missing definition).
    Unknown,

    /// Reserved for CAS-tracked symbolic expressions (REDUCE today, Lean
    /// planned). Currently unused by the type checker; here so consumers can
    /// already match on it without a breaking change later.
    Symbolic,
}

impl Type {
    /// Display name for diagnostics (`"Num"`, `"Vec3"`, `"(Num, Bool)"`, etc.).
    pub fn display(&self) -> String {
        match self {
            Type::Num => "Num".to_string(),
            Type::Bool => "Bool".to_string(),
            Type::Vec2 => "Vec2".to_string(),
            Type::Vec3 => "Vec3".to_string(),
            Type::Vec4 => "Vec4".to_string(),
            Type::Range => "Range".to_string(),
            Type::Tuple(items) => {
                let parts: Vec<String> = items.iter().map(|t| t.display()).collect();
                format!("({})", parts.join(", "))
            }
            Type::Array(elem) => format!("Array<{}>", elem.display()),
            Type::Void => "Void".to_string(),
            Type::Function(params, ret) => {
                let parts: Vec<String> = params.iter().map(|t| t.display()).collect();
                format!("({}) -> {}", parts.join(", "), ret.display())
            }
            Type::Unknown => "?".to_string(),
            Type::Symbolic => "Symbolic".to_string(),
        }
    }

    /// True for `Vec2`/`Vec3`/`Vec4`. Useful when picking overloads that
    /// accept "any vector."
    pub fn is_vec(&self) -> bool {
        matches!(self, Type::Vec2 | Type::Vec3 | Type::Vec4)
    }
}
