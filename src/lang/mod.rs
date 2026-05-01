pub mod ast;
pub mod compute_gen;
pub mod highlight;
pub mod interpreter;
pub mod lang_service;
pub mod lexer;
pub mod parser;
pub mod reduce;
pub mod token;
pub mod wgsl_gen;

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

/// Check whether the AST contains nodes that require the interpreter
/// (arrays, parallel for) rather than the fragment shader path.
pub fn needs_interpreter(ast: &ast::AstNode) -> bool {
    match ast {
        ast::AstNode::ArrayLiteral(_)
        | ast::AstNode::IndexAccess { .. }
        | ast::AstNode::ParallelFor { .. }
        | ast::AstNode::IndexAssign { .. } => true,
        ast::AstNode::Block(stmts) => stmts.iter().any(needs_interpreter),
        ast::AstNode::Binding { value, .. } => needs_interpreter(value),
        ast::AstNode::TupleBinding { value, .. } => needs_interpreter(value),
        ast::AstNode::FunctionDef { body, .. } => needs_interpreter(body),
        ast::AstNode::IfExpr {
            condition,
            then_branch,
            else_branch,
        } => {
            needs_interpreter(condition)
                || needs_interpreter(then_branch)
                || else_branch.as_ref().is_some_and(|e| needs_interpreter(e))
        }
        ast::AstNode::Apply { args, .. } => args.iter().any(needs_interpreter),
        ast::AstNode::WhileLoop { condition, body } => {
            needs_interpreter(condition) || needs_interpreter(body)
        }
        _ => false,
    }
}

/// Detected print/plot actions in a cell's AST.
#[derive(Debug)]
pub struct CellActions {
    /// Index of the first `print(...)` statement in the block (if any).
    pub first_print: Option<usize>,
    /// Index of the last `plot(...)` statement in the block (if any).
    pub last_plot: Option<usize>,
}

impl CellActions {
    pub fn has_action(&self) -> bool {
        self.first_print.is_some() || self.last_plot.is_some()
    }
}

/// Walk the AST to find all print() and plot() action statements.
pub fn detect_cell_actions(ast: &ast::AstNode) -> CellActions {
    let stmts: &[ast::AstNode] = match ast {
        ast::AstNode::Block(stmts) => stmts,
        other => std::slice::from_ref(other),
    };

    let mut result = CellActions {
        first_print: None,
        last_plot: None,
    };

    for (i, stmt) in stmts.iter().enumerate() {
        if let ast::AstNode::Apply { name, .. } = stmt {
            match name.as_str() {
                "print" if result.first_print.is_none() => {
                    result.first_print = Some(i);
                }
                "plot" => {
                    result.last_plot = Some(i);
                }
                _ => {}
            }
        }
    }

    result
}

/// Build an AST for evaluating a print expression.
/// Includes all non-action statements before the print index,
/// plus the unwrapped inner expression from print().
pub fn build_print_ast(ast: &ast::AstNode, print_index: usize) -> ast::AstNode {
    let stmts = match ast {
        ast::AstNode::Block(stmts) => stmts,
        other => {
            if let ast::AstNode::Apply { name, args } = other {
                if name == "print" && args.len() == 1 {
                    return args[0].clone();
                }
            }
            return other.clone();
        }
    };

    let mut result = Vec::new();
    for (i, stmt) in stmts.iter().enumerate() {
        if i == print_index {
            if let ast::AstNode::Apply { name, args } = stmt {
                if name == "print" && args.len() == 1 {
                    result.push(args[0].clone());
                }
            }
            break;
        }
        if let ast::AstNode::Apply { name, .. } = stmt {
            if name == "print" || name == "plot" {
                continue;
            }
        }
        result.push(stmt.clone());
    }

    match result.len() {
        1 => result.remove(0),
        _ => ast::AstNode::Block(result),
    }
}

/// Build an AST for plotting. Includes all non-action statements
/// plus the unwrapped inner expression from plot().
pub fn build_plot_ast(ast: &ast::AstNode, plot_index: usize) -> ast::AstNode {
    let stmts = match ast {
        ast::AstNode::Block(stmts) => stmts,
        other => {
            if let ast::AstNode::Apply { name, args } = other {
                if name == "plot" && args.len() == 1 {
                    return args[0].clone();
                }
            }
            return other.clone();
        }
    };

    let mut result = Vec::new();
    for (i, stmt) in stmts.iter().enumerate() {
        if i == plot_index {
            if let ast::AstNode::Apply { name, args } = stmt {
                if name == "plot" && args.len() == 1 {
                    result.push(args[0].clone());
                }
            }
            continue;
        }
        if let ast::AstNode::Apply { name, .. } = stmt {
            if name == "print" || name == "plot" {
                continue;
            }
        }
        result.push(stmt.clone());
    }

    match result.len() {
        1 => result.remove(0),
        _ => ast::AstNode::Block(result),
    }
}

/// Lex and parse source code into an AST.
pub fn parse(source: &str) -> Result<ast::AstNode, String> {
    let mut lex = lexer::Lexer::new(source);
    let tokens = lex.tokenize()?;
    let mut p = parser::Parser::new(tokens, source.to_string());
    p.parse()
}

