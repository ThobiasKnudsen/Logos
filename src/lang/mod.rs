pub mod check;
pub mod compute_gen;
pub mod highlight;
pub mod interpreter;
pub mod ir;
pub mod lang_service;
pub mod lexer;
pub mod lower;
pub mod notebook_format;
pub mod parser;
pub mod reduce;
pub mod symbolic;
pub mod token;
pub mod wgsl_gen;

use ir::{Ir, Span};

/// A diagnostic with optional source-span info for richer formatting.
///
/// `span == (0, 0)` is the "no span" sentinel — callers fall back to plain
/// message display. Otherwise the offset addresses byte `span.0` in the
/// source and `Diagnostic::format` produces a `Line N, Col M:` message with
/// the source line and a caret.
///
/// `comptime_level` is the compile-time-evaluation level the diagnostic was
/// produced at. Today every diagnostic is level 0 (user source); the field
/// is present so future multi-level metaprogramming can attribute errors to
/// the right level without a wire-format change.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub message: String,
    pub span: Span,
    pub comptime_level: usize,
}

impl Diagnostic {
    /// Build a diagnostic with a real source span and level 0.
    pub fn at(span: Span, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span,
            comptime_level: 0,
        }
    }

    /// Build a diagnostic without source-span info — formats as bare message.
    pub fn without_span(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span: (0, 0),
            comptime_level: 0,
        }
    }

    /// Render this diagnostic against the original source. When the
    /// diagnostic carries a real span (not the `(0, 0)` sentinel) the
    /// message gets a `Line N, Col M:` prefix, the offending source line,
    /// and a caret. Otherwise the bare message is returned.
    pub fn format(&self, source: &str) -> String {
        if self.span == (0, 0) {
            return self.message.clone();
        }
        format_error_at(source, self.span.0, &self.message)
    }
}

impl From<String> for Diagnostic {
    fn from(message: String) -> Self {
        Self::without_span(message)
    }
}

/// Convert a byte offset in source to (line, col) — both 1-based.
pub(crate) fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let mut line = 1;
    let mut line_start = 0;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let col = source[line_start..offset].chars().count() + 1;
    (line, col)
}

/// Format a rich error message with source context and caret.
pub fn format_error_at(source: &str, offset: usize, message: &str) -> String {
    let (line, col) = offset_to_line_col(source, offset);
    let source_line = source.lines().nth(line - 1).unwrap_or("");
    let line_str = line.to_string();
    let pad = " ".repeat(line_str.len());
    format!(
        "Line {}, Col {}: {}\n  {} | {}\n  {} | {}^",
        line,
        col,
        message,
        line_str,
        source_line,
        pad,
        " ".repeat(col.saturating_sub(1))
    )
}

/// Check whether the IR contains nodes that require the interpreter
/// (arrays, parallel for, first-class function values) rather than the
/// fragment shader path.
pub fn needs_interpreter(ir: &Ir) -> bool {
    if uses_first_class_functions(ir) {
        return true;
    }
    needs_interpreter_struct(ir)
}

fn needs_interpreter_struct(ir: &Ir) -> bool {
    match ir {
        Ir::ArrayLiteral { .. }
        | Ir::IndexAccess { .. }
        | Ir::ParallelFor { .. }
        | Ir::IndexAssign { .. } => true,
        Ir::Block { items: stmts, .. } => stmts.iter().any(needs_interpreter_struct),
        Ir::Binding { value, .. } => needs_interpreter_struct(value),
        Ir::TupleBinding { value, .. } => needs_interpreter_struct(value),
        Ir::FunctionDef { body, .. } => needs_interpreter_struct(body),
        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            needs_interpreter_struct(condition)
                || needs_interpreter_struct(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| needs_interpreter_struct(e))
        }
        Ir::Apply { args, .. } => args.iter().any(needs_interpreter_struct),
        Ir::WhileLoop {
            condition, body, ..
        } => needs_interpreter_struct(condition) || needs_interpreter_struct(body),
        _ => false,
    }
}

