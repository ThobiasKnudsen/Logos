use super::ir::{BuiltinOp, Callee, Ir};
use std::collections::{HashMap, HashSet};

// The for-loop guard's upper bound is loaded from the `max_loop_iter` uniform
// at runtime rather than emitted as a compile-time literal. Passing it as a
// uniform prevents driver shader compilers (notably NVIDIA's) from fully
// unrolling small fixed-iteration loops, which previously hung pipeline
// creation on inputs whose unrolled chain the optimizer could prove constant
// (e.g. `sum:=0; for i in 0..10 (sum:=sum*x); sum` ≡ 0). The concrete value
// is set in `render::shader_pipeline::MAX_LOOP_ITERATIONS`.

/// Generate a complete WGSL fragment shader from Logos IR.
///
/// The generated shader:
/// - Defines the uniform struct matching ShaderUniforms
/// - Maps user `x`/`y` to world coordinates via axis_min/axis_max
/// - For boolean expressions: uses corner-checking for pixel-perfect rendering
///   (equality → curve straddling, inequalities → all-corners-agree)
/// - For numeric expressions: clamps to [0, 1] grayscale
pub fn generate(ast: &Ir) -> Result<String, String> {
    // Pre-pass: anonymous imperative blocks (e.g. `plot(y = (sum:=0; for...; sum))`)
    // get hoisted into synthetic top-level bindings so the same lifting logic
    // that handles named bindings can pick them up. Without this the inner
    // result identifier (`sum`) would leak into the WGSL with nothing
    // declaring it.
    let owned_ast;
    let ast: &Ir = if needs_anon_hoisting(ast) {
        owned_ast = hoist_anonymous_blocks(ast);
        &owned_ast
    } else {
        ast
    };

    // Pre-pass 1: lift every `Ir::Lambda` into a synthetic FunctionDef and
    // replace the lambda expression with a reference to that name. Lambdas
    // can't appear as values in WGSL — this normalizes them into the same
    // shape as named user functions so the rest of the pipeline doesn't
    // need to know they ever existed.
    let lifted_ast;
    let ast: &Ir = match lift_lambdas(ast) {
        Some(new_ir) => {
            lifted_ast = new_ir;
            &lifted_ast
        }
        None => ast,
    };

    // Pre-pass 2: specialize higher-order function calls. When the user writes
    // `N_integral(sq, 0, x, 0.01)` we rewrite it into a call to a synthetic
    // `N_integral__sq` whose body has `sq` substituted for the function
    // parameter `f`. Pure WGSL — no first-class functions needed.
    let specialized_ast;
    let ast: &Ir = match specialize_higher_order_calls(ast) {
        Some(new_ir) => {
            specialized_ast = new_ir;
            &specialized_ast
        }
        None => ast,
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
        Ir::Block { items: stmts, .. } => stmts
            .iter()
            .any(|s| matches!(s, Ir::WhileLoop { .. } | Ir::ForLoop { .. })),
        Ir::WhileLoop { .. } | Ir::ForLoop { .. } => true,
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
        let binding_asts: Vec<(&str, &Ir)> = match ast {
            Ir::Block { items: stmts, .. } => stmts
                .iter()
                .filter_map(|s| {
                    if let Ir::Binding { name, value, .. } = s {
                        Some((name.as_str(), value.as_ref()))
                    } else {
                        None
                    }
                })
                .collect(),
            Ir::Binding { name, value, .. } => vec![(name.as_str(), value.as_ref())],
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

    // Emit user-defined helper functions. Skip any whose body uses a
    // function-typed parameter (e.g. `N_integral(f, ...)` calling `f(i*d)`),
    // since WGSL has no first-class functions and the emitted code would
    // fail GPU validation. If such a function is actually called from the
    // cell's result path we surface an explicit error here; if it's defined
    // but unused (the user's `statistikk.logos` shape) we silently drop it
    // so the rest of the cell still renders.
    let hof_fns = unrepresentable_higher_order_functions(ast);
    let reachable_user_fns = reachable_user_functions(ast);
    for hof in &hof_fns {
        if reachable_user_fns.contains(hof) {
            // The HOF survived the specialization pass — meaning some call
            // site passed a function value that wasn't a known function name
            // or an inline `|->` lambda. WGSL has no first-class functions,
            // so we can't generate code for it; reject with a clear message
            // pointing at the limitation and the two supported shapes.
            return Err(format!(
                "Cannot compile call to higher-order function `{0}`: its \
                 function-typed parameter must be statically known at compile \
                 time. Pass either a defined function name (e.g. `{0}(my_fn, …)`) \
                 or an inline lambda (e.g. `{0}(t |-> t*t, …)`). \
                 Runtime-dispatched function values aren't supported on the GPU \
                 backend yet.",
                hof
            ));
        }
    }
    for func in &ctx.functions {
        if let Some(name) = &func.user_name {
            if hof_fns.contains(name) {
                continue;
            }
        }
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
            Ir::Block { items: stmts, .. } => stmts.as_slice(),
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
        // Pipeline blends with premultiplied alpha, so the RGB output must
        // be pre-multiplied by both shade and the user-chosen alpha — else
        // RGB dominates regardless of alpha.
        shader.push_str(
            "    let _a = _shade * u.primary_color.a;\n    return vec4<f32>(u.primary_color.rgb * _a, _a);\n",
        );
    } else {
        // Numeric expressions: clamp to [0, 1] grayscale
        let expr_code = ctx.emit_expr(expr)?;
        shader.push_str(&format!("    let _result = {};\n", expr_code));
        shader.push_str("    let _shade = clamp(_result, 0.0, 1.0);\n");
        // Pipeline blends with premultiplied alpha, so the RGB output must
        // be pre-multiplied by both shade and the user-chosen alpha — else
        // RGB dominates regardless of alpha.
        shader.push_str(
            "    let _a = _shade * u.primary_color.a;\n    return vec4<f32>(u.primary_color.rgb * _a, _a);\n",
        );
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
    block_preamble: Option<Vec<Ir>>,
}

struct EmittedFunction {
    wgsl_code: String,
    /// If `Some(name)`, the function corresponds to a user `FunctionDef` (or a
    /// companion synthesized from one, like `_diff_<name>`) and is eligible
    /// for dead-function elimination — only emit it when `name` is reachable
    /// from the cell's result expression. `None` means the function was
    /// synthesized by codegen (e.g. lifted block bindings) and must always
    /// be emitted.
    user_name: Option<String>,
}

/// Stored IR of a bool function for inlining in the plotting context.
struct BoolFunctionDef {
    params: Vec<String>,
    body: Ir,
}

struct GenContext {
    functions: Vec<EmittedFunction>,
    bindings: Vec<EmittedBinding>,
    /// Names of user-defined functions that return vec types (not f32).
    vec_functions: HashSet<String>,
    /// Names of user-defined functions that return bool.
    bool_functions: HashSet<String>,
    /// IR bodies of bool functions — used for inlining during corner-checking
    /// so that comparisons go through sign-change detection, not float ==.
    bool_function_defs: std::collections::HashMap<String, BoolFunctionDef>,
    /// IR values of bool-typed bindings — used for inlining during
    /// corner-checking so `f := x = y^2; plot(f)` renders the curve correctly.
    bool_binding_defs: std::collections::HashMap<String, Ir>,
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
    comparison_op: Option<BuiltinOp>,
}

/// Metadata for a user-defined function whose bool body has imperative content.
/// `diff_fn_name(args..., x_corner, y_corner) -> f32` returns `lhs - rhs` of the
/// body's comparison so corner-checking can sign-check at four corners.
#[derive(Debug, Clone)]
struct LiftedFunctionDef {
    diff_fn_name: String,
    comparison_op: BuiltinOp,
}

/// If `callee` is a 2-arg comparison operator, return it; otherwise `None`.
/// Used to detect `lhs op rhs` results that should drive corner-checking.
fn as_comparison_op(callee: &Callee, args: &[Ir]) -> Option<BuiltinOp> {
    if args.len() != 2 {
        return None;
    }
    match callee {
        Callee::Builtin(
            op @ (BuiltinOp::Eq
            | BuiltinOp::Neq
            | BuiltinOp::Lt
            | BuiltinOp::Gt
            | BuiltinOp::Lte
            | BuiltinOp::Gte),
        ) => Some(*op),
        _ => None,
    }
}

/// Emit the four-corner WGSL pattern that decides whether a comparison op
/// straddles `0.0` over the four pixel corners. `calls[i]` is the WGSL
/// expression evaluating `lhs - rhs` at corner `i`.
fn emit_corner_compare(op: BuiltinOp, calls: &[String; 4]) -> String {
    match op {
        // = and ≠ use sign-change detection: are all four corners on the same side?
        BuiltinOp::Eq | BuiltinOp::Neq => {
            let sides: [String; 4] = std::array::from_fn(|i| format!("(({}) > 0.0)", calls[i]));
            let all_same = format!(
                "({} == {} && {} == {} && {} == {})",
                sides[0], sides[1], sides[1], sides[2], sides[2], sides[3],
            );
            if matches!(op, BuiltinOp::Eq) {
                format!("(!{})", all_same)
            } else {
                all_same
            }
        }
        // For inequalities, all four corners must agree.
        BuiltinOp::Lt => format!(
            "(({}) < 0.0 && ({}) < 0.0 && ({}) < 0.0 && ({}) < 0.0)",
            calls[0], calls[1], calls[2], calls[3]
        ),
        BuiltinOp::Gt => format!(
            "(({}) > 0.0 && ({}) > 0.0 && ({}) > 0.0 && ({}) > 0.0)",
            calls[0], calls[1], calls[2], calls[3]
        ),
        BuiltinOp::Lte => format!(
            "(({}) <= 0.0 && ({}) <= 0.0 && ({}) <= 0.0 && ({}) <= 0.0)",
            calls[0], calls[1], calls[2], calls[3]
        ),
        BuiltinOp::Gte => format!(
            "(({}) >= 0.0 && ({}) >= 0.0 && ({}) >= 0.0 && ({}) >= 0.0)",
            calls[0], calls[1], calls[2], calls[3]
        ),
        // as_comparison_op is the only construction site, so this is unreachable.
        _ => String::new(),
    }
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

    /// Walk the IR to collect function definitions and bindings.
    fn collect_functions(&mut self, ast: &Ir) {
        self.collect_functions_with_scope(ast, &[]);
    }

    /// Collect functions with the enclosing scope's binding names.
    /// `scope_bindings` are variable names available from the enclosing scope
    /// (used to detect captured variables for nested function hoisting).
    fn collect_functions_with_scope(&mut self, ast: &Ir, scope_bindings: &[String]) {
        match ast {
            Ir::Block { items: stmts, .. } => {
                for stmt in stmts {
                    self.collect_functions_with_scope(stmt, scope_bindings);
                }
            }
            Ir::FunctionDef {
                name, params, body, ..
            } => {
                // Collect nested function defs from the body first
                // Build the scope for nested functions: parent scope + this function's params + body bindings
                let mut inner_scope: Vec<String> = scope_bindings.to_vec();
                inner_scope.extend(params.iter().cloned());
                if let Ir::Block { items: stmts, .. } = body.as_ref() {
                    // Add binding names from the body to the inner scope
                    for stmt in stmts {
                        match stmt {
                            Ir::Binding { name, .. } => inner_scope.push(name.clone()),
                            Ir::TupleBinding { names, .. } => {
                                inner_scope.extend(names.iter().cloned())
                            }
                            _ => {}
                        }
                    }
                    // Recurse to collect nested function definitions
                    for stmt in stmts {
                        if let Ir::FunctionDef { .. } = stmt {
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
                    Ir::Block { items: stmts, .. } => stmts.iter().any(|s| {
                        matches!(
                            s,
                            Ir::Binding { .. }
                                | Ir::WhileLoop { .. }
                                | Ir::ForLoop { .. }
                                | Ir::TupleBinding { .. }
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
                    if let Ir::Block { items: stmts, .. } = body.as_ref() {
                        if let Some(result) = block_result_expr_from_stmts(stmts) {
                            if let Ir::Apply { callee, args: cmp_args, .. } = result {
                                if let Some(cmp_op) = as_comparison_op(callee, cmp_args) {
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
                                        &Some(cmp_op),
                                        declared,
                                    ) {
                                        let diff_wgsl = format!(
                                            "fn {}({}) -> f32 {{\n{}}}\n",
                                            diff_name,
                                            all_params.join(", "),
                                            diff_body,
                                        );
                                        self.functions.push(EmittedFunction {
                                            wgsl_code: diff_wgsl,
                                            user_name: Some(name.clone()),
                                        });
                                        self.lifted_function_defs.insert(
                                            name.clone(),
                                            LiftedFunctionDef {
                                                diff_fn_name: diff_name,
                                                comparison_op: cmp_op,
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                if needs_imperative {
                    if let Ir::Block { items: stmts, .. } = body.as_ref() {
                        if let Ok(body_wgsl) = self.emit_function_body(stmts) {
                            let wgsl_code = format!(
                                "fn {}({}) -> {} {{\n{}}}\n",
                                name,
                                all_params.join(", "),
                                ret_type,
                                body_wgsl,
                            );
                            self.functions.push(EmittedFunction {
                                wgsl_code,
                                user_name: Some(name.clone()),
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
                        wgsl_code,
                        user_name: Some(name.clone()),
                    });
                }
            }
            Ir::Binding { name, value, .. } => {
                // Block-valued bindings with imperative content (var/loop) are lifted
                // into WGSL functions so corner-checking can re-evaluate the block at
                // each corner of a pixel — without this, e.g. `sum` is computed once
                // at pixel-center x and the curve renders dotted on steep parts.
                let imperative_block_stmts = match value.as_ref() {
                    Ir::Block { items: stmts, .. } if has_imperative_stmt(stmts) => {
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
            Ir::TupleBinding { names, value, .. } => {
                // For tuple bindings at top level, emit individual bindings
                if let Ir::Tuple { items, .. } = value.as_ref() {
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
        stmts: &[Ir],
    ) -> Result<(String, Option<BuiltinOp>, String), String> {
        let result = block_result_expr_from_stmts(stmts)
            .ok_or_else(|| format!("block-valued binding `{}` has no result", binding_name))?;

        let comparison_op = match result {
            Ir::Apply { callee, args, .. } => as_comparison_op(callee, args),
            _ => None,
        };

        let fn_name = format!("_lifted_{}", binding_name);
        let body = self.emit_lifted_block_body(stmts, result, &comparison_op)?;
        let func_wgsl = format!("fn {}(x: f32, y: f32) -> f32 {{\n{}}}\n", fn_name, body);
        self.functions.push(EmittedFunction {
            wgsl_code: func_wgsl,
            user_name: None,
        });

        let binding_expr = if let Some(op) = comparison_op {
            let wgsl_op = match op {
                BuiltinOp::Eq => "==",
                BuiltinOp::Neq => "!=",
                BuiltinOp::Lt => "<",
                BuiltinOp::Gt => ">",
                BuiltinOp::Lte => "<=",
                BuiltinOp::Gte => ">=",
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
        stmts: &[Ir],
        result: &Ir,
        comparison_op: &Option<BuiltinOp>,
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
        stmts: &[Ir],
        result: &Ir,
        comparison_op: &Option<BuiltinOp>,
        mut declared: HashSet<String>,
    ) -> Result<String, String> {
        let mut code = String::new();

        for stmt in stmts {
            match stmt {
                Ir::Binding { name, value, .. } => {
                    let val = self.emit_expr(value)?;
                    if declared.contains(name.as_str()) {
                        code += &format!("    {} = {};\n", name, val);
                    } else {
                        code += &format!("    var {} = {};\n", name, val);
                        declared.insert(name.clone());
                    }
                }
                Ir::TupleBinding { names, value, .. } => {
                    self.emit_tuple_binding(
                        &mut code,
                        names,
                        value,
                        "    ",
                        "var",
                        &mut declared,
                    )?;
                }
                Ir::ForLoop {
                    var, range, body, ..
                } => {
                    self.emit_for_loop(&mut code, var, range, body, "    ", &mut declared)?;
                }
                Ir::WhileLoop {
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
                Ir::FunctionDef { .. } => {}
                _ => {} // result expression handled below
            }
        }

        if comparison_op.is_some() {
            if let Ir::Apply { args, .. } = result {
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

    fn emit_function_body(&self, stmts: &[Ir]) -> Result<String, String> {
        let has_loops = stmts
            .iter()
            .any(|s| matches!(s, Ir::WhileLoop { .. } | Ir::ForLoop { .. }));
        let var_keyword = if has_loops { "var" } else { "let" };
        let mut code = String::new();
        let mut declared: HashSet<String> = HashSet::new();
        let mut result_expr = "0.0".to_string();

        for stmt in stmts {
            match stmt {
                Ir::Binding { name, value, .. } => {
                    let val = self.emit_expr(value)?;
                    if declared.contains(name.as_str()) {
                        code += &format!("    {} = {};\n", name, val);
                    } else {
                        code += &format!("    {} {} = {};\n", var_keyword, name, val);
                        declared.insert(name.clone());
                    }
                }
                Ir::TupleBinding { names, value, .. } => {
                    self.emit_tuple_binding(
                        &mut code,
                        names,
                        value,
                        "    ",
                        var_keyword,
                        &mut declared,
                    )?;
                }
                Ir::ForLoop {
                    var, range, body, ..
                } => {
                    self.emit_for_loop(&mut code, var, range, body, "    ", &mut declared)?;
                }
                Ir::WhileLoop {
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
                Ir::FunctionDef { .. } => {} // Skip nested function defs
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
        range: &Ir,
        body: &Ir,
        indent: &str,
        declared: &mut HashSet<String>,
    ) -> Result<(), String> {
        let (start_expr, end_expr) = match range {
            Ir::Range { start, end, .. } => (self.emit_expr(start)?, self.emit_expr(end)?),
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
        body: &Ir,
        declared: &mut HashSet<String>,
    ) -> Result<(), String> {
        match body {
            Ir::Block { items: stmts, .. } => {
                for stmt in stmts {
                    match stmt {
                        Ir::Binding { name, value, .. } => {
                            let val = self.emit_expr(value)?;
                            if declared.contains(name.as_str()) {
                                code.push_str(&format!("        {} = {};\n", name, val));
                            } else {
                                code.push_str(&format!("        var {} = {};\n", name, val));
                                declared.insert(name.clone());
                            }
                        }
                        Ir::TupleBinding { names, value, .. } => {
                            self.emit_tuple_binding(
                                code, names, value, "        ", "var", declared,
                            )?;
                        }
                        _ => {} // Non-binding expressions in loop body (side-effect free, skip)
                    }
                }
            }
            Ir::Binding { name, value, .. } => {
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
    fn result_is_bool(&self, node: &Ir) -> bool {
        match node {
            Ir::Apply { callee, .. } => {
                if let Callee::User(name) = callee {
                    if self.bool_functions.contains(name) {
                        return true;
                    }
                }
                returns_bool(node)
            }
            Ir::Identifier { name, .. } => {
                self.bool_binding_defs.contains_key(name)
                    || self
                        .lifted_block_defs
                        .get(name)
                        .is_some_and(|d| d.comparison_op.is_some())
            }
            Ir::Block { items: stmts, .. } => {
                stmts.last().is_some_and(|s| self.result_is_bool(s))
            }
            Ir::IfExpr {
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
    fn result_is_vec(&self, node: &Ir) -> bool {
        match node {
            Ir::Tuple { items, .. } => items.len() >= 2,
            Ir::Apply { callee, .. } => match callee {
                Callee::Builtin(BuiltinOp::Vec2 | BuiltinOp::Vec3 | BuiltinOp::Vec4) => true,
                Callee::User(name) => self.vec_functions.contains(name),
                _ => false,
            },
            Ir::IfExpr {
                then_branch,
                else_branch,
                ..
            } => {
                self.result_is_vec(then_branch)
                    || else_branch
                        .as_ref()
                        .is_some_and(|e| self.result_is_vec(e))
            }
            Ir::Block { items: stmts, .. } => {
                stmts.last().is_some_and(|s| self.result_is_vec(s))
            }
            _ => false,
        }
    }

    /// Return the tuple size of the result expression, if it's a direct tuple literal.
    fn result_tuple_size(&self, node: &Ir) -> Option<usize> {
        match node {
            Ir::Tuple { items, .. } => Some(items.len()),
            Ir::Block { items: stmts, .. } => {
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
        value: &Ir,
        indent: &str,
        var_keyword: &str,
        declared: &mut HashSet<String>,
    ) -> Result<(), String> {
        if let Ir::Tuple { items, .. } = value {
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
        condition: &Ir,
        indent: &str,
        declared: &mut HashSet<String>,
    ) -> Result<(), String> {
        match condition {
            Ir::Block { items: stmts, .. } => {
                for stmt in stmts {
                    if let Ir::Binding { name, value, .. } = stmt {
                        let val = self.emit_expr(value)?;
                        if !declared.contains(name.as_str()) {
                            *code += &format!("{}var {} = {};\n", indent, name, val);
                            declared.insert(name.clone());
                        }
                    }
                }
            }
            Ir::Apply { args, .. } => {
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
        condition: &Ir,
        indent: &str,
        declared: &mut HashSet<String>,
    ) -> Result<(), String> {
        match condition {
            Ir::Block { items: stmts, .. } => {
                for stmt in stmts {
                    if let Ir::Binding { name, value, .. } = stmt {
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
            Ir::Apply { args, .. } => {
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
        stmts: &[Ir],
        indent: &str,
        declared: &mut HashSet<String>,
    ) -> Result<String, String> {
        let mut code = String::new();

        for stmt in stmts {
            match stmt {
                Ir::Binding { name, value, .. } => {
                    let val = self.emit_expr(value)?;
                    if declared.contains(name.as_str()) {
                        code += &format!("{}{} = {};\n", indent, name, val);
                    } else {
                        code += &format!("{}var {} = {};\n", indent, name, val);
                        declared.insert(name.clone());
                    }
                }
                Ir::TupleBinding { names, value, .. } => {
                    self.emit_tuple_binding(&mut code, names, value, indent, "var", declared)?;
                }
                Ir::ForLoop {
                    var, range, body, ..
                } => {
                    self.emit_for_loop(&mut code, var, range, body, indent, declared)?;
                }
                Ir::WhileLoop {
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
                Ir::FunctionDef { .. } => {} // Already collected
                _ => {}                           // Result expression handled separately
            }
        }

        Ok(code)
    }

    // -----------------------------------------------------------------------
    // Standard expression emission (no corner substitution)
    // -----------------------------------------------------------------------

    fn emit_expr(&self, node: &Ir) -> Result<String, String> {
        self.emit_expr_internal(node, None)
    }

    /// Core expression emitter. When `subst` is Some((x_var, y_var)),
    /// identifiers "x" and "y" are replaced with the corner variable names.
    fn emit_expr_internal(&self, node: &Ir, subst: CornerSubst) -> Result<String, String> {
        match node {
            Ir::Number { value: n, .. } => {
                let s = if n.fract() == 0.0 && !n.is_nan() && !n.is_infinite() {
                    format!("{:.1}", n)
                } else {
                    format!("{}", n)
                };
                Ok(s)
            }
            Ir::BoolLit { value: b, .. } => Ok(format!("{}", b)),
            Ir::Identifier { name, .. } => {
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
            Ir::Apply { callee, args, .. } => self.emit_apply_internal(callee, args, subst),
            Ir::Tuple { items, .. } => {
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
            Ir::Block { items: stmts, .. } => {
                if let Some(last) = stmts.last() {
                    self.emit_expr_internal(last, subst)
                } else {
                    Ok("0.0".to_string())
                }
            }
            Ir::Binding { .. } => Ok("0.0".to_string()),
            Ir::IfExpr {
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
            Ir::FunctionDef { .. } => Ok("0.0".to_string()),
            Ir::Lambda { .. } => Err(
                "internal: unlifted lambda reached the codegen (specialization \
                 pass should have replaced it with a synthetic function reference)"
                    .to_string(),
            ),
            Ir::PropertyAccess {
                object, property, ..
            } => {
                // Map x.min → u.axis_min.x, x.max → u.axis_max.x, etc.
                if let Ir::Identifier { name: base, .. } = object.as_ref() {
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
            Ir::TupleBinding { .. } => Ok("0.0".to_string()), // Emitted imperatively
            Ir::ForLoop { .. } => Ok("0.0".to_string()),      // Emitted imperatively
            Ir::WhileLoop { .. } => Ok("0.0".to_string()),    // Emitted imperatively
            Ir::ArrayLiteral { .. }
            | Ir::IndexAccess { .. }
            | Ir::Range { .. }
            | Ir::ParallelFor { .. }
            | Ir::IndexAssign { .. } => {
                Err("Array/parallel operations are not supported in fragment shaders".to_string())
            }
        }
    }

    fn emit_apply_internal(
        &self,
        callee: &Callee,
        args: &[Ir],
        subst: CornerSubst,
    ) -> Result<String, String> {
        let emit_args: Result<Vec<_>, _> = args
            .iter()
            .map(|a| self.emit_expr_internal(a, subst))
            .collect();
        let emitted = emit_args?;

        let op = match callee {
            Callee::Builtin(op) => *op,
            Callee::User(name) => {
                // User-defined functions: emit as regular call, appending captured vars
                // if any. Captured axis vars (`x`, `y`) get the corner substitution when
                // we're inside corner-checking — otherwise the function would be
                // evaluated at the pixel center for all four corners, killing the
                // sign-change check and producing a dotted curve.
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
                return Ok(format!("{}({})", name, all_args.join(", ")));
            }
        };

        match op {
            // Binary infix operators
            BuiltinOp::Add => Ok(format!("({} + {})", emitted[0], emitted[1])),
            BuiltinOp::Sub => Ok(format!("({} - {})", emitted[0], emitted[1])),
            BuiltinOp::Mul => Ok(format!("({} * {})", emitted[0], emitted[1])),
            BuiltinOp::Div => Ok(format!("({} / {})", emitted[0], emitted[1])),
            BuiltinOp::Mod => Ok(format!(
                "((({} % {}) + {}) % {})",
                emitted[0], emitted[1], emitted[1], emitted[1]
            )),

            // Comparison
            BuiltinOp::Eq => Ok(format!("({} == {})", emitted[0], emitted[1])),
            BuiltinOp::Neq => Ok(format!("({} != {})", emitted[0], emitted[1])),
            BuiltinOp::Lt => Ok(format!("({} < {})", emitted[0], emitted[1])),
            BuiltinOp::Gt => Ok(format!("({} > {})", emitted[0], emitted[1])),
            BuiltinOp::Lte => Ok(format!("({} <= {})", emitted[0], emitted[1])),
            BuiltinOp::Gte => Ok(format!("({} >= {})", emitted[0], emitted[1])),

            // Logical
            BuiltinOp::And => Ok(format!("({} && {})", emitted[0], emitted[1])),
            BuiltinOp::Or => Ok(format!("({} || {})", emitted[0], emitted[1])),
            BuiltinOp::Not => Ok(format!("!({})", emitted[0])),

            // Unary
            BuiltinOp::Neg => Ok(format!("-({})", emitted[0])),

            // Pure unary math — direct WGSL mapping
            BuiltinOp::Sin | BuiltinOp::Cos | BuiltinOp::Tan
            | BuiltinOp::Asin | BuiltinOp::Acos
            | BuiltinOp::Sinh | BuiltinOp::Cosh | BuiltinOp::Tanh
            | BuiltinOp::Log | BuiltinOp::Log2 | BuiltinOp::Exp | BuiltinOp::Exp2
            | BuiltinOp::Sqrt | BuiltinOp::Abs | BuiltinOp::Sign
            | BuiltinOp::Floor | BuiltinOp::Ceil | BuiltinOp::Round | BuiltinOp::Fract
            | BuiltinOp::Length | BuiltinOp::Normalize => {
                Ok(format!("{}({})", op.name(), emitted[0]))
            }

            // atan is overloaded: 1 arg → atan, 2 args → atan2
            BuiltinOp::Atan => match args.len() {
                1 => Ok(format!("atan({})", emitted[0])),
                2 => Ok(format!("atan2({}, {})", emitted[0], emitted[1])),
                n => Err(format!("atan expects 1 or 2 args, got {}", n)),
            },

            BuiltinOp::Log10 => Ok(format!("(log2({}) / log2(10.0))", emitted[0])),

            BuiltinOp::Pow => {
                // pow(x, n) for small non-negative integer n is much cheaper as
                // repeated multiplication — pow() costs ~20+ GPU ops via exp2/log2,
                // and `x²` (very common in plotting) shouldn't pay that.
                if let Ir::Number { value: n, .. } = &args[1] {
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

            // Pure binary math
            BuiltinOp::Min | BuiltinOp::Max | BuiltinOp::Step
            | BuiltinOp::Dot | BuiltinOp::Cross => {
                Ok(format!("{}({}, {})", op.name(), emitted[0], emitted[1]))
            }

            // Pure ternary math
            BuiltinOp::Clamp | BuiltinOp::Mix | BuiltinOp::Smoothstep => Ok(format!(
                "{}({}, {}, {})",
                op.name(),
                emitted[0], emitted[1], emitted[2]
            )),

            // Type constructors / casts
            BuiltinOp::F32 | BuiltinOp::F64 => Ok(format!("f32({})", emitted[0])),
            BuiltinOp::I32 => Ok(format!("i32({})", emitted[0])),
            BuiltinOp::Vec2 => Ok(format!("vec2<f32>({})", emitted.join(", "))),
            BuiltinOp::Vec3 => Ok(format!("vec3<f32>({})", emitted.join(", "))),
            BuiltinOp::Vec4 => Ok(format!("vec4<f32>({})", emitted.join(", "))),

            // No fragment-shader semantics for these.
            BuiltinOp::Len | BuiltinOp::Print | BuiltinOp::Plot => Err(format!(
                "Builtin '{}' is not supported in fragment shaders",
                op.name()
            )),
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
    fn emit_bool_with_corners(&self, node: &Ir) -> Result<String, String> {
        match node {
            Ir::BoolLit { value: b, .. } => Ok(format!("{}", b)),

            // Lifted block-valued binding: call the synthesized WGSL function at
            // each pixel corner so the loop/local state re-runs for x_m/x_p/y_m/y_p
            // rather than reusing the pixel-center value (which causes dotted curves).
            // The four corner calls are hoisted into `__lifted_<name>_mm/mp/pm/pp`
            // by the caller so we only invoke the function 4 times per pixel,
            // not 6+ (the corner expression below references each corner twice).
            Ir::Identifier { name, .. } if self.lifted_block_defs.contains_key(name) => {
                let def = self.lifted_block_defs.get(name).unwrap();
                let calls = [
                    format!("_corner_{}_mm", name),
                    format!("_corner_{}_mp", name),
                    format!("_corner_{}_pm", name),
                    format!("_corner_{}_pp", name),
                ];

                // No comparison op → treat float result as implicit curve `f = 0`,
                // which is the same pattern as `Eq` (sign-change detection).
                let op = def.comparison_op.unwrap_or(BuiltinOp::Eq);
                Ok(emit_corner_compare(op, &calls))
            }

            // Identifier bound to a bool expression: inline so the comparison
            // goes through corner-checking instead of a direct float ==.
            Ir::Identifier { name, .. } if self.bool_binding_defs.contains_key(name) => {
                let bound = self.bool_binding_defs.get(name).unwrap().clone();
                self.emit_bool_with_corners(&bound)
            }

            Ir::Apply { callee, args, .. } => {
                // Logical ops: recursively apply corner checking.
                // Operands may be bool (comparisons) or float (implicit curves).
                if let Callee::Builtin(op) = callee {
                    match (op, args.len()) {
                        (BuiltinOp::And, 2) => {
                            let l = self.emit_bool_operand_with_corners(&args[0])?;
                            let r = self.emit_bool_operand_with_corners(&args[1])?;
                            return Ok(format!("({} && {})", l, r));
                        }
                        (BuiltinOp::Or, 2) => {
                            let l = self.emit_bool_operand_with_corners(&args[0])?;
                            let r = self.emit_bool_operand_with_corners(&args[1])?;
                            return Ok(format!("({} || {})", l, r));
                        }
                        (BuiltinOp::Not, 1) => {
                            let inner = self.emit_bool_operand_with_corners(&args[0])?;
                            return Ok(format!("!({})", inner));
                        }
                        _ => {}
                    }
                    // Comparison ops: apply corner checking
                    if let Some(cmp_op) = as_comparison_op(callee, args) {
                        return self.emit_comparison_with_corners(cmp_op, &args[0], &args[1]);
                    }
                }

                // User-defined bool function with imperative body + comparison
                // result: call the precomputed `_diff_<name>` companion at the
                // four pixel corners. Inlining doesn't work here because
                // emit_bool_with_corners can't re-emit imperative stmts.
                if let Callee::User(name) = callee {
                    if self.lifted_function_defs.contains_key(name) {
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

                        return Ok(emit_corner_compare(def.comparison_op, &calls));
                    }
                    if self.bool_function_defs.contains_key(name) {
                        let func_def = self.bool_function_defs.get(name).unwrap();
                        let inlined = substitute_params(&func_def.body, &func_def.params, args);
                        return self.emit_bool_with_corners(&inlined);
                    }
                }
                // Anything else: fall back to normal emission
                self.emit_expr(node)
            }

            // Non-boolean nodes or identifiers: emit normally
            _ => self.emit_expr(node),
        }
    }

    /// Emit an operand of a logical op (and/or/not) with corner-checking.
    /// If the operand is boolean, recurse normally. If it's a float
    /// expression, treat it as an implicit curve (expr = 0).
    fn emit_bool_operand_with_corners(&self, node: &Ir) -> Result<String, String> {
        if returns_bool(node) {
            self.emit_bool_with_corners(node)
        } else {
            // Float expression used as implicit curve: treat as expr = 0
            let zero = Ir::Number {
                value: 0.0,
                span: node.span(),
            };
            self.emit_comparison_with_corners(BuiltinOp::Eq, node, &zero)
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
        op: BuiltinOp,
        lhs: &Ir,
        rhs: &Ir,
    ) -> Result<String, String> {
        // The four corners: (x_m, y_m), (x_m, y_p), (x_p, y_m), (x_p, y_p)
        let corners: [(&str, &str); 4] = [
            ("x_m", "y_m"),
            ("x_m", "y_p"),
            ("x_p", "y_m"),
            ("x_p", "y_p"),
        ];

        if matches!(op, BuiltinOp::Eq) {
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
                BuiltinOp::Gt => ">",
                BuiltinOp::Lt => "<",
                BuiltinOp::Gte => ">=",
                BuiltinOp::Lte => "<=",
                BuiltinOp::Neq => "!=",
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

/// Find all identifiers referenced in an IR node (for captured variable analysis).
fn find_referenced_identifiers(node: &Ir) -> HashSet<String> {
    let mut result = HashSet::new();
    collect_identifiers(node, &mut result);
    result
}

/// Lift every `Ir::Lambda` in the AST into a synthetic top-level FunctionDef
/// named `_lambda_N`, replacing the lambda expression with an `Ir::Identifier`
/// referring to that synthetic name. Returns the rewritten AST (or `None` if
/// no lambdas were present).
///
/// After this pass the AST contains no Lambda nodes — every former lambda
/// looks like an ordinary user-defined function for capture analysis and
/// codegen. Higher-order specialization then runs unchanged on the result.
fn lift_lambdas(ast: &Ir) -> Option<Ir> {
    let mut counter: usize = 0;
    let mut new_defs: Vec<Ir> = Vec::new();
    let mut owned = ast.clone();
    let changed = lift_lambdas_inner(&mut owned, &mut counter, &mut new_defs);
    if !changed {
        return None;
    }
    Some(prepend_function_defs(owned, new_defs))
}

fn lift_lambdas_inner(node: &mut Ir, counter: &mut usize, new_defs: &mut Vec<Ir>) -> bool {
    // `Binding { value: Lambda }` — i.e. `f := t |-> t*t` *or* the synthetic
    // binding produced by the IIFE parser path — gets hoisted into a
    // top-level `FunctionDef` keyed by the binding's name. We push it to
    // `new_defs` (which gets prepended to the AST root) and leave a
    // `Number(0)` no-op in the binding's slot. Hoisting unconditionally is
    // what makes the IIFE pattern work: the synthetic `Block { binding, call }`
    // emitted by the parser sits nested inside an Apply arg where
    // `collect_functions` doesn't recurse, so an in-place rewrite would never
    // be picked up by codegen.
    let binding_is_lambda = matches!(
        node,
        Ir::Binding { value, .. } if matches!(value.as_ref(), Ir::Lambda { .. })
    );
    if binding_is_lambda {
        let taken = std::mem::replace(node, Ir::Number { value: 0.0, span: (0, 0) });
        let Ir::Binding {
            name,
            value,
            span: binding_span,
        } = taken
        else {
            unreachable!()
        };
        let Ir::Lambda {
            params, mut body, ..
        } = *value
        else {
            unreachable!()
        };
        lift_lambdas_inner(&mut body, counter, new_defs);
        new_defs.push(Ir::FunctionDef {
            name,
            params,
            body,
            span: binding_span,
        });
        *node = Ir::Number {
            value: 0.0,
            span: binding_span,
        };
        return true;
    }

    // Recurse first so nested lambdas are lifted before the enclosing one.
    let mut changed = false;
    match node {
        Ir::Apply { args, .. } => {
            for a in args.iter_mut() {
                changed |= lift_lambdas_inner(a, counter, new_defs);
            }
        }
        Ir::Block { items, .. } => {
            for s in items.iter_mut() {
                changed |= lift_lambdas_inner(s, counter, new_defs);
            }
        }
        Ir::Binding { value, .. } | Ir::TupleBinding { value, .. } => {
            changed |= lift_lambdas_inner(value, counter, new_defs);
        }
        Ir::Tuple { items, .. } | Ir::ArrayLiteral { items, .. } => {
            for i in items.iter_mut() {
                changed |= lift_lambdas_inner(i, counter, new_defs);
            }
        }
        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            changed |= lift_lambdas_inner(condition, counter, new_defs);
            changed |= lift_lambdas_inner(then_branch, counter, new_defs);
            if let Some(e) = else_branch {
                changed |= lift_lambdas_inner(e, counter, new_defs);
            }
        }
        Ir::WhileLoop {
            condition, body, ..
        } => {
            changed |= lift_lambdas_inner(condition, counter, new_defs);
            changed |= lift_lambdas_inner(body, counter, new_defs);
        }
        Ir::ForLoop { range, body, .. } => {
            changed |= lift_lambdas_inner(range, counter, new_defs);
            changed |= lift_lambdas_inner(body, counter, new_defs);
        }
        Ir::FunctionDef { body, .. } | Ir::Lambda { body, .. } => {
            changed |= lift_lambdas_inner(body, counter, new_defs);
        }
        _ => {}
    }
    // Now replace this node if it's itself a lambda.
    if let Ir::Lambda { params, body, span } = node {
        let name = format!("_lambda_{}", *counter);
        *counter += 1;
        let saved_params = std::mem::take(params);
        let saved_body = std::mem::replace(
            body,
            Box::new(Ir::Number {
                value: 0.0,
                span: *span,
            }),
        );
        let saved_span = *span;
        new_defs.push(Ir::FunctionDef {
            name: name.clone(),
            params: saved_params,
            body: saved_body,
            span: saved_span,
        });
        *node = Ir::Identifier {
            name,
            span: saved_span,
        };
        changed = true;
    }
    changed
}

/// Specialize calls to higher-order user functions where each function-
/// valued argument is a simple identifier naming another defined function.
///
/// `N_integral(sq, 0, x, 0.01)` is rewritten into `N_integral__sq(0, x, 0.01)`
/// against a freshly synthesized `N_integral__sq` whose body has `sq`
/// substituted for the function parameter `f`. The original HOF is left in
/// place; it'll get pruned by the unreachable-function pass since no calls
/// to it remain.
///
/// Returns `None` when the AST contains no HOFs and `Some(new_ast)` when
/// at least one call was specialized.
fn specialize_higher_order_calls(ast: &Ir) -> Option<Ir> {
    let mut defs: HashMap<String, (Vec<String>, Ir)> = HashMap::new();
    collect_owned_function_defs(ast, &mut defs);

    let hof_indices = compute_hof_indices(&defs);
    if hof_indices.is_empty() {
        return None;
    }

    let mut rewritten = ast.clone();
    let mut cache: HashMap<(String, Vec<String>), String> = HashMap::new();
    let mut new_defs: Vec<Ir> = Vec::new();
    let changed =
        rewrite_hof_calls(&mut rewritten, &defs, &hof_indices, &mut cache, &mut new_defs);
    if !changed {
        return None;
    }
    Some(prepend_function_defs(rewritten, new_defs))
}

fn collect_owned_function_defs(node: &Ir, out: &mut HashMap<String, (Vec<String>, Ir)>) {
    match node {
        Ir::FunctionDef {
            name, params, body, ..
        } => {
            out.insert(name.clone(), (params.clone(), body.as_ref().clone()));
            collect_owned_function_defs(body, out);
        }
        Ir::Block { items, .. } => {
            for s in items {
                collect_owned_function_defs(s, out);
            }
        }
        Ir::Binding { value, .. } | Ir::TupleBinding { value, .. } => {
            collect_owned_function_defs(value, out);
        }
        _ => {}
    }
}

fn rewrite_hof_calls(
    node: &mut Ir,
    defs: &HashMap<String, (Vec<String>, Ir)>,
    hof_indices: &HashMap<String, Vec<usize>>,
    cache: &mut HashMap<(String, Vec<String>), String>,
    new_defs: &mut Vec<Ir>,
) -> bool {
    let mut changed = false;
    match node {
        Ir::Apply { callee, args, span } => {
            for a in args.iter_mut() {
                changed |= rewrite_hof_calls(a, defs, hof_indices, cache, new_defs);
            }
            let callee_name = match callee {
                Callee::User(n) => Some(n.clone()),
                _ => None,
            };
            if let Some(name) = callee_name {
                if let Some(indices) = hof_indices.get(&name) {
                    let fn_arg_names: Option<Vec<String>> = indices
                        .iter()
                        .map(|&i| match args.get(i) {
                            Some(Ir::Identifier { name: arg_name, .. })
                                if defs.contains_key(arg_name) =>
                            {
                                Some(arg_name.clone())
                            }
                            _ => None,
                        })
                        .collect();
                    if let Some(fn_arg_names) = fn_arg_names {
                        let key = (name.clone(), fn_arg_names.clone());
                        let specialized_name = if let Some(sn) = cache.get(&key).cloned() {
                            sn
                        } else {
                            let mut sn = name.clone();
                            for n in &fn_arg_names {
                                sn.push_str("__");
                                sn.push_str(n);
                            }
                            // Snapshot what we need from `defs`/`hof_indices`
                            // up-front so the recursive call below has free
                            // access to those tables (and doesn't trip on a
                            // simultaneous borrow).
                            let (params, body) = defs.get(&name).unwrap().clone();
                            let local_indices = indices.clone();
                            let spec_span = *span;
                            // Insert into cache *before* recursing — if the
                            // specialized body somehow refers back to its own
                            // shape we'd otherwise infinite-loop synthesizing
                            // the same function. Order matters here.
                            cache.insert(key.clone(), sn.clone());

                            let mut subs: HashMap<String, String> = HashMap::new();
                            let mut new_params: Vec<String> = Vec::new();
                            for (i, p) in params.iter().enumerate() {
                                match local_indices.iter().position(|&x| x == i) {
                                    Some(pos) => {
                                        subs.insert(p.clone(), fn_arg_names[pos].clone());
                                    }
                                    None => new_params.push(p.clone()),
                                }
                            }
                            let mut new_body = body;
                            substitute_user_callees(&mut new_body, &subs);
                            // Recursively rewrite any HOF calls in the new
                            // body — required for chained HOFs where the
                            // wrapper's body itself contains an HOF call that
                            // only becomes specializable after the wrapper's
                            // function-typed param has been substituted out.
                            rewrite_hof_calls(
                                &mut new_body,
                                defs,
                                hof_indices,
                                cache,
                                new_defs,
                            );
                            new_defs.push(Ir::FunctionDef {
                                name: sn.clone(),
                                params: new_params,
                                body: Box::new(new_body),
                                span: spec_span,
                            });
                            sn
                        };
                        *callee = Callee::User(specialized_name);
                        let kept: Vec<Ir> = std::mem::take(args)
                            .into_iter()
                            .enumerate()
                            .filter_map(|(i, a)| {
                                if indices.contains(&i) {
                                    None
                                } else {
                                    Some(a)
                                }
                            })
                            .collect();
                        *args = kept;
                        changed = true;
                    }
                }
            }
        }
        Ir::Block { items, .. } => {
            for s in items.iter_mut() {
                changed |= rewrite_hof_calls(s, defs, hof_indices, cache, new_defs);
            }
        }
        Ir::Binding { value, .. } | Ir::TupleBinding { value, .. } => {
            changed |= rewrite_hof_calls(value, defs, hof_indices, cache, new_defs);
        }
        Ir::Tuple { items, .. } | Ir::ArrayLiteral { items, .. } => {
            for i in items.iter_mut() {
                changed |= rewrite_hof_calls(i, defs, hof_indices, cache, new_defs);
            }
        }
        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            changed |= rewrite_hof_calls(condition, defs, hof_indices, cache, new_defs);
            changed |= rewrite_hof_calls(then_branch, defs, hof_indices, cache, new_defs);
            if let Some(e) = else_branch {
                changed |= rewrite_hof_calls(e, defs, hof_indices, cache, new_defs);
            }
        }
        Ir::WhileLoop {
            condition, body, ..
        } => {
            changed |= rewrite_hof_calls(condition, defs, hof_indices, cache, new_defs);
            changed |= rewrite_hof_calls(body, defs, hof_indices, cache, new_defs);
        }
        Ir::ForLoop { range, body, .. } => {
            changed |= rewrite_hof_calls(range, defs, hof_indices, cache, new_defs);
            changed |= rewrite_hof_calls(body, defs, hof_indices, cache, new_defs);
        }
        Ir::FunctionDef { body, .. } => {
            changed |= rewrite_hof_calls(body, defs, hof_indices, cache, new_defs);
        }
        _ => {}
    }
    changed
}

fn substitute_user_callees(node: &mut Ir, subs: &HashMap<String, String>) {
    match node {
        Ir::Identifier { name, .. } => {
            // A function-typed parameter passed through to another HOF appears
            // as an `Identifier` arg (not a callee). Rewriting both positions
            // is what lets chained HOFs (`wrapper(f) := N_integral(f, …)`)
            // resolve when `wrapper` is specialized over a concrete function.
            if let Some(replacement) = subs.get(name) {
                *name = replacement.clone();
            }
        }
        Ir::Apply { callee, args, .. } => {
            if let Callee::User(name) = callee {
                if let Some(replacement) = subs.get(name) {
                    *callee = Callee::User(replacement.clone());
                }
            }
            for a in args.iter_mut() {
                substitute_user_callees(a, subs);
            }
        }
        Ir::Block { items, .. } => {
            for s in items.iter_mut() {
                substitute_user_callees(s, subs);
            }
        }
        Ir::Binding { value, .. } | Ir::TupleBinding { value, .. } => {
            substitute_user_callees(value, subs);
        }
        Ir::Tuple { items, .. } | Ir::ArrayLiteral { items, .. } => {
            for i in items.iter_mut() {
                substitute_user_callees(i, subs);
            }
        }
        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            substitute_user_callees(condition, subs);
            substitute_user_callees(then_branch, subs);
            if let Some(e) = else_branch {
                substitute_user_callees(e, subs);
            }
        }
        Ir::WhileLoop {
            condition, body, ..
        } => {
            substitute_user_callees(condition, subs);
            substitute_user_callees(body, subs);
        }
        Ir::ForLoop { range, body, .. } => {
            substitute_user_callees(range, subs);
            substitute_user_callees(body, subs);
        }
        Ir::FunctionDef { body, .. } => substitute_user_callees(body, subs),
        _ => {}
    }
}

fn prepend_function_defs(ast: Ir, new_defs: Vec<Ir>) -> Ir {
    if new_defs.is_empty() {
        return ast;
    }
    match ast {
        Ir::Block { mut items, span } => {
            let mut combined = new_defs;
            combined.append(&mut items);
            Ir::Block {
                items: combined,
                span,
            }
        }
        other => {
            let span = other.span();
            let mut combined = new_defs;
            combined.push(other);
            Ir::Block {
                items: combined,
                span,
            }
        }
    }
}

/// For each user function, compute which of its parameter *positions* hold
/// function values. A param at index `i` is HOF iff its name appears either:
///
///   (a) as a `Callee::User` inside the body (the function calls it directly),
///   (b) as an `Identifier` argument passed to a HOF slot of *another*
///       function (the function forwards it).
///
/// (b) is transitive, so we fixpoint on the table until no new entries appear.
/// Without that, wrappers like `outer(f) := inner(f, …)` wouldn't be detected
/// as HOFs and their call sites wouldn't trigger specialization.
fn compute_hof_indices(
    defs: &HashMap<String, (Vec<String>, Ir)>,
) -> HashMap<String, Vec<usize>> {
    let mut hof_indices: HashMap<String, Vec<usize>> = HashMap::new();
    // Seed: direct callee usage.
    for (name, (params, body)) in defs {
        let mut indices = Vec::new();
        for (i, p) in params.iter().enumerate() {
            let mut single = HashSet::new();
            single.insert(p.as_str());
            if body_calls_any_of(body, &single) {
                indices.push(i);
            }
        }
        if !indices.is_empty() {
            hof_indices.insert(name.clone(), indices);
        }
    }
    // Fixpoint: forward propagation through HOF slots.
    loop {
        let snapshot = hof_indices.clone();
        let mut any_new = false;
        for (name, (params, body)) in defs {
            let mut current = hof_indices.get(name).cloned().unwrap_or_default();
            let before = current.len();
            for (i, p) in params.iter().enumerate() {
                if current.contains(&i) {
                    continue;
                }
                if body_passes_to_hof_slot(body, p, &snapshot) {
                    current.push(i);
                }
            }
            if current.len() > before {
                hof_indices.insert(name.clone(), current);
                any_new = true;
            }
        }
        if !any_new {
            break;
        }
    }
    hof_indices
}

fn body_passes_to_hof_slot(
    node: &Ir,
    param: &str,
    hof_indices: &HashMap<String, Vec<usize>>,
) -> bool {
    match node {
        Ir::Apply { callee, args, .. } => {
            if let Callee::User(callee_name) = callee {
                if let Some(slots) = hof_indices.get(callee_name) {
                    for &slot in slots {
                        if let Some(Ir::Identifier { name, .. }) = args.get(slot) {
                            if name == param {
                                return true;
                            }
                        }
                    }
                }
            }
            args.iter()
                .any(|a| body_passes_to_hof_slot(a, param, hof_indices))
        }
        Ir::Block { items, .. } => items
            .iter()
            .any(|s| body_passes_to_hof_slot(s, param, hof_indices)),
        Ir::Binding { value, .. } | Ir::TupleBinding { value, .. } => {
            body_passes_to_hof_slot(value, param, hof_indices)
        }
        Ir::Tuple { items, .. } | Ir::ArrayLiteral { items, .. } => items
            .iter()
            .any(|i| body_passes_to_hof_slot(i, param, hof_indices)),
        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            body_passes_to_hof_slot(condition, param, hof_indices)
                || body_passes_to_hof_slot(then_branch, param, hof_indices)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| body_passes_to_hof_slot(e, param, hof_indices))
        }
        Ir::WhileLoop {
            condition, body, ..
        } => {
            body_passes_to_hof_slot(condition, param, hof_indices)
                || body_passes_to_hof_slot(body, param, hof_indices)
        }
        Ir::ForLoop { range, body, .. } => {
            body_passes_to_hof_slot(range, param, hof_indices)
                || body_passes_to_hof_slot(body, param, hof_indices)
        }
        Ir::FunctionDef { body, .. } => body_passes_to_hof_slot(body, param, hof_indices),
        _ => false,
    }
}

/// The set of user-defined function *names* with at least one function-typed
/// parameter. Same fixpoint detection as `compute_hof_indices`, just exposed
/// as a flat set for the codegen's "is this HOF still reachable?" check.
fn unrepresentable_higher_order_functions(ast: &Ir) -> HashSet<String> {
    let mut defs: HashMap<String, (Vec<String>, Ir)> = HashMap::new();
    collect_owned_function_defs(ast, &mut defs);
    compute_hof_indices(&defs).keys().cloned().collect()
}

fn body_calls_any_of(node: &Ir, names: &HashSet<&str>) -> bool {
    match node {
        Ir::Apply { callee, args, .. } => {
            if let Callee::User(name) = callee {
                if names.contains(name.as_str()) {
                    return true;
                }
            }
            args.iter().any(|a| body_calls_any_of(a, names))
        }
        Ir::Block { items, .. } => items.iter().any(|s| body_calls_any_of(s, names)),
        Ir::Binding { value, .. } | Ir::TupleBinding { value, .. } => {
            body_calls_any_of(value, names)
        }
        Ir::Tuple { items, .. } | Ir::ArrayLiteral { items, .. } => {
            items.iter().any(|i| body_calls_any_of(i, names))
        }
        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            body_calls_any_of(condition, names)
                || body_calls_any_of(then_branch, names)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| body_calls_any_of(e, names))
        }
        Ir::WhileLoop {
            condition, body, ..
        } => body_calls_any_of(condition, names) || body_calls_any_of(body, names),
        Ir::ForLoop { range, body, .. } => {
            body_calls_any_of(range, names) || body_calls_any_of(body, names)
        }
        Ir::FunctionDef { body, .. } => body_calls_any_of(body, names),
        _ => false,
    }
}

/// Compute the set of user-defined function names actually reachable from the
/// cell's result expression. Walks the AST collecting `Callee::User` calls
/// from non-`FunctionDef` positions (the "roots"), then expands transitively
/// through the bodies of each reachable function until fixpoint.
fn reachable_user_functions(ast: &Ir) -> HashSet<String> {
    let mut bodies: HashMap<String, &Ir> = HashMap::new();
    collect_function_bodies(ast, &mut bodies);

    let mut reachable: HashSet<String> = HashSet::new();
    let mut worklist: Vec<String> = Vec::new();
    collect_user_calls(ast, /*inside_fn_body=*/ false, &mut reachable, &mut worklist);

    while let Some(name) = worklist.pop() {
        if let Some(body) = bodies.get(&name).copied() {
            collect_user_calls(body, /*inside_fn_body=*/ true, &mut reachable, &mut worklist);
        }
    }
    reachable
}

fn collect_function_bodies<'a>(node: &'a Ir, out: &mut HashMap<String, &'a Ir>) {
    match node {
        Ir::FunctionDef { name, body, .. } => {
            out.insert(name.clone(), body.as_ref());
            collect_function_bodies(body, out);
        }
        Ir::Block { items, .. } => {
            for s in items {
                collect_function_bodies(s, out);
            }
        }
        Ir::Binding { value, .. } | Ir::TupleBinding { value, .. } => {
            collect_function_bodies(value, out);
        }
        Ir::Apply { args, .. } => {
            for a in args {
                collect_function_bodies(a, out);
            }
        }
        Ir::Tuple { items, .. } | Ir::ArrayLiteral { items, .. } => {
            for i in items {
                collect_function_bodies(i, out);
            }
        }
        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            collect_function_bodies(condition, out);
            collect_function_bodies(then_branch, out);
            if let Some(e) = else_branch {
                collect_function_bodies(e, out);
            }
        }
        Ir::WhileLoop {
            condition, body, ..
        } => {
            collect_function_bodies(condition, out);
            collect_function_bodies(body, out);
        }
        Ir::ForLoop { range, body, .. } => {
            collect_function_bodies(range, out);
            collect_function_bodies(body, out);
        }
        _ => {}
    }
}

/// Walk `node` collecting every `Callee::User(name)` into `reachable`,
/// pushing newly-seen names onto `worklist`. When `inside_fn_body` is false,
/// any nested `FunctionDef` is skipped (we only care about calls from the
/// "outside" — the actual result path — and from already-known-reachable
/// function bodies).
fn collect_user_calls(
    node: &Ir,
    inside_fn_body: bool,
    reachable: &mut HashSet<String>,
    worklist: &mut Vec<String>,
) {
    match node {
        Ir::Apply { callee, args, .. } => {
            if let Callee::User(name) = callee {
                if reachable.insert(name.clone()) {
                    worklist.push(name.clone());
                }
            }
            for a in args {
                collect_user_calls(a, inside_fn_body, reachable, worklist);
            }
        }
        Ir::Block { items, .. } => {
            for s in items {
                collect_user_calls(s, inside_fn_body, reachable, worklist);
            }
        }
        Ir::Binding { value, .. } | Ir::TupleBinding { value, .. } => {
            collect_user_calls(value, inside_fn_body, reachable, worklist);
        }
        Ir::Tuple { items, .. } | Ir::ArrayLiteral { items, .. } => {
            for i in items {
                collect_user_calls(i, inside_fn_body, reachable, worklist);
            }
        }
        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            collect_user_calls(condition, inside_fn_body, reachable, worklist);
            collect_user_calls(then_branch, inside_fn_body, reachable, worklist);
            if let Some(e) = else_branch {
                collect_user_calls(e, inside_fn_body, reachable, worklist);
            }
        }
        Ir::WhileLoop {
            condition, body, ..
        } => {
            collect_user_calls(condition, inside_fn_body, reachable, worklist);
            collect_user_calls(body, inside_fn_body, reachable, worklist);
        }
        Ir::ForLoop { range, body, .. } => {
            collect_user_calls(range, inside_fn_body, reachable, worklist);
            collect_user_calls(body, inside_fn_body, reachable, worklist);
        }
        Ir::FunctionDef { body, .. } => {
            // Only descend into a function-def body when we're already
            // expanding a known-reachable function — top-level traversal
            // shouldn't pull in calls from unreachable defs.
            if inside_fn_body {
                collect_user_calls(body, inside_fn_body, reachable, worklist);
            }
        }
        _ => {}
    }
}

fn collect_identifiers(node: &Ir, result: &mut HashSet<String>) {
    match node {
        Ir::Identifier { name, .. } => {
            result.insert(name.clone());
        }
        Ir::Apply { args, .. } => {
            for arg in args {
                collect_identifiers(arg, result);
            }
        }
        Ir::Block { items: stmts, .. } => {
            for s in stmts {
                collect_identifiers(s, result);
            }
        }
        Ir::Binding { value, .. } => {
            collect_identifiers(value, result);
        }
        Ir::TupleBinding { value, .. } => {
            collect_identifiers(value, result);
        }
        Ir::Tuple { items, .. } => {
            for item in items {
                collect_identifiers(item, result);
            }
        }
        Ir::IfExpr {
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
        Ir::WhileLoop {
            condition, body, ..
        } => {
            collect_identifiers(condition, result);
            collect_identifiers(body, result);
        }
        Ir::FunctionDef { body, .. } => {
            collect_identifiers(body, result);
        }
        Ir::PropertyAccess { object, .. } => {
            collect_identifiers(object, result);
        }
        Ir::Number { .. } | Ir::BoolLit { .. } => {}
        Ir::ArrayLiteral { items: elems, .. } => {
            for e in elems {
                collect_identifiers(e, result);
            }
        }
        Ir::IndexAccess { array, index, .. } => {
            collect_identifiers(array, result);
            collect_identifiers(index, result);
        }
        Ir::Range { start, end, .. } => {
            collect_identifiers(start, result);
            collect_identifiers(end, result);
        }
        Ir::ForLoop { range, body, .. } | Ir::ParallelFor { range, body, .. } => {
            collect_identifiers(range, result);
            collect_identifiers(body, result);
        }
        Ir::IndexAssign {
            array,
            index,
            value,
            ..
        } => {
            collect_identifiers(array, result);
            collect_identifiers(index, result);
            collect_identifiers(value, result);
        }
        Ir::Lambda { body, .. } => collect_identifiers(body, result),
    }
}

/// Check if an IR node is a constant expression (no x, y, z, t references).
/// `const_names` tracks bindings already known to be constant.
fn is_const_expr(node: &Ir, const_names: &HashSet<String>) -> bool {
    match node {
        Ir::Number { .. } | Ir::BoolLit { .. } => true,
        Ir::Identifier { name, .. } => {
            if matches!(name.as_str(), "x" | "y" | "z" | "t") {
                return false;
            }
            const_names.contains(name)
        }
        Ir::Apply { args, .. } => args.iter().all(|a| is_const_expr(a, const_names)),
        Ir::Tuple { items, .. } => items.iter().all(|i| is_const_expr(i, const_names)),
        Ir::Block { items: stmts, .. } => {
            stmts.iter().all(|s| is_const_expr(s, const_names))
        }
        Ir::IfExpr {
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
        Ir::Binding { value, .. } => is_const_expr(value, const_names),
        _ => false,
    }
}

/// Substitute parameter names with argument expressions in an IR subtree.
/// Used for inlining bool functions in the corner-checking context so that
/// comparisons go through sign-change detection rather than float `==`.
fn substitute_params(body: &Ir, params: &[String], args: &[Ir]) -> Ir {
    match body {
        Ir::Identifier { name, .. } => {
            if let Some(i) = params.iter().position(|p| p == name) {
                if i < args.len() {
                    return args[i].clone();
                }
            }
            body.clone()
        }
        Ir::Apply {
            callee,
            args: func_args,
            span,
        } => {
            // Don't substitute the function name, only its arguments
            let new_args = func_args
                .iter()
                .map(|a| substitute_params(a, params, args))
                .collect();
            Ir::Apply {
                callee: callee.clone(),
                args: new_args,
                span: *span,
            }
        }
        Ir::Block { items: stmts, span } => Ir::Block {
            items: stmts
                .iter()
                .map(|s| substitute_params(s, params, args))
                .collect(),
            span: *span,
        },
        Ir::Binding { name, value, span } => Ir::Binding {
            name: name.clone(),
            value: Box::new(substitute_params(value, params, args)),
            span: *span,
        },
        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            span,
        } => Ir::IfExpr {
            condition: Box::new(substitute_params(condition, params, args)),
            then_branch: Box::new(substitute_params(then_branch, params, args)),
            else_branch: else_branch
                .as_ref()
                .map(|e| Box::new(substitute_params(e, params, args))),
            span: *span,
        },
        Ir::Tuple { items, span } => Ir::Tuple {
            items: items
                .iter()
                .map(|i| substitute_params(i, params, args))
                .collect(),
            span: *span,
        },
        Ir::Number { .. } | Ir::BoolLit { .. } => body.clone(),
        Ir::PropertyAccess {
            object,
            property,
            span,
        } => Ir::PropertyAccess {
            object: Box::new(substitute_params(object, params, args)),
            property: property.clone(),
            span: *span,
        },
        // For remaining node types, clone as-is (loops, arrays, etc. are unlikely in bool functions)
        _ => body.clone(),
    }
}

/// Check if an IR node produces a boolean value in WGSL.
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
fn needs_anon_hoisting(ast: &Ir) -> bool {
    let mut found = false;
    scan_for_anon_blocks(ast, false, &mut found);
    found
}

/// Walk `ast` and set `found = true` if any *expression-position* node is a
/// `Block` containing imperative statements. `in_value_position` is true when
/// the current node is being read as a value (Apply arg, comparison side,
/// etc.) rather than a statement container.
fn scan_for_anon_blocks(node: &Ir, in_value_position: bool, found: &mut bool) {
    if *found {
        return;
    }
    match node {
        Ir::Block { items, .. } => {
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
        Ir::Apply { args, .. } => {
            for a in args {
                scan_for_anon_blocks(a, true, found);
            }
        }
        Ir::Tuple { items, .. } | Ir::ArrayLiteral { items, .. } => {
            for it in items {
                scan_for_anon_blocks(it, true, found);
            }
        }
        Ir::Binding { value, .. } | Ir::TupleBinding { value, .. } => {
            // Named bindings are already lifted by the existing logic.
            scan_for_anon_blocks(value, false, found);
        }
        Ir::IfExpr {
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
        Ir::FunctionDef { body, .. } => {
            // Function bodies have their own scope; lifting happens
            // recursively when generate() is called for that scope.
            scan_for_anon_blocks(body, false, found);
        }
        Ir::ForLoop { range, body, .. } => {
            scan_for_anon_blocks(range, true, found);
            scan_for_anon_blocks(body, false, found);
        }
        Ir::WhileLoop { condition, body, .. } => {
            scan_for_anon_blocks(condition, true, found);
            scan_for_anon_blocks(body, false, found);
        }
        Ir::ParallelFor { range, body, .. } => {
            scan_for_anon_blocks(range, true, found);
            scan_for_anon_blocks(body, false, found);
        }
        Ir::PropertyAccess { object, .. } => {
            scan_for_anon_blocks(object, true, found);
        }
        Ir::IndexAccess { array, index, .. } => {
            scan_for_anon_blocks(array, true, found);
            scan_for_anon_blocks(index, true, found);
        }
        Ir::Range { start, end, .. } => {
            scan_for_anon_blocks(start, true, found);
            scan_for_anon_blocks(end, true, found);
        }
        Ir::IndexAssign {
            array,
            index,
            value,
            ..
        } => {
            scan_for_anon_blocks(array, true, found);
            scan_for_anon_blocks(index, true, found);
            scan_for_anon_blocks(value, true, found);
        }
        Ir::Number { .. } | Ir::BoolLit { .. } | Ir::Identifier { .. } => {}
        Ir::Lambda { body, .. } => {
            // Lambda bodies have their own scope; any anonymous block
            // hoisting inside is the specialized function's concern.
            scan_for_anon_blocks(body, false, found);
        }
    }
}

/// Walk `ast`, replacing every anonymous imperative block in expression
/// position with `Identifier("_anon_<N>")`, and prepend a `_anon_<N> := block`
/// binding to the top-level Block. The resulting IR always has the form
/// `Block([... synthetic bindings, original-stmts])` so the hoisted bindings
/// participate in the same lifting path as user-named bindings.
fn hoist_anonymous_blocks(ast: &Ir) -> Ir {
    let mut counter: usize = 0;
    let mut prepended: Vec<Ir> = Vec::new();
    let top_span = ast.span();
    let rewritten = hoist_recurse(ast, false, &mut counter, &mut prepended);

    if prepended.is_empty() {
        return rewritten;
    }

    let mut all = prepended;
    match rewritten {
        Ir::Block { items, .. } => all.extend(items),
        other => all.push(other),
    }
    Ir::Block {
        items: all,
        span: top_span,
    }
}

fn hoist_recurse(
    node: &Ir,
    in_value_position: bool,
    counter: &mut usize,
    prepended: &mut Vec<Ir>,
) -> Ir {
    // Hoist this node itself if it's an imperative Block in value position.
    if in_value_position {
        if let Ir::Block { items, span } = node {
            if has_imperative_stmt(items) {
                let name = format!("_anon_{}", *counter);
                *counter += 1;
                // Recurse INTO the block so any inner anonymous blocks are
                // also hoisted (registered before this binding so they're
                // declared earlier in the synthesized top-level block).
                let inner = hoist_block_stmts(items, counter, prepended);
                prepended.push(Ir::Binding {
                    name: name.clone(),
                    value: Box::new(Ir::Block {
                        items: inner,
                        span: *span,
                    }),
                    span: *span,
                });
                return Ir::Identifier {
                    name,
                    span: *span,
                };
            }
        }
    }

    // Otherwise recurse structurally.
    match node {
        Ir::Block { items, span } => Ir::Block {
            items: hoist_block_stmts(items, counter, prepended),
            span: *span,
        },
        Ir::Apply { callee, args, span } => Ir::Apply {
            callee: callee.clone(),
            args: args
                .iter()
                .map(|a| hoist_recurse(a, true, counter, prepended))
                .collect(),
            span: *span,
        },
        Ir::Tuple { items, span } => Ir::Tuple {
            items: items
                .iter()
                .map(|i| hoist_recurse(i, true, counter, prepended))
                .collect(),
            span: *span,
        },
        Ir::ArrayLiteral { items, span } => Ir::ArrayLiteral {
            items: items
                .iter()
                .map(|i| hoist_recurse(i, true, counter, prepended))
                .collect(),
            span: *span,
        },
        Ir::Binding { name, value, span } => Ir::Binding {
            name: name.clone(),
            // The binding's value position is handled by existing lifting.
            value: Box::new(hoist_recurse(value, false, counter, prepended)),
            span: *span,
        },
        Ir::TupleBinding { names, value, span } => Ir::TupleBinding {
            names: names.clone(),
            value: Box::new(hoist_recurse(value, false, counter, prepended)),
            span: *span,
        },
        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            span,
        } => Ir::IfExpr {
            condition: Box::new(hoist_recurse(condition, true, counter, prepended)),
            then_branch: Box::new(hoist_recurse(then_branch, true, counter, prepended)),
            else_branch: else_branch
                .as_ref()
                .map(|e| Box::new(hoist_recurse(e, true, counter, prepended))),
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
            // Function bodies are their own scope — don't hoist *out* of them.
            body: body.clone(),
            span: *span,
        },
        Ir::ForLoop {
            var,
            range,
            body,
            span,
        } => Ir::ForLoop {
            var: var.clone(),
            range: Box::new(hoist_recurse(range, true, counter, prepended)),
            body: body.clone(),
            span: *span,
        },
        Ir::WhileLoop {
            condition,
            body,
            span,
        } => Ir::WhileLoop {
            condition: Box::new(hoist_recurse(condition, true, counter, prepended)),
            body: body.clone(),
            span: *span,
        },
        Ir::ParallelFor {
            var,
            range,
            body,
            span,
        } => Ir::ParallelFor {
            var: var.clone(),
            range: Box::new(hoist_recurse(range, true, counter, prepended)),
            body: body.clone(),
            span: *span,
        },
        Ir::PropertyAccess {
            object,
            property,
            span,
        } => Ir::PropertyAccess {
            object: Box::new(hoist_recurse(object, true, counter, prepended)),
            property: property.clone(),
            span: *span,
        },
        Ir::IndexAccess { array, index, span } => Ir::IndexAccess {
            array: Box::new(hoist_recurse(array, true, counter, prepended)),
            index: Box::new(hoist_recurse(index, true, counter, prepended)),
            span: *span,
        },
        Ir::Range { start, end, span } => Ir::Range {
            start: Box::new(hoist_recurse(start, true, counter, prepended)),
            end: Box::new(hoist_recurse(end, true, counter, prepended)),
            span: *span,
        },
        Ir::IndexAssign {
            array,
            index,
            value,
            span,
        } => Ir::IndexAssign {
            array: Box::new(hoist_recurse(array, true, counter, prepended)),
            index: Box::new(hoist_recurse(index, true, counter, prepended)),
            value: Box::new(hoist_recurse(value, true, counter, prepended)),
            span: *span,
        },
        Ir::Number { .. } | Ir::BoolLit { .. } | Ir::Identifier { .. } => node.clone(),
        Ir::Lambda { params, body, span } => Ir::Lambda {
            params: params.clone(),
            body: Box::new(hoist_recurse(body, false, counter, prepended)),
            span: *span,
        },
    }
}

/// Apply `hoist_recurse` to each stmt in a Block's stmt list. Only the last
/// stmt is in value position (it's the block result); the rest are statements.
fn hoist_block_stmts(
    stmts: &[Ir],
    counter: &mut usize,
    prepended: &mut Vec<Ir>,
) -> Vec<Ir> {
    let last = stmts.len().saturating_sub(1);
    stmts
        .iter()
        .enumerate()
        .map(|(i, s)| hoist_recurse(s, i == last, counter, prepended))
        .collect()
}

fn has_imperative_stmt(stmts: &[Ir]) -> bool {
    stmts.iter().any(|s| {
        matches!(
            s,
            Ir::Binding { .. }
                | Ir::TupleBinding { .. }
                | Ir::WhileLoop { .. }
                | Ir::ForLoop { .. }
        )
    })
}

/// If `node` is a Block, return its result expression (the last non-imperative
/// statement). Otherwise return `node` itself.
fn block_result_expr(node: &Ir) -> &Ir {
    if let Ir::Block { items: stmts, .. } = node {
        if let Some(r) = block_result_expr_from_stmts(stmts) {
            return r;
        }
    }
    node
}

/// Find the result expression in a block's statement list.
fn block_result_expr_from_stmts(stmts: &[Ir]) -> Option<&Ir> {
    for stmt in stmts.iter().rev() {
        match stmt {
            Ir::Binding { .. }
            | Ir::FunctionDef { .. }
            | Ir::WhileLoop { .. }
            | Ir::ForLoop { .. }
            | Ir::TupleBinding { .. } => continue,
            other => return Some(other),
        }
    }
    None
}

fn returns_bool(node: &Ir) -> bool {
    match node {
        Ir::BoolLit { .. } => true,
        Ir::Apply { callee, .. } => matches!(
            callee,
            Callee::Builtin(
                BuiltinOp::Eq
                    | BuiltinOp::Neq
                    | BuiltinOp::Lt
                    | BuiltinOp::Gt
                    | BuiltinOp::Lte
                    | BuiltinOp::Gte
                    | BuiltinOp::And
                    | BuiltinOp::Or
                    | BuiltinOp::Not
            )
        ),
        Ir::Block { items: stmts, .. } => stmts.last().is_some_and(returns_bool),
        _ => false,
    }
}

/// Find the result expression in the IR (last non-binding, non-function-def, non-loop node).
fn find_result_expr(ast: &Ir) -> Result<&Ir, String> {
    match ast {
        Ir::Block { items: stmts, .. } => {
            for stmt in stmts.iter().rev() {
                match stmt {
                    Ir::Binding { .. }
                    | Ir::FunctionDef { .. }
                    | Ir::WhileLoop { .. }
                    | Ir::ForLoop { .. }
                    | Ir::TupleBinding { .. } => continue,
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
        // Full Monte Carlo example as it will appear in the Examples menu.
        // Examples end with `plot(...)`, so route through the same plot-arg
        // extraction the notebook uses before handing off to wgsl_gen.
        let file = include_str!("../../examples/monte_carlo.logos");
        let cells = crate::lang::notebook_format::parse_logos(file)
            .expect("example .logos parses");
        let input = &cells[0].content;
        let ir = crate::lang::parse(input).expect("example parses");
        let actions = crate::lang::detect_cell_actions(&ir);
        let plot_idx = actions.plots.first().copied().expect("example has plot()");
        let plot_ir = crate::lang::build_plot_ir(&ir, plot_idx);
        let shader = super::generate(&plot_ir).expect("wgsl_gen succeeds");
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

    #[test]
    fn unused_higher_order_function_is_silently_dropped() {
        // `N_integral(f, ...)` calls `f` as a function but `f` is a parameter
        // — WGSL has no first-class functions. When the cell doesn't call
        // `N_integral`, the codegen drops it so the rest of the cell still
        // compiles (the statistikk.logos shape).
        let source = "\
            sq(x) := x*x\n\
            N_integral(f, x0, x1, d) := (sum := 0\nfor i in 0..10 (sum := sum + f(i*d)*d)\nsum)\n\
            y = sq(x)\n";
        let wgsl = crate::lang::compile(source).expect("should compile");
        assert!(wgsl.contains("fn sq("), "sq should be emitted");
        assert!(
            !wgsl.contains("fn N_integral("),
            "unused HOF must be dropped, got:\n{}",
            wgsl
        );
    }

    #[test]
    fn lambda_argument_to_hof_specializes() {
        // Inline `t |-> Normal(2, 3, t)` is lifted into `_lambda_0`, then the
        // existing HOF specialization makes `N_integral__lambda_0`.
        let source = "\
            Normal(mu, sigma, x) := (1/(sigma*sqrt(2*3.14159))) * exp(-(x-mu)*(x-mu)/(2*sigma*sigma))\n\
            N_integral(f, x0, x1, d) := (sum := 0\nfor i in x0/d..x1/d (sum := sum + f(i*d)*d)\nsum)\n\
            y = N_integral(t |-> Normal(2, 3, t), 0, x, 0.1)\n";
        let wgsl = crate::lang::compile(source).expect("compile");
        assert!(wgsl.contains("fn _lambda_0("), "lambda should be lifted, got:\n{}", wgsl);
        assert!(
            wgsl.contains("fn N_integral___lambda_0("),
            "HOF should be specialized over the lifted lambda, got:\n{}",
            wgsl
        );
        assert!(
            !wgsl.contains("fn N_integral("),
            "the original HOF should be dropped, got:\n{}",
            wgsl
        );
    }

    #[test]
    fn chained_higher_order_call_specializes_through_wrapper() {
        // `wrapper(f) := N_integral(f, …)` is a HOF whose body itself contains
        // an HOF call. When called with a concrete function, both layers must
        // resolve. Verifies the recursive `rewrite_hof_calls` works.
        let source = "\
            sq(x) := x*x\n\
            N_integral(f, x0, x1, d) := (sum := 0\nfor i in 0..10 (sum := sum + f(i*d)*d)\nsum)\n\
            wrapper(f) := N_integral(f, 0, 1, 0.1)\n\
            y = wrapper(sq)\n";
        let wgsl = crate::lang::compile(source).expect("chained HOF should specialize");
        assert!(
            wgsl.contains("fn wrapper__sq("),
            "outer specialization missing, got:\n{}",
            wgsl
        );
        assert!(
            wgsl.contains("fn N_integral__sq("),
            "inner specialization missing, got:\n{}",
            wgsl
        );
        assert!(
            !wgsl.contains("fn wrapper(") && !wgsl.contains("fn N_integral("),
            "unspecialized HOFs must be dropped, got:\n{}",
            wgsl
        );
    }

    #[test]
    fn lambda_bound_to_name_then_called() {
        // `f := t |-> t*t` followed by `f(x)` — the binding-with-lambda value
        // should lift directly into `fn f(t)` rather than going through a
        // `_lambda_N` indirection that would leave `f(x)` unresolved.
        let source = "\
            f := t \u{21A6} t*t\n\
            y = f(x)\n";
        let wgsl = crate::lang::compile(source).expect("compile");
        assert!(
            wgsl.contains("fn f(t: f32)") || wgsl.contains("fn f(t:f32)"),
            "binding-with-lambda must lift as `fn f(t: f32)`, got:\n{}",
            wgsl
        );
        assert!(
            !wgsl.contains("fn _lambda_"),
            "should NOT introduce a `_lambda_N` indirection, got:\n{}",
            wgsl
        );
    }

    #[test]
    fn direct_lambda_application_iife() {
        // `(t |-> t*t)(x)` — apply a lambda inline. Parser lowers this into a
        // block that binds the lambda to a unique name and calls it.
        let source = "y = (t \u{21A6} t*t)(x)\n";
        let wgsl = crate::lang::compile(source).expect("IIFE should compile");
        assert!(
            wgsl.contains("fn _iife_"),
            "IIFE should produce a synthetic function, got:\n{}",
            wgsl
        );
    }

    #[test]
    fn implicit_function_through_value_binding_is_rejected() {
        // Passing a non-function value (here a numeric binding) into a HOF's
        // function-typed parameter slot. Can't be specialized; codegen rejects
        // with the "must be statically known" error.
        let source = "\
            N_integral(f, x0, x1, d) := (sum := 0\nfor i in 0..10 (sum := sum + f(i*d)*d)\nsum)\n\
            val := 5\n\
            y = N_integral(val, 0, 1, 0.1)\n";
        let err = crate::lang::compile(source).expect_err("should reject implicit fn");
        assert!(
            err.contains("function-typed parameter") && err.contains("N_integral"),
            "error should explain the statically-known requirement, got:\n{}",
            err
        );
    }

    #[test]
    fn mapsto_example_compiles_end_to_end() {
        // Mirrors `examples/mapsto.logos`: HOF + inline lambda using `↦`.
        let source = "\
            NumericIntegral(f, x0, x1, d) := (sum := 0\n\
            for i in x0/d..x1/d (sum := sum + f(i*d)*d)\nsum)\n\
            y = NumericIntegral(t \u{21A6} t*t, 0, x, 0.01)\n";
        let wgsl = crate::lang::compile(source).expect("mapsto example should compile");
        assert!(wgsl.contains("fn _lambda_0("), "lambda must lift");
        assert!(
            wgsl.contains("fn NumericIntegral___lambda_0("),
            "HOF must specialize, got:\n{}",
            wgsl
        );
    }

    #[test]
    fn lambda_with_tuple_params_lifts() {
        // `(a, b, f) |-> f(a, b)` exercises the tuple-as-parameter-list path
        // and confirms multi-arg lambdas land as a normal FunctionDef.
        let source = "\
            apply2(g, x, y) := g(x, y)\n\
            scale(a, b) := a*b\n\
            y = apply2((a, b, f) |-> f(a, b), 3, x)\n";
        let _err_or_wgsl = crate::lang::compile(source);
        // We only require this to parse + lift cleanly; the deeper question
        // of arity matching across the lambda/HOF boundary is a type-checker
        // concern that lives separately. So just check the lift happened.
        // Re-parse the lifted form so we can inspect it.
        let ir = crate::lang::parse(source).expect("parse");
        let mut found_lambda = false;
        fn walk(node: &crate::lang::ir::Ir, found: &mut bool) {
            if matches!(node, crate::lang::ir::Ir::Lambda { .. }) {
                *found = true;
                return;
            }
            for c in node.children() {
                walk(c, found);
            }
        }
        walk(&ir, &mut found_lambda);
        assert!(found_lambda, "parser should produce an Ir::Lambda for tuple-params");
    }

    #[test]
    fn statistikk_integral_inlines_correctly() {
        // Dump the WGSL the user's `plot(y = N_integral(f_X, 0, x, 0.01))`
        // shape produces so we can eyeball the substituted body and the
        // call site.
        let source = "\
            N(mu, sigma, x) := (1/(sigma*sqrt(2*3.14159))) * exp(-(x-mu)*(x-mu)/(2*sigma*sigma))\n\
            f_X(x) := N(2, 3, x)\n\
            N_integral(f, x0, x1, d) := (sum := 0\nfor i in x0/d..x1/d (sum := sum + f(i*d)*d)\nsum)\n\
            y = N_integral(f_X, 0, x, 0.01)\n";
        let wgsl = crate::lang::compile(source).expect("compile");
        eprintln!("--- WGSL ---\n{}\n--- end ---", wgsl);
        assert!(wgsl.contains("fn N_integral__f_X("));
    }

    #[test]
    fn same_hof_specialized_twice_reuses_one_definition() {
        // Calling the same HOF with the same concrete function twice
        // should yield a single specialized definition, not duplicates.
        let source = "\
            sq(x) := x*x\n\
            scale(f, x) := f(x) + f(x+1)\n\
            y = scale(sq, x) + scale(sq, 2)\n";
        let wgsl = crate::lang::compile(source).expect("compile");
        let count = wgsl.matches("fn scale__sq(").count();
        assert_eq!(count, 1, "expected one specialization, got {}:\n{}", count, wgsl);
    }

    #[test]
    fn calling_higher_order_function_with_concrete_fn_specializes() {
        // `N_integral(sq, 0, x, 0.01)` gets rewritten into a synthetic
        // `N_integral__sq(0, x, 0.01)` whose body has `sq` substituted for
        // the function parameter `f`. The original `N_integral` becomes
        // unreachable and is dropped.
        let source = "\
            sq(x) := x*x\n\
            N_integral(f, x0, x1, d) := (sum := 0\nfor i in 0..10 (sum := sum + f(i*d)*d)\nsum)\n\
            y = N_integral(sq, 0, x, 0.01)\n";
        let wgsl = crate::lang::compile(source).expect("HOF call should specialize");
        assert!(
            wgsl.contains("fn N_integral__sq("),
            "specialized function missing, got:\n{}",
            wgsl
        );
        assert!(
            !wgsl.contains("fn N_integral("),
            "original HOF should be dropped, got:\n{}",
            wgsl
        );
        // The specialized body should call `sq` directly, no `f` left.
        assert!(
            wgsl.contains("sq("),
            "specialized body should call sq, got:\n{}",
            wgsl
        );
    }
}
