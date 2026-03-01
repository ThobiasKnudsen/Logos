use super::ast::AstNode;

/// Generate a complete WGSL fragment shader from an AST.
///
/// The generated shader:
/// - Defines the uniform struct matching ShaderUniforms
/// - Maps user `x`/`y` to world coordinates via axis_min/axis_max
/// - Maps the final expression to a color output
pub fn generate(ast: &AstNode) -> Result<String, String> {
    let mut ctx = GenContext::new();

    // Collect top-level function definitions
    ctx.collect_functions(ast);

    // Find the expression to evaluate (last non-binding, non-function-def statement)
    let expr = find_result_expr(ast)?;

    // Generate the expression code
    let expr_code = ctx.emit_expr(expr)?;

    // Build the full WGSL shader
    let mut shader = String::new();

    // Uniform struct
    shader.push_str(UNIFORM_STRUCT);
    shader.push('\n');

    // Emit user-defined helper functions
    for func in &ctx.functions {
        shader.push_str(&func.wgsl_code);
        shader.push('\n');
    }

    // Fragment entry point
    shader.push_str("@fragment\n");
    shader.push_str("fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {\n");
    shader.push_str("    let world = mix(u.axis_min, u.axis_max, uv);\n");
    shader.push_str("    let x = world.x;\n");
    shader.push_str("    let y = world.y;\n");
    shader.push_str("    let time = u.time;\n");

    // Emit bindings
    for binding in &ctx.bindings {
        shader.push_str(&format!("    let {} = {};\n", binding.name, binding.expr));
    }

    // Map result to color
    shader.push_str(&format!("    let _result = {};\n", expr_code));
    if returns_bool(expr) {
        // Boolean expressions: convert to 0.0/1.0 via select
        shader.push_str("    let c = select(0.0, 1.0, _result);\n");
    } else {
        // Numeric expressions: clamp to [0, 1] grayscale
        shader.push_str("    let c = clamp(_result, 0.0, 1.0);\n");
    }
    shader.push_str("    return vec4<f32>(c, c, c, 1.0);\n");
    shader.push_str("}\n");

    Ok(shader)
}

/// Generate WGSL where the expression is expected to produce a vec4 color directly.
pub fn generate_color(ast: &AstNode) -> Result<String, String> {
    let mut ctx = GenContext::new();
    ctx.collect_functions(ast);
    let expr = find_result_expr(ast)?;
    let expr_code = ctx.emit_expr(expr)?;

    let mut shader = String::new();
    shader.push_str(UNIFORM_STRUCT);
    shader.push('\n');

    for func in &ctx.functions {
        shader.push_str(&func.wgsl_code);
        shader.push('\n');
    }

    shader.push_str("@fragment\n");
    shader.push_str("fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {\n");
    shader.push_str("    let world = mix(u.axis_min, u.axis_max, uv);\n");
    shader.push_str("    let x = world.x;\n");
    shader.push_str("    let y = world.y;\n");
    shader.push_str("    let time = u.time;\n");

    for binding in &ctx.bindings {
        shader.push_str(&format!("    let {} = {};\n", binding.name, binding.expr));
    }

    shader.push_str(&format!("    return {};\n", expr_code));
    shader.push_str("}\n");

    Ok(shader)
}

const UNIFORM_STRUCT: &str = r#"struct Uniforms {
    time: f32,
    _pad0: f32,
    resolution: vec2<f32>,
    mouse: vec2<f32>,
    zoom: f32,
    _pad1: f32,
    pan: vec2<f32>,
    axis_min: vec2<f32>,
    axis_max: vec2<f32>,
    _pad2: vec2<f32>,
    primary_color: vec4<f32>,
    secondary_color: vec4<f32>,
    background_color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;"#;

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

struct EmittedBinding {
    name: String,
    expr: String,
}

struct EmittedFunction {
    #[allow(dead_code)]
    name: String,
    wgsl_code: String,
}

struct GenContext {
    functions: Vec<EmittedFunction>,
    bindings: Vec<EmittedBinding>,
}

impl GenContext {
    fn new() -> Self {
        Self {
            functions: Vec::new(),
            bindings: Vec::new(),
        }
    }

    /// Walk the AST to collect function definitions.
    fn collect_functions(&mut self, ast: &AstNode) {
        match ast {
            AstNode::Block(stmts) => {
                for stmt in stmts {
                    self.collect_functions(stmt);
                }
            }
            AstNode::FunctionDef { name, params, body } => {
                if let Ok(body_code) = self.emit_expr(body) {
                    let param_list: Vec<String> = params
                        .iter()
                        .map(|p| format!("{}: f32", p))
                        .collect();
                    let wgsl_code = format!(
                        "fn {}({}) -> f32 {{\n    return {};\n}}\n",
                        name,
                        param_list.join(", "),
                        body_code,
                    );
                    self.functions.push(EmittedFunction {
                        name: name.clone(),
                        wgsl_code,
                    });
                }
            }
            AstNode::Binding { name, value } => {
                if let Ok(expr_code) = self.emit_expr(value) {
                    self.bindings.push(EmittedBinding {
                        name: name.clone(),
                        expr: expr_code,
                    });
                }
            }
            _ => {}
        }
    }