/// True when `ir` references one of its own user functions as a value
/// (rather than calling it directly). E.g. `f := if cond then sq else cube`
/// reads `sq` and `cube` in value position. wgsl_gen can't lower these —
/// the cell routes to the CPU interpreter, which handles `Value::Function`
/// via captured environments (issue #29).
fn uses_first_class_functions(ir: &Ir) -> bool {
    let mut func_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    collect_function_names(ir, &mut func_names);
    if func_names.is_empty() {
        return false;
    }
    has_function_value_reference(ir, &func_names)
}

fn collect_function_names(ir: &Ir, out: &mut std::collections::HashSet<String>) {
    if let Ir::FunctionDef { name, .. } = ir {
        out.insert(name.clone());
    }
    for child in ir.children() {
        collect_function_names(child, out);
    }
}

fn has_function_value_reference(
    ir: &Ir,
    func_names: &std::collections::HashSet<String>,
) -> bool {
    if let Ir::Identifier { name, .. } = ir {
        return func_names.contains(name);
    }
    // For `Apply`, the callee position is *invocation*, not value-position;
    // we only need to scan the args. Everything else recurses through all
    // children — that's still correct because the only structural place a
    // function name in identifier form means "value, not callee" is inside
    // an `Apply.args` (or some other non-Apply node).
    if let Ir::Apply { args, .. } = ir {
        return args.iter().any(|a| has_function_value_reference(a, func_names));
    }
    ir.children()
        .iter()
        .any(|c| has_function_value_reference(c, func_names))
}

/// Detected print/plot actions in a cell's IR.
#[derive(Debug)]
pub struct CellActions {
    /// Indices of every `print(...)` statement in the block, in source order.
    /// One output line is emitted per index.
    pub prints: Vec<usize>,
    /// Indices of every `plot(...)` statement in the block, in source order.
    /// One shader is emitted per index.
    pub plots: Vec<usize>,
}

impl CellActions {
    pub fn has_action(&self) -> bool {
        !self.prints.is_empty() || !self.plots.is_empty()
    }
}

/// True if `ir` contains a `print(...)` call inside any nested scope —
/// a for/while body, an if branch, a function body, etc. — that
/// `detect_cell_actions` wouldn't see because it only walks top-level
/// items. The notebook uses this to decide between the per-print
/// extraction path (top-level only, with REDUCE fallback per print) and
/// the whole-cell `eval_with_stdout` path (every print, regardless of
/// nesting).
pub fn has_nested_print(ir: &Ir) -> bool {
    let stmts: &[Ir] = match ir {
        Ir::Block { items, .. } => items,
        other => std::slice::from_ref(other),
    };
    // Skip top-level Apply(Print) statements — they're already handled
    // by `detect_cell_actions`. Recurse into everything else.
    for stmt in stmts {
        match stmt {
            Ir::Apply { callee: ir::Callee::Builtin(ir::BuiltinOp::Print), .. } => {
                // Top-level print — don't count, but still scan its args
                // in case someone wrote `print(for i in 0..n ( print(i) ))`.
                if let Ir::Apply { args, .. } = stmt {
                    if args.iter().any(contains_print) {
                        return true;
                    }
                }
            }
            other => {
                if contains_print(other) {
                    return true;
                }
            }
        }
    }
    false
}

fn contains_print(node: &Ir) -> bool {
    if let Ir::Apply {
        callee: ir::Callee::Builtin(ir::BuiltinOp::Print),
        ..
    } = node
    {
        return true;
    }
    node.children().iter().any(|c| contains_print(c))
}