/// Compile source code through the full pipeline: lex → parse → WGSL gen.
/// Returns the complete WGSL shader source string.
pub fn compile(source: &str) -> Result<String, String> {
    let mut lex = lexer::Lexer::new(source);
    let tokens = lex.tokenize()?;
    let mut parser = parser::Parser::new(tokens, source.to_string());
    let ast = parser.parse()?;
    wgsl_gen::generate(&ast)
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

    // ─── UI test harness ──────────────────────────────────────────────────
    //
    // Run cell text through the same pipeline `trigger_cell_play` uses, with
    // shader compilation and print rendering mocked. Lets tests use raw multi-
    // line strings exactly as a user would type them in the UI.

    #[derive(Debug, Default)]
    struct CellOutcome {
        /// Generated WGSL when a `plot(...)` action was detected and built.
        shader: Option<String>,
        /// Concrete print result when a `print(...)` action evaluated to a value
        /// in the interpreter. Symbolic prints (free variables → REDUCE in real
        /// UI) leave this `None` and set `interpreter_error` instead.
        print: Option<String>,
        /// Error from parse / wgsl_gen / naga validation.
        error: Option<String>,
        /// Reason the interpreter couldn't evaluate the print expression. Often
        /// expected (e.g. `Undefined variable: x` because x is an axis variable).
        interpreter_error: Option<String>,
    }

    /// Strip the smallest leading indent shared by all non-empty lines and
    /// trim outer blank lines. Lets tests use raw strings with natural Rust
    /// indentation: `r#" f := ( ... ) plot(f) "#` parses as if unindented.
    fn dedent(s: &str) -> String {
        let lines: Vec<&str> = s.lines().collect();
        let min_indent = lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.chars().take_while(|c| *c == ' ' || *c == '\t').count())
            .min()
            .unwrap_or(0);
        let stripped: Vec<String> = lines
            .iter()
            .map(|l| {
                if l.len() >= min_indent {
                    l[min_indent..].to_string()
                } else {
                    l.to_string()
                }
            })
            .collect();
        stripped.join("\n").trim_matches('\n').to_string()
    }

    /// Run a single cell's text through the full UI pipeline. Mocks shader
    /// compilation (returns the WGSL string) and print rendering (returns the
    /// formatted value).
    fn run_cell(source: &str) -> CellOutcome {
        let mut out = CellOutcome::default();
        let dedented = dedent(source);

        let ast = match crate::lang::parse(&dedented) {
            Ok(a) => a,
            Err(e) => {
                out.error = Some(format!("parse: {}", e));
                return out;
            }
        };

        let actions = crate::lang::detect_cell_actions(&ast);

        if let Some(print_idx) = actions.first_print {
            let eval_ast = crate::lang::build_print_ast(&ast, print_idx);
            let gpu = crate::lang::interpreter::CpuFallback;
            match crate::lang::interpreter::eval(&eval_ast, &gpu) {
                Ok(val) => out.print = Some(format!("{}", val)),
                Err(e) => out.interpreter_error = Some(e),
            }
        }

        if let Some(plot_idx) = actions.last_plot {
            let plot_ast = crate::lang::build_plot_ast(&ast, plot_idx);
            match crate::lang::wgsl_gen::generate(&plot_ast) {
                Ok(wgsl) => {
                    if let Err(e) = validate_wgsl(&wgsl) {
                        out.error = Some(format!("naga: {}", e));
                    }
                    out.shader = Some(wgsl);
                }
                Err(e) => out.error = Some(format!("wgsl_gen: {}", e)),
            }
        }

        out
    }

    #[test]
    fn test_user_function_with_axis_capture() {
        // `f(n)` has parameter `n` but body uses `x` and `y` (axis variables);
        // they're captured implicitly. `plot(f(1))` then needs corner-checking
        // via a `_diff_f` companion, since inlining the imperative body inside
        // `emit_bool_with_corners` would drop the loop's side effects.
        let outcome = run_cell(
            r#"
            f(n) := (
                sum := x
                for i in 0..n (sum := sum^x)
                y = sum
            )
            plot(f(1))
            "#,
        );
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        let wgsl = outcome.shader.expect("plot generated");
        // Function takes captured x, y as extra params.
        assert!(
            wgsl.contains("fn f(n: f32, x: f32, y: f32) -> bool"),
            "axis variables should be captured as params; got:\n{}",
            wgsl
        );
        // Companion diff function for corner-checking.
        assert!(
            wgsl.contains("fn _diff_f(n: f32, x: f32, y: f32) -> f32"),
            "comparison-result functions need a `_diff_<name>` companion; got:\n{}",
            wgsl
        );
        // Corner-checking calls _diff_f at the four pixel corners.
        assert!(
            wgsl.contains("_diff_f(1.0, x_m, y_m)") && wgsl.contains("_diff_f(1.0, x_p, y_p)"),
            "corner-checking should call _diff_f at each corner; got:\n{}",
            wgsl
        );
    }

    #[test]
    fn test_user_block_with_loop_plot() {
        // Source is exactly what the user typed in the UI. The harness runs
        // it through parse → detect_cell_actions → build_plot_ast → wgsl_gen.
        let outcome = run_cell(
            r#"
            f := (
                sum := 0
                for i in 0..10 (
                    sum := sum + x²+4*x²
                )
                y = sum
            )
            plot(f)
            "#,
        );

        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        let wgsl = outcome.shader.expect("plot generated");

        // pow() is ~25 GPU ops — small-int exponents must expand to mul.
        assert!(!wgsl.contains("pow("), "got:\n{}", wgsl);
        // Block-on-RHS-of-`:=` is lifted into a WGSL function so corner-checking
        // re-evaluates it at each corner.
        assert!(wgsl.contains("fn _lifted_f("), "got:\n{}", wgsl);
        // Each corner is computed exactly once and reused across the corner-check.
        assert!(
            wgsl.contains("let _corner_f_mm = _lifted_f(x_m, y_m);"),
            "got:\n{}",
            wgsl
        );
        // The unused `let f = _lifted_f(x,y) <op> 0.0` binding is skipped —
        // the bool path uses corner-checking, never the binding.
        assert!(!wgsl.contains("let f ="), "got:\n{}", wgsl);
    }

    #[test]
    fn test_user_print_simple_numeric() {
        let outcome = run_cell(
            r#"
            print(3 + 4)
            "#,
        );
        assert_eq!(outcome.print.as_deref(), Some("7"));
        assert!(outcome.shader.is_none());
    }

    #[test]
    fn test_user_print_symbolic_falls_to_reduce() {
        // `f` references `x` (axis variable), so the interpreter can't produce
        // a concrete number — in the UI this falls through to REDUCE for
        // symbolic simplification.
        let outcome = run_cell(
            r#"
            f := x + 2*x
            print(f)
            "#,
        );
        assert!(outcome.print.is_none());
        assert!(
            outcome
                .interpreter_error
                .as_deref()
                .unwrap_or("")
                .contains("Undefined variable: x"),
            "expected interpreter to fail on free `x`; got: {:?}",
            outcome.interpreter_error
        );
    }

    #[test]
    fn test_user_plot_and_print_coexist() {
        let outcome = run_cell(
            r#"
            f := x + 2*x
            print(f)
            plot(y = f)
            "#,
        );
        // Print: interpreter fails on `x`, that's expected (UI then routes to REDUCE).
        assert!(outcome.interpreter_error.is_some());
        // Plot: should render.
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        let wgsl = outcome.shader.expect("plot generated");
        assert!(wgsl.contains("fn fs_main"));
    }

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
        let ast = parse("print(3+9)").unwrap();
        let actions = detect_cell_actions(&ast);
        assert!(actions.first_print.is_some());
        assert!(actions.last_plot.is_none());
    }

    #[test]
    fn test_detect_plot() {
        let ast = parse("plot(x+y)").unwrap();
        let actions = detect_cell_actions(&ast);
        assert!(actions.first_print.is_none());
        assert!(actions.last_plot.is_some());
    }

    #[test]
    fn test_detect_none() {
        let ast = parse("3+9").unwrap();
        let actions = detect_cell_actions(&ast);
        assert!(!actions.has_action());
    }

    #[test]
    fn test_detect_both_print_and_plot() {
        let ast = parse("f := x+2*x\nprint(f)\nplot(y=f)").unwrap();
        let actions = detect_cell_actions(&ast);
        assert!(actions.first_print.is_some());
        assert!(actions.last_plot.is_some());
    }

    #[test]
    fn test_build_print_ast_single() {
        let ast = parse("print(3+9)").unwrap();
        let print_ast = build_print_ast(&ast, 0);
        assert!(matches!(print_ast, ast::AstNode::Apply { ref name, .. } if name == "add"));
    }

    #[test]
    fn test_build_print_ast_with_bindings() {
        let ast = parse("f := 5\nprint(f)").unwrap();
        let actions = detect_cell_actions(&ast);
        let print_ast = build_print_ast(&ast, actions.first_print.unwrap());
        if let ast::AstNode::Block(stmts) = &print_ast {
            assert_eq!(stmts.len(), 2);
            assert!(matches!(&stmts[0], ast::AstNode::Binding { .. }));
            assert!(matches!(&stmts[1], ast::AstNode::Identifier(_)));
        } else {
            panic!("Expected Block, got {:?}", print_ast);
        }
    }

    #[test]
    fn test_build_plot_ast_strips_print() {
        let ast = parse("f := x\nprint(f)\nplot(y=f)").unwrap();
        let actions = detect_cell_actions(&ast);
        let plot_ast = build_plot_ast(&ast, actions.last_plot.unwrap());
        if let ast::AstNode::Block(stmts) = &plot_ast {
            for stmt in stmts {
                if let ast::AstNode::Apply { name, .. } = stmt {
                    assert_ne!(name, "print", "print should be stripped from plot AST");
                }
            }
        }
    }
}
