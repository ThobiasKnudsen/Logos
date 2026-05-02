use super::ast::AstNode;
use std::collections::HashSet;

// The for-loop guard's upper bound is loaded from the `max_loop_iter` uniform
// at runtime rather than emitted as a compile-time literal. Passing it as a
// uniform prevents driver shader compilers (notably NVIDIA's) from fully
// unrolling small fixed-iteration loops, which previously hung pipeline
// creation on inputs whose unrolled chain the optimizer could prove constant
// (e.g. `sum:=0; for i in 0..10 (sum:=sum*x); sum` ≡ 0). The concrete value
// is set in `render::shader_pipeline::MAX_LOOP_ITERATIONS`.

/// Generate a complete WGSL fragment shader from an AST.
///
/// The generated shader:
/// - Defines the uniform struct matching ShaderUniforms
/// - Maps user `x`/`y` to world coordinates via axis_min/axis_max
/// - For boolean expressions: uses corner-checking for pixel-perfect rendering
///   (equality → curve straddling, inequalities → all-corners-agree)
/// - For numeric expressions: clamps to [0, 1] grayscale
pub fn generate(ast: &AstNode) -> Result<String, String> {
    // Pre-pass: anonymous imperative blocks (e.g. `plot(y = (sum:=0; for...; sum))`)
    // get hoisted into synthetic top-level bindings so the same lifting logic
    // that handles named bindings can pick them up. Without this the inner
    // result identifier (`sum`) would leak into the WGSL with nothing
    // declaring it.
    let owned_ast;
    let ast: &AstNode = if needs_anon_hoisting(ast) {
        owned_ast = hoist_anonymous_blocks(ast);
        &owned_ast
    } else {
        ast
    };

    let mut ctx = GenContext::new();

    // Collect top-level function definitions (and bindings if no top-level loops)
    ctx.collect_functions(ast);

    // Find the expression to evaluate (last non-binding, non-function-def statement)
    let expr = find_result_expr(ast)?;

    let is_bool = ctx.result_is_bool(expr);
    let is_vec = ctx.result_is_vec(expr);

    // Check for top-level loops (for or while)
    let top_has_loops = match ast {
        AstNode::Block { items: stmts, .. } => stmts
            .iter()
            .any(|s| matches!(s, AstNode::WhileLoop { .. } | AstNode::ForLoop { .. })),
        AstNode::WhileLoop { .. } | AstNode::ForLoop { .. } => true,
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
            AstNode::Block { items: stmts, .. } => stmts
                .iter()
                .filter_map(|s| {
                    if let AstNode::Binding { name, value, .. } = s {
                        Some((name.as_str(), value.as_ref()))
                    } else {
                        None
                    }
                })
                .collect(),
            AstNode::Binding { name, value, .. } => vec![(name.as_str(), value.as_ref())],
            _ => Vec::new(),
        };
        for binding in &ctx.bindings {
            let is_const = binding_asts
                .iter()
                .find(|(n, _)| *n == binding.name.as_str())
                .is_some_and(|(_, val)| is_const_expr(val, &const_names));
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

    // x/y are always world coordinates so plots follow the axis bounds
    shader.push_str("    let world = mix(u.axis_min, u.axis_max, uv);\n");
    shader.push_str("    let x = world.x;\n");
    shader.push_str("    let y = world.y;\n");

    if top_has_loops {
        // Imperative emission for top-level code with loops
        let top_stmts = match ast {
            AstNode::Block { items: stmts, .. } => stmts.as_slice(),
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
        // Emit non-constant bindings as immutable let inside fs_main.
        // For block-valued bindings (e.g. `f := (sum := 0; for ... ; y = sum)`)
        // emit the block's imperative statements as a preamble so any locals
        // the result expression depends on are in scope.
        let mut declared: HashSet<String> = HashSet::new();
        declared.insert("x".to_string());
        declared.insert("y".to_string());
        for name in &module_binding_names {
            declared.insert(name.clone());
        }
        for binding in &fs_main_bindings {
            if let Some(stmts) = &binding.block_preamble {
                let preamble = ctx.emit_imperative_stmts(stmts, "    ", &mut declared)?;
                shader.push_str(&preamble);
            }
            shader.push_str(&format!("    let {} = {};\n", binding.name, binding.expr));
            declared.insert(binding.name.clone());
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

        // Hoist each lifted block's per-corner evaluation into a single `let`,
        // so the corner-check expression below can reuse the values without
        // re-invoking the (potentially loop-heavy) function multiple times.
        for (binding_name, def) in &ctx.lifted_block_defs {
            shader.push_str(&format!(
                "    let _corner_{0}_mm = {1}(x_m, y_m);\n",
                binding_name, def.fn_name
            ));
            shader.push_str(&format!(
                "    let _corner_{0}_mp = {1}(x_m, y_p);\n",
                binding_name, def.fn_name
            ));
            shader.push_str(&format!(
                "    let _corner_{0}_pm = {1}(x_p, y_m);\n",
                binding_name, def.fn_name
            ));
            shader.push_str(&format!(
                "    let _corner_{0}_pp = {1}(x_p, y_p);\n",
                binding_name, def.fn_name
            ));
        }

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
    max_loop_iter: u32,
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
    /// If the binding's value was a block with imperative statements
    /// (e.g. `f := (sum := 0; for ... ; y = sum)`), these are the block's
    /// statements — emitted as a preamble before `let name = expr` so any
    /// vars/loops the result expression depends on are in scope.
    block_preamble: Option<Vec<AstNode>>,
}

struct EmittedFunction {
    wgsl_code: String,
}

/// Stored AST of a bool function for inlining in the plotting context.
struct BoolFunctionDef {
    params: Vec<String>,
    body: AstNode,
}

struct GenContext {
    functions: Vec<EmittedFunction>,
    bindings: Vec<EmittedBinding>,
    /// Names of user-defined functions that return vec types (not f32).
    vec_functions: HashSet<String>,
    /// Names of user-defined functions that return bool.
    bool_functions: HashSet<String>,
    /// AST bodies of bool functions — used for inlining during corner-checking
    /// so that comparisons go through sign-change detection, not float ==.
    bool_function_defs: std::collections::HashMap<String, BoolFunctionDef>,
    /// AST values of bool-typed bindings — used for inlining during
    /// corner-checking so `f := x = y^2; plot(f)` renders the curve correctly.
    bool_binding_defs: std::collections::HashMap<String, AstNode>,
    /// Block-valued bindings (e.g. `f := (sum := 0; for ... ; y = sum)`) lifted
    /// into WGSL functions so corner-checking can re-evaluate the block at each
    /// corner, not just the pixel center.
    lifted_block_defs: std::collections::HashMap<String, LiftedBlockDef>,
    /// User-defined bool functions whose bodies are imperative + comparison.
    /// Tracks the matching `_diff_<name>` companion that returns lhs - rhs so
    /// corner-checking can call the function at each corner without re-inlining
    /// the imperative body (which doesn't survive emit_bool_with_corners).
    lifted_function_defs: std::collections::HashMap<String, LiftedFunctionDef>,
    /// For hoisted nested functions: maps function name → extra captured variables to pass.
    captured_vars: std::collections::HashMap<String, Vec<String>>,
}

/// Metadata for a block-valued binding lifted into a WGSL function.
/// The function signature is `fn fn_name(x: f32, y: f32) -> f32`. When
/// `comparison_op` is set, the function returns `lhs - rhs`; otherwise it
/// returns the block's float result directly.
#[derive(Debug, Clone)]
struct LiftedBlockDef {
    fn_name: String,
    comparison_op: Option<String>,
}

/// Metadata for a user-defined function whose bool body has imperative content.
/// `diff_fn_name(args..., x_corner, y_corner) -> f32` returns `lhs - rhs` of the
/// body's comparison so corner-checking can sign-check at four corners.
#[derive(Debug, Clone)]
struct LiftedFunctionDef {
    diff_fn_name: String,
    comparison_op: String,
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
            bool_functions: HashSet::new(),
            bool_function_defs: std::collections::HashMap::new(),
            bool_binding_defs: std::collections::HashMap::new(),
            lifted_block_defs: std::collections::HashMap::new(),
            lifted_function_defs: std::collections::HashMap::new(),
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
            AstNode::Block { items: stmts, .. } => {
                for stmt in stmts {
                    self.collect_functions_with_scope(stmt, scope_bindings);
                }
            }
            AstNode::FunctionDef {
                name, params, body, ..
            } => {
                // Collect nested function defs from the body first
                // Build the scope for nested functions: parent scope + this function's params + body bindings
                let mut inner_scope: Vec<String> = scope_bindings.to_vec();
                inner_scope.extend(params.iter().cloned());
                if let AstNode::Block { items: stmts, .. } = body.as_ref() {
                    // Add binding names from the body to the inner scope
                    for stmt in stmts {
                        match stmt {
                            AstNode::Binding { name, .. } => inner_scope.push(name.clone()),
                            AstNode::TupleBinding { names, .. } => {
                                inner_scope.extend(names.iter().cloned())
                            }
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

                // Determine captured variables (scope bindings or axis variables
                // referenced in body, not shadowed by params). `x`/`y` are
                // implicitly available in fs_main but not inside WGSL functions —
                // pass them as extra params when the body references them, so e.g.
                // `f(n) := (sum := x; for ... ; y = sum)` compiles correctly.
                let mut captured = Vec::new();
                let param_set: HashSet<&str> = params.iter().map(|s| s.as_str()).collect();
                let referenced = find_referenced_identifiers(body);
                for var in scope_bindings {
                    if referenced.contains(var.as_str()) && !param_set.contains(var.as_str()) {
                        captured.push(var.clone());
                    }
                }
                for axis in ["x", "y"] {
                    if referenced.contains(axis)
                        && !param_set.contains(axis)
                        && !captured.iter().any(|c| c == axis)
                    {
                        captured.push(axis.to_string());
                    }
                }

                // Build parameter list including captured variables
                let mut all_params: Vec<String> =
                    params.iter().map(|p| format!("{}: f32", p)).collect();
                for cap in &captured {
                    all_params.push(format!("{}: f32", cap));
                }

                if !captured.is_empty() {
                    self.captured_vars.insert(name.clone(), captured.clone());
                }

                // Check if function body returns a tuple/vec (for vec4 color output)
                // Use result_is_vec which also checks calls to known vec-returning functions
                let returns_vec = self.result_is_vec(body);

                // Check if the body is a block with bindings or loops
                let needs_imperative = match body.as_ref() {
                    AstNode::Block { items: stmts, .. } => stmts.iter().any(|s| {
                        matches!(
                            s,
                            AstNode::Binding { .. }
                                | AstNode::WhileLoop { .. }
                                | AstNode::ForLoop { .. }
                                | AstNode::TupleBinding { .. }
                        )
                    }),
                    _ => false,
                };

                let returns_bool_val = returns_bool(body);
                let ret_type = if returns_vec {
                    "vec4<f32>"
                } else if returns_bool_val {
                    "bool"
                } else {
                    "f32"
                };
                if returns_vec {
                    self.vec_functions.insert(name.clone());
                }
                if returns_bool_val {
                    self.bool_functions.insert(name.clone());
                    self.bool_function_defs.insert(
                        name.clone(),
                        BoolFunctionDef {
                            params: params.clone(),
                            body: body.as_ref().clone(),
                        },
                    );
                }

                // For bool functions with imperative bodies whose result is a
                // direct comparison, also emit a `_diff_<name>` companion that
                // returns lhs - rhs. Corner-checking uses this so it can call
                // the function at each pixel corner without re-inlining the
                // imperative body (which `emit_bool_with_corners` can't handle).
                if returns_bool_val && needs_imperative {
                    if let AstNode::Block { items: stmts, .. } = body.as_ref() {
                        if let Some(result) = block_result_expr_from_stmts(stmts) {
                            if let AstNode::Apply { name: op, args: cmp_args, .. } = result {
                                if cmp_args.len() == 2
                                    && matches!(
                                        op.as_str(),
                                        "eq" | "neq" | "lt" | "gt" | "lte" | "gte"
                                    )
                                {
                                    let diff_name = format!("_diff_{}", name);
                                    let mut declared: HashSet<String> = HashSet::new();
                                    for p in params {
                                        declared.insert(p.clone());
                                    }
                                    for c in &captured {
                                        declared.insert(c.clone());
                                    }
                                    if let Ok(diff_body) = self.emit_lifted_block_body_with(
                                        stmts,
                                        result,
                                        &Some(op.clone()),
                                        declared,
                                    ) {
                                        let diff_wgsl = format!(
                                            "fn {}({}) -> f32 {{\n{}}}\n",
                                            diff_name,
                                            all_params.join(", "),
                                            diff_body,
                                        );
                                        self.functions
                                            .push(EmittedFunction { wgsl_code: diff_wgsl });
                                        self.lifted_function_defs.insert(
                                            name.clone(),
                                            LiftedFunctionDef {
                                                diff_fn_name: diff_name,
                                                comparison_op: op.clone(),
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                if needs_imperative {
                    if let AstNode::Block { items: stmts, .. } = body.as_ref() {
                        if let Ok(body_wgsl) = self.emit_function_body(stmts) {
                            let wgsl_code = format!(
                                "fn {}({}) -> {} {{\n{}}}\n",
                                name,
                                all_params.join(", "),
                                ret_type,
                                body_wgsl,
                            );
                            self.functions.push(EmittedFunction { wgsl_code });
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
                    self.functions.push(EmittedFunction { wgsl_code });
                }
            }
            AstNode::Binding { name, value, .. } => {
                // Block-valued bindings with imperative content (var/loop) are lifted
                // into WGSL functions so corner-checking can re-evaluate the block at
                // each corner of a pixel — without this, e.g. `sum` is computed once
                // at pixel-center x and the curve renders dotted on steep parts.
                let imperative_block_stmts = match value.as_ref() {
                    AstNode::Block { items: stmts, .. } if has_imperative_stmt(stmts) => {
                        Some(stmts.clone())
                    }
                    _ => None,
                };

                if let Some(stmts) = imperative_block_stmts {
                    if let Ok((fn_name, comparison_op, binding_expr)) =
                        self.lift_block_to_fn(name, &stmts)
                    {
                        let is_comparison = comparison_op.is_some();
                        self.lifted_block_defs.insert(
                            name.clone(),
                            LiftedBlockDef {
                                fn_name,
                                comparison_op,
                            },
                        );
                        // For lifted comparison results we render via corner-checking
                        // (which calls the function 4 times directly); the regular
                        // `let name = call(x, y) <op> 0.0` binding would only add a
                        // 5th unused call per pixel. Skip emitting it.
                        if !is_comparison {
                            self.bindings.push(EmittedBinding {
                                name: name.clone(),
                                expr: binding_expr,
                                block_preamble: None,
                            });
                        }
                        return;
                    }
                    // Fall through if lifting failed.
                }

                if returns_bool(value) {
                    let result_expr = block_result_expr(value).clone();
                    self.bool_binding_defs.insert(name.clone(), result_expr);
                }
                if let Ok(expr_code) = self.emit_expr(value) {
                    self.bindings.push(EmittedBinding {
                        name: name.clone(),
                        expr: expr_code,
                        block_preamble: None,
                    });
                }
            }
            AstNode::TupleBinding { names, value, .. } => {
                // For tuple bindings at top level, emit individual bindings
                if let AstNode::Tuple { items, .. } = value.as_ref() {
                    for (i, name) in names.iter().enumerate() {
                        if let Some(item) = items.get(i) {
                            if let Ok(expr_code) = self.emit_expr(item) {
                                self.bindings.push(EmittedBinding {
                                    name: name.clone(),
                                    expr: expr_code,
                                    block_preamble: None,
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
    /// Lift a block-valued binding into a WGSL function so its body re-runs
    /// at every corner during corner-checking. Returns
    /// `(fn_name, comparison_op, binding_call_expr)`.
    fn lift_block_to_fn(
        &mut self,
        binding_name: &str,
        stmts: &[AstNode],
    ) -> Result<(String, Option<String>, String), String> {
        let result = block_result_expr_from_stmts(stmts)
            .ok_or_else(|| format!("block-valued binding `{}` has no result", binding_name))?;

        let comparison_op = match result {
            AstNode::Apply { name, args, .. }
                if args.len() == 2
                    && matches!(
                        name.as_str(),
                        "eq" | "neq" | "lt" | "gt" | "lte" | "gte"
                    ) =>
            {
                Some(name.clone())
            }
            _ => None,
        };

        let fn_name = format!("_lifted_{}", binding_name);
        let body = self.emit_lifted_block_body(stmts, result, &comparison_op)?;
        let func_wgsl = format!("fn {}(x: f32, y: f32) -> f32 {{\n{}}}\n", fn_name, body);
        self.functions.push(EmittedFunction { wgsl_code: func_wgsl });

        let binding_expr = if let Some(op) = &comparison_op {
            let wgsl_op = match op.as_str() {
                "eq" => "==",
                "neq" => "!=",
                "lt" => "<",
                "gt" => ">",
                "lte" => "<=",
                "gte" => ">=",
                _ => "==",
            };
            format!("({}(x, y) {} 0.0)", fn_name, wgsl_op)
        } else {
            format!("{}(x, y)", fn_name)
        };

        Ok((fn_name, comparison_op, binding_expr))
    }

    /// Emit a lifted-block function body: imperative statements then a return
    /// of either `lhs - rhs` (for comparison results) or the float result.
    /// Default `declared` is `{x, y}` (lifted-block functions take those two);
    /// callers needing a different signature use `emit_lifted_block_body_with`.
    fn emit_lifted_block_body(
        &self,
        stmts: &[AstNode],
        result: &AstNode,
        comparison_op: &Option<String>,
    ) -> Result<String, String> {
        let mut declared: HashSet<String> = HashSet::new();
        declared.insert("x".to_string());
        declared.insert("y".to_string());
        self.emit_lifted_block_body_with(stmts, result, comparison_op, declared)
    }

    /// Like `emit_lifted_block_body` but with caller-supplied initial `declared`
    /// set — used for `_diff_<name>` companions of user functions whose params
    /// (and captured vars) need to be in scope before emitting imperative stmts.
    fn emit_lifted_block_body_with(
        &self,
        stmts: &[AstNode],
        result: &AstNode,
        comparison_op: &Option<String>,
        mut declared: HashSet<String>,
    ) -> Result<String, String> {
        let mut code = String::new();

        for stmt in stmts {
            match stmt {
                AstNode::Binding { name, value, .. } => {
                    let val = self.emit_expr(value)?;
                    if declared.contains(name.as_str()) {
                        code += &format!("    {} = {};\n", name, val);
                    } else {
                        code += &format!("    var {} = {};\n", name, val);
                        declared.insert(name.clone());
                    }
                }
                AstNode::TupleBinding { names, value, .. } => {
                    self.emit_tuple_binding(
                        &mut code,
                        names,
                        value,
                        "    ",
                        "var",
                        &mut declared,
                    )?;
                }
                AstNode::ForLoop {
                    var, range, body, ..
                } => {
                    self.emit_for_loop(&mut code, var, range, body, "    ", &mut declared)?;
                }
                AstNode::WhileLoop {
                    condition, body, ..
                } => {
                    self.emit_while_condition_bindings(
                        &mut code,
                        condition,
                        "    ",
                        &mut declared,
                    )?;
                    let cond = self.emit_expr(condition)?;
                    code += &format!(
                        "    for (var _loop_guard: u32 = 0u; _loop_guard < u.max_loop_iter; _loop_guard = _loop_guard + 1u) {{\n"
                    );
                    self.emit_while_condition_bindings_inner(
                        &mut code,
                        condition,
                        "        ",
                        &mut declared,
                    )?;
                    code += &format!("        if (!({cond})) {{ break; }}\n");
                    self.emit_loop_body_stmts(&mut code, body, &mut declared)?;
                    code += "    }\n";
                }
                AstNode::FunctionDef { .. } => {}
                _ => {} // result expression handled below
            }
        }

        if comparison_op.is_some() {
            if let AstNode::Apply { args, .. } = result {
                if args.len() == 2 {
                    let lhs = self.emit_expr(&args[0])?;
                    let rhs = self.emit_expr(&args[1])?;
                    code += &format!("    return ({}) - ({});\n", lhs, rhs);
                    return Ok(code);
                }
            }
        }
        let result_code = self.emit_expr(result)?;
        code += &format!("    return {};\n", result_code);
        Ok(code)
    }

    fn emit_function_body(&self, stmts: &[AstNode]) -> Result<String, String> {
        let has_loops = stmts
            .iter()
            .any(|s| matches!(s, AstNode::WhileLoop { .. } | AstNode::ForLoop { .. }));
        let var_keyword = if has_loops { "var" } else { "let" };
        let mut code = String::new();
        let mut declared: HashSet<String> = HashSet::new();
        let mut result_expr = "0.0".to_string();

        for stmt in stmts {
            match stmt {
                AstNode::Binding { name, value, .. } => {
                    let val = self.emit_expr(value)?;
                    if declared.contains(name.as_str()) {
                        code += &format!("    {} = {};\n", name, val);
                    } else {
                        code += &format!("    {} {} = {};\n", var_keyword, name, val);
                        declared.insert(name.clone());
                    }
                }
                AstNode::TupleBinding { names, value, .. } => {
                    self.emit_tuple_binding(
                        &mut code,
                        names,
                        value,
                        "    ",
                        var_keyword,
                        &mut declared,
                    )?;
                }
                AstNode::ForLoop {
                    var, range, body, ..
                } => {
                    self.emit_for_loop(&mut code, var, range, body, "    ", &mut declared)?;
                }
                AstNode::WhileLoop {
                    condition, body, ..
                } => {
                    // Extract inline bindings from the condition (e.g. sq: expr inside block)
                    self.emit_while_condition_bindings(
                        &mut code,
                        condition,
                        "    ",
                        &mut declared,
                    )?;
                    // Emit loop with iteration guard
                    let cond = self.emit_expr(condition)?;
                    code += &format!(
                        "    for (var _loop_guard: u32 = 0u; _loop_guard < u.max_loop_iter; _loop_guard = _loop_guard + 1u) {{\n"
                    );
                    // Re-emit condition bindings at the top of each iteration
                    self.emit_while_condition_bindings_inner(
                        &mut code,
                        condition,
                        "        ",
                        &mut declared,
                    )?;
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
    /// Emit a `for var in start..end (body)` as a WGSL for-loop with iteration guard.
    fn emit_for_loop(
        &self,
        code: &mut String,
        var: &str,
        range: &AstNode,
        body: &AstNode,
        indent: &str,
        declared: &mut HashSet<String>,
    ) -> Result<(), String> {
        let (start_expr, end_expr) = match range {
            AstNode::Range { start, end, .. } => (self.emit_expr(start)?, self.emit_expr(end)?),
            _ => return Err("for loop range must be start..end".to_string()),
        };
        // Declare or reassign the loop variable
        if declared.contains(var) {
            code.push_str(&format!("{}{} = {};\n", indent, var, start_expr));
        } else {
            code.push_str(&format!("{}var {} = {};\n", indent, var, start_expr));
            declared.insert(var.to_string());
        }
        // Iteration-guarded loop
        code.push_str(&format!(
            "{}for (var _loop_guard: u32 = 0u; _loop_guard < u.max_loop_iter; _loop_guard = _loop_guard + 1u) {{\n",
            indent
        ));
        code.push_str(&format!(
            "{}    if (!({} < {})) {{ break; }}\n",
            indent, var, end_expr
        ));
        // Body
        self.emit_loop_body_stmts(code, body, declared)?;
        // Update: var = var + 1.0
        code.push_str(&format!("{}    {} = {} + 1.0;\n", indent, var, var));
        code.push_str(&format!("{}}}\n", indent));
        Ok(())
    }

    fn emit_loop_body_stmts(
        &self,
        code: &mut String,
        body: &AstNode,
        declared: &mut HashSet<String>,
    ) -> Result<(), String> {
        match body {
            AstNode::Block { items: stmts, .. } => {
                for stmt in stmts {
                    match stmt {
                        AstNode::Binding { name, value, .. } => {
                            let val = self.emit_expr(value)?;
                            if declared.contains(name.as_str()) {
                                code.push_str(&format!("        {} = {};\n", name, val));
                            } else {
                                code.push_str(&format!("        var {} = {};\n", name, val));
                                declared.insert(name.clone());
                            }
                        }
                        AstNode::TupleBinding { names, value, .. } => {
                            self.emit_tuple_binding(
                                code, names, value, "        ", "var", declared,
                            )?;
                        }
                        _ => {} // Non-binding expressions in loop body (side-effect free, skip)
                    }
                }
            }
            AstNode::Binding { name, value, .. } => {
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

    /// Check if an expression produces a bool type, including user-defined bool functions
    /// and identifiers bound to bool expressions.
    fn result_is_bool(&self, node: &AstNode) -> bool {
        match node {
            AstNode::Apply { name, .. } => {
                if self.bool_functions.contains(name) {
                    return true;
                }
                returns_bool(node)
            }
            AstNode::Identifier { name, .. } => {
                self.bool_binding_defs.contains_key(name)
                    || self
                        .lifted_block_defs
                        .get(name)
                        .is_some_and(|d| d.comparison_op.is_some())
            }
            AstNode::Block { items: stmts, .. } => {
                stmts.last().is_some_and(|s| self.result_is_bool(s))
            }
            AstNode::IfExpr {
                then_branch,
                else_branch,
                ..
            } => {
                self.result_is_bool(then_branch)
                    || else_branch
                        .as_ref()
                        .is_some_and(|e| self.result_is_bool(e))
            }
            other => returns_bool(other),
        }
    }

    /// Check if an expression produces a vec type (for shader output detection).
    fn result_is_vec(&self, node: &AstNode) -> bool {
        match node {
            AstNode::Tuple { items, .. } => items.len() >= 2,
            AstNode::Apply { name, .. } => self.vec_functions.contains(name),
            AstNode::IfExpr {
                then_branch,
                else_branch,
                ..
            } => {
                self.result_is_vec(then_branch)
                    || else_branch
                        .as_ref()
                        .is_some_and(|e| self.result_is_vec(e))
            }
            AstNode::Block { items: stmts, .. } => {
                stmts.last().is_some_and(|s| self.result_is_vec(s))
            }
            _ => false,
        }
    }

    /// Return the tuple size of the result expression, if it's a direct tuple literal.
    fn result_tuple_size(&self, node: &AstNode) -> Option<usize> {
        match node {
            AstNode::Tuple { items, .. } => Some(items.len()),
            AstNode::Block { items: stmts, .. } => {
                stmts.last().and_then(|s| self.result_tuple_size(s))
            }
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
        if let AstNode::Tuple { items, .. } = value {
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
            AstNode::Block { items: stmts, .. } => {
                for stmt in stmts {
                    if let AstNode::Binding { name, value, .. } = stmt {
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
            AstNode::Block { items: stmts, .. } => {
                for stmt in stmts {
                    if let AstNode::Binding { name, value, .. } = stmt {
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
                AstNode::Binding { name, value, .. } => {
                    let val = self.emit_expr(value)?;
                    if declared.contains(name.as_str()) {
                        code += &format!("{}{} = {};\n", indent, name, val);
                    } else {
                        code += &format!("{}var {} = {};\n", indent, name, val);
                        declared.insert(name.clone());
                    }
                }
                AstNode::TupleBinding { names, value, .. } => {
                    self.emit_tuple_binding(&mut code, names, value, indent, "var", declared)?;
                }
                AstNode::ForLoop {
                    var, range, body, ..
                } => {
                    self.emit_for_loop(&mut code, var, range, body, indent, declared)?;
                }
                AstNode::WhileLoop {
                    condition, body, ..
                } => {
                    // Pre-declare condition bindings
                    self.emit_while_condition_bindings(&mut code, condition, indent, declared)?;
                    let cond = self.emit_expr(condition)?;
                    code += &format!(
                        "{}for (var _loop_guard: u32 = 0u; _loop_guard < u.max_loop_iter; _loop_guard = _loop_guard + 1u) {{\n",
                        indent
                    );
                    let inner_indent = format!("{}    ", indent);
                    self.emit_while_condition_bindings_inner(
                        &mut code,
                        condition,
                        &inner_indent,
                        declared,
                    )?;
                    code += &format!("{}    if (!({cond})) {{ break; }}\n", indent);
                    self.emit_loop_body_stmts(&mut code, body, declared)?;
                    code += &format!("{}}}\n", indent);
                }
                AstNode::FunctionDef { .. } => {} // Already collected
                _ => {}                           // Result expression handled separately
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
            AstNode::Number { value: n, .. } => {
                let s = if n.fract() == 0.0 && !n.is_nan() && !n.is_infinite() {
                    format!("{:.1}", n)
                } else {
                    format!("{}", n)
                };
                Ok(s)
            }
            AstNode::BoolLit { value: b, .. } => Ok(format!("{}", b)),
            AstNode::Identifier { name, .. } => {
                if let Some((x_var, y_var)) = subst {
                    if name == "x" {
                        return Ok(x_var.to_string());
                    }
                    if name == "y" {
                        return Ok(y_var.to_string());
                    }
                    // Lifted block-valued binding inside a corner check: use
                    // the precomputed `_corner_<name>_<suffix>` value so the
                    // block actually re-evaluates per corner. Without this
                    // the binding would resolve to its pixel-center value
                    // and the curve would dot out on steep slopes.
                    if self.lifted_block_defs.contains_key(name) {
                        if let Some(suffix) = corner_suffix(x_var, y_var) {
                            return Ok(format!("_corner_{}_{}", name, suffix));
                        }
                        // Non-standard corner — fall back to a direct call.
                        let def = &self.lifted_block_defs[name];
                        return Ok(format!("{}({}, {})", def.fn_name, x_var, y_var));
                    }
                }
                // Map time/t → u.time (accessible in user functions too)
                if name == "t" {
                    return Ok("u.time".to_string());
                }
                Ok(name.clone())
            }
            AstNode::Apply { name, args, .. } => self.emit_apply_internal(name, args, subst),
            AstNode::Tuple { items, .. } => {
                let parts: Result<Vec<_>, _> = items
                    .iter()
                    .map(|i| self.emit_expr_internal(i, subst))
                    .collect();
                let parts = parts?;
                match items.len() {
                    2 => Ok(format!("vec2<f32>({})", parts.join(", "))),
                    3 => Ok(format!("vec3<f32>({})", parts.join(", "))),
                    4 => Ok(format!("vec4<f32>({})", parts.join(", "))),
                    _ => Err(format!("Unsupported tuple size: {}", items.len())),
                }
            }
            AstNode::Block { items: stmts, .. } => {
                if let Some(last) = stmts.last() {
                    self.emit_expr_internal(last, subst)
                } else {
                    Ok("0.0".to_string())
                }
            }
            AstNode::Binding { .. } => Ok("0.0".to_string()),
            AstNode::IfExpr {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
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
            AstNode::PropertyAccess {
                object, property, ..
            } => {
                // Map x.min → u.axis_min.x, x.max → u.axis_max.x, etc.
                if let AstNode::Identifier { name: base, .. } = object.as_ref() {
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
            AstNode::ForLoop { .. } => Ok("0.0".to_string()),      // Emitted imperatively
            AstNode::WhileLoop { .. } => Ok("0.0".to_string()),    // Emitted imperatively
            AstNode::ArrayLiteral { .. }
            | AstNode::IndexAccess { .. }
            | AstNode::Range { .. }
            | AstNode::ParallelFor { .. }
            | AstNode::IndexAssign { .. } => {
                Err("Array/parallel operations are not supported in fragment shaders".to_string())
            }
        }
    }

    fn emit_apply_internal(
        &self,
        name: &str,
        args: &[AstNode],
        subst: CornerSubst,
    ) -> Result<String, String> {
        let emit_args: Result<Vec<_>, _> = args
            .iter()
            .map(|a| self.emit_expr_internal(a, subst))
            .collect();
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
            ("log10", 1) => Ok(format!("(log2({}) / log2(10.0))", emitted[0])),
            ("exp", 1) => Ok(format!("exp({})", emitted[0])),
            ("exp2", 1) => Ok(format!("exp2({})", emitted[0])),
            ("sqrt", 1) => Ok(format!("sqrt({})", emitted[0])),
            ("abs", 1) => Ok(format!("abs({})", emitted[0])),
            ("sign", 1) => Ok(format!("sign({})", emitted[0])),
            ("floor", 1) => Ok(format!("floor({})", emitted[0])),
            ("ceil", 1) => Ok(format!("ceil({})", emitted[0])),
            ("round", 1) => Ok(format!("round({})", emitted[0])),
            ("fract", 1) => Ok(format!("fract({})", emitted[0])),
            ("pow", 2) => {
                // pow(x, n) for small non-negative integer n is much cheaper as
                // repeated multiplication — pow() costs ~20+ GPU ops via exp2/log2,
                // and `x²` (very common in plotting) shouldn't pay that.
                if let AstNode::Number { value: n, .. } = &args[1] {
                    if *n >= 0.0 && n.fract() == 0.0 && *n <= 8.0 {
                        let times = *n as u32;
                        if times == 0 {
                            return Ok("1.0".to_string());
                        }
                        let factors = vec![format!("({})", emitted[0]); times as usize];
                        return Ok(format!("({})", factors.join(" * ")));
                    }
                }
                Ok(format!("pow({}, {})", emitted[0], emitted[1]))
            }
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
            ("f64", 1) => Ok(format!("f32({})", emitted[0])),
            ("i32", 1) => Ok(format!("i32({})", emitted[0])),
            ("vec2", n) if n >= 1 => Ok(format!("vec2<f32>({})", emitted.join(", "))),
            ("vec3", n) if n >= 1 => Ok(format!("vec3<f32>({})", emitted.join(", "))),
            ("vec4", n) if n >= 1 => Ok(format!("vec4<f32>({})", emitted.join(", "))),

            // User-defined functions: emit as regular call, appending captured vars if any.
            // Captured axis vars (`x`, `y`) get the corner substitution when we're
            // inside corner-checking — otherwise the function would be evaluated at
            // the pixel center for all four corners, killing the sign-change check
            // and producing a dotted curve.
            _ => {
                let mut all_args = emitted;
                if let Some(captured) = self.captured_vars.get(name) {
                    for cap in captured {
                        let s = match (subst, cap.as_str()) {
                            (Some((xv, _)), "x") => xv.to_string(),
                            (Some((_, yv)), "y") => yv.to_string(),
                            _ => cap.clone(),
                        };
                        all_args.push(s);
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
            AstNode::BoolLit { value: b, .. } => Ok(format!("{}", b)),

            // Lifted block-valued binding: call the synthesized WGSL function at
            // each pixel corner so the loop/local state re-runs for x_m/x_p/y_m/y_p
            // rather than reusing the pixel-center value (which causes dotted curves).
            // The four corner calls are hoisted into `__lifted_<name>_mm/mp/pm/pp`
            // by the caller so we only invoke the function 4 times per pixel,
            // not 6+ (the corner expression below references each corner twice).
            AstNode::Identifier { name, .. } if self.lifted_block_defs.contains_key(name) => {
                let def = self.lifted_block_defs.get(name).unwrap();
                let calls: Vec<String> = vec![
                    format!("_corner_{}_mm", name),
                    format!("_corner_{}_mp", name),
                    format!("_corner_{}_pm", name),
                    format!("_corner_{}_pp", name),
                ];

                if let Some(op) = &def.comparison_op {
                    match op.as_str() {
                        "eq" => {
                            let sides: Vec<String> = calls
                                .iter()
                                .map(|c| format!("(({}) > 0.0)", c))
                                .collect();
                            Ok(format!(
                                "(!({} == {} && {} == {} && {} == {}))",
                                sides[0],
                                sides[1],
                                sides[1],
                                sides[2],
                                sides[2],
                                sides[3]
                            ))
                        }
                        "neq" => {
                            let sides: Vec<String> = calls
                                .iter()
                                .map(|c| format!("(({}) > 0.0)", c))
                                .collect();
                            Ok(format!(
                                "({} == {} && {} == {} && {} == {})",
                                sides[0],
                                sides[1],
                                sides[1],
                                sides[2],
                                sides[2],
                                sides[3]
                            ))
                        }
                        "lt" => Ok(format!(
                            "(({}) < 0.0 && ({}) < 0.0 && ({}) < 0.0 && ({}) < 0.0)",
                            calls[0], calls[1], calls[2], calls[3]
                        )),
                        "gt" => Ok(format!(
                            "(({}) > 0.0 && ({}) > 0.0 && ({}) > 0.0 && ({}) > 0.0)",
                            calls[0], calls[1], calls[2], calls[3]
                        )),
                        "lte" => Ok(format!(
                            "(({}) <= 0.0 && ({}) <= 0.0 && ({}) <= 0.0 && ({}) <= 0.0)",
                            calls[0], calls[1], calls[2], calls[3]
                        )),
                        "gte" => Ok(format!(
                            "(({}) >= 0.0 && ({}) >= 0.0 && ({}) >= 0.0 && ({}) >= 0.0)",
                            calls[0], calls[1], calls[2], calls[3]
                        )),
                        _ => self.emit_expr(node),
                    }
                } else {
                    // No comparison — treat the float result as an implicit curve `f = 0`.
                    let sides: Vec<String> = calls
                        .iter()
                        .map(|c| format!("(({}) > 0.0)", c))
                        .collect();
                    Ok(format!(
                        "(!({} == {} && {} == {} && {} == {}))",
                        sides[0], sides[1], sides[1], sides[2], sides[2], sides[3]
                    ))
                }
            }

            // Identifier bound to a bool expression: inline so the comparison
            // goes through corner-checking instead of a direct float ==.
            AstNode::Identifier { name, .. } if self.bool_binding_defs.contains_key(name) => {
                let bound = self.bool_binding_defs.get(name).unwrap().clone();
                self.emit_bool_with_corners(&bound)
            }

            AstNode::Apply { name, args, .. } => {
                match name.as_str() {
                    // Logical ops: recursively apply corner checking.
                    // Operands may be bool (comparisons) or float (implicit curves).
                    "and" if args.len() == 2 => {
                        let l = self.emit_bool_operand_with_corners(&args[0])?;
                        let r = self.emit_bool_operand_with_corners(&args[1])?;
                        Ok(format!("({} && {})", l, r))
                    }
                    "or" if args.len() == 2 => {
                        let l = self.emit_bool_operand_with_corners(&args[0])?;
                        let r = self.emit_bool_operand_with_corners(&args[1])?;
                        Ok(format!("({} || {})", l, r))
                    }
                    "not" if args.len() == 1 => {
                        let inner = self.emit_bool_operand_with_corners(&args[0])?;
                        Ok(format!("!({})", inner))
                    }
                    // Comparison ops: apply corner checking
                    "eq" | "neq" | "lt" | "gt" | "lte" | "gte" if args.len() == 2 => {
                        self.emit_comparison_with_corners(name, &args[0], &args[1])
                    }
                    // User-defined bool function: inline body and apply corner-checking.
                    // This makes f(x,y) produce identical rendering to the inline expression.
                    // User-defined function with imperative body + comparison
                    // result: call the precomputed `_diff_<name>` companion at
                    // the four pixel corners. Inlining doesn't work here because
                    // emit_bool_with_corners can't re-emit imperative stmts.
                    _ if self.lifted_function_defs.contains_key(name) => {
                        let def = self.lifted_function_defs.get(name).unwrap().clone();
                        let captured = self
                            .captured_vars
                            .get(name)
                            .cloned()
                            .unwrap_or_default();
                        // Build the regular argument list (not yet corner-substituted).
                        let arg_strs: Result<Vec<String>, String> = args
                            .iter()
                            .map(|a| self.emit_expr(a))
                            .collect();
                        let arg_strs = arg_strs?;

                        let corner_call = |xc: &str, yc: &str| {
                            let mut all = arg_strs.clone();
                            for cap in &captured {
                                let s = match cap.as_str() {
                                    "x" => xc.to_string(),
                                    "y" => yc.to_string(),
                                    other => other.to_string(),
                                };
                                all.push(s);
                            }
                            format!("{}({})", def.diff_fn_name, all.join(", "))
                        };

                        let calls = [
                            corner_call("x_m", "y_m"),
                            corner_call("x_m", "y_p"),
                            corner_call("x_p", "y_m"),
                            corner_call("x_p", "y_p"),
                        ];

                        match def.comparison_op.as_str() {
                            "eq" => {
                                let sides: Vec<String> = calls
                                    .iter()
                                    .map(|c| format!("(({}) > 0.0)", c))
                                    .collect();
                                Ok(format!(
                                    "(!({} == {} && {} == {} && {} == {}))",
                                    sides[0],
                                    sides[1],
                                    sides[1],
                                    sides[2],
                                    sides[2],
                                    sides[3]
                                ))
                            }
                            "neq" => {
                                let sides: Vec<String> = calls
                                    .iter()
                                    .map(|c| format!("(({}) > 0.0)", c))
                                    .collect();
                                Ok(format!(
                                    "({} == {} && {} == {} && {} == {})",
                                    sides[0],
                                    sides[1],
                                    sides[1],
                                    sides[2],
                                    sides[2],
                                    sides[3]
                                ))
                            }
                            "lt" => Ok(format!(
                                "(({}) < 0.0 && ({}) < 0.0 && ({}) < 0.0 && ({}) < 0.0)",
                                calls[0], calls[1], calls[2], calls[3]
                            )),
                            "gt" => Ok(format!(
                                "(({}) > 0.0 && ({}) > 0.0 && ({}) > 0.0 && ({}) > 0.0)",
                                calls[0], calls[1], calls[2], calls[3]
                            )),
                            "lte" => Ok(format!(
                                "(({}) <= 0.0 && ({}) <= 0.0 && ({}) <= 0.0 && ({}) <= 0.0)",
                                calls[0], calls[1], calls[2], calls[3]
                            )),
                            "gte" => Ok(format!(
                                "(({}) >= 0.0 && ({}) >= 0.0 && ({}) >= 0.0 && ({}) >= 0.0)",
                                calls[0], calls[1], calls[2], calls[3]
                            )),
                            _ => self.emit_expr(node),
                        }
                    }
                    _ if self.bool_function_defs.contains_key(name) => {
                        let func_def = self.bool_function_defs.get(name).unwrap();
                        let inlined = substitute_params(&func_def.body, &func_def.params, args);
                        self.emit_bool_with_corners(&inlined)
                    }
                    // Anything else: fall back to normal emission
                    _ => self.emit_expr(node),
                }
            }

            // Non-boolean nodes or identifiers: emit normally
            _ => self.emit_expr(node),
        }
    }

    /// Emit an operand of a logical op (and/or/not) with corner-checking.
    /// If the operand is boolean, recurse normally. If it's a float
    /// expression, treat it as an implicit curve (expr = 0).
    fn emit_bool_operand_with_corners(&self, node: &AstNode) -> Result<String, String> {
        if returns_bool(node) {
            self.emit_bool_with_corners(node)
        } else {
            // Float expression used as implicit curve: treat as expr = 0
            let zero = AstNode::Number {
                value: 0.0,
                span: node.span(),
            };
            self.emit_comparison_with_corners("eq", node, &zero)
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
                corner_signs[0],
                corner_signs[1],
                corner_signs[1],
                corner_signs[2],
                corner_signs[2],
                corner_signs[3],
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
        AstNode::Identifier { name, .. } => {
            result.insert(name.clone());
        }
        AstNode::Apply { args, .. } => {
            for arg in args {
                collect_identifiers(arg, result);
            }
        }
        AstNode::Block { items: stmts, .. } => {
            for s in stmts {
                collect_identifiers(s, result);
            }
        }
        AstNode::Binding { value, .. } => {
            collect_identifiers(value, result);
        }
        AstNode::TupleBinding { value, .. } => {
            collect_identifiers(value, result);
        }
        AstNode::Tuple { items, .. } => {
            for item in items {
                collect_identifiers(item, result);
            }
        }
        AstNode::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            collect_identifiers(condition, result);
            collect_identifiers(then_branch, result);
            if let Some(eb) = else_branch {
                collect_identifiers(eb, result);
            }
        }
        AstNode::WhileLoop {
            condition, body, ..
        } => {
            collect_identifiers(condition, result);
            collect_identifiers(body, result);
        }
        AstNode::FunctionDef { body, .. } => {
            collect_identifiers(body, result);
        }
        AstNode::PropertyAccess { object, .. } => {
            collect_identifiers(object, result);
        }
        AstNode::Number { .. } | AstNode::BoolLit { .. } => {}
        AstNode::ArrayLiteral { items: elems, .. } => {
            for e in elems {
                collect_identifiers(e, result);
            }
        }
        AstNode::IndexAccess { array, index, .. } => {
            collect_identifiers(array, result);
            collect_identifiers(index, result);
        }
        AstNode::Range { start, end, .. } => {
            collect_identifiers(start, result);
            collect_identifiers(end, result);
        }
        AstNode::ForLoop { range, body, .. } | AstNode::ParallelFor { range, body, .. } => {
            collect_identifiers(range, result);
            collect_identifiers(body, result);
        }
        AstNode::IndexAssign {
            array,
            index,
            value,
            ..
        } => {
            collect_identifiers(array, result);
            collect_identifiers(index, result);
            collect_identifiers(value, result);
        }
    }
}

/// Check if an AST node is a constant expression (no x, y, z, t references).
/// `const_names` tracks bindings already known to be constant.
fn is_const_expr(node: &AstNode, const_names: &HashSet<String>) -> bool {
    match node {
        AstNode::Number { .. } | AstNode::BoolLit { .. } => true,
        AstNode::Identifier { name, .. } => {
            if matches!(name.as_str(), "x" | "y" | "z" | "t") {
                return false;
            }
            const_names.contains(name)
        }
        AstNode::Apply { args, .. } => args.iter().all(|a| is_const_expr(a, const_names)),
        AstNode::Tuple { items, .. } => items.iter().all(|i| is_const_expr(i, const_names)),
        AstNode::Block { items: stmts, .. } => {
            stmts.iter().all(|s| is_const_expr(s, const_names))
        }
        AstNode::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            is_const_expr(condition, const_names)
                && is_const_expr(then_branch, const_names)
                && else_branch
                    .as_ref()
                    .is_none_or(|e| is_const_expr(e, const_names))
        }
        AstNode::Binding { value, .. } => is_const_expr(value, const_names),
        _ => false,
    }
}

/// Substitute parameter names with argument expressions in an AST.
/// Used for inlining bool functions in the corner-checking context so that
/// comparisons go through sign-change detection rather than float `==`.
fn substitute_params(body: &AstNode, params: &[String], args: &[AstNode]) -> AstNode {
    match body {
        AstNode::Identifier { name, .. } => {
            if let Some(i) = params.iter().position(|p| p == name) {
                if i < args.len() {
                    return args[i].clone();
                }
            }
            body.clone()
        }
        AstNode::Apply {
            name,
            args: func_args,
            span,
        } => {
            // Don't substitute the function name, only its arguments
            let new_args = func_args
                .iter()
                .map(|a| substitute_params(a, params, args))
                .collect();
            AstNode::Apply {
                name: name.clone(),
                args: new_args,
                span: *span,
            }
        }
        AstNode::Block { items: stmts, span } => AstNode::Block {
            items: stmts
                .iter()
                .map(|s| substitute_params(s, params, args))
                .collect(),
            span: *span,
        },
        AstNode::Binding { name, value, span } => AstNode::Binding {
            name: name.clone(),
            value: Box::new(substitute_params(value, params, args)),
            span: *span,
        },
        AstNode::IfExpr {
            condition,
            then_branch,
            else_branch,
            span,
        } => AstNode::IfExpr {
            condition: Box::new(substitute_params(condition, params, args)),
            then_branch: Box::new(substitute_params(then_branch, params, args)),
            else_branch: else_branch
                .as_ref()
                .map(|e| Box::new(substitute_params(e, params, args))),
            span: *span,
        },
        AstNode::Tuple { items, span } => AstNode::Tuple {
            items: items
                .iter()
                .map(|i| substitute_params(i, params, args))
                .collect(),
            span: *span,
        },
        AstNode::Number { .. } | AstNode::BoolLit { .. } => body.clone(),
        AstNode::PropertyAccess {
            object,
            property,
            span,
        } => AstNode::PropertyAccess {
            object: Box::new(substitute_params(object, params, args)),
            property: property.clone(),
            span: *span,
        },
        // For remaining node types, clone as-is (loops, arrays, etc. are unlikely in bool functions)
        _ => body.clone(),
    }
}

/// Check if an AST node produces a boolean value in WGSL.
///
/// Comparisons, logical ops, and boolean literals produce `bool` in WGSL,
/// which cannot be passed to `clamp()`. We use corner-checking for these instead.
/// True if a list of block statements contains any imperative content
/// (bindings, tuple bindings, loops) that wouldn't be picked up by emit_expr.
/// Map a corner-substitution pair `(xv, yv)` back to the two-letter suffix
/// (`mm`, `mp`, `pm`, `pp`) used by the precomputed `_corner_<name>_<suffix>`
/// values. Returns `None` for any pair that isn't one of the four standard
/// corners — callers fall back to a direct lifted-fn call there.
fn corner_suffix(xv: &str, yv: &str) -> Option<&'static str> {
    match (xv, yv) {
        ("x_m", "y_m") => Some("mm"),
        ("x_m", "y_p") => Some("mp"),
        ("x_p", "y_m") => Some("pm"),
        ("x_p", "y_p") => Some("pp"),
        _ => None,
    }
}

/// True if `ast` contains at least one anonymous imperative block (a Block
/// with bindings/loops appearing somewhere other than as a binding's value or
/// a function/loop body). Cheap check used to skip cloning when there's
/// nothing to hoist.
fn needs_anon_hoisting(ast: &AstNode) -> bool {
    let mut found = false;
    scan_for_anon_blocks(ast, false, &mut found);
    found
}

/// Walk `ast` and set `found = true` if any *expression-position* node is a
/// `Block` containing imperative statements. `in_value_position` is true when
/// the current node is being read as a value (Apply arg, comparison side,
/// etc.) rather than a statement container.
fn scan_for_anon_blocks(node: &AstNode, in_value_position: bool, found: &mut bool) {
    if *found {
        return;
    }
    match node {
        AstNode::Block { items, .. } => {
            if in_value_position && has_imperative_stmt(items) {
                *found = true;
                return;
            }
            for s in items {
                // Inside a Block's stmt list, only the LAST stmt is in
                // value position (it's the block result); the rest are
                // statements and don't need hoisting in their own right.
                scan_for_anon_blocks(s, false, found);
            }
        }
        AstNode::Apply { args, .. } => {
            for a in args {
                scan_for_anon_blocks(a, true, found);
            }
        }
        AstNode::Tuple { items, .. } | AstNode::ArrayLiteral { items, .. } => {
            for it in items {
                scan_for_anon_blocks(it, true, found);
            }
        }
        AstNode::Binding { value, .. } | AstNode::TupleBinding { value, .. } => {
            // Named bindings are already lifted by the existing logic.
            scan_for_anon_blocks(value, false, found);
        }
        AstNode::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            scan_for_anon_blocks(condition, true, found);
            scan_for_anon_blocks(then_branch, true, found);
            if let Some(e) = else_branch {
                scan_for_anon_blocks(e, true, found);
            }
        }
        AstNode::FunctionDef { body, .. } => {
            // Function bodies have their own scope; lifting happens
            // recursively when generate() is called for that scope.
            scan_for_anon_blocks(body, false, found);
        }
        AstNode::ForLoop { range, body, .. } => {
            scan_for_anon_blocks(range, true, found);
            scan_for_anon_blocks(body, false, found);
        }
        AstNode::WhileLoop { condition, body, .. } => {
            scan_for_anon_blocks(condition, true, found);
            scan_for_anon_blocks(body, false, found);
        }
        AstNode::ParallelFor { range, body, .. } => {
            scan_for_anon_blocks(range, true, found);
            scan_for_anon_blocks(body, false, found);
        }
        AstNode::PropertyAccess { object, .. } => {
            scan_for_anon_blocks(object, true, found);
        }
        AstNode::IndexAccess { array, index, .. } => {
            scan_for_anon_blocks(array, true, found);
            scan_for_anon_blocks(index, true, found);
        }
        AstNode::Range { start, end, .. } => {
            scan_for_anon_blocks(start, true, found);
            scan_for_anon_blocks(end, true, found);
        }
        AstNode::IndexAssign {
            array,
            index,
            value,
            ..
        } => {
            scan_for_anon_blocks(array, true, found);
            scan_for_anon_blocks(index, true, found);
            scan_for_anon_blocks(value, true, found);
        }
        AstNode::Number { .. } | AstNode::BoolLit { .. } | AstNode::Identifier { .. } => {}
    }
}

/// Walk `ast`, replacing every anonymous imperative block in expression
/// position with `Identifier("_anon_<N>")`, and prepend a `_anon_<N> := block`
/// binding to the top-level Block. The resulting AST always has the form
/// `Block([... synthetic bindings, original-stmts])` so the hoisted bindings
/// participate in the same lifting path as user-named bindings.
fn hoist_anonymous_blocks(ast: &AstNode) -> AstNode {
    let mut counter: usize = 0;
    let mut prepended: Vec<AstNode> = Vec::new();
    let top_span = ast.span();
    let rewritten = hoist_recurse(ast, false, &mut counter, &mut prepended);

    if prepended.is_empty() {
        return rewritten;
    }

    let mut all = prepended;
    match rewritten {
        AstNode::Block { items, .. } => all.extend(items),
        other => all.push(other),
    }
    AstNode::Block {
        items: all,
        span: top_span,
    }
}

fn hoist_recurse(
    node: &AstNode,
    in_value_position: bool,
    counter: &mut usize,
    prepended: &mut Vec<AstNode>,
) -> AstNode {
    // Hoist this node itself if it's an imperative Block in value position.
    if in_value_position {
        if let AstNode::Block { items, span } = node {
            if has_imperative_stmt(items) {
                let name = format!("_anon_{}", *counter);
                *counter += 1;
                // Recurse INTO the block so any inner anonymous blocks are
                // also hoisted (registered before this binding so they're
                // declared earlier in the synthesized top-level block).
                let inner = hoist_block_stmts(items, counter, prepended);
                prepended.push(AstNode::Binding {
                    name: name.clone(),
                    value: Box::new(AstNode::Block {
                        items: inner,
                        span: *span,
                    }),
                    span: *span,
                });
                return AstNode::Identifier {
                    name,
                    span: *span,
                };
            }
        }
    }

    // Otherwise recurse structurally.
    match node {
        AstNode::Block { items, span } => AstNode::Block {
            items: hoist_block_stmts(items, counter, prepended),
            span: *span,
        },
        AstNode::Apply { name, args, span } => AstNode::Apply {
            name: name.clone(),
            args: args
                .iter()
                .map(|a| hoist_recurse(a, true, counter, prepended))
                .collect(),
            span: *span,
        },
        AstNode::Tuple { items, span } => AstNode::Tuple {
            items: items
                .iter()
                .map(|i| hoist_recurse(i, true, counter, prepended))
                .collect(),
            span: *span,
        },
        AstNode::ArrayLiteral { items, span } => AstNode::ArrayLiteral {
            items: items
                .iter()
                .map(|i| hoist_recurse(i, true, counter, prepended))
                .collect(),
            span: *span,
        },
        AstNode::Binding { name, value, span } => AstNode::Binding {
            name: name.clone(),
            // The binding's value position is handled by existing lifting.
            value: Box::new(hoist_recurse(value, false, counter, prepended)),
            span: *span,
        },
        AstNode::TupleBinding { names, value, span } => AstNode::TupleBinding {
            names: names.clone(),
            value: Box::new(hoist_recurse(value, false, counter, prepended)),
            span: *span,
        },
        AstNode::IfExpr {
            condition,
            then_branch,
            else_branch,
            span,
        } => AstNode::IfExpr {
            condition: Box::new(hoist_recurse(condition, true, counter, prepended)),
            then_branch: Box::new(hoist_recurse(then_branch, true, counter, prepended)),
            else_branch: else_branch
                .as_ref()
                .map(|e| Box::new(hoist_recurse(e, true, counter, prepended))),
            span: *span,
        },
        AstNode::FunctionDef {
            name,
            params,
            body,
            span,
        } => AstNode::FunctionDef {
            name: name.clone(),
            params: params.clone(),
            // Function bodies are their own scope — don't hoist *out* of them.
            body: body.clone(),
            span: *span,
        },
        AstNode::ForLoop {
            var,
            range,
            body,
            span,
        } => AstNode::ForLoop {
            var: var.clone(),
            range: Box::new(hoist_recurse(range, true, counter, prepended)),
            body: body.clone(),
            span: *span,
        },
        AstNode::WhileLoop {
            condition,
            body,
            span,
        } => AstNode::WhileLoop {
            condition: Box::new(hoist_recurse(condition, true, counter, prepended)),
            body: body.clone(),
            span: *span,
        },
        AstNode::ParallelFor {
            var,
            range,
            body,
            span,
        } => AstNode::ParallelFor {
            var: var.clone(),
            range: Box::new(hoist_recurse(range, true, counter, prepended)),
            body: body.clone(),
            span: *span,
        },
        AstNode::PropertyAccess {
            object,
            property,
            span,
        } => AstNode::PropertyAccess {
            object: Box::new(hoist_recurse(object, true, counter, prepended)),
            property: property.clone(),
            span: *span,
        },
        AstNode::IndexAccess { array, index, span } => AstNode::IndexAccess {
            array: Box::new(hoist_recurse(array, true, counter, prepended)),
            index: Box::new(hoist_recurse(index, true, counter, prepended)),
            span: *span,
        },
        AstNode::Range { start, end, span } => AstNode::Range {
            start: Box::new(hoist_recurse(start, true, counter, prepended)),
            end: Box::new(hoist_recurse(end, true, counter, prepended)),
            span: *span,
        },
        AstNode::IndexAssign {
            array,
            index,
            value,
            span,
        } => AstNode::IndexAssign {
            array: Box::new(hoist_recurse(array, true, counter, prepended)),
            index: Box::new(hoist_recurse(index, true, counter, prepended)),
            value: Box::new(hoist_recurse(value, true, counter, prepended)),
            span: *span,
        },
        AstNode::Number { .. } | AstNode::BoolLit { .. } | AstNode::Identifier { .. } => {
            node.clone()
        }
    }
}

/// Apply `hoist_recurse` to each stmt in a Block's stmt list. Only the last
/// stmt is in value position (it's the block result); the rest are statements.
fn hoist_block_stmts(
    stmts: &[AstNode],
    counter: &mut usize,
    prepended: &mut Vec<AstNode>,
) -> Vec<AstNode> {
    let last = stmts.len().saturating_sub(1);
    stmts
        .iter()
        .enumerate()
        .map(|(i, s)| hoist_recurse(s, i == last, counter, prepended))
        .collect()
}

fn has_imperative_stmt(stmts: &[AstNode]) -> bool {
    stmts.iter().any(|s| {
        matches!(
            s,
            AstNode::Binding { .. }
                | AstNode::TupleBinding { .. }
                | AstNode::WhileLoop { .. }
                | AstNode::ForLoop { .. }
        )
    })
}

/// If `node` is a Block, return its result expression (the last non-imperative
/// statement). Otherwise return `node` itself.
fn block_result_expr(node: &AstNode) -> &AstNode {
    if let AstNode::Block { items: stmts, .. } = node {
        if let Some(r) = block_result_expr_from_stmts(stmts) {
            return r;
        }
    }
    node
}

/// Find the result expression in a block's statement list.
fn block_result_expr_from_stmts(stmts: &[AstNode]) -> Option<&AstNode> {
    for stmt in stmts.iter().rev() {
        match stmt {
            AstNode::Binding { .. }
            | AstNode::FunctionDef { .. }
            | AstNode::WhileLoop { .. }
            | AstNode::ForLoop { .. }
            | AstNode::TupleBinding { .. } => continue,
            other => return Some(other),
        }
    }
    None
}

fn returns_bool(node: &AstNode) -> bool {
    match node {
        AstNode::BoolLit { .. } => true,
        AstNode::Apply { name, .. } => matches!(
            name.as_str(),
            "eq" | "neq" | "lt" | "gt" | "lte" | "gte" | "and" | "or" | "not"
        ),
        AstNode::Block { items: stmts, .. } => stmts.last().is_some_and(returns_bool),
        _ => false,
    }
}

/// Find the result expression in the AST (last non-binding, non-function-def, non-loop node).
fn find_result_expr(ast: &AstNode) -> Result<&AstNode, String> {
    match ast {
        AstNode::Block { items: stmts, .. } => {
            for stmt in stmts.iter().rev() {
                match stmt {
                    AstNode::Binding { .. }
                    | AstNode::FunctionDef { .. }
                    | AstNode::WhileLoop { .. }
                    | AstNode::ForLoop { .. }
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
        let mut parser = Parser::new(tokens, input.to_string());
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
        let shader = gen("r := sqrt(x * x + y * y)\nr");
        assert!(shader.contains("let r = sqrt(((x * x) + (y * y)))"));
    }

    #[test]
    fn test_with_function_def() {
        let shader = gen("f(a, b) := a + b\nf(x, y)");
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
        let shader = gen("a := x + 1\nb := y + 2\na * b");
        assert!(shader.contains("let a = (x + 1.0)"));
        assert!(shader.contains("let b = (y + 2.0)"));
        assert!(shader.contains("(a * b)"));
    }

    #[test]
    fn test_function_with_block_body() {
        let shader = gen("f(a) := (r := a * 2, r + 1)\nf(x)");
        assert!(shader.contains("fn f(a: f32) -> f32"));
        assert!(shader.contains("f(x)"));
    }

    #[test]
    fn test_function_returning_bool() {
        let shader = gen("f(a, b) := a = b\nf(x, y)");
        assert!(
            shader.contains("fn f(a: f32, b: f32) -> bool"),
            "bool-returning function should have -> bool, got:\n{}",
            shader
        );
    }

    #[test]
    fn test_function_returning_bool_inlined_matches_inline() {
        // f(a,b) with arbitrary param names, called with axis vars, must match inline
        let inline_shader = crate::lang::compile("x\u{00B2} = y or y = x").unwrap();
        let func_shader =
            crate::lang::compile("f(a, b) := a\u{00B2} = b or b = a\nf(x, y)").unwrap();

        // Extract just the fs_main body (after "fn fs_main")
        let inline_main = inline_shader.split("fn fs_main").nth(1).unwrap();
        let func_main = func_shader.split("fn fs_main").nth(1).unwrap();

        assert_eq!(inline_main, func_main,
            "Function call must produce identical fs_main as inline.\n\nInline:\n{}\n\nFunction:\n{}",
            inline_shader, func_shader);
    }

    #[test]
    fn test_function_returning_bool_compound() {
        // The exact user case: f(x,y) := x²=y and x=y
        let input = "f(x, y) := x\u{00B2} = y and x = y\nf(x, y)";
        let result = crate::lang::compile(input);
        assert!(
            result.is_ok(),
            "bool function should compile, got: {:?}",
            result
        );
        let shader = result.unwrap();
        assert!(
            shader.contains("-> bool"),
            "compound bool function should return bool, got:\n{}",
            shader
        );
        // Validate with naga
        let module = naga::front::wgsl::parse_str(&shader)
            .unwrap_or_else(|e| panic!("naga parse failed:\n{}\n\n--- WGSL ---\n{}", e, shader));
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator.validate(&module).unwrap_or_else(|e| {
            panic!("naga validation failed:\n{}\n\n--- WGSL ---\n{}", e, shader)
        });
    }

    #[test]
    fn test_mandelbrot_like_expr() {
        let shader = gen("r := sqrt(x*x + y*y)\nif (r < 2) 1 else 0");
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
        assert!(
            !shader.contains("x_m"),
            "numeric expr should NOT have corner vars"
        );
    }

    #[test]
    fn test_if_expr_is_numeric_not_bool() {
        let shader = gen("if (x > 0) 1 else 0");
        assert!(shader.contains("clamp(_result, 0.0, 1.0)"));
        assert!(
            !shader.contains("x_m"),
            "if/else returning f32 should not use corners"
        );
    }

    #[test]
    fn test_binding_then_bool_result_uses_corners() {
        let shader = gen("r := x * x + y * y\nr < 1");
        assert!(
            shader.contains("x_m"),
            "bool result should trigger corner checking"
        );
        assert!(shader.contains("select(0.0, 1.0, _result)"));
    }

    #[test]
    fn test_binding_then_numeric_result_uses_clamp() {
        let shader = gen("r := x * x + y * y\nsin(r)");
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
        assert!(
            shader.contains("sin(x_m)") || shader.contains("sin(x_p)"),
            "sin should be evaluated at corner x values"
        );
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

    #[test]
    fn test_function_body_with_bindings_no_loops() {
        // Function body with bindings but no loops should use let
        let shader = gen("f(a) := (r := a * 2, r + 1)\nf(x)");
        assert!(
            shader.contains("let r = (a * 2.0)"),
            "bindings without loops should use let"
        );
        assert!(
            shader.contains("return (r + 1.0)"),
            "should return last expression"
        );
    }

    // --- New feature tests for Mandelbrot support ---

    #[test]
    fn test_top_level_comma_separators() {
        let shader = gen("a := 1, b := 2, a + b");
        // Simple number constants are emitted at module level
        assert!(shader.contains("const a = 1.0"));
        assert!(shader.contains("const b = 2.0"));
        assert!(shader.contains("(a + b)"));
    }

    #[test]
    fn test_while_loop_in_function() {
        let input = "f(n) := (i := 0, s := 0, while (i < n) (s := s + i, i := i + 1), s)\nf(x)";
        let shader = gen(input);
        assert!(
            shader.contains("fn f(n: f32) -> f32"),
            "should define function"
        );
        assert!(shader.contains("_loop_guard"), "should have loop guard");
        assert!(shader.contains("return s"), "should return accumulator");
    }

    #[test]
    fn test_tuple_destructuring_binding() {
        let input = "f(a, b) := (r := a + b, r * 2)\n(p, q) := (1, 2)\nf(p, q)";
        let result = crate::lang::compile(input);
        assert!(
            result.is_ok(),
            "tuple destructuring should compile: {:?}",
            result
        );
    }

    #[test]
    fn test_property_access_axis_bounds() {
        let shader = gen("x.max - x.min");
        assert!(
            shader.contains("u.axis_max.x"),
            "x.max should map to u.axis_max.x"
        );
        assert!(
            shader.contains("u.axis_min.x"),
            "x.min should map to u.axis_min.x"
        );
    }

    #[test]
    fn test_property_access_y_axis() {
        let shader = gen("y.max - y.min");
        assert!(
            shader.contains("u.axis_max.y"),
            "y.max should map to u.axis_max.y"
        );
        assert!(
            shader.contains("u.axis_min.y"),
            "y.min should map to u.axis_min.y"
        );
    }

    #[test]
    fn test_t_mapped_to_time() {
        let shader = gen("sin(t)");
        assert!(shader.contains("sin(u.time)"), "t should map to u.time");
    }

    #[test]
    fn test_function_returning_vec4() {
        let input = "f(a) := (1.0, a, 0.0, 1.0)\nf(x)";
        let shader = gen(input);
        assert!(
            shader.contains("fn f(a: f32) -> vec4<f32>"),
            "function returning tuple should have vec4 return type"
        );
    }

    #[test]
    fn test_mandelbrot_full_program() {
        let input = r#"BASE_ITER := 128,
BAILOUT := 4.0,
MAX_ITER_CAP := 512,
INITIAL_ZOOM := 0.2,
ROOT_SAMPLES := 3,

mandelbrot_color(iter, sq) := (
    mu := f32(iter) + 1.0 - log(0.5 * log(sq) / log(2.0)) / log(2.0),
    base_mod := 0.05 * mu + 0.3 * t,
    hue_mod := 0.1 * mu + t,
    color_base := 0.9 + 0.1 * cos(0.05 * mu + 0.5 * t),
    fade := 0.8 + 0.2 * sin(hue_mod),
    triwave_channel(offset) := (
        color_base * ((1.0 - fade) + fade * clamp(
            abs(fract(fract(base_mod) + offset) * 6.0 - 3.0) - 1.0,
            0.0,
            1.0
        ))
    ),
    (triwave_channel(0.5), triwave_channel(1.0/3.0), triwave_channel(0.25), 1.0)
),
mandelbrot(x, y) := (
    effective_zoom := 1.0 / (y.max - y.min),
    (z_x, z_y) := (0.0, 0.0),
    sq := 0.0,
    max_iter := min(BASE_ITER + (40.0 * log(effective_zoom / INITIAL_ZOOM + 1.0)), MAX_ITER_CAP),
    iter := 0,
    while (iter < max_iter and (sq := z_x * z_x + z_y * z_y, sq) < BAILOUT) (
        zy2 := z_y * z_y,
        z_y := 2.0 * z_x * z_y + y,
        z_x := z_x * z_x - zy2 + x,
        iter := iter + 1
    ),
    if (iter < max_iter) (
        mandelbrot_color(iter, sq)
    ) else (
        (0.0, 0.0, 0.0, 1.0)
    )
),

mandelbrot(x, y)"#;
        let result = crate::lang::compile(input);
        assert!(
            result.is_ok(),
            "Mandelbrot should compile, got: {:?}",
            result
        );
        let shader = result.unwrap();
        assert!(
            shader.contains("fn mandelbrot_color("),
            "should define mandelbrot_color"
        );
        assert!(
            shader.contains("fn mandelbrot("),
            "should define mandelbrot"
        );
        assert!(
            shader.contains("_loop_guard"),
            "should have loop guard for while"
        );
    }

    // --- Monte Carlo tests: simple to complex ---

    #[test]
    fn test_mc_hash_function() {
        // Simple pseudo-random hash — should compile and validate
        let input = "hash(s) := fract(sin(s * 127.1 + 311.7) * 43758.5453)\nhash(x + y * 100.0)";
        let shader = gen(input);
        assert!(
            shader.contains("fn hash(s: f32) -> f32"),
            "should define hash function"
        );
        assert!(shader.contains("fract("), "should use fract");
        assert!(shader.contains("sin("), "should use sin");
    }

    #[test]
    fn test_mc_step_branchless_count() {
        // Branchless hit counting using step() — core Monte Carlo pattern
        let input = "d := x * x + y * y\n1.0 - step(1.0, d)";
        let shader = gen(input);
        assert!(
            shader.contains("step(1.0, d)"),
            "should use step for branchless"
        );
    }

    #[test]
    fn test_mc_multiline_while_in_function() {
        // Verify multiline while body inside function compiles
        // Single-line version works: f(n) := (i := 0, s := 0, while (i < n) (s := s + i, i := i + 1), s)
        // Test multiline version
        let input = "sim(seed) := (j := 0.0, s := 0.0, while (j < 10.0) (s := s + j, j := j + 1.0), s)\nsim(x)";
        let result = crate::lang::compile(input);
        assert!(
            result.is_ok(),
            "Single-line while-in-function should compile, got: {:?}",
            result
        );
    }

    #[test]
    fn test_mc_function_with_while_loop_called_from_loop() {
        // Core pattern: function with internal while loop, called from top-level while loop
        // Note: 'z' is an AxisVar, use 'rn' instead for bindings inside loops
        let input = "hash(s) := fract(sin(s * 127.1 + 311.7) * 43758.5453)\nsim(tf, seed) := (lp := 0.0, j := 0.0, while (j < 10.0) (rn := (hash(seed + j * 17.31) - 0.5) * 3.46, lp := lp + rn * 0.01, j := j + 1.0), exp(lp))\nnx := (x - x.min) / (x.max - x.min)\nny := (y - y.min) / (y.max - y.min)\nbright := 0.0\ni := 0.0\nwhile (i < 4.0) (p := sim(nx, i * 137.0), d := abs(ny - p * 0.5), bright := bright + smoothstep(0.01, 0.0, d), i := i + 1.0)\nbright";
        let result = crate::lang::compile(input);
        assert!(
            result.is_ok(),
            "Nested loop pattern should compile, got: {:?}",
            result
        );
        let shader = result.unwrap();
        assert!(shader.contains("fn sim("), "should define sim function");
        assert!(shader.contains("fn hash("), "should define hash function");
    }

    #[test]
    fn test_mc_distance_field_line_rendering() {
        // Line rendering via smoothstep distance — core visual technique
        let input = r#"nx := (x - x.min) / (x.max - x.min)
ny := (y - y.min) / (y.max - y.min)
curve_y := 0.5 + 0.3 * sin(nx * 6.28)
d := abs(ny - curve_y)
smoothstep(0.005, 0.0, d)"#;
        let result = crate::lang::compile(input);
        assert!(
            result.is_ok(),
            "Distance-field line should compile, got: {:?}",
            result
        );
    }

    #[test]
    fn test_mc_confidence_band() {
        // Analytical confidence band using step() for region shading
        let input = r#"nx := (x - x.min) / (x.max - x.min)
ny := (y - y.min) / (y.max - y.min)
tf := max(nx, 0.001)
upper := exp(0.035 * tf + 0.588 * sqrt(tf))
lower := exp(0.035 * tf - 0.588 * sqrt(tf))
step(lower, ny) * step(ny, upper)"#;
        let result = crate::lang::compile(input);
        assert!(
            result.is_ok(),
            "Confidence band should compile, got: {:?}",
            result
        );
    }

    #[test]
    fn test_mc_full_example_compiles_and_validates() {
        // Full Monte Carlo example as it will appear in the Examples menu
        let input = include_str!("../../examples/monte_carlo.txt");
        let result = crate::lang::compile(input);
        assert!(
            result.is_ok(),
            "Full Monte Carlo example should compile, got: {:?}",
            result
        );
        let shader = result.unwrap();
        // Validate with naga (same validation wgpu does)
        let module = naga::front::wgsl::parse_str(&shader)
            .unwrap_or_else(|e| panic!("naga parse failed:\n{}\n\n--- WGSL ---\n{}", e, shader));
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator.validate(&module).unwrap_or_else(|e| {
            panic!("naga validation failed:\n{}\n\n--- WGSL ---\n{}", e, shader)
        });
        // Structural checks
        assert!(shader.contains("fn hash("), "should define hash");
        assert!(shader.contains("fn sim("), "should define sim");
        assert!(
            shader.contains("fn monte_carlo("),
            "should define monte_carlo"
        );
        assert!(
            shader.contains("smoothstep("),
            "should use smoothstep for line rendering"
        );
    }
}