/// Walk the IR to find all print() and plot() action statements.
pub fn detect_cell_actions(ir: &Ir) -> CellActions {
    let stmts: &[Ir] = match ir {
        Ir::Block { items: stmts, .. } => stmts,
        other => std::slice::from_ref(other),
    };

    let mut result = CellActions {
        prints: Vec::new(),
        plots: Vec::new(),
    };

    for (i, stmt) in stmts.iter().enumerate() {
        if let Ir::Apply {
            callee: ir::Callee::Builtin(op),
            ..
        } = stmt
        {
            match op {
                ir::BuiltinOp::Print => {
                    result.prints.push(i);
                }
                ir::BuiltinOp::Plot => {
                    result.plots.push(i);
                }
                _ => {}
            }
        }
    }

    result
}

/// Build an IR subtree for evaluating a print expression.
/// Includes all non-action statements before the print index,
/// plus the unwrapped inner expression from print().
pub fn build_print_ir(ir: &Ir, print_index: usize) -> Ir {
    let stmts = match ir {
        Ir::Block { items: stmts, .. } => stmts,
        other => {
            if let Ir::Apply {
                callee: ir::Callee::Builtin(ir::BuiltinOp::Print),
                args,
                ..
            } = other
            {
                if args.len() == 1 {
                    return args[0].clone();
                }
            }
            return other.clone();
        }
    };

    let mut result = Vec::new();
    for (i, stmt) in stmts.iter().enumerate() {
        if i == print_index {
            if let Ir::Apply {
                callee: ir::Callee::Builtin(ir::BuiltinOp::Print),
                args,
                ..
            } = stmt
            {
                if args.len() == 1 {
                    result.push(args[0].clone());
                }
            }
            break;
        }
        if let Ir::Apply {
            callee: ir::Callee::Builtin(ir::BuiltinOp::Print | ir::BuiltinOp::Plot),
            ..
        } = stmt
        {
            continue;
        }
        result.push(stmt.clone());
    }

    match result.len() {
        1 => result.remove(0),
        _ => {
            let span = if result.is_empty() {
                ir.span()
            } else {
                (
                    result.first().unwrap().span().0,
                    result.last().unwrap().span().1,
                )
            };
            Ir::Block {
                items: result,
                span,
            }
        }
    }
}

/// Build an IR subtree for plotting. Includes all non-action statements
/// plus the unwrapped inner expression from plot().
pub fn build_plot_ir(ir: &Ir, plot_index: usize) -> Ir {
    let stmts = match ir {
        Ir::Block { items: stmts, .. } => stmts,
        other => {
            if let Ir::Apply {
                callee: ir::Callee::Builtin(ir::BuiltinOp::Plot),
                args,
                ..
            } = other
            {
                if args.len() == 1 {
                    return args[0].clone();
                }
            }
            return other.clone();
        }
    };

    let mut result = Vec::new();
    for (i, stmt) in stmts.iter().enumerate() {
        if i == plot_index {
            if let Ir::Apply {
                callee: ir::Callee::Builtin(ir::BuiltinOp::Plot),
                args,
                ..
            } = stmt
            {
                if args.len() == 1 {
                    result.push(args[0].clone());
                }
            }
            continue;
        }
        if let Ir::Apply {
            callee: ir::Callee::Builtin(ir::BuiltinOp::Print | ir::BuiltinOp::Plot),
            ..
        } = stmt
        {
            continue;
        }
        result.push(stmt.clone());
    }

    match result.len() {
        1 => result.remove(0),
        _ => {
            let span = if result.is_empty() {
                ir.span()
            } else {
                (
                    result.first().unwrap().span().0,
                    result.last().unwrap().span().1,
                )
            };
            Ir::Block {
                items: result,
                span,
            }
        }
    }
}

/// Lex and parse source code into Logos IR.
pub fn parse(source: &str) -> Result<Ir, String> {
    let mut lex = lexer::Lexer::new(source);
    let tokens = lex.tokenize()?;
    let mut p = parser::Parser::new(tokens, source.to_string());
    p.parse()
}

