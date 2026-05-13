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

use ir::Ir;

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
/// (arrays, parallel for) rather than the fragment shader path.
pub fn needs_interpreter(ir: &Ir) -> bool {
    match ir {
        Ir::ArrayLiteral { .. }
        | Ir::IndexAccess { .. }
        | Ir::ParallelFor { .. }
        | Ir::IndexAssign { .. } => true,
        Ir::Block { items: stmts, .. } => stmts.iter().any(needs_interpreter),
        Ir::Binding { value, .. } => needs_interpreter(value),
        Ir::TupleBinding { value, .. } => needs_interpreter(value),
        Ir::FunctionDef { body, .. } => needs_interpreter(body),
        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            needs_interpreter(condition)
                || needs_interpreter(then_branch)
                || else_branch.as_ref().is_some_and(|e| needs_interpreter(e))
        }
        Ir::Apply { args, .. } => args.iter().any(needs_interpreter),
        Ir::WhileLoop {
            condition, body, ..
        } => needs_interpreter(condition) || needs_interpreter(body),
        _ => false,
    }
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
/// plus the canonicalized plot body from `plot(...)`.
///
/// The plot's first argument is canonicalized via `canonicalize_plot_body`
/// so explicit lambdas (`(x) ↦ sin(x)`, `(x, y) ↦ x*y`) and implicit
/// comparisons (`y = sin(x)`, bare value expressions) all produce the same
/// downstream shape: a binding to a synthesized helper function plus a
/// call wired to the canonical axis variables.
///
/// If `plot()` carries a second positional arg, it's the per-pixel color
/// (see issue #24). `canonicalize_plot_color` lifts it into a synthetic
/// `_plot_color_<span>` binding so wgsl_gen can pick it up and use the
/// result vec4 in place of `u.primary_color`.
pub fn build_plot_ir(ir: &Ir, plot_index: usize) -> Result<Ir, String> {
    let stmts = match ir {
        Ir::Block { items: stmts, .. } => stmts,
        other => {
            if let Ir::Apply {
                callee: ir::Callee::Builtin(ir::BuiltinOp::Plot),
                args,
                ..
            } = other
            {
                if !args.is_empty() {
                    let mut canonical = Vec::new();
                    if let Some(color) = args.get(1) {
                        canonical.push(canonicalize_plot_color(color));
                    }
                    canonical.extend(canonicalize_plot_body(&args[0])?);
                    return Ok(coalesce_block(canonical, other.span()));
                }
            }
            return Ok(other.clone());
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
                if !args.is_empty() {
                    if let Some(color) = args.get(1) {
                        result.push(canonicalize_plot_color(color));
                    }
                    result.extend(canonicalize_plot_body(&args[0])?);
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

    Ok(coalesce_block(result, ir.span()))
}

/// Wrap a list of statements as a single `Ir`. One statement returns
/// directly; zero or many become a `Block` covering their span (falling
/// back to `fallback_span` when empty).
fn coalesce_block(mut items: Vec<Ir>, fallback_span: ir::Span) -> Ir {
    match items.len() {
        1 => items.remove(0),
        _ => {
            let span = if items.is_empty() {
                fallback_span
            } else {
                (
                    items.first().unwrap().span().0,
                    items.last().unwrap().span().1,
                )
            };
            Ir::Block { items, span }
        }
    }
}

/// Canonicalize `plot()`'s first argument into a sequence of IR
/// statements ending in the plottable result expression.
///
/// Accepts three input shapes:
///
/// - Explicit 1-arg lambda `(p) ↦ body` — synthesizes a binding
///   `_plot_fn_<span> := (p) ↦ body` (which `lower::lift_lambdas`
///   converts to a top-level `FunctionDef`) and emits the result
///   `y = _plot_fn_<span>(x)` so the implicit-curve corner-checking
///   path renders it.
///
/// - Explicit 2-arg lambda `(p, q) ↦ body` — emits a binding plus a
///   call `_plot_fn_<span>(x, y)` whose numeric output drives the
///   2D-surface grayscale path.
///
/// - Anything else (implicit comparison `y = sin(x)`, bare expression
///   like `x*x + y*y`, etc.) — returned as a single-element list with
///   the expression unchanged. Existing implicit forms keep their
///   current behaviour.
///
/// Errors on lambdas with arity 0 or ≥ 3. ND plots with extras as
/// tunable parameters are tracked separately (see issue #27).
///
/// Wraps the lambda in a binding (instead of substituting axis-var
/// names into the body) so capture analysis stays correct in the
/// presence of nested bindings that shadow the lambda's parameter.
fn canonicalize_plot_body(arg: &Ir) -> Result<Vec<Ir>, String> {
    let Ir::Lambda { params, span, .. } = arg else {
        return Ok(vec![arg.clone()]);
    };

    match params.len() {
        0 => Err("plot lambda must have at least one parameter".to_string()),
        n @ (1 | 2) => {
            let synth_name = format!("_plot_fn_{}_{}", span.0, span.1);
            let binding = Ir::Binding {
                name: synth_name.clone(),
                value: Box::new(arg.clone()),
                span: *span,
                value_ty: None,
            };
            let axis_args: Vec<Ir> = (0..n)
                .map(|i| Ir::Identifier {
                    name: match i {
                        0 => "x".to_string(),
                        1 => "y".to_string(),
                        _ => unreachable!(),
                    },
                    span: *span,
                    resolved: None,
                })
                .collect();
            let call = Ir::Apply {
                callee: ir::Callee::User(synth_name),
                args: axis_args,
                span: *span,
                result_ty: None,
            };
            let result = if n == 1 {
                // 1D: render `y = f(x)` as a curve via corner-checking.
                let y_axis = Ir::Identifier {
                    name: "y".to_string(),
                    span: *span,
                    resolved: None,
                };
                Ir::Apply {
                    callee: ir::Callee::Builtin(ir::BuiltinOp::Eq),
                    args: vec![y_axis, call],
                    span: *span,
                    result_ty: None,
                }
            } else {
                // 2D: render `f(x, y)` as a numeric grayscale field.
                call
            };
            Ok(vec![binding, result])
        }
        n => Err(format!(
            "plot lambda with {} parameters is not yet supported; \
             ND plots beyond 2D require tunable parameter sliders",
            n
        )),
    }
}

/// Lift `plot()`'s optional second argument into a synthetic
/// `_plot_color_<span>` binding that `wgsl_gen` recognizes and uses
/// as the per-pixel RGBA output in place of `u.primary_color`.
///
/// Two input shapes are accepted, both desugared into the same lambda
/// shape so the downstream pipeline (lift_lambdas, capture analysis,
/// codegen) handles them uniformly:
///
/// - Explicit lambda `(x) ↦ (r,g,b,a)` / `(x, y) ↦ (r,g,b,a)` — passed
///   through; lift_lambdas turns the binding into a top-level
///   `FunctionDef`.
///
/// - Implicit expression `(sin(x), cos(y), x*y, 1)` — wrapped in a
///   0-parameter lambda. Capture analysis then promotes referenced
///   axis vars to extra function arguments at the call site, so the
///   user can mix `x`, `y` and any in-scope bindings freely.
///
/// The synthesized binding is uniquely named by source span so multi-
/// plot cells stay free of name collisions, and any unexpected color
/// value type surfaces through the normal type-checker once the
/// surrounding expression is inferred.
fn canonicalize_plot_color(arg: &Ir) -> Ir {
    let span = arg.span();
    let synth_name = format!("_plot_color_{}_{}", span.0, span.1);
    let lambda_value = match arg {
        Ir::Lambda { .. } => arg.clone(),
        other => Ir::Lambda {
            params: Vec::new(),
            body: Box::new(other.clone()),
            span,
        },
    };
    Ir::Binding {
        name: synth_name,
        value: Box::new(lambda_value),
        span,
        value_ty: None,
    }
}

/// Prefix used by `canonicalize_plot_color` for the synthesized
/// color-function binding. Public so `wgsl_gen` can locate the
/// matching `FunctionDef` after `lift_lambdas` runs without
/// re-deriving the naming convention.
pub(crate) const PLOT_COLOR_FN_PREFIX: &str = "_plot_color_";

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
    wgsl_gen::generate(&ir)
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

    /// Compile a `plot(...)` cell's body the same way the notebook does:
    /// extract via `build_plot_ir`, type-check, then run wgsl_gen.
    /// Panics with diagnostics on any failure.
    fn compile_plot(source: &str) -> String {
        let ir = parse(source).unwrap_or_else(|e| panic!("parse({:?}): {}", source, e));
        let actions = detect_cell_actions(&ir);
        let plot_idx = *actions
            .plots
            .first()
            .unwrap_or_else(|| panic!("{:?} has no plot()", source));
        let plot_ir = build_plot_ir(&ir, plot_idx)
            .unwrap_or_else(|e| panic!("build_plot_ir({:?}): {}", source, e));
        type_check(&plot_ir, source)
            .unwrap_or_else(|e| panic!("type_check({:?}): {}", source, e));
        let wgsl = wgsl_gen::generate(&plot_ir)
            .unwrap_or_else(|e| panic!("wgsl_gen({:?}): {}", source, e));
        validate_wgsl(&wgsl)
            .unwrap_or_else(|e| panic!("naga validation for {:?}:\n{}", source, e));
        wgsl
    }

    /// `plot((x) ↦ sin(x))` and `plot(y = sin(x))` both render the same
    /// curve: explicit lambda and implicit comparison desugar through
    /// the same corner-checking codegen path. We don't require byte-
    /// identical WGSL (the lambda form synthesizes an extra helper
    /// function), but both must trigger the corner-checking branch and
    /// emit the same body expression.
    #[test]
    fn test_plot_explicit_lambda_matches_implicit_curve() {
        let lambda_wgsl = compile_plot("plot((x) |-> sin(x))");
        let implicit_wgsl = compile_plot("plot(y = sin(x))");
        for wgsl in [&lambda_wgsl, &implicit_wgsl] {
            assert!(
                wgsl.contains("x_m") && wgsl.contains("x_p"),
                "expected corner-checking path; got:\n{}",
                wgsl
            );
            assert!(wgsl.contains("sin("), "missing sin call; got:\n{}", wgsl);
        }
    }

    /// 2-arg lambda gives a numeric surface — grayscale path, no
    /// corner-checking (which would draw an isoline, not a heatmap).
    #[test]
    fn test_plot_two_arg_lambda_compiles_as_field() {
        let wgsl = compile_plot("plot((u, v) |-> u*v)");
        assert!(
            !wgsl.contains("x_m"),
            "2-arg lambda should NOT trigger corner-checking; got:\n{}",
            wgsl
        );
        assert!(
            wgsl.contains("clamp(") || wgsl.contains("_shade"),
            "expected numeric grayscale path; got:\n{}",
            wgsl
        );
    }

    /// Lambdas using non-axis parameter names still wire up to the
    /// canonical axes. `(u) ↦ sin(u)` is the same function as
    /// `(x) ↦ sin(x)`; capture analysis on the lifted helper takes
    /// care of the rename via parameter passing.
    #[test]
    fn test_plot_lambda_param_can_be_any_name() {
        let wgsl = compile_plot("plot((u) |-> sin(u))");
        assert!(wgsl.contains("_plot_fn_"), "expected lifted helper fn");
    }

    /// Implicit color tuple in a 1D plot must lift to a `_plot_color_*`
    /// helper, swap `u.primary_color` for `_color`, and route the
    /// captured axis vars to the call site. Without all three, the
    /// shader either references undeclared captures or keeps using the
    /// uniform color the user explicitly overrode.
    #[test]
    fn test_plot_color_implicit_tuple_replaces_primary_color() {
        let wgsl = compile_plot("plot(y = sin(x), (sin(x), cos(y), 0.5, 1))");
        assert!(
            wgsl.contains("fn _plot_color_"),
            "expected lifted color fn; got:\n{}",
            wgsl
        );
        assert!(
            wgsl.contains("let _color = _plot_color_"),
            "expected `let _color = _plot_color_…(…)`; got:\n{}",
            wgsl
        );
        assert!(
            !wgsl.contains("u.primary_color"),
            "u.primary_color should not appear in the curve return; got:\n{}",
            wgsl
        );
        assert!(
            wgsl.contains("_color.rgb") && wgsl.contains("_color.a"),
            "the return path must use _color, got:\n{}",
            wgsl
        );
    }

    /// Explicit 1-arg color lambda: param name irrelevant, axis vars
    /// flow in through the lifted call. Validates that an explicit
    /// lambda path goes through the same `_plot_color_*` machinery
    /// as the implicit tuple form.
    #[test]
    fn test_plot_color_explicit_lambda_compiles() {
        let wgsl = compile_plot(
            "plot((x) |-> sin(x), (u) |-> (sin(u), cos(u), 0.5, 1))",
        );
        assert!(wgsl.contains("fn _plot_color_"), "expected lifted color fn");
        assert!(wgsl.contains("let _color = _plot_color_"));
    }

    /// 2D surface plots take the color path through the numeric
    /// branch instead of the corner-check branch — the same color
    /// helper still gets called, and `u.primary_color` still drops
    /// out of the return.
    #[test]
    fn test_plot_color_two_d_field_uses_color() {
        let wgsl = compile_plot(
            "plot((u, v) |-> u*v, (u, v) |-> (u, v, 0.5, 1))",
        );
        assert!(wgsl.contains("fn _plot_color_"), "got:\n{}", wgsl);
        assert!(wgsl.contains("_color.rgb"), "got:\n{}", wgsl);
        assert!(
            !wgsl.contains("u.primary_color"),
            "u.primary_color should be replaced; got:\n{}",
            wgsl
        );
    }

    /// Without a color arg, `u.primary_color` remains the source of
    /// truth — guards the default path from regression as the color
    /// branch lands.
    #[test]
    fn test_plot_without_color_keeps_primary_color() {
        let wgsl = compile_plot("plot(y = sin(x))");
        assert!(
            wgsl.contains("u.primary_color"),
            "expected default uniform color; got:\n{}",
            wgsl
        );
        assert!(
            !wgsl.contains("let _color ="),
            "no color helper should be emitted; got:\n{}",
            wgsl
        );
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
        let plot_ir = build_plot_ir(&ir, *actions.plots.last().unwrap()).unwrap();
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

    /// Plot of an explicit 1-arg lambda must desugar into a synthetic
    /// `_plot_fn_…` binding plus a `y = _plot_fn_…(x)` curve expression.
    /// Without this shape lift_lambdas would emit a `FunctionDef` but
    /// nothing in the result position would call it.
    #[test]
    fn test_build_plot_ir_one_arg_lambda_desugars_to_curve() {
        let ir = parse("plot((u) |-> sin(u))").unwrap();
        let actions = detect_cell_actions(&ir);
        let plot_ir = build_plot_ir(&ir, actions.plots[0]).unwrap();
        let Ir::Block { items, .. } = &plot_ir else {
            panic!("expected Block, got {:?}", plot_ir);
        };
        assert_eq!(items.len(), 2, "expected binding + curve, got {:?}", items);
        let Ir::Binding { name, value, .. } = &items[0] else {
            panic!("expected Binding first, got {:?}", items[0]);
        };
        assert!(name.starts_with("_plot_fn_"), "got name {}", name);
        assert!(
            matches!(value.as_ref(), Ir::Lambda { params, .. } if params.len() == 1),
            "binding value should still be the 1-arg lambda"
        );
        let Ir::Apply {
            callee, args, ..
        } = &items[1] else {
            panic!("expected Apply (y = …), got {:?}", items[1]);
        };
        assert_eq!(callee.name(), "eq");
        assert!(matches!(&args[0], Ir::Identifier { name, .. } if name == "y"));
        let Ir::Apply { callee: inner, args: inner_args, .. } = &args[1] else {
            panic!("expected inner Apply call, got {:?}", args[1]);
        };
        assert_eq!(inner.name(), name);
        assert_eq!(inner_args.len(), 1);
        assert!(matches!(&inner_args[0], Ir::Identifier { name, .. } if name == "x"));
    }

    /// 2-arg lambdas desugar into a numeric field expression — no
    /// outer `y = …` wrapper, since the result is a scalar at every
    /// `(x, y)` instead of a curve.
    #[test]
    fn test_build_plot_ir_two_arg_lambda_desugars_to_field() {
        let ir = parse("plot((p, q) |-> p*q)").unwrap();
        let actions = detect_cell_actions(&ir);
        let plot_ir = build_plot_ir(&ir, actions.plots[0]).unwrap();
        let Ir::Block { items, .. } = &plot_ir else {
            panic!("expected Block, got {:?}", plot_ir);
        };
        assert_eq!(items.len(), 2);
        let Ir::Apply { callee, args, .. } = &items[1] else {
            panic!("expected call as result, got {:?}", items[1]);
        };
        let Ir::Binding { name: bound, .. } = &items[0] else {
            panic!("expected Binding first");
        };
        assert_eq!(callee.name(), bound);
        assert_eq!(args.len(), 2);
        assert!(matches!(&args[0], Ir::Identifier { name, .. } if name == "x"));
        assert!(matches!(&args[1], Ir::Identifier { name, .. } if name == "y"));
    }

    /// Implicit-comparison and bare-expression forms predate explicit
    /// lambdas; they must keep flowing through unchanged so the rest of
    /// the pipeline still sees the same shape it did before.
    #[test]
    fn test_build_plot_ir_implicit_comparison_passes_through() {
        let ir = parse("plot(y = sin(x))").unwrap();
        let actions = detect_cell_actions(&ir);
        let plot_ir = build_plot_ir(&ir, actions.plots[0]).unwrap();
        match &plot_ir {
            Ir::Apply { callee, .. } => assert_eq!(callee.name(), "eq"),
            other => panic!("expected single Apply, got {:?}", other),
        }
    }

    #[test]
    fn test_build_plot_ir_bare_expr_passes_through() {
        let ir = parse("plot(x*x + y*y)").unwrap();
        let actions = detect_cell_actions(&ir);
        let plot_ir = build_plot_ir(&ir, actions.plots[0]).unwrap();
        assert!(
            matches!(&plot_ir, Ir::Apply { callee, .. } if callee.name() == "add"),
            "bare expression should remain a single Apply"
        );
    }

    /// 0-arg lambdas have no axis to attach to. Pre-empt the cryptic
    /// downstream error with a clear message at canonicalization time.
    #[test]
    fn test_build_plot_ir_zero_arg_lambda_errors() {
        // `() |-> 5` parses as 0-tuple LHS → 0-param lambda.
        let ir = parse("plot(() |-> 5)").unwrap();
        let actions = detect_cell_actions(&ir);
        let err = build_plot_ir(&ir, actions.plots[0]).unwrap_err();
        assert!(err.contains("at least one parameter"), "got: {}", err);
    }

    /// Lambdas with arity ≥ 3 are reserved for the ND-with-tunable-
    /// parameter-sliders work. They must error cleanly today, not
    /// silently mis-render.
    #[test]
    fn test_build_plot_ir_three_arg_lambda_errors() {
        let ir = parse("plot((a, b, c) |-> a+b+c)").unwrap();
        let actions = detect_cell_actions(&ir);
        let err = build_plot_ir(&ir, actions.plots[0]).unwrap_err();
        assert!(err.contains("3 parameters"), "got: {}", err);
    }
}
