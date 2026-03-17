use std::collections::HashSet;
use super::ast::AstNode;

/// Maximum iterations for generated WGSL `for` loops.
/// Prevents GPU hang from unbounded loops in user code.
const MAX_LOOP_ITERATIONS: u32 = 10000;

/// Generate a complete WGSL fragment shader from an AST.
///
/// The generated shader:
/// - Defines the uniform struct matching ShaderUniforms
/// - Maps user `x`/`y` to world coordinates via axis_min/axis_max
/// - For boolean expressions: uses corner-checking for pixel-perfect rendering
///   (equality → curve straddling, inequalities → all-corners-agree)
/// - For numeric expressions: clamps to [0, 1] grayscale
pub fn generate(ast: &AstNode) -> Result<String, String> {
    let mut ctx = GenContext::new();

    // Collect top-level function definitions (and bindings if no top-level loops)
    ctx.collect_functions(ast);

    // Find the expression to evaluate (last non-binding, non-function-def statement)
    let expr = find_result_expr(ast)?;

    let is_bool = returns_bool(expr);
    let is_vec = ctx.result_is_vec(expr);

    // Check for top-level loops (for or while)
    let top_has_loops = match ast {
        AstNode::Block(stmts) => stmts.iter().any(|s| matches!(s, AstNode::ForLoop { .. } | AstNode::WhileLoop { .. })),
        AstNode::ForLoop { .. } | AstNode::WhileLoop { .. } => true,
        _ => false,
    };

    // Build the full WGSL shader
    let mut shader = String::new();

    // Uniform struct
    shader.push_str(UNIFORM_STRUCT);
    shader.push('\n');

    // Partition bindings: constant expressions go to module scope (so helper
    // functions can reference them), runtime-dependent ones stay in fs_main.
    let mut const_names: HashSet<String> = HashSet::new();
    let mut module_binding_names: HashSet<String> = HashSet::new();
    let mut fs_main_bindings = Vec::new();
    {
        let binding_asts: Vec<(&str, &AstNode)> = match ast {
            AstNode::Block(stmts) => stmts
                .iter()
                .filter_map(|s| {
                    if let AstNode::Binding { name, value } = s {
                        Some((name.as_str(), value.as_ref()))
                    } else {
                        None
                    }
                })
                .collect(),
            AstNode::Binding { name, value } => vec![(name.as_str(), value.as_ref())],
            _ => Vec::new(),
        };
        for binding in &ctx.bindings {
            let is_const = binding_asts
                .iter()
                .find(|(n, _)| *n == binding.name.as_str())
                .map_or(false, |(_, val)| is_const_expr(val, &const_names));
            if is_const {
                const_names.insert(binding.name.clone());
                shader.push_str(&format!("const {} = {};\n", binding.name, binding.expr));
                module_binding_names.insert(binding.name.clone());
            } else {
                fs_main_bindings.push(binding);
            }
        }
    }
    if !module_binding_names.is_empty() {
        shader.push('\n');
    }

    // Emit user-defined helper functions
    for func in &ctx.functions {
        shader.push_str(&func.wgsl_code);
        shader.push('\n');
    }

    // Fragment entry point
    shader.push_str("@fragment\n");
    shader.push_str("fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {\n");

    if is_vec {
        // Vec4 output (e.g. Mandelbrot): x/y are UV [0,1] coordinates.
        // User code handles its own UV→world mapping via x.min/x.max properties.
        // This matches the Zig version's convention.
        shader.push_str("    let x = uv.x;\n");
        shader.push_str("    let y = uv.y;\n");
    } else {
        // Scalar/boolean output: x/y are world coordinates.
        // Simple expressions like sin(x) work directly in the visible viewport.
        shader.push_str("    let world = mix(u.axis_min, u.axis_max, uv);\n");
        shader.push_str("    let x = world.x;\n");
        shader.push_str("    let y = world.y;\n");
    }

    if top_has_loops {
        // Imperative emission for top-level code with loops
        let top_stmts = match ast {
            AstNode::Block(stmts) => stmts.as_slice(),
            _ => std::slice::from_ref(ast),
        };
        let mut declared: HashSet<String> = HashSet::new();
        declared.insert("x".to_string());
        declared.insert("y".to_string());
        // Module-level consts are already in scope
        for name in &module_binding_names {
            declared.insert(name.clone());
        }
        let imperative_code = ctx.emit_imperative_stmts(top_stmts, "    ", &mut declared)?;
        shader.push_str(&imperative_code);
    } else {
        // Emit non-constant bindings as immutable let inside fs_main
        for binding in &fs_main_bindings {
            shader.push_str(&format!("    let {} = {};\n", binding.name, binding.expr));
        }
    }

    if is_vec {
        // Vec color output: use the result directly as RGBA color
        let expr_code = ctx.emit_expr(expr)?;
        shader.push_str(&format!("    let _result = {};\n", expr_code));
        match ctx.result_tuple_size(expr) {
            Some(3) => shader.push_str("    return vec4<f32>(_result, 1.0);\n"),
            Some(2) => shader.push_str("    return vec4<f32>(_result, 0.0, 1.0);\n"),
            _ => shader.push_str("    return _result;\n"), // vec4 or function call
        }
    } else if is_bool {
        // Boolean expressions: use corner-checking for pixel-perfect curve rendering.
        // Compute pixel size in world coordinates, then evaluate at 4 corners.
        shader.push_str("    let pixel_size = (u.axis_max - u.axis_min) / u.resolution;\n");
        shader.push_str("    let half_px = pixel_size.x;\n");
        shader.push_str("    let half_py = pixel_size.y;\n");
        shader.push_str("    let x_m = x - half_px;\n");
        shader.push_str("    let x_p = x + half_px;\n");
        shader.push_str("    let y_m = y - half_py;\n");
        shader.push_str("    let y_p = y + half_py;\n");

        let corner_code = ctx.emit_bool_with_corners(expr)?;
        shader.push_str(&format!("    let _result = {};\n", corner_code));
        shader.push_str("    let _shade = select(0.0, 1.0, _result);\n");
        shader.push_str("    return vec4<f32>(u.primary_color.rgb * _shade, _shade);\n");
    } else {
        // Numeric expressions: clamp to [0, 1] grayscale
        let expr_code = ctx.emit_expr(expr)?;
        shader.push_str(&format!("    let _result = {};\n", expr_code));
        shader.push_str("    let _shade = clamp(_result, 0.0, 1.0);\n");
        shader.push_str("    return vec4<f32>(u.primary_color.rgb * _shade, _shade);\n");
    }
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
    /// Names of user-defined functions that return vec types (not f32).
    vec_functions: HashSet<String>,
    /// For hoisted nested functions: maps function name → extra captured variables to pass.
    captured_vars: std::collections::HashMap<String, Vec<String>>,
}