/// Type-check an IR tree, formatting any error with source-location context.
/// Production callers run this between parse and codegen so that semantic
/// mistakes surface as `Line N, Col M: ...` messages instead of opaque GPU
/// shader-validation errors.
pub fn type_check(ir: &Ir, source: &str) -> Result<(), String> {
    check::check(ir)
        .map(|_| ())
        .map_err(|e| format_error_at(source, e.span.0, &e.message))
}

/// Compile source code through the full pipeline: lex → parse → type check → WGSL gen.
/// Returns the complete WGSL shader source string.
///
/// Production callers go through `Notebook` which keeps the IR around
/// (see `program_ir`); this end-to-end convenience exists for tests.
#[cfg(test)]
pub fn compile(source: &str) -> Result<String, String> {
    let mut lex = lexer::Lexer::new(source);
    let tokens = lex.tokenize()?;
    let mut parser = parser::Parser::new(tokens, source.to_string());
    let ir = parser.parse()?;
    type_check(&ir, source)?;
    wgsl_gen::generate(&ir).map_err(|d| d.format(source))
}

#[cfg(test)]
mod notebook_tests;

#[cfg(test)]
mod integration_tests {
    use super::*;

    /// Validate a WGSL shader string using naga — the same validation
    /// wgpu performs before sending to the GPU.
    fn validate_wgsl(wgsl: &str) -> Result<(), String> {
        let module = naga::front::wgsl::parse_str(wgsl).map_err(|e| {
            format!(
                "naga WGSL parse error: {}\n\n--- Generated WGSL ---\n{}",
                e, wgsl
            )
        })?;
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator.validate(&module).map_err(|e| {
            format!(
                "naga validation error: {}\n\n--- Generated WGSL ---\n{}",
                e, wgsl
            )
        })?;
        Ok(())
    }

    /// Full pipeline: lex → parse → WGSL gen → naga validate.
    /// Matches what the real app does. Panics with diagnostics on failure.
    fn compile_and_validate(source: &str) -> String {
        let wgsl =
            compile(source).unwrap_or_else(|e| panic!("compile({:?}) failed: {}", source, e));
        validate_wgsl(&wgsl)
            .unwrap_or_else(|e| panic!("WGSL validation failed for {:?}:\n{}", source, e));
        wgsl
    }

    #[test]
    fn test_empty_input_compiles() {
        let shader = compile_and_validate("");
        assert!(shader.contains("fn fs_main"));
    }

    #[test]
    fn test_single_var_compiles() {
        let shader = compile_and_validate("x");
        assert!(shader.contains("let _result = x;"));
    }

    #[test]
    fn test_simple_math_compiles() {
        let shader = compile_and_validate("x * x + y * y");
        assert!(shader.contains("((x * x) + (y * y))"));
    }

    #[test]
    fn test_pow_with_small_int_exponent_uses_multiplication() {
        // pow() costs 20+ GPU ops; x² should compile to (x * x), not pow(x, 2.0).
        let shader = compile_and_validate("y = x^2");
        assert!(
            !shader.contains("pow("),
            "x^2 should NOT use pow(); got:\n{}",
            shader
        );
        assert!(
            shader.contains("(x) * (x)") || shader.contains("(x_m) * (x_m)"),
            "x^2 should compile to repeated multiplication; got:\n{}",
            shader
        );
    }

    // The user-level / cell-level scenarios these tests used to cover
    // (function-with-axis-capture, block-binding-with-loop, symbolic
    // print, plot+print-coexist) live in `src/notebook/tests.rs`; they
    // exercise the real `Notebook` API. The wgsl_gen-only tests below
    // remain in this file as low-level codegen checks.