    fn emit_expr(&self, node: &AstNode) -> Result<String, String> {
        match node {
            AstNode::Number(n) => {
                // Ensure float literal for WGSL
                let s = if n.fract() == 0.0 && !n.is_nan() && !n.is_infinite() {
                    format!("{:.1}", n)
                } else {
                    format!("{}", n)
                };
                Ok(s)
            }
            AstNode::BoolLit(b) => Ok(format!("{}", b)),
            AstNode::Identifier(name) => Ok(name.clone()),
            AstNode::Apply { name, args } => self.emit_apply(name, args),
            AstNode::Tuple(items) => {
                let parts: Result<Vec<_>, _> = items.iter().map(|i| self.emit_expr(i)).collect();
                let parts = parts?;
                match items.len() {
                    2 => Ok(format!("vec2<f32>({})", parts.join(", "))),
                    3 => Ok(format!("vec3<f32>({})", parts.join(", "))),
                    4 => Ok(format!("vec4<f32>({})", parts.join(", "))),
                    _ => Err(format!("Unsupported tuple size: {}", items.len())),
                }
            }
            AstNode::Block(stmts) => {
                // Return the last expression
                if let Some(last) = stmts.last() {
                    self.emit_expr(last)
                } else {
                    Ok("0.0".to_string())
                }
            }
            AstNode::Binding { .. } => {
                // Bindings are handled at the top level
                Ok("0.0".to_string())
            }
            AstNode::IfExpr { condition, then_branch, else_branch } => {
                let cond = self.emit_expr(condition)?;
                let then_code = self.emit_expr(then_branch)?;
                if let Some(else_b) = else_branch {
                    let else_code = self.emit_expr(else_b)?;
                    Ok(format!("select({}, {}, {})", else_code, then_code, cond))
                } else {
                    Ok(format!("select(0.0, {}, {})", then_code, cond))
                }
            }
            AstNode::FunctionDef { .. } => {
                // Already collected
                Ok("0.0".to_string())
            }
        }
    }