/// Optional x/y substitution for corner-checking.
/// When `Some`, identifiers "x" and "y" are replaced with the given variable names.
type CornerSubst<'a> = Option<(&'a str, &'a str)>;

impl GenContext {
    fn new() -> Self {
        Self {
            functions: Vec::new(),
            bindings: Vec::new(),
            vec_functions: HashSet::new(),
            captured_vars: std::collections::HashMap::new(),
        }
    }

    /// Walk the AST to collect function definitions and bindings.
    fn collect_functions(&mut self, ast: &AstNode) {
        self.collect_functions_with_scope(ast, &[]);
    }

    /// Collect functions with the enclosing scope's binding names.
    /// `scope_bindings` are variable names available from the enclosing scope
    /// (used to detect captured variables for nested function hoisting).
    fn collect_functions_with_scope(&mut self, ast: &AstNode, scope_bindings: &[String]) {
        match ast {
            AstNode::Block(stmts) => {
                for stmt in stmts {
                    self.collect_functions_with_scope(stmt, scope_bindings);
                }
            }
            AstNode::FunctionDef { name, params, body } => {
                // Collect nested function defs from the body first
                // Build the scope for nested functions: parent scope + this function's params + body bindings
                let mut inner_scope: Vec<String> = scope_bindings.to_vec();
                inner_scope.extend(params.iter().cloned());
                if let AstNode::Block(stmts) = body.as_ref() {
                    // Add binding names from the body to the inner scope
                    for stmt in stmts {
                        match stmt {
                            AstNode::Binding { name, .. } => inner_scope.push(name.clone()),
                            AstNode::TupleBinding { names, .. } => inner_scope.extend(names.iter().cloned()),
                            _ => {}
                        }
                    }
                    // Recurse to collect nested function definitions
                    for stmt in stmts {
                        if let AstNode::FunctionDef { .. } = stmt {
                            self.collect_functions_with_scope(stmt, &inner_scope);
                        }
                    }
                }

                // Determine captured variables (scope bindings referenced in body, not in params)
                let mut captured = Vec::new();
                if !scope_bindings.is_empty() {
                    let param_set: HashSet<&str> = params.iter().map(|s| s.as_str()).collect();
                    let referenced = find_referenced_identifiers(body);
                    for var in scope_bindings {
                        if referenced.contains(var.as_str()) && !param_set.contains(var.as_str()) {
                            captured.push(var.clone());
                        }
                    }
                }

                // Build parameter list including captured variables
                let mut all_params: Vec<String> = params
                    .iter()
                    .map(|p| format!("{}: f32", p))
                    .collect();
                for cap in &captured {
                    all_params.push(format!("{}: f32", cap));
                }

                if !captured.is_empty() {
                    self.captured_vars.insert(name.clone(), captured);
                }

                // Check if function body returns a tuple/vec (for vec4 color output)
                let returns_vec = body_returns_tuple(body);

                // Check if the body is a block with bindings or loops
                let needs_imperative = match body.as_ref() {
                    AstNode::Block(stmts) => stmts.iter().any(|s| {
                        matches!(s, AstNode::Binding { .. } | AstNode::ForLoop { .. }
                            | AstNode::WhileLoop { .. } | AstNode::TupleBinding { .. })
                    }),
                    _ => false,
                };

                let ret_type = if returns_vec { "vec4<f32>" } else { "f32" };
                if returns_vec {
                    self.vec_functions.insert(name.clone());
                }

                if needs_imperative {
                    if let AstNode::Block(stmts) = body.as_ref() {
                        if let Ok(body_wgsl) = self.emit_function_body(stmts) {
                            let wgsl_code = format!(
                                "fn {}({}) -> {} {{\n{}}}\n",
                                name,
                                all_params.join(", "),
                                ret_type,
                                body_wgsl,
                            );
                            self.functions.push(EmittedFunction {
                                name: name.clone(),
                                wgsl_code,
                            });
                        }
                    }
                } else if let Ok(body_code) = self.emit_expr(body) {
                    let wgsl_code = format!(
                        "fn {}({}) -> {} {{\n    return {};\n}}\n",
                        name,
                        all_params.join(", "),
                        ret_type,
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
            AstNode::TupleBinding { names, value } => {
                // For tuple bindings at top level, emit individual bindings
                if let AstNode::Tuple(items) = value.as_ref() {
                    for (i, name) in names.iter().enumerate() {
                        if let Some(item) = items.get(i) {
                            if let Ok(expr_code) = self.emit_expr(item) {
                                self.bindings.push(EmittedBinding {
                                    name: name.clone(),
                                    expr: expr_code,
                                });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Emit a function body that contains bindings and/or loops.
    /// Returns the WGSL body code including the final `return` statement.
    fn emit_function_body(&self, stmts: &[AstNode]) -> Result<String, String> {
        let has_loops = stmts.iter().any(|s| matches!(s, AstNode::ForLoop { .. } | AstNode::WhileLoop { .. }));
        let var_keyword = if has_loops { "var" } else { "let" };
        let mut code = String::new();
        let mut declared: HashSet<String> = HashSet::new();
        let mut result_expr = "0.0".to_string();

        for stmt in stmts {
            match stmt {
                AstNode::Binding { name, value } => {
                    let val = self.emit_expr(value)?;
                    if declared.contains(name.as_str()) {
                        code += &format!("    {} = {};\n", name, val);
                    } else {
                        code += &format!("    {} {} = {};\n", var_keyword, name, val);
                        declared.insert(name.clone());
                    }
                }
                AstNode::TupleBinding { names, value } => {
                    self.emit_tuple_binding(&mut code, names, value, "    ", var_keyword, &mut declared)?;
                }
                AstNode::ForLoop { init, condition, update, body } => {
                    // Emit init binding
                    if let AstNode::Binding { name, value } = init.as_ref() {
                        let val = self.emit_expr(value)?;
                        if declared.contains(name.as_str()) {
                            code += &format!("    {} = {};\n", name, val);
                        } else {
                            code += &format!("    var {} = {};\n", name, val);
                            declared.insert(name.clone());
                        }
                    }
                    // Emit loop with iteration guard
                    let cond = self.emit_expr(condition)?;
                    code += &format!(
                        "    for (var _loop_guard: u32 = 0u; _loop_guard < {}u; _loop_guard = _loop_guard + 1u) {{\n",
                        MAX_LOOP_ITERATIONS
                    );
                    code += &format!("        if (!({cond})) {{ break; }}\n");
                    // Emit body
                    self.emit_loop_body_stmts(&mut code, body, &mut declared)?;
                    // Emit update
                    if let AstNode::Binding { name, value } = update.as_ref() {
                        let val = self.emit_expr(value)?;
                        code += &format!("        {} = {};\n", name, val);
                    }
                    code += "    }\n";
                }
                AstNode::WhileLoop { condition, body } => {
                    // Extract inline bindings from the condition (e.g. sq: expr inside block)
                    self.emit_while_condition_bindings(&mut code, condition, "    ", &mut declared)?;
                    // Emit loop with iteration guard
                    let cond = self.emit_expr(condition)?;
                    code += &format!(
                        "    for (var _loop_guard: u32 = 0u; _loop_guard < {}u; _loop_guard = _loop_guard + 1u) {{\n",
                        MAX_LOOP_ITERATIONS
                    );
                    // Re-emit condition bindings at the top of each iteration
                    self.emit_while_condition_bindings_inner(&mut code, condition, "        ", &mut declared)?;
                    code += &format!("        if (!({cond})) {{ break; }}\n");
                    // Emit body
                    self.emit_loop_body_stmts(&mut code, body, &mut declared)?;
                    code += "    }\n";
                }
                AstNode::FunctionDef { .. } => {} // Skip nested function defs
                _ => {
                    result_expr = self.emit_expr(stmt)?;
                }
            }
        }

        code += &format!("    return {};\n", result_expr);
        Ok(code)
    }

    /// Emit statements inside a loop body.
    fn emit_loop_body_stmts(
        &self,
        code: &mut String,
        body: &AstNode,
        declared: &mut HashSet<String>,
    ) -> Result<(), String> {
        match body {
            AstNode::Block(stmts) => {
                for stmt in stmts {
                    match stmt {
                        AstNode::Binding { name, value } => {
                            let val = self.emit_expr(value)?;
                            if declared.contains(name.as_str()) {
                                code.push_str(&format!("        {} = {};\n", name, val));
                            } else {
                                code.push_str(&format!("        var {} = {};\n", name, val));
                                declared.insert(name.clone());
                            }
                        }
                        AstNode::TupleBinding { names, value } => {
                            self.emit_tuple_binding(code, names, value, "        ", "var", declared)?;
                        }
                        _ => {} // Non-binding expressions in loop body (side-effect free, skip)
                    }
                }
            }
            AstNode::Binding { name, value } => {
                let val = self.emit_expr(value)?;
                if declared.contains(name.as_str()) {
                    code.push_str(&format!("        {} = {};\n", name, val));
                } else {
                    code.push_str(&format!("        var {} = {};\n", name, val));
                    declared.insert(name.clone());
                }
            }
            _ => {} // Single expression body — no side effects to emit
        }
        Ok(())
    }

    /// Check if an expression produces a vec type (for shader output detection).
    fn result_is_vec(&self, node: &AstNode) -> bool {
        match node {
            AstNode::Tuple(items) => items.len() >= 2,
            AstNode::Apply { name, .. } => self.vec_functions.contains(name),
            AstNode::IfExpr { then_branch, else_branch, .. } => {
                self.result_is_vec(then_branch)
                    || else_branch.as_ref().map_or(false, |e| self.result_is_vec(e))
            }
            AstNode::Block(stmts) => {
                stmts.last().map_or(false, |s| self.result_is_vec(s))
            }
            _ => false,
        }
    }

    /// Return the tuple size of the result expression, if it's a direct tuple literal.
    fn result_tuple_size(&self, node: &AstNode) -> Option<usize> {
        match node {
            AstNode::Tuple(items) => Some(items.len()),
            AstNode::Block(stmts) => stmts.last().and_then(|s| self.result_tuple_size(s)),
            _ => None,
        }
    }

    /// Emit a tuple destructuring binding as individual var/let declarations.
    fn emit_tuple_binding(
        &self,
        code: &mut String,
        names: &[String],
        value: &AstNode,
        indent: &str,
        var_keyword: &str,
        declared: &mut HashSet<String>,
    ) -> Result<(), String> {
        if let AstNode::Tuple(items) = value {
            for (i, name) in names.iter().enumerate() {
                if let Some(item) = items.get(i) {
                    let val = self.emit_expr(item)?;
                    if declared.contains(name.as_str()) {
                        *code += &format!("{}{} = {};\n", indent, name, val);
                    } else {
                        *code += &format!("{}{} {} = {};\n", indent, var_keyword, name, val);
                        declared.insert(name.clone());
                    }
                }
            }
        }
        Ok(())
    }

    /// Walk a while-loop condition to find Block nodes with bindings.
    /// Declare those variables with `var` before the loop starts (initial values).
    fn emit_while_condition_bindings(
        &self,
        code: &mut String,
        condition: &AstNode,
        indent: &str,
        declared: &mut HashSet<String>,
    ) -> Result<(), String> {
        match condition {
            AstNode::Block(stmts) => {
                for stmt in stmts {
                    if let AstNode::Binding { name, value } = stmt {
                        let val = self.emit_expr(value)?;
                        if !declared.contains(name.as_str()) {
                            *code += &format!("{}var {} = {};\n", indent, name, val);
                            declared.insert(name.clone());
                        }
                    }
                }
            }
            AstNode::Apply { args, .. } => {
                for arg in args {
                    self.emit_while_condition_bindings(code, arg, indent, declared)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Emit bindings found inside a while condition at the top of each loop iteration.
    /// These update the mutable variables each time the condition is re-evaluated.
    fn emit_while_condition_bindings_inner(
        &self,
        code: &mut String,
        condition: &AstNode,
        indent: &str,
        declared: &mut HashSet<String>,
    ) -> Result<(), String> {
        match condition {
            AstNode::Block(stmts) => {
                for stmt in stmts {
                    if let AstNode::Binding { name, value } = stmt {
                        let val = self.emit_expr(value)?;
                        if declared.contains(name.as_str()) {
                            *code += &format!("{}{} = {};\n", indent, name, val);
                        } else {
                            *code += &format!("{}var {} = {};\n", indent, name, val);
                            declared.insert(name.clone());
                        }
                    }
                }
            }
            AstNode::Apply { args, .. } => {
                for arg in args {
                    self.emit_while_condition_bindings_inner(code, arg, indent, declared)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Emit top-level statements imperatively (for blocks with loops).
    /// Only emits bindings and loops; skips FunctionDefs and result expressions.
    fn emit_imperative_stmts(
        &self,
        stmts: &[AstNode],
        indent: &str,
        declared: &mut HashSet<String>,
    ) -> Result<String, String> {
        let mut code = String::new();

        for stmt in stmts {
            match stmt {
                AstNode::Binding { name, value } => {
                    let val = self.emit_expr(value)?;
                    if declared.contains(name.as_str()) {
                        code += &format!("{}{} = {};\n", indent, name, val);
                    } else {
                        code += &format!("{}var {} = {};\n", indent, name, val);
                        declared.insert(name.clone());
                    }
                }
                AstNode::TupleBinding { names, value } => {
                    self.emit_tuple_binding(&mut code, names, value, indent, "var", declared)?;
                }
                AstNode::ForLoop { init, condition, update, body } => {
                    if let AstNode::Binding { name, value } = init.as_ref() {
                        let val = self.emit_expr(value)?;
                        if declared.contains(name.as_str()) {
                            code += &format!("{}{} = {};\n", indent, name, val);
                        } else {
                            code += &format!("{}var {} = {};\n", indent, name, val);
                            declared.insert(name.clone());
                        }
                    }
                    let cond = self.emit_expr(condition)?;
                    code += &format!(
                        "{}for (var _loop_guard: u32 = 0u; _loop_guard < {}u; _loop_guard = _loop_guard + 1u) {{\n",
                        indent, MAX_LOOP_ITERATIONS
                    );
                    code += &format!("{}    if (!({cond})) {{ break; }}\n", indent);
                    self.emit_loop_body_stmts(&mut code, body, declared)?;
                    if let AstNode::Binding { name, value } = update.as_ref() {
                        let val = self.emit_expr(value)?;
                        code += &format!("{}    {} = {};\n", indent, name, val);
                    }
                    code += &format!("{}}}\n", indent);
                }
                AstNode::WhileLoop { condition, body } => {
                    // Pre-declare condition bindings
                    self.emit_while_condition_bindings(&mut code, condition, indent, declared)?;
                    let cond = self.emit_expr(condition)?;
                    code += &format!(
                        "{}for (var _loop_guard: u32 = 0u; _loop_guard < {}u; _loop_guard = _loop_guard + 1u) {{\n",
                        indent, MAX_LOOP_ITERATIONS
                    );
                    let inner_indent = format!("{}    ", indent);
                    self.emit_while_condition_bindings_inner(&mut code, condition, &inner_indent, declared)?;
                    code += &format!("{}    if (!({cond})) {{ break; }}\n", indent);
                    self.emit_loop_body_stmts(&mut code, body, declared)?;
                    code += &format!("{}}}\n", indent);
                }
                AstNode::FunctionDef { .. } => {} // Already collected
                _ => {} // Result expression handled separately
            }
        }

        Ok(code)
    }

    // -----------------------------------------------------------------------
    // Standard expression emission (no corner substitution)
    // -----------------------------------------------------------------------

    fn emit_expr(&self, node: &AstNode) -> Result<String, String> {
        self.emit_expr_internal(node, None)
    }

    /// Core expression emitter. When `subst` is Some((x_var, y_var)),
    /// identifiers "x" and "y" are replaced with the corner variable names.
    fn emit_expr_internal(&self, node: &AstNode, subst: CornerSubst) -> Result<String, String> {
        match node {
            AstNode::Number(n) => {
                let s = if n.fract() == 0.0 && !n.is_nan() && !n.is_infinite() {
                    format!("{:.1}", n)
                } else {
                    format!("{}", n)
                };
                Ok(s)
            }
            AstNode::BoolLit(b) => Ok(format!("{}", b)),
            AstNode::Identifier(name) => {
                if let Some((x_var, y_var)) = subst {
                    if name == "x" {
                        return Ok(x_var.to_string());
                    }
                    if name == "y" {
                        return Ok(y_var.to_string());
                    }
                }
                // Map time/time_s → u.time (accessible in user functions too)
                if name == "time_s" || name == "time" {
                    return Ok("u.time".to_string());
                }
                Ok(name.clone())
            }
            AstNode::Apply { name, args } => self.emit_apply_internal(name, args, subst),
            AstNode::Tuple(items) => {
                let parts: Result<Vec<_>, _> =
                    items.iter().map(|i| self.emit_expr_internal(i, subst)).collect();
                let parts = parts?;
                match items.len() {
                    2 => Ok(format!("vec2<f32>({})", parts.join(", "))),
                    3 => Ok(format!("vec3<f32>({})", parts.join(", "))),
                    4 => Ok(format!("vec4<f32>({})", parts.join(", "))),
                    _ => Err(format!("Unsupported tuple size: {}", items.len())),
                }
            }
            AstNode::Block(stmts) => {
                if let Some(last) = stmts.last() {
                    self.emit_expr_internal(last, subst)
                } else {
                    Ok("0.0".to_string())
                }
            }
            AstNode::Binding { .. } => Ok("0.0".to_string()),
            AstNode::IfExpr { condition, then_branch, else_branch } => {
                let cond = self.emit_expr_internal(condition, subst)?;
                let then_code = self.emit_expr_internal(then_branch, subst)?;
                if let Some(else_b) = else_branch {
                    let else_code = self.emit_expr_internal(else_b, subst)?;
                    Ok(format!("select({}, {}, {})", else_code, then_code, cond))
                } else {
                    Ok(format!("select(0.0, {}, {})", then_code, cond))
                }
            }
            AstNode::FunctionDef { .. } => Ok("0.0".to_string()),
            AstNode::ForLoop { .. } => Ok("0.0".to_string()), // Loops emitted imperatively, not as expressions
            AstNode::WhileLoop { .. } => Ok("0.0".to_string()), // Loops emitted imperatively
            AstNode::PropertyAccess { object, property } => {
                // Map x.min → u.axis_min.x, x.max → u.axis_max.x, etc.
                if let AstNode::Identifier(ref base) = **object {
                    match (base.as_str(), property.as_str()) {
                        ("x", "min") => return Ok("u.axis_min.x".to_string()),
                        ("x", "max") => return Ok("u.axis_max.x".to_string()),
                        ("x", "res") => return Ok("u.resolution.x".to_string()),
                        ("y", "min") => return Ok("u.axis_min.y".to_string()),
                        ("y", "max") => return Ok("u.axis_max.y".to_string()),
                        ("y", "res") => return Ok("u.resolution.y".to_string()),
                        _ => {}
                    }
                }
                let obj = self.emit_expr_internal(object, subst)?;
                Ok(format!("{}.{}", obj, property))
            }
            AstNode::TupleBinding { .. } => Ok("0.0".to_string()), // Emitted imperatively
        }
    }

    #[allow(dead_code)]
    fn emit_apply(&self, name: &str, args: &[AstNode]) -> Result<String, String> {
        self.emit_apply_internal(name, args, None)
    }

    fn emit_apply_internal(
        &self,
        name: &str,
        args: &[AstNode],
        subst: CornerSubst,
    ) -> Result<String, String> {
        let emit_args: Result<Vec<_>, _> =
            args.iter().map(|a| self.emit_expr_internal(a, subst)).collect();
        let emitted = emit_args?;

        match (name, args.len()) {
            // Binary infix operators
            ("add", 2) => Ok(format!("({} + {})", emitted[0], emitted[1])),
            ("sub", 2) => Ok(format!("({} - {})", emitted[0], emitted[1])),
            ("mul", 2) => Ok(format!("({} * {})", emitted[0], emitted[1])),
            ("div", 2) => Ok(format!("({} / {})", emitted[0], emitted[1])),
            ("mod", 2) => Ok(format!(
                "((({} % {}) + {}) % {})",
                emitted[0], emitted[1], emitted[1], emitted[1]
            )),

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
            ("clamp", 3) => Ok(format!(
                "clamp({}, {}, {})",
                emitted[0], emitted[1], emitted[2]
            )),
            ("mix", 3) => Ok(format!(
                "mix({}, {}, {})",
                emitted[0], emitted[1], emitted[2]
            )),
            ("step", 2) => Ok(format!("step({}, {})", emitted[0], emitted[1])),
            ("smoothstep", 3) => Ok(format!(
                "smoothstep({}, {}, {})",
                emitted[0], emitted[1], emitted[2]
            )),
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

            // User-defined functions: emit as regular call, appending captured vars if any
            _ => {
                let mut all_args = emitted;
                if let Some(captured) = self.captured_vars.get(name) {
                    for cap in captured {
                        all_args.push(cap.clone());
                    }
                }
                Ok(format!("{}({})", name, all_args.join(", ")))
            }
        }
    }

    // -----------------------------------------------------------------------
    // Corner-checking emission for boolean expressions
    // -----------------------------------------------------------------------
    // For equations (=): the curve passes through a pixel if the expression
    //   (LHS - RHS) changes sign across the pixel's four corners.
    // For inequalities (>, <, etc.): all four corners must satisfy the condition.
    // For logical ops (and, or, not): recursively apply corner checking.

    /// Emit a boolean expression with corner-checking.
    /// Assumes x_m, x_p, y_m, y_p variables are available in scope.
    fn emit_bool_with_corners(&self, node: &AstNode) -> Result<String, String> {
        match node {
            AstNode::BoolLit(b) => Ok(format!("{}", b)),

            AstNode::Apply { name, args } => {
                match name.as_str() {
                    // Logical ops: recursively apply corner checking
                    "and" if args.len() == 2 => {
                        let l = self.emit_bool_with_corners(&args[0])?;
                        let r = self.emit_bool_with_corners(&args[1])?;
                        Ok(format!("({} && {})", l, r))
                    }
                    "or" if args.len() == 2 => {
                        let l = self.emit_bool_with_corners(&args[0])?;
                        let r = self.emit_bool_with_corners(&args[1])?;
                        Ok(format!("({} || {})", l, r))
                    }
                    "not" if args.len() == 1 => {
                        let inner = self.emit_bool_with_corners(&args[0])?;
                        Ok(format!("!({})", inner))
                    }
                    // Comparison ops: apply corner checking
                    "eq" | "neq" | "lt" | "gt" | "lte" | "gte" if args.len() == 2 => {
                        self.emit_comparison_with_corners(name, &args[0], &args[1])
                    }
                    // Anything else: fall back to normal emission
                    _ => self.emit_expr(node),
                }
            }

            // Non-boolean nodes or identifiers: emit normally
            _ => self.emit_expr(node),
        }
    }

    /// Emit a comparison with corner checking.
    ///
    /// For equality (`eq`): checks if (LHS - RHS) changes sign across 4 corners
    ///   → `!(sign_at_c1 == sign_at_c2 && sign_at_c2 == sign_at_c3 && sign_at_c3 == sign_at_c4)`
    ///   This means the curve passes through the pixel.
    ///
    /// For inequalities: all 4 corners must satisfy the condition.
    fn emit_comparison_with_corners(
        &self,
        op: &str,
        lhs: &AstNode,
        rhs: &AstNode,
    ) -> Result<String, String> {
        // The four corners: (x_m, y_m), (x_m, y_p), (x_p, y_m), (x_p, y_p)
        let corners: [(&str, &str); 4] = [
            ("x_m", "y_m"),
            ("x_m", "y_p"),
            ("x_p", "y_m"),
            ("x_p", "y_p"),
        ];

        if op == "eq" {
            // Equality: curve straddles pixel if corners don't all agree on sign of (LHS - RHS).
            // We check: (LHS > RHS) at each corner, then test if they're all the same.
            // If NOT all the same → curve passes through → true.
            let mut corner_signs = Vec::new();
            for (xv, yv) in &corners {
                let l = self.emit_expr_internal(lhs, Some((xv, yv)))?;
                let r = self.emit_expr_internal(rhs, Some((xv, yv)))?;
                corner_signs.push(format!("(({}) > ({}))", l, r));
            }
            // !(c1 == c2 && c2 == c3 && c3 == c4)
            Ok(format!(
                "(!({} == {} && {} == {} && {} == {}))",
                corner_signs[0], corner_signs[1],
                corner_signs[1], corner_signs[2],
                corner_signs[2], corner_signs[3],
            ))
        } else {
            // Inequalities: all four corners must satisfy the condition
            let wgsl_op = match op {
                "gt" => ">",
                "lt" => "<",
                "gte" => ">=",
                "lte" => "<=",
                "neq" => "!=",
                _ => "==",
            };
            let mut parts = Vec::new();
            for (xv, yv) in &corners {
                let l = self.emit_expr_internal(lhs, Some((xv, yv)))?;
                let r = self.emit_expr_internal(rhs, Some((xv, yv)))?;
                parts.push(format!("(({}) {} ({}))", l, wgsl_op, r));
            }
            Ok(format!(
                "({} && {} && {} && {})",
                parts[0], parts[1], parts[2], parts[3]
            ))
        }
    }
}

/// Find all identifiers referenced in an AST node (for captured variable analysis).
fn find_referenced_identifiers(node: &AstNode) -> HashSet<String> {
    let mut result = HashSet::new();
    collect_identifiers(node, &mut result);
    result
}

fn collect_identifiers(node: &AstNode, result: &mut HashSet<String>) {
    match node {
        AstNode::Identifier(name) => { result.insert(name.clone()); }
        AstNode::Apply { args, .. } => {
            for arg in args { collect_identifiers(arg, result); }
        }
        AstNode::Block(stmts) => {
            for s in stmts { collect_identifiers(s, result); }
        }
        AstNode::Binding { value, .. } => { collect_identifiers(value, result); }
        AstNode::TupleBinding { value, .. } => { collect_identifiers(value, result); }
        AstNode::Tuple(items) => {
            for item in items { collect_identifiers(item, result); }
        }
        AstNode::IfExpr { condition, then_branch, else_branch } => {
            collect_identifiers(condition, result);
            collect_identifiers(then_branch, result);
            if let Some(eb) = else_branch { collect_identifiers(eb, result); }
        }
        AstNode::ForLoop { init, condition, update, body } => {
            collect_identifiers(init, result);
            collect_identifiers(condition, result);
            collect_identifiers(update, result);
            collect_identifiers(body, result);
        }
        AstNode::WhileLoop { condition, body } => {
            collect_identifiers(condition, result);
            collect_identifiers(body, result);
        }
        AstNode::FunctionDef { body, .. } => { collect_identifiers(body, result); }
        AstNode::PropertyAccess { object, .. } => { collect_identifiers(object, result); }
        AstNode::Number(_) | AstNode::BoolLit(_) => {}
    }
}

/// Check if an AST node is a constant expression (no x, y, z, time references).
/// `const_names` tracks bindings already known to be constant.
fn is_const_expr(node: &AstNode, const_names: &HashSet<String>) -> bool {
    match node {
        AstNode::Number(_) | AstNode::BoolLit(_) => true,
        AstNode::Identifier(name) => {
            if matches!(name.as_str(), "x" | "y" | "z" | "time") {
                return false;
            }
            const_names.contains(name)
        }
        AstNode::Apply { args, .. } => args.iter().all(|a| is_const_expr(a, const_names)),
        AstNode::Tuple(items) => items.iter().all(|i| is_const_expr(i, const_names)),
        AstNode::Block(stmts) => stmts.iter().all(|s| is_const_expr(s, const_names)),
        AstNode::IfExpr { condition, then_branch, else_branch } => {
            is_const_expr(condition, const_names)
                && is_const_expr(then_branch, const_names)
                && else_branch.as_ref().map_or(true, |e| is_const_expr(e, const_names))
        }
        AstNode::Binding { value, .. } => is_const_expr(value, const_names),
        _ => false,
    }
}

/// Check if a function body's result expression is a tuple (for vec return type).
/// This is a conservative check — it only detects direct tuple literals and
/// if/else branches that contain tuple literals.
fn body_returns_tuple(body: &AstNode) -> bool {
    match body {
        AstNode::Tuple(items) => items.len() >= 2,
        AstNode::Block(stmts) => {
            // Check the last non-binding statement
            for stmt in stmts.iter().rev() {
                match stmt {
                    AstNode::Binding { .. } | AstNode::FunctionDef { .. }
                    | AstNode::ForLoop { .. } | AstNode::WhileLoop { .. }
                    | AstNode::TupleBinding { .. } => continue,
                    other => return body_returns_tuple(other),
                }
            }
            false
        }
        AstNode::IfExpr { then_branch, else_branch, .. } => {
            body_returns_tuple(then_branch)
                || else_branch.as_ref().map_or(false, |e| body_returns_tuple(e))
        }
        _ => false,
    }
}

/// Check if an AST node produces a boolean value in WGSL.
///
/// Comparisons, logical ops, and boolean literals produce `bool` in WGSL,
/// which cannot be passed to `clamp()`. We use corner-checking for these instead.
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

/// Find the result expression in the AST (last non-binding, non-function-def, non-loop node).
fn find_result_expr(ast: &AstNode) -> Result<&AstNode, String> {
    match ast {
        AstNode::Block(stmts) => {
            for stmt in stmts.iter().rev() {
                match stmt {
                    AstNode::Binding { .. } | AstNode::FunctionDef { .. }
                    | AstNode::ForLoop { .. } | AstNode::WhileLoop { .. }
                    | AstNode::TupleBinding { .. } => continue,
                    other => return Ok(other),
                }
            }
            Err("No result expression found — all statements are bindings, function definitions, or loops"
                .to_string())
        }
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

    // --- Corner-checking tests for boolean expressions ---

    #[test]
    fn test_equality_uses_corner_checking() {
        // `x = y` should use corner straddling, not simple `==`
        let shader = gen("x = y");
        assert!(shader.contains("x_m"), "should declare corner variables");
        assert!(shader.contains("x_p"), "should declare corner variables");
        assert!(shader.contains("y_m"), "should declare corner variables");
        assert!(shader.contains("y_p"), "should declare corner variables");
        assert!(shader.contains("pixel_size"), "should compute pixel size");
        // The result should negate all-same-sign check
        assert!(shader.contains("!("), "equality uses !(all_same_sign)");
    }

    #[test]
    fn test_equality_x_eq_0_uses_corners() {
        let shader = gen("x = 0");
        assert!(shader.contains("x_m"));
        assert!(shader.contains("x_p"));
        assert!(shader.contains("!("), "equality straddle check");
        assert!(shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_inequality_gt_uses_all_corners() {
        let shader = gen("x > y");
        assert!(shader.contains("x_m"));
        // Inequality: all corners must agree → 4 ANDed conditions
        assert!(shader.contains("&&"), "inequality ANDs corner checks");
        assert!(shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_inequality_lt_uses_all_corners() {
        let shader = gen("x < 1");
        assert!(shader.contains("x_m"));
        assert!(shader.contains("&&"));
    }

    #[test]
    fn test_inequality_lte_uses_all_corners() {
        let shader = gen("x <= 1");
        assert!(shader.contains("x_m"));
        assert!(shader.contains("<="));
    }

    #[test]
    fn test_inequality_gte_uses_all_corners() {
        let shader = gen("x >= 0");
        assert!(shader.contains("x_m"));
        assert!(shader.contains(">="));
    }

    #[test]
    fn test_inequality_neq_uses_all_corners() {
        let shader = gen("x != 0");
        assert!(shader.contains("x_m"));
        assert!(shader.contains("!="));
    }

    #[test]
    fn test_logical_and_recursively_applies_corners() {
        let shader = gen("x > 0 and y > 0");
        assert!(shader.contains("x_m"));
        // Should have corner checks for both sides of `and`
        assert!(shader.contains("&&"));
    }

    #[test]
    fn test_logical_or_recursively_applies_corners() {
        let shader = gen("x < 0 or x > 1");
        assert!(shader.contains("x_m"));
        assert!(shader.contains("||"));
    }

    #[test]
    fn test_logical_not_applies_corners() {
        let shader = gen("not (x > 0)");
        assert!(shader.contains("x_m"));
        assert!(shader.contains("!("));
    }

    #[test]
    fn test_bool_literal_uses_select() {
        let shader = gen("true");
        assert!(shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_numeric_expr_still_uses_clamp() {
        let shader = gen("x * x + y * y");
        assert!(shader.contains("clamp(_result, 0.0, 1.0)"));
        assert!(!shader.contains("x_m"), "numeric expr should NOT have corner vars");
    }

    #[test]
    fn test_if_expr_is_numeric_not_bool() {
        let shader = gen("if (x > 0) 1 else 0");
        assert!(shader.contains("clamp(_result, 0.0, 1.0)"));
        assert!(!shader.contains("x_m"), "if/else returning f32 should not use corners");
    }

    #[test]
    fn test_binding_then_bool_result_uses_corners() {
        let shader = gen("r: x * x + y * y\nr < 1");
        assert!(shader.contains("x_m"), "bool result should trigger corner checking");
        assert!(shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_binding_then_numeric_result_uses_clamp() {
        let shader = gen("r: x * x + y * y\nsin(r)");
        assert!(shader.contains("clamp(_result, 0.0, 1.0)"));
    }

    #[test]
    fn test_equality_substitutes_x_y_at_corners() {
        // For `x = y`, the corner check should substitute x→x_m/x_p and y→y_m/y_p
        let shader = gen("x = y");
        // Should contain expressions with corner vars, e.g. "(x_m) > (y_m)"
        assert!(shader.contains("x_m") && shader.contains("y_m"));
        assert!(shader.contains("x_p") && shader.contains("y_p"));
    }

    #[test]
    fn test_complex_equality_sin_x_eq_y() {
        // sin(x) = y → should evaluate sin(x_m) vs y_m at corners
        let shader = gen("sin(x) = y");
        assert!(shader.contains("sin(x_m)") || shader.contains("sin(x_p)"),
            "sin should be evaluated at corner x values");
        assert!(shader.contains("y_m") && shader.contains("y_p"));
    }

    #[test]
    fn test_equality_circle_equation() {
        // x*x + y*y = 1 → unit circle, should be visible with corner checking
        let shader = gen("x*x + y*y = 1");
        assert!(shader.contains("x_m"));
        assert!(shader.contains("y_p"));
        assert!(shader.contains("!("), "equality uses straddle check");
    }

    // --- For loop tests ---

    #[test]
    fn test_for_loop_in_function_body() {
        let shader = gen("f(x): (sum:0, delta:0.01, for(i:0, i<x, i:i+delta) (sum:sum+i*delta), sum)\nf(x)");
        assert!(shader.contains("fn f(x: f32) -> f32"), "should emit function def");
        assert!(shader.contains("var sum = 0.0"), "should emit var for mutable binding");
        assert!(shader.contains("var delta = 0.01"), "should emit var for delta");
        assert!(shader.contains("var i = 0.0"), "should emit var for loop init");
        assert!(shader.contains("_loop_guard"), "should have loop guard");
        assert!(shader.contains("10000u"), "should have max iteration limit");
        assert!(shader.contains("if (!("), "should have condition check with break");
        assert!(shader.contains("return sum"), "should return the accumulator");
    }

    #[test]
    fn test_for_loop_with_newlines() {
        // Newlines inside parens should work as separators
        let input = "f(n): (\n  sum:0\n  for(i:0, i<n, i:i+1) (\n    sum:sum+i\n  )\n  sum\n)\nf(x)";
        let shader = gen(input);
        assert!(shader.contains("var sum = 0.0"), "should handle newline-separated block");
        assert!(shader.contains("var i = 0.0"), "should handle for loop");
        assert!(shader.contains("return sum"), "should return accumulator");
    }

    #[test]
    fn test_for_loop_function_called_with_corners() {
        // Function with for loop called in boolean expression uses corner checking
        let input = "F(n): (sum:0, for(i:0, i<n, i:i+1) (sum:sum+i), sum)\nF(x) + F(y) = 9";
        let shader = gen(input);
        assert!(shader.contains("fn F(n: f32) -> f32"), "should emit F function");
        assert!(shader.contains("_loop_guard"), "should have loop guard");
        assert!(shader.contains("x_m"), "boolean eq should use corner checking");
        assert!(shader.contains("F(x_m)") || shader.contains("F(x_p)"),
            "F should be called with corner values");
    }

    #[test]
    fn test_function_body_with_bindings_no_loops() {
        // Function body with bindings but no loops should use let
        let shader = gen("f(a): (r: a * 2, r + 1)\nf(x)");
        assert!(shader.contains("let r = (a * 2.0)"), "bindings without loops should use let");
        assert!(shader.contains("return (r + 1.0)"), "should return last expression");
    }

    #[test]
    fn test_users_exact_input() {
        // The user's exact code (with commas)
        let input = "f(x): x\u{00B2}\nF(x): (sum:0, delta:0.01, for(i:0, i<x, i:i+delta) (sum:sum+f(x)*delta), sum)\nF(x) + F(y) = 9";
        let result = crate::lang::compile(input);
        assert!(result.is_ok(), "User's code should compile, got: {:?}", result);
        let shader = result.unwrap();
        assert!(shader.contains("fn f(x: f32) -> f32"), "should define f");
        assert!(shader.contains("fn F(x: f32) -> f32"), "should define F");
        assert!(shader.contains("_loop_guard"), "should have loop guard in F");
        assert!(shader.contains("x_m"), "result should use corner checking");
    }

    // --- New feature tests for Mandelbrot support ---

    #[test]
    fn test_top_level_comma_separators() {
        let shader = gen("a: 1, b: 2, a + b");
        // Simple number constants are emitted at module level
        assert!(shader.contains("const a = 1.0"));
        assert!(shader.contains("const b = 2.0"));
        assert!(shader.contains("(a + b)"));
    }

    #[test]
    fn test_while_loop_in_function() {
        let input = "f(n): (i: 0, s: 0, while (i < n) (s: s + i, i: i + 1), s)\nf(x)";
        let shader = gen(input);
        assert!(shader.contains("fn f(n: f32) -> f32"), "should define function");
        assert!(shader.contains("_loop_guard"), "should have loop guard");
        assert!(shader.contains("return s"), "should return accumulator");
    }

    #[test]
    fn test_tuple_destructuring_binding() {
        let input = "f(a, b): (r: a + b, r * 2)\n(p, q): (1, 2)\nf(p, q)";
        let result = crate::lang::compile(input);
        assert!(result.is_ok(), "tuple destructuring should compile: {:?}", result);
    }

    #[test]
    fn test_property_access_axis_bounds() {
        let shader = gen("x.max - x.min");
        assert!(shader.contains("u.axis_max.x"), "x.max should map to u.axis_max.x");
        assert!(shader.contains("u.axis_min.x"), "x.min should map to u.axis_min.x");
    }

    #[test]
    fn test_property_access_y_axis() {
        let shader = gen("y.max - y.min");
        assert!(shader.contains("u.axis_max.y"), "y.max should map to u.axis_max.y");
        assert!(shader.contains("u.axis_min.y"), "y.min should map to u.axis_min.y");
    }

    #[test]
    fn test_time_s_mapped_to_time() {
        let shader = gen("sin(time_s)");
        assert!(shader.contains("sin(u.time)"), "time_s should map to u.time");
        assert!(!shader.contains("time_s"), "time_s should not appear in output");
    }

    #[test]
    fn test_function_returning_vec4() {
        let input = "f(a): (1.0, a, 0.0, 1.0)\nf(x)";
        let shader = gen(input);
        assert!(shader.contains("fn f(a: f32) -> vec4<f32>"), "function returning tuple should have vec4 return type");
    }

    #[test]
    fn test_mandelbrot_full_program() {
        let input = r#"BASE_ITER: 128,
BAILOUT: 4.0,
MAX_ITER_CAP: 512,
INITIAL_ZOOM: 0.2,
ROOT_SAMPLES: 3,

mandelbrot_color(iter, sq): (
    mu: f32(iter) + 1.0 - log(0.5 * log(sq) / log(2.0)) / log(2.0),
    base_mod: 0.05 * mu + 0.3 * time_s,
    hue_mod: 0.1 * mu + time_s,
    color_base: 0.9 + 0.1 * cos(0.05 * mu + 0.5 * time_s),
    fade: 0.8 + 0.2 * sin(hue_mod),
    triwave_channel(offset): (
        color_base * ((1.0 - fade) + fade * clamp(
            abs(fract(fract(base_mod) + offset) * 6.0 - 3.0) - 1.0,
            0.0,
            1.0
        ))
    ),
    (triwave_channel(0.5), triwave_channel(1.0/3.0), triwave_channel(0.25), 1.0)
),
mandelbrot(x, y): (
    (width_x, width_y): (x.max - x.min, y.max - y.min),
    effective_zoom: 1.0 / width_y,
    (c_x, c_y): (x.min + x * width_x, y.min + y * width_y),
    (z_x, z_y): (0.0, 0.0),
    sq: 0.0,
    max_iter: min(BASE_ITER + (40.0 * log(effective_zoom / INITIAL_ZOOM + 1.0)), MAX_ITER_CAP),
    iter: 0,
    while (iter < max_iter and (sq: z_x * z_x + z_y * z_y, sq) < BAILOUT) (
        zy2: z_y * z_y,
        z_y: 2.0 * z_x * z_y + c_y,
        z_x: z_x * z_x - zy2 + c_x,
        iter: iter + 1
    ),
    if (iter < max_iter) (
        mandelbrot_color(iter, sq)
    ) else (
        (0.0, 0.0, 0.0, 1.0)
    )
),

mandelbrot(x, y)"#;
        let result = crate::lang::compile(input);
        assert!(result.is_ok(), "Mandelbrot should compile, got: {:?}", result);
        let shader = result.unwrap();
        assert!(shader.contains("fn mandelbrot_color("), "should define mandelbrot_color");
        assert!(shader.contains("fn mandelbrot("), "should define mandelbrot");
        assert!(shader.contains("u.axis_min"), "should reference axis bounds");
        assert!(shader.contains("_loop_guard"), "should have loop guard for while");
    }
}