    #[test]
    fn test_block_binding_with_loop_lifts_to_fn() {
        // Block on RHS of := with imperative content (var + for loop) must be
        // lifted into a WGSL function so corner-checking can re-evaluate the
        // loop at each pixel corner — without this, steep curves render dotted.
        let source = "f := (\n  sum := 0\n  for i in 0..10 (\n    sum := sum + x*x+4*x*x\n  )\n  y = sum\n)\nf";
        let shader = compile_and_validate(source);
        assert!(
            shader.contains("fn _lifted_f("),
            "block-valued binding should be lifted into a WGSL function; got:\n{}",
            shader
        );
        assert!(
            shader.contains("var sum"),
            "lifted function should declare `sum`; got:\n{}",
            shader
        );
        assert!(
            shader.contains("for ("),
            "lifted function should emit the for-loop; got:\n{}",
            shader
        );
        assert!(
            shader.contains("let _corner_f_mm = _lifted_f(x_m, y_m);"),
            "corner values should be hoisted to avoid duplicate calls; got:\n{}",
            shader
        );
        assert!(
            shader.contains("let _corner_f_pp = _lifted_f(x_p, y_p);"),
            "all 4 corners should be hoisted; got:\n{}",
            shader
        );
    }


    #[test]
    fn test_bool_binding_plot_uses_corner_checking() {
        // f := x = y^2 ; f → should plot the curve via corner-checking,
        // not via direct float == (which would render nothing).
        let shader = compile_and_validate("f := x = y^2\nf");
        assert!(
            shader.contains("x_m"),
            "bool binding result should trigger corner-checking; got:\n{}",
            shader
        );
        assert!(
            shader.contains("!("),
            "equality should use straddle check; got:\n{}",
            shader
        );
    }

    #[test]
    fn test_paraboloid() {
        let shader = compile_and_validate("x * x + y * y");
        assert!(shader.contains("@fragment"));
        assert!(shader.contains("let x = world.x"));
        assert!(shader.contains("let y = world.y"));
    }

    #[test]
    fn test_with_bindings_and_result() {
        let shader = compile_and_validate("r := sqrt(x*x + y*y)\nsin(r * 10) / r");
        assert!(shader.contains("let r = sqrt("));
        assert!(shader.contains("(sin((r * 10.0)) / r)"));
    }

    #[test]
    fn test_function_def_and_call() {
        let shader = compile_and_validate("dist(a, b) := sqrt(a*a + b*b)\ndist(x, y)");
        assert!(shader.contains("fn dist(a: f32, b: f32) -> f32"));
        assert!(shader.contains("dist(x, y)"));
    }

    #[test]
    fn test_conditional_coloring() {
        let shader = compile_and_validate("if (x*x + y*y < 1) 1 else 0");
        assert!(shader.contains("select("));
    }

    #[test]
    fn test_trig_composition() {
        let shader = compile_and_validate("sin(x * 3) * cos(y * 3)");
        assert!(shader.contains("sin((x * 3.0))"));
        assert!(shader.contains("cos((y * 3.0))"));
    }

    #[test]
    fn test_time_animation() {
        let shader = compile_and_validate("sin(x + t)");
        assert!(shader.contains("sin((x + u.time))"));
    }

    #[test]
    fn test_nested_blocks() {
        let shader = compile_and_validate("f(a) := (b := a * 2, b + 1)\nf(x)");
        assert!(shader.contains("fn f("));
    }

    #[test]
    fn test_equality_operator() {
        let shader = compile_and_validate("x = 0");
        assert!(
            shader.contains("x_m"),
            "equality should use corner checking"
        );
        assert!(
            shader.contains("!("),
            "equality should negate all-same-sign"
        );
    }

    #[test]
    fn test_complex_mandelbrot_style() {
        let shader = compile_and_validate("r := x*x + y*y\nif (r < 4) (1 - r/4) else 0");
        assert!(shader.contains("let r ="));
        assert!(shader.contains("select("));
    }

    #[test]
    fn test_multiple_builtins() {
        let shader = compile_and_validate("clamp(abs(sin(x * 6) - 0.5), 0, 1)");
        assert!(shader.contains("clamp("));
        assert!(shader.contains("abs("));
        assert!(shader.contains("sin("));
    }

