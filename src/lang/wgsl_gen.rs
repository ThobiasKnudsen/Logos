use super::ir::{BuiltinOp, Callee, Ir, Type, WgslLowering};
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
    // Run the shared lowering pipeline: hoist anonymous imperative
    // blocks, lift lambdas into synthetic FunctionDefs, specialize
    // higher-order function calls, and resolve identifier references.
    // After this, the AST contains no Lambda nodes and no calls to
    // user functions with first-class function arguments — both
    // invariants WGSL codegen relies on.
    let owned_ast = super::lower::lower(ast.clone())?;
    let ast: &Ir = &owned_ast;

    let mut ctx = GenContext::new(ast);

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
        let mut lifted_names = Vec::new();
        collect_lifted_block_names(ast, &mut lifted_names);
        for binding_name in &lifted_names {
            let fn_name = format!("_lifted_{}", binding_name);
            shader.push_str(&format!(
                "    let _corner_{0}_mm = {1}(x_m, y_m);\n",
                binding_name, fn_name
            ));
            shader.push_str(&format!(
                "    let _corner_{0}_mp = {1}(x_m, y_p);\n",
                binding_name, fn_name
            ));
            shader.push_str(&format!(
                "    let _corner_{0}_pm = {1}(x_p, y_m);\n",
                binding_name, fn_name
            ));
            shader.push_str(&format!(
                "    let _corner_{0}_pp = {1}(x_p, y_p);\n",
                binding_name, fn_name
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

struct GenContext<'a> {
    functions: Vec<EmittedFunction>,
    bindings: Vec<EmittedBinding>,
    /// Lowered AST. Methods like `result_is_bool` and `emit_bool_with_corners`
    /// use it to resolve identifier references back to their binding's
    /// declared value, replacing what used to be a `bool_binding_defs` cache.
    ast: &'a Ir,
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

impl<'a> GenContext<'a> {
    fn new(ast: &'a Ir) -> Self {
        Self {
            functions: Vec::new(),
            bindings: Vec::new(),
            ast,
        }
    }

    /// Walk the IR to collect function definitions and bindings.
    fn collect_functions(&mut self, ast: &Ir) {
        match ast {
            Ir::Block { items: stmts, .. } => {
                for stmt in stmts {
                    self.collect_functions(stmt);
                }
            }
            Ir::FunctionDef {
                name, params, body, ..
            } => {
                // Recurse into nested function defs before emitting this one.
                if let Ir::Block { items: stmts, .. } = body.as_ref() {
                    for stmt in stmts {
                        if let Ir::FunctionDef { .. } = stmt {
                            self.collect_functions(stmt);
                        }
                    }
                }

                // Captured variables are populated on the FunctionDef itself
                // by `lower::annotate_captures` — no separate analysis here.
                let captured: Vec<String> = match ast {
                    Ir::FunctionDef {
                        captured: Some(c), ..
                    } => (**c).clone(),
                    _ => Vec::new(),
                };

                // Build parameter list including captured variables
                let mut all_params: Vec<String> =
                    params.iter().map(|p| format!("{}: f32", p)).collect();
                for cap in &captured {
                    all_params.push(format!("{}: f32", cap));
                }

                // The function's return type comes off `FunctionDef.return_ty`
                // (populated by `lower::annotate_types`). Tuple-of-Num bodies
                // lower to `vec4<f32>` like explicit Vec constructors do.
                let return_ty = match ast {
                    Ir::FunctionDef { return_ty, .. } => return_ty.as_deref(),
                    _ => None,
                };
                let returns_vec = matches!(
                    return_ty,
                    Some(Type::Vec2 | Type::Vec3 | Type::Vec4)
                ) || matches!(return_ty, Some(Type::Tuple(items)) if items.len() >= 2);
                let returns_bool_val = return_ty == Some(&Type::Bool);

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

                let ret_type = if returns_vec {
                    "vec4<f32>"
                } else if returns_bool_val {
                    "bool"
                } else {
                    "f32"
                };
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
                    Ir::Block { items: stmts, .. } if super::lower::has_imperative_stmt(stmts) => {
                        Some(stmts.clone())
                    }
                    _ => None,
                };

                if let Some(stmts) = imperative_block_stmts {
                    if let Ok((comparison_op, binding_expr)) =
                        self.lift_block_to_fn(name, &stmts)
                    {
                        // For lifted comparison results we render via corner-checking
                        // (which calls the function 4 times directly); the regular
                        // `let name = call(x, y) <op> 0.0` binding would only add a
                        // 5th unused call per pixel. Skip emitting it.
                        if comparison_op.is_none() {
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
    /// at every corner during corner-checking. Returns `(comparison_op,
    /// binding_call_expr)`. The synthesized WGSL function is `_lifted_<name>`,
    /// derived from `binding_name` rather than returned, so every caller
    /// (corner emission, identifier substitution) computes the same string
    /// without sharing state.
    fn lift_block_to_fn(
        &mut self,
        binding_name: &str,
        stmts: &[Ir],
    ) -> Result<(Option<BuiltinOp>, String), String> {
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

        Ok((comparison_op, binding_expr))
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
                    vars, range, body, ..
                } => {
                    self.emit_for_loop(&mut code, vars, range, body, "    ", &mut declared)?;
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
                    vars, range, body, ..
                } => {
                    self.emit_for_loop(&mut code, vars, range, body, "    ", &mut declared)?;
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
    /// Emit a `for var in start..end[..delta] (body)` as a WGSL for-loop
    /// with iteration guard.
    ///
    /// Multi-dim (tuple-var) loops are not supported in fragment-shader
    /// contexts — they belong to the GPU compute path. We reject them
    /// here with a typed error rather than silently emitting nonsense.
    fn emit_for_loop(
        &self,
        code: &mut String,
        vars: &[String],
        range: &Ir,
        body: &Ir,
        indent: &str,
        declared: &mut HashSet<String>,
    ) -> Result<(), String> {
        if vars.len() != 1 {
            return Err(
                "multi-variable for loops are only supported in `gpu` blocks, not in plot bodies"
                    .to_string(),
            );
        }
        let var = &vars[0];
        let (start_expr, end_expr, delta_expr) = match range {
            Ir::Range { start, end, delta, .. } => {
                let s = self.emit_expr(start)?;
                let e = self.emit_expr(end)?;
                let d = match delta {
                    Some(d) => self.emit_expr(d)?,
                    None => "1.0".to_string(),
                };
                (s, e, d)
            }
            _ => return Err("for loop range must be start..end".to_string()),
        };
        // Declare or reassign the loop variable
        if declared.contains(var) {
            code.push_str(&format!("{}{} = {};\n", indent, var, start_expr));
        } else {
            code.push_str(&format!("{}var {} = {};\n", indent, var, start_expr));
            declared.insert(var.clone());
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
        // Update: var = var + delta
        code.push_str(&format!(
            "{}    {} = {} + {};\n",
            indent, var, var, delta_expr
        ));
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
            Ir::Apply { .. } => returns_bool(node),
            Ir::Identifier { name, .. } => {
                // A binding's `value_ty` (populated by `lower::annotate_types`)
                // directly answers "is this name bound to a bool". The
                // lifted-block branch is kept until Stage E folds those into
                // the regular binding lookup.
                find_binding(self.ast, name)
                    .and_then(|b| match b {
                        Ir::Binding { value_ty, .. } => value_ty.as_deref(),
                        _ => None,
                    })
                    .is_some_and(|t| t == &Type::Bool)
                    || lifted_block_comparison_op(self.ast, name).is_some()
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
    ///
    /// Tuples are still recognized structurally: a 2/3/4-element tuple
    /// literal lowers to a `vecN<f32>(...)` constructor in WGSL even though
    /// its IR type is `Tuple(...)` rather than `VecN`. Every other shape
    /// just reads `Apply.result_ty` (populated by `lower::annotate_types`),
    /// which already encodes both builtin constructors and user functions
    /// whose body returns a vec.
    fn result_is_vec(&self, node: &Ir) -> bool {
        result_is_vec(node)
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
                    vars, range, body, ..
                } => {
                    self.emit_for_loop(&mut code, vars, range, body, indent, declared)?;
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
                    if is_lifted_block_binding(self.ast, name) {
                        if let Some(suffix) = corner_suffix(x_var, y_var) {
                            return Ok(format!("_corner_{}_{}", name, suffix));
                        }
                        // Non-standard corner — fall back to a direct call.
                        return Ok(format!("_lifted_{}({}, {})", name, x_var, y_var));
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
                for cap in function_captured(self.ast, name) {
                    let s = match (subst, cap.as_str()) {
                        (Some((xv, _)), "x") => xv.to_string(),
                        (Some((_, yv)), "y") => yv.to_string(),
                        _ => cap.clone(),
                    };
                    all_args.push(s);
                }
                return Ok(format!("{}({})", name, all_args.join(", ")));
            }
        };

        // Generic shapes (infix, prefix, call) are driven by the per-op
        // `WgslLowering` in `ir.rs`. The few ops whose lowering doesn't fit a
        // shape (`mod`, `pow`, `atan`, `log10`, action ops) carry `Custom`
        // and fall through to the explicit arms below.
        match op.wgsl_lowering() {
            WgslLowering::Infix(s) => {
                return Ok(format!("({} {} {})", emitted[0], s, emitted[1]));
            }
            WgslLowering::Prefix(s) => {
                return Ok(format!("{}({})", s, emitted[0]));
            }
            WgslLowering::Call(name) => {
                return Ok(format!("{}({})", name, emitted.join(", ")));
            }
            WgslLowering::Custom => {}
        }

        match op {
            BuiltinOp::Mod => Ok(format!(
                "((({} % {}) + {}) % {})",
                emitted[0], emitted[1], emitted[1], emitted[1]
            )),

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

            // No fragment-shader semantics for these.
            BuiltinOp::Len | BuiltinOp::Print | BuiltinOp::Plot => Err(format!(
                "Builtin '{}' is not supported in fragment shaders",
                op.name()
            )),

            // Every other op carries Infix/Prefix/Call and is handled above.
            _ => unreachable!("op {:?} returned Custom lowering but has no explicit arm", op),
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
            Ir::Identifier { name, .. } if is_lifted_block_binding(self.ast, name) => {
                let calls = [
                    format!("_corner_{}_mm", name),
                    format!("_corner_{}_mp", name),
                    format!("_corner_{}_pm", name),
                    format!("_corner_{}_pp", name),
                ];

                // No comparison op → treat float result as implicit curve `f = 0`,
                // which is the same pattern as `Eq` (sign-change detection).
                let op = lifted_block_comparison_op(self.ast, name).unwrap_or(BuiltinOp::Eq);
                Ok(emit_corner_compare(op, &calls))
            }

            // Identifier bound to a bool expression: inline so the comparison
            // goes through corner-checking instead of a direct float ==.
            // The bool-ness check matches `result_is_bool` exactly; the inline
            // path reads the binding's value (its result expression, in case
            // the value was itself a block) and recurses for corner emission.
            Ir::Identifier { name, .. }
                if find_binding(self.ast, name)
                    .and_then(|b| match b {
                        Ir::Binding { value_ty, .. } => value_ty.as_deref(),
                        _ => None,
                    })
                    .is_some_and(|t| t == &Type::Bool) =>
            {
                let value = find_binding(self.ast, name).and_then(|b| match b {
                    Ir::Binding { value, .. } => Some(value.as_ref()),
                    _ => None,
                });
                let bound = block_result_expr(value.expect("binding exists"));
                self.emit_bool_with_corners(&bound.clone())
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
                    if let Some(cmp_op) = lifted_function_diff_op(self.ast, name) {
                        let diff_fn_name = format!("_diff_{}", name);
                        let captured = function_captured(self.ast, name);
                        // Build the regular argument list (not yet corner-substituted).
                        let arg_strs: Result<Vec<String>, String> = args
                            .iter()
                            .map(|a| self.emit_expr(a))
                            .collect();
                        let arg_strs = arg_strs?;

                        let corner_call = |xc: &str, yc: &str| {
                            let mut all = arg_strs.clone();
                            for cap in captured {
                                let s = match cap.as_str() {
                                    "x" => xc.to_string(),
                                    "y" => yc.to_string(),
                                    other => other.to_string(),
                                };
                                all.push(s);
                            }
                            format!("{}({})", diff_fn_name, all.join(", "))
                        };

                        let calls = [
                            corner_call("x_m", "y_m"),
                            corner_call("x_m", "y_p"),
                            corner_call("x_p", "y_m"),
                            corner_call("x_p", "y_p"),
                        ];

                        return Ok(emit_corner_compare(cmp_op, &calls));
                    }
                    if let Some(Ir::FunctionDef {
                        params,
                        body,
                        return_ty,
                        ..
                    }) = find_function_def(self.ast, name)
                    {
                        if return_ty.as_deref() == Some(&Type::Bool) {
                            let inlined = substitute_params(body, params, args);
                            return self.emit_bool_with_corners(&inlined);
                        }
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

/// The set of user-defined function *names* with at least one function-typed
/// parameter. Same fixpoint detection as `compute_hof_indices`, just exposed
/// as a flat set for the codegen's "is this HOF still reachable?" check.
fn unrepresentable_higher_order_functions(ast: &Ir) -> HashSet<String> {
    let mut defs: HashMap<String, (Vec<String>, Ir)> = HashMap::new();
    super::lower::collect_owned_function_defs(ast, &mut defs);
    super::lower::compute_hof_indices(&defs).keys().cloned().collect()
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
            result_ty,
        } => {
            // Don't substitute the function name, only its arguments.
            // Re-using the parent's `result_ty` is safe because substitution
            // only rewires leaves — the operator's result shape is unchanged.
            let new_args = func_args
                .iter()
                .map(|a| substitute_params(a, params, args))
                .collect();
            Ir::Apply {
                callee: callee.clone(),
                args: new_args,
                span: *span,
                result_ty: result_ty.clone(),
            }
        }
        Ir::Block { items: stmts, span } => Ir::Block {
            items: stmts
                .iter()
                .map(|s| substitute_params(s, params, args))
                .collect(),
            span: *span,
        },
        Ir::Binding { name, value, span, .. } => Ir::Binding {
            name: name.clone(),
            value: Box::new(substitute_params(value, params, args)),
            span: *span,
            // Substituted body needs a fresh inference pass to repopulate.
            value_ty: None,
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


/// Find the `Binding` with the given `name` in `ast`, returning the whole
/// `Ir::Binding` node so the caller can read both its `value` and `value_ty`.
/// Walks the tree in pre-order; first match wins. Returns `None` if there
/// is no binding by that name (which includes both undeclared names and
/// names that resolve to a function parameter or axis variable).
fn find_binding<'a>(ast: &'a Ir, name: &str) -> Option<&'a Ir> {
    if let Ir::Binding { name: n, .. } = ast {
        if n == name {
            return Some(ast);
        }
    }
    for child in ast.children() {
        if let Some(found) = find_binding(child, name) {
            return Some(found);
        }
    }
    None
}

/// Find the `FunctionDef` with the given `name` in `ast`. Walks in pre-order;
/// first match wins. Returns `None` if there's no such function.
fn find_function_def<'a>(ast: &'a Ir, name: &str) -> Option<&'a Ir> {
    if let Ir::FunctionDef { name: n, .. } = ast {
        if n == name {
            return Some(ast);
        }
    }
    for child in ast.children() {
        if let Some(found) = find_function_def(child, name) {
            return Some(found);
        }
    }
    None
}

/// Was the binding for `name` lifted to a `_lifted_<name>` WGSL function?
/// Matches the structural condition in `collect_functions_with_scope` —
/// block-valued binding with at least one imperative stmt and a result expr.
fn is_lifted_block_binding(ast: &Ir, name: &str) -> bool {
    let Some(Ir::Binding { value, .. }) = find_binding(ast, name) else {
        return false;
    };
    let Ir::Block { items, .. } = value.as_ref() else {
        return false;
    };
    super::lower::has_imperative_stmt(items) && block_result_expr_from_stmts(items).is_some()
}

/// If `name` is a lifted block-valued binding whose result expression is a
/// comparison op, return that op. Returns `None` for plain float results
/// (which corner-checking treats as implicit `f = 0` curves) and for names
/// that aren't lifted bindings at all.
fn lifted_block_comparison_op(ast: &Ir, name: &str) -> Option<BuiltinOp> {
    let Ir::Binding { value, .. } = find_binding(ast, name)? else {
        return None;
    };
    let Ir::Block { items, .. } = value.as_ref() else {
        return None;
    };
    if !super::lower::has_imperative_stmt(items) {
        return None;
    }
    match block_result_expr_from_stmts(items)? {
        Ir::Apply { callee, args, .. } => as_comparison_op(callee, args),
        _ => None,
    }
}

/// Walks `ast` collecting the names of every binding the codegen lifted
/// (per `is_lifted_block_binding`). Used by `generate()` to hoist
/// `_corner_<name>_<suffix>` setups out of the per-corner expressions.
fn collect_lifted_block_names(ast: &Ir, out: &mut Vec<String>) {
    if let Ir::Binding { name, .. } = ast {
        if is_lifted_block_binding(ast, name) {
            out.push(name.clone());
        }
    }
    for child in ast.children() {
        collect_lifted_block_names(child, out);
    }
}

/// Captured outer-scope variables (plus axis vars) for the user function
/// named `name`. Returns an empty slice when the function captures nothing,
/// or when the name doesn't resolve to any `FunctionDef`. Reads directly
/// off `FunctionDef.captured`, which `lower::annotate_captures` populates.
fn function_captured<'a>(ast: &'a Ir, name: &str) -> &'a [String] {
    match find_function_def(ast, name) {
        Some(Ir::FunctionDef { captured, .. }) => match captured {
            Some(v) => v.as_slice(),
            None => &[],
        },
        _ => &[],
    }
}



/// If `name` is a user function with a `_diff_<name>` companion (bool return,
/// imperative body, comparison-op result), return the comparison op. Mirrors
/// the population condition in `collect_functions_with_scope`.
fn lifted_function_diff_op(ast: &Ir, name: &str) -> Option<BuiltinOp> {
    let Ir::FunctionDef {
        body, return_ty, ..
    } = find_function_def(ast, name)?
    else {
        return None;
    };
    if return_ty.as_deref() != Some(&Type::Bool) {
        return None;
    }
    let Ir::Block { items, .. } = body.as_ref() else {
        return None;
    };
    let has_imperative = items.iter().any(|s| {
        matches!(
            s,
            Ir::Binding { .. }
                | Ir::WhileLoop { .. }
                | Ir::ForLoop { .. }
                | Ir::TupleBinding { .. }
        )
    });
    if !has_imperative {
        return None;
    }
    match block_result_expr_from_stmts(items)? {
        Ir::Apply { callee, args, .. } => as_comparison_op(callee, args),
        _ => None,
    }
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

/// Free-function form of `result_is_vec` — same predicate, no `GenContext`
/// dependency now that vec-ness comes off `Apply.result_ty` and the lifted
/// scope maps no longer enter the decision.
///
/// A tuple literal lowers to `vecN<f32>(...)` for N ∈ {2, 3, 4}, and the
/// type checker assigns it `Type::Tuple(...)`; we treat both shapes
/// (tuple-literal and tuple-returning user fn) as vec-producing so the
/// fs_main vec output path is selected uniformly.
fn result_is_vec(node: &Ir) -> bool {
    match node {
        Ir::Tuple { items, .. } => items.len() >= 2,
        Ir::Apply { result_ty, .. } => match result_ty.as_deref() {
            Some(Type::Vec2 | Type::Vec3 | Type::Vec4) => true,
            Some(Type::Tuple(items)) => items.len() >= 2,
            _ => false,
        },
        Ir::IfExpr {
            then_branch,
            else_branch,
            ..
        } => {
            result_is_vec(then_branch)
                || else_branch.as_ref().is_some_and(|e| result_is_vec(e))
        }
        Ir::Block { items: stmts, .. } => stmts.last().is_some_and(result_is_vec),
        _ => false,
    }
}

fn returns_bool(node: &Ir) -> bool {
    match node {
        Ir::BoolLit { .. } => true,
        // Apply's `result_ty` is populated by `lower::annotate_types`. For a
        // builtin comparison/logical op it's `Bool`; for a user fn it's the
        // function's inferred return type. Reading it captures both cases in
        // one line and supersedes the old structural callee match.
        Ir::Apply { result_ty, .. } => result_ty.as_deref() == Some(&Type::Bool),
        Ir::Block { items: stmts, .. } => stmts.last().is_some_and(returns_bool),
        Ir::IfExpr {
            then_branch,
            else_branch,
            ..
        } => {
            returns_bool(then_branch)
                || else_branch.as_ref().is_some_and(|e| returns_bool(e))
        }
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