    fn emit_apply(&self, name: &str, args: &[AstNode]) -> Result<String, String> {
        let emit_args: Result<Vec<_>, _> = args.iter().map(|a| self.emit_expr(a)).collect();
        let emitted = emit_args?;

        match (name, args.len()) {
            // Binary infix operators
            ("add", 2) => Ok(format!("({} + {})", emitted[0], emitted[1])),
            ("sub", 2) => Ok(format!("({} - {})", emitted[0], emitted[1])),
            ("mul", 2) => Ok(format!("({} * {})", emitted[0], emitted[1])),
            ("div", 2) => Ok(format!("({} / {})", emitted[0], emitted[1])),
            ("mod", 2) => Ok(format!("((({} % {}) + {}) % {})", emitted[0], emitted[1], emitted[1], emitted[1])),

            // Comparison
            ("eq", 2) => Ok(format!("({} == {})", emitted[0], emitted[1])),
            ("neq", 2) => Ok(format!("({} != {})", emitted[0], emitted[1])),
            ("lt", 2) => Ok(format!("({} < {})", emitted[0], emitted[1])),
            ("gt", 2) => Ok(format!("({} > {})", emitted[0], emitted[1])),
            ("lte", 2) => Ok(format!("({} <= {})", emitted[0], emitted[1])),
            ("gte", 2) => Ok(format!("({} >= {})", emitted[0], emitted[1])),

            // Logical
            ("and", 2) => Ok(format!("({} && {})", emitted[0], emitted[1])),
            ("or", 2) => Ok(format!("({} || {})", emitted[0], emitted[1])),
            ("not", 1) => Ok(format!("!({})", emitted[0])),

            // Unary
            ("neg", 1) => Ok(format!("-({})", emitted[0])),

            // Math builtins — direct WGSL mapping
            ("sin", 1) => Ok(format!("sin({})", emitted[0])),
            ("cos", 1) => Ok(format!("cos({})", emitted[0])),
            ("tan", 1) => Ok(format!("tan({})", emitted[0])),
            ("asin", 1) => Ok(format!("asin({})", emitted[0])),
            ("acos", 1) => Ok(format!("acos({})", emitted[0])),
            ("atan", 1) => Ok(format!("atan({})", emitted[0])),
            ("atan", 2) => Ok(format!("atan2({}, {})", emitted[0], emitted[1])),
            ("sinh", 1) => Ok(format!("sinh({})", emitted[0])),
            ("cosh", 1) => Ok(format!("cosh({})", emitted[0])),
            ("tanh", 1) => Ok(format!("tanh({})", emitted[0])),
            ("log", 1) => Ok(format!("log({})", emitted[0])),
            ("log2", 1) => Ok(format!("log2({})", emitted[0])),
            ("exp", 1) => Ok(format!("exp({})", emitted[0])),
            ("exp2", 1) => Ok(format!("exp2({})", emitted[0])),
            ("sqrt", 1) => Ok(format!("sqrt({})", emitted[0])),
            ("abs", 1) => Ok(format!("abs({})", emitted[0])),
            ("sign", 1) => Ok(format!("sign({})", emitted[0])),
            ("floor", 1) => Ok(format!("floor({})", emitted[0])),
            ("ceil", 1) => Ok(format!("ceil({})", emitted[0])),
            ("round", 1) => Ok(format!("round({})", emitted[0])),
            ("fract", 1) => Ok(format!("fract({})", emitted[0])),
            ("pow", 2) => Ok(format!("pow({}, {})", emitted[0], emitted[1])),
            ("min", 2) => Ok(format!("min({}, {})", emitted[0], emitted[1])),
            ("max", 2) => Ok(format!("max({}, {})", emitted[0], emitted[1])),
            ("clamp", 3) => Ok(format!("clamp({}, {}, {})", emitted[0], emitted[1], emitted[2])),
            ("mix", 3) => Ok(format!("mix({}, {}, {})", emitted[0], emitted[1], emitted[2])),
            ("step", 2) => Ok(format!("step({}, {})", emitted[0], emitted[1])),
            ("smoothstep", 3) => Ok(format!("smoothstep({}, {}, {})", emitted[0], emitted[1], emitted[2])),
            ("length", 1) => Ok(format!("length({})", emitted[0])),
            ("normalize", 1) => Ok(format!("normalize({})", emitted[0])),
            ("dot", 2) => Ok(format!("dot({}, {})", emitted[0], emitted[1])),
            ("cross", 2) => Ok(format!("cross({}, {})", emitted[0], emitted[1])),

            // Type constructors / casts
            ("f32", 1) => Ok(format!("f32({})", emitted[0])),
            ("i32", 1) => Ok(format!("i32({})", emitted[0])),
            ("vec2", n) if n >= 1 => Ok(format!("vec2<f32>({})", emitted.join(", "))),
            ("vec3", n) if n >= 1 => Ok(format!("vec3<f32>({})", emitted.join(", "))),
            ("vec4", n) if n >= 1 => Ok(format!("vec4<f32>({})", emitted.join(", "))),

            // User-defined functions: emit as regular call
            _ => Ok(format!("{}({})", name, emitted.join(", "))),
        }
    }
}

/// Check if an AST node produces a boolean value in WGSL.
///
/// Comparisons, logical ops, and boolean literals produce `bool` in WGSL,
/// which cannot be passed to `clamp()`. We use `select()` for these instead.
fn returns_bool(node: &AstNode) -> bool {
    match node {
        AstNode::BoolLit(_) => true,
        AstNode::Apply { name, .. } => matches!(
            name.as_str(),
            "eq" | "neq" | "lt" | "gt" | "lte" | "gte" | "and" | "or" | "not"
        ),
        AstNode::Block(stmts) => stmts.last().map_or(false, returns_bool),
        _ => false,
    }
}