    #[test]
    fn test_whitespace_resilience() {
        let s1 = compile_and_validate("x+y");
        let s2 = compile_and_validate("x + y");
        let s3 = compile_and_validate("  x  +  y  ");
        assert!(s1.contains("(x + y)"));
        assert!(s2.contains("(x + y)"));
        assert!(s3.contains("(x + y)"));
    }

    #[test]
    fn test_unicode_superscript_square() {
        // x² + y² = 9 — should compile via repeated multiplication, not pow().
        let shader = compile_and_validate("x\u{00B2} + y\u{00B2} = 9");
        assert!(
            shader.contains("(x_m) * (x_m)") || shader.contains("(x) * (x)"),
            "x² should compile to (x * x); got:\n{}",
            shader
        );
        assert!(
            !shader.contains("\u{00B2}"),
            "Unicode ² should NOT appear in WGSL output"
        );
    }

    #[test]
    fn test_unicode_superscript_cube() {
        // sin(x)³ — should compile via repeated multiplication.
        let shader = compile_and_validate("sin(x)\u{00B3}");
        let s = shader.replace(' ', "");
        assert!(
            s.contains("(sin(x))*(sin(x))*(sin(x))"),
            "sin(x)³ should compile to repeated multiplication; got:\n{}",
            shader
        );
    }

    #[test]
    fn test_complex_unicode_expression_compiles() {
        // x²*y²+sin(x)³-sin(y²)²=9  — full pipeline including naga validation
        let input = "x\u{00B2}*y\u{00B2}+sin(x)\u{00B3}-sin(y\u{00B2})\u{00B2}=9";
        let shader = compile_and_validate(input);

        assert!(
            !shader.contains("square"),
            "bare 'square' must not appear in WGSL"
        );
        assert!(
            !shader.contains("cube"),
            "bare 'cube' must not appear in WGSL"
        );
        assert!(
            !shader.contains("pow("),
            "small-int exponents should expand to multiplication, not pow(); got:\n{}",
            shader
        );
        assert!(
            shader.contains("sin("),
            "should contain sin() calls, got:\n{}",
            shader
        );
        assert!(
            shader.contains("x_m") || shader.contains("x_p"),
            "=9 should trigger corner-checking, got:\n{}",
            shader
        );
    }

    #[test]
    fn test_cas_symbols_no_panic() {
        // CAS-only symbols (∫, ∂, ∑, ∏) have no WGSL equivalent.
        // They should not panic — they may compile (if WGSL gen handles them)
        // or fail with an error, but never crash.
        for &(sym, name) in &[
            ("\u{222B}", "∫"),
            ("\u{2202}", "∂"),
            ("\u{2211}", "∑"),
            ("\u{220F}", "∏"),
        ] {
            let result = compile(sym);
            // If compile succeeds, validate the WGSL too
            if let Ok(ref wgsl) = result {
                if let Err(e) = validate_wgsl(wgsl) {
                    // Expected: naga rejects undeclared identifiers.
                    // This is acceptable — the symbol has no shader meaning.
                    eprintln!(
                        "{} produces invalid WGSL (expected): {}",
                        name,
                        e.lines().next().unwrap_or("")
                    );
                }
            }
            // The key assertion: no panic occurred
        }
    }

    #[test]
    fn test_integral_with_args_no_panic() {
        // ∫(x) — must not panic. Will likely fail (no WGSL "integral" function).
        let result = compile("\u{222B}(x)");
        if let Ok(ref wgsl) = result {
            let _ = validate_wgsl(wgsl); // may fail, that's fine
        }
        // No panic = success
    }
}

#[cfg(test)]
mod action_tests {
    use super::*;

    #[test]
    fn test_detect_print() {
        let ir = parse("print(3+9)").unwrap();
        let actions = detect_cell_actions(&ir);
        assert_eq!(actions.prints, vec![0]);
        assert!(actions.plots.is_empty());
    }

    #[test]
    fn test_detect_multiple_prints() {
        let ir = parse("print(3+9)\nprint(5*2)\nprint(7)").unwrap();
        let actions = detect_cell_actions(&ir);
        assert_eq!(actions.prints, vec![0, 1, 2]);
    }

    #[test]
    fn test_detect_plot() {
        let ir = parse("plot(x+y)").unwrap();
        let actions = detect_cell_actions(&ir);
        assert!(actions.prints.is_empty());
        assert_eq!(actions.plots, vec![0]);
    }

    #[test]
    fn test_detect_multiple_plots() {
        let ast = parse("plot(y = x)\nplot(y = x*x)\nplot(y = x*x*x)").unwrap();
        let actions = detect_cell_actions(&ast);
        assert_eq!(actions.plots, vec![0, 1, 2]);
    }

    #[test]
    fn test_detect_none() {
        let ir = parse("3+9").unwrap();
        let actions = detect_cell_actions(&ir);
        assert!(!actions.has_action());
    }

    #[test]
    fn test_detect_both_print_and_plot() {
        let ir = parse("f := x+2*x\nprint(f)\nplot(y=f)").unwrap();
        let actions = detect_cell_actions(&ir);
        assert_eq!(actions.prints, vec![1]);
        assert_eq!(actions.plots, vec![2]);
    }

    #[test]
    fn test_build_print_ir_single() {
        let ir = parse("print(3+9)").unwrap();
        let print_ir = build_print_ir(&ir, 0);
        assert!(matches!(
            print_ir,
            Ir::Apply {
                callee: ir::Callee::Builtin(ir::BuiltinOp::Add),
                ..
            }
        ));
    }

    #[test]
    fn test_build_print_ir_with_bindings() {
        let ir = parse("f := 5\nprint(f)").unwrap();
        let actions = detect_cell_actions(&ir);
        let print_ir = build_print_ir(&ir, actions.prints[0]);
        if let Ir::Block { items: stmts, .. } = &print_ir {
            assert_eq!(stmts.len(), 2);
            assert!(matches!(&stmts[0], Ir::Binding { .. }));
            assert!(matches!(&stmts[1], Ir::Identifier { .. }));
        } else {
            panic!("Expected Block, got {:?}", print_ir);
        }
    }

    #[test]
    fn test_build_plot_ir_strips_print() {
        let ir = parse("f := x\nprint(f)\nplot(y=f)").unwrap();
        let actions = detect_cell_actions(&ir);
        let plot_ir = build_plot_ir(&ir, *actions.plots.last().unwrap());
        if let Ir::Block { items: stmts, .. } = &plot_ir {
            for stmt in stmts {
                if let Ir::Apply { callee, .. } = stmt {
                    assert_ne!(
                        callee.name(),
                        "print",
                        "print should be stripped from plot IR"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod needs_interpreter_tests {
    use super::*;

    #[test]
    fn detects_first_class_function_via_if_branch() {
        // Routing trigger (issue #29): a cell that binds a function value
        // via `if` must route to the CPU interpreter, since wgsl_gen can't
        // lower dynamic dispatch.
        let ir = parse(
            "sq(x) := x*x\n\
             cube(x) := x*x*x\n\
             c := 1\n\
             f := if (c) sq else cube\n\
             f(5)",
        )
        .unwrap();
        assert!(needs_interpreter(&ir));
    }

    #[test]
    fn detects_function_alias_binding() {
        // `g := sq` is the simplest first-class function use.
        let ir = parse("sq(x) := x*x\ng := sq\ng(7)").unwrap();
        assert!(needs_interpreter(&ir));
    }

    #[test]
    fn plain_arithmetic_does_not_need_interpreter() {
        let ir = parse("x + 1").unwrap();
        assert!(!needs_interpreter(&ir));
    }

    #[test]
    fn ordinary_function_call_does_not_need_interpreter() {
        // `sq(x)` invokes sq — that's a Callee, not a first-class value.
        let ir = parse("sq(x) := x*x\nsq(3)").unwrap();
        assert!(!needs_interpreter(&ir));
    }
}