/// Find the result expression in the AST (last non-binding, non-function-def node).
fn find_result_expr(ast: &AstNode) -> Result<&AstNode, String> {
    match ast {
        AstNode::Block(stmts) => {
            // Walk from the end to find the last expression that isn't a binding or func def
            for stmt in stmts.iter().rev() {
                match stmt {
                    AstNode::Binding { .. } | AstNode::FunctionDef { .. } => continue,
                    other => return Ok(other),
                }
            }
            Err("No result expression found — all statements are bindings or function definitions".to_string())
        }
        // Single expression at the top level
        other => Ok(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::lexer::Lexer;
    use crate::lang::parser::Parser;

    fn gen(input: &str) -> String {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let ast = parser.parse().unwrap();
        generate(&ast).unwrap()
    }

    #[test]
    fn test_simple_expr() {
        let shader = gen("x + y");
        assert!(shader.contains("(x + y)"));
        assert!(shader.contains("@fragment"));
        assert!(shader.contains("fn fs_main"));
    }

    #[test]
    fn test_function_call() {
        let shader = gen("sin(x) * cos(y)");
        assert!(shader.contains("sin(x)"));
        assert!(shader.contains("cos(y)"));
    }

    #[test]
    fn test_complex_expr() {
        let shader = gen("x * x + y * y");
        assert!(shader.contains("((x * x) + (y * y))"));
    }

    #[test]
    fn test_with_binding() {
        let shader = gen("r: sqrt(x * x + y * y)\nr");
        assert!(shader.contains("let r = sqrt(((x * x) + (y * y)))"));
    }

    #[test]
    fn test_with_function_def() {
        let shader = gen("f(a, b): a + b\nf(x, y)");
        assert!(shader.contains("fn f(a: f32, b: f32) -> f32"));
        assert!(shader.contains("f(x, y)"));
    }

    #[test]
    fn test_equality_generates_comparison() {
        // `=` in Logos is equality, should generate `==` in WGSL
        let shader = gen("x = 5");
        assert!(shader.contains("(x == 5.0)"));
    }

    #[test]
    fn test_empty_input() {
        let shader = gen("");
        assert!(shader.contains("@fragment"));
        assert!(shader.contains("fn fs_main"));
    }

    #[test]
    fn test_binding_and_use_multiline() {
        let shader = gen("a: x + 1\nb: y + 2\na * b");
        assert!(shader.contains("let a = (x + 1.0)"));
        assert!(shader.contains("let b = (y + 2.0)"));
        assert!(shader.contains("(a * b)"));
    }

    #[test]
    fn test_function_with_block_body() {
        let shader = gen("f(a): (r: a * 2, r + 1)\nf(x)");
        assert!(shader.contains("fn f(a: f32) -> f32"));
        assert!(shader.contains("f(x)"));
    }

    #[test]
    fn test_mandelbrot_like_expr() {
        // Complex expression similar to real mandelbrot code
        let shader = gen("r: sqrt(x*x + y*y)\nif (r < 2) 1 else 0");
        assert!(shader.contains("let r = sqrt("));
        assert!(shader.contains("select("));
    }

    #[test]
    fn test_if_expr() {
        let shader = gen("if (x > 0) 1 else 0");
        assert!(shader.contains("select("));
    }

    #[test]
    fn test_uniform_struct() {
        let shader = gen("x");
        assert!(shader.contains("struct Uniforms"));
        assert!(shader.contains("var<uniform> u: Uniforms"));
    }

    #[test]
    fn test_tuple_to_vec() {
        let shader = gen("(x, y, 0)");
        assert!(shader.contains("vec3<f32>(x, y, 0.0)"));
    }

    // --- Bool-to-f32 conversion tests ---
    // WGSL doesn't allow bool in clamp(). Boolean results must use select().

    #[test]
    fn test_equality_uses_select_not_clamp() {
        // `x = 0` produces bool in WGSL; must use select(), not clamp()
        let shader = gen("x = 0");
        assert!(shader.contains("select(0.0, 1.0, _result)"), "bool result should use select");
        assert!(!shader.contains("clamp(_result"), "bool result must NOT use clamp");
    }

    #[test]
    fn test_comparison_lt_uses_select() {
        let shader = gen("x < 1");
        assert!(shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_comparison_gt_uses_select() {
        let shader = gen("x > 0");
        assert!(shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_comparison_neq_uses_select() {
        let shader = gen("x != 0");
        assert!(shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_comparison_lte_uses_select() {
        let shader = gen("x <= 1");
        assert!(shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_comparison_gte_uses_select() {
        let shader = gen("x >= 0");
        assert!(shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_logical_and_uses_select() {
        let shader = gen("x > 0 and x < 1");
        assert!(shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_logical_or_uses_select() {
        let shader = gen("x < 0 or x > 1");
        assert!(shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_logical_not_uses_select() {
        let shader = gen("not (x > 0)");
        assert!(shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_bool_literal_uses_select() {
        let shader = gen("true");
        assert!(shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_numeric_expr_still_uses_clamp() {
        // Normal numeric expressions should still use clamp
        let shader = gen("x * x + y * y");
        assert!(shader.contains("clamp(_result, 0.0, 1.0)"));
        assert!(!shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_if_expr_is_numeric_not_bool() {
        // `if (cond) 1 else 0` uses select() in the expression itself,
        // but the result type is f32, so the wrapper should use clamp
        let shader = gen("if (x > 0) 1 else 0");
        assert!(shader.contains("clamp(_result, 0.0, 1.0)"));
    }

    #[test]
    fn test_binding_then_bool_result_uses_select() {
        // Multi-line: binding + boolean result expression
        let shader = gen("r: x * x + y * y\nr < 1");
        assert!(shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_binding_then_numeric_result_uses_clamp() {
        let shader = gen("r: x * x + y * y\nsin(r)");
        assert!(shader.contains("clamp(_result, 0.0, 1.0)"));
    }
}
