//! CPU evaluator and canonical WGSL for the vertex-plot path (issue #28).
//!
//! When `plot()`'s first arg is an array of 2D points, the notebook
//! detects that here and produces a `ShaderSpec` carrying both the
//! prebuilt vertex+fragment WGSL and the materialized vertex buffer.
//! The renderer then drives a vertex pipeline instead of the
//! fullscreen-fragment one used by analytic curves and surfaces.
//!
//! The CPU evaluator is intentionally minimal — it walks array
//! literals of 2-tuples whose components are constant-foldable.
//! This is enough for the explicit form in issue #28
//! (`plot([(0, 0), (1, 1), (2, 0)])`); more elaborate producers
//! (loop bodies, GPU-resident vertex buffers) land alongside the
//! interpreter / compute-pipeline integration described in issue #26.

use std::collections::HashMap;

use crate::lang::ir::{BuiltinOp, Callee, Ir};

use super::shader::VertexData;

/// Try to materialize `arg` as a CPU-side vertex array. Returns `None`
/// when the arg isn't an obvious vertex producer — callers should
/// fall through to the analytic plot path in that case.
///
/// `combined` is the cell's combined IR up to and including the plot
/// statement; we walk preceding top-level bindings so identifier
/// references like `verts := …; plot(verts)` resolve.
pub fn try_evaluate(combined: &Ir, arg: &Ir) -> Option<VertexData> {
    let bindings = collect_vertex_bindings(combined);
    let positions = eval_vertex_expr(arg, &bindings)?;
    Some(VertexData::from_positions(positions))
}

/// Walk every top-level `name := <vertex array>` binding in `combined`,
/// stashing the materialized positions so a later `plot(name)` reference
/// can resolve. Bindings whose RHS isn't a constant-foldable array of
/// 2-tuples are silently skipped — the caller's `eval_vertex_expr`
/// returns `None` for them when probed.
fn collect_vertex_bindings(combined: &Ir) -> HashMap<String, Vec<[f32; 2]>> {
    let mut out = HashMap::new();
    let stmts: &[Ir] = match combined {
        Ir::Block { items, .. } => items.as_slice(),
        single => std::slice::from_ref(single),
    };
    for stmt in stmts {
        if let Ir::Binding { name, value, .. } = stmt {
            if let Some(positions) = eval_vertex_expr(value, &out) {
                out.insert(name.clone(), positions);
            }
        }
    }
    out
}

/// Evaluate an IR node as `Vec<[f32; 2]>`. Returns `None` for any
/// shape the constant evaluator doesn't recognize — callers should
/// fall back to the analytic plot path.
fn eval_vertex_expr(node: &Ir, bindings: &HashMap<String, Vec<[f32; 2]>>) -> Option<Vec<[f32; 2]>> {
    match node {
        Ir::Identifier { name, .. } => bindings.get(name).cloned(),
        Ir::ArrayLiteral { items, .. } => {
            let mut positions = Vec::with_capacity(items.len());
            for item in items {
                let tup = match item {
                    Ir::Tuple { items, .. } => items,
                    _ => return None,
                };
                if tup.len() != 2 {
                    return None;
                }
                let x = eval_const_num(&tup[0])?;
                let y = eval_const_num(&tup[1])?;
                positions.push([x, y]);
            }
            Some(positions)
        }
        Ir::Block { items, .. } => items.last().and_then(|last| eval_vertex_expr(last, bindings)),
        _ => None,
    }
}

/// Constant-fold a numeric expression to `f32`. Covers literals,
/// unary negation, and the four basic arithmetic ops — enough for the
/// "(sin(0.1), cos(0.1))"-style explicit lists without dragging in the
/// full interpreter. Anything else returns `None`.
fn eval_const_num(node: &Ir) -> Option<f32> {
    match node {
        Ir::Number { value, .. } => Some(*value as f32),
        Ir::Apply { callee, args, .. } => {
            let op = match callee {
                Callee::Builtin(op) => *op,
                _ => return None,
            };
            match (op, args.as_slice()) {
                (BuiltinOp::Neg, [a]) => eval_const_num(a).map(|v| -v),
                (BuiltinOp::Add, [a, b]) => {
                    Some(eval_const_num(a)? + eval_const_num(b)?)
                }
                (BuiltinOp::Sub, [a, b]) => {
                    Some(eval_const_num(a)? - eval_const_num(b)?)
                }
                (BuiltinOp::Mul, [a, b]) => {
                    Some(eval_const_num(a)? * eval_const_num(b)?)
                }
                (BuiltinOp::Div, [a, b]) => {
                    let denom = eval_const_num(b)?;
                    (denom != 0.0).then_some(eval_const_num(a)? / denom)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Canonical vertex+fragment WGSL for vertex plots. The vertex shader
/// maps each `vec2<f32>` position from world coordinates into clip
/// space using the standard `axis_min` / `axis_max` uniform so vertex
/// plots pan and zoom with the render area just like analytic shaders
/// do. The fragment shader emits the cell's primary color, premulti-
/// plied to match the rest of the pipeline's blend setup.
///
/// **Y convention**: `render::frame` passes `axis_bounds` to the shader
/// pipeline as `[x_min, y_MAX, x_max, y_MIN]` — y is intentionally
/// swapped (see `src/render/frame.rs` near `shader_pipeline.render`)
/// so the analytic shader's already-y-flipped UV math ends up with
/// world-y-up rendering. That means `u.axis_min.y` is the math y at
/// the *top* of the screen and `u.axis_max.y` is the math y at the
/// *bottom*. The vertex shader compensates by inverting the y leg of
/// the NDC mapping; without that, a vertex at `(0, 1)` would render
/// below a vertex at `(0, 0)`.
pub const VERTEX_PLOT_WGSL: &str = r#"struct Uniforms {
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

@group(0) @binding(0) var<uniform> u: Uniforms;

@vertex
fn vs_main(@location(0) world_pos: vec2<f32>) -> @builtin(position) vec4<f32> {
    let range = u.axis_max - u.axis_min;
    let normalized = (world_pos - u.axis_min) / range;
    // X: standard [0, 1] -> [-1, 1] mapping.
    // Y: inverted because `u.axis_min.y` is the math y at the *top* of
    // the viewport (see the comment on `VERTEX_PLOT_WGSL` above); without
    // this flip vertex plots render upside-down relative to the analytic
    // plot path.
    let ndc = vec2<f32>(normalized.x * 2.0 - 1.0, 1.0 - normalized.y * 2.0);
    return vec4<f32>(ndc, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    let a = u.primary_color.a;
    return vec4<f32>(u.primary_color.rgb * a, a);
}
"#;

/// Vertex+fragment WGSL for a vertex plot with a user-supplied color
/// expression (issue #46). The pipeline is the same as
/// `VERTEX_PLOT_WGSL` except that:
///
/// - The vertex shader passes `world_pos` to the fragment as a
///   varying, so each fragment knows the world-space coordinate of
///   the line segment it is rasterizing.
/// - The fragment shader calls a synthesized `_plot_color_<span>`
///   function (lifted from the user's color expression by
///   `canonicalize_plot_color` + `wgsl_gen`) and uses its `vec4<f32>`
///   result in place of `u.primary_color`.
///
/// The function definition itself is shared between vertex plots and
/// analytic plots — we synthesize a trivial analytic IR carrying just
/// the color and let `wgsl_gen::generate` lift, type-check, and emit
/// it; then we strip its `@fragment fn fs_main` and append our own.
/// Going through `wgsl_gen` instead of re-implementing the emit logic
/// here means feature parity (captures, time uniform, cell bindings)
/// comes for free as those evolve in the analytic path.
pub fn shader_with_color(color_arg: &Ir) -> Result<String, String> {
    // Build a synthetic analytic IR: the color expression bound to a
    // synthesized name, followed by a trivial numeric result. The
    // numeric result keeps `is_vec` false in `wgsl_gen::generate`, which
    // is the only path that emits `let _color = _plot_color_<…>(…);` —
    // exactly the call expression we want to lift back out.
    let color_binding = crate::lang::canonicalize_plot_color(color_arg);
    let synth_ir = Ir::Block {
        items: vec![
            color_binding,
            Ir::Number {
                value: 0.0,
                span: (0, 0),
            },
        ],
        span: (0, 0),
    };

    // `wgsl_gen::generate` returns a `Diagnostic`; render it as a bare
    // string for this internal codegen path so the rest of the function
    // can keep its `Result<String, String>` shape. The synth IR has
    // no real source, so spans wouldn't add useful context here.
    let analytic_wgsl =
        crate::lang::wgsl_gen::generate(&synth_ir).map_err(|d| d.message)?;

    // Keep everything up to (but not including) the analytic fs_main:
    // uniform struct, bind-group declaration, helper functions, and the
    // synthesized `fn _plot_color_<span>` definition.
    let fs_split = analytic_wgsl
        .find("@fragment")
        .ok_or_else(|| "internal: synthesized shader missing @fragment".to_string())?;
    let preamble = &analytic_wgsl[..fs_split];

    // Pull the color-call expression off the analytic fs_main's
    // `let _color = …;` line so the call signature (params + appended
    // captures) is exactly what `wgsl_gen` produces. Re-deriving this
    // by walking the AST would duplicate `emit_apply` and quickly drift.
    let let_marker = "let _color = ";
    let lc_start = analytic_wgsl
        .find(let_marker)
        .ok_or_else(|| "internal: synthesized shader missing `_color` binding".to_string())?;
    let value_start = lc_start + let_marker.len();
    let value_end = analytic_wgsl[value_start..]
        .find(';')
        .ok_or_else(|| "internal: malformed `_color` binding".to_string())?
        + value_start;
    let call_expr = &analytic_wgsl[value_start..value_end];

    Ok(format!(
        "{preamble}\n\
         struct VsOut {{\n\
         \x20   @builtin(position) position: vec4<f32>,\n\
         \x20   @location(0) world: vec2<f32>,\n\
         }};\n\
         \n\
         @vertex\n\
         fn vs_main(@location(0) world_pos: vec2<f32>) -> VsOut {{\n\
         \x20   let range = u.axis_max - u.axis_min;\n\
         \x20   let normalized = (world_pos - u.axis_min) / range;\n\
         \x20   let ndc = vec2<f32>(normalized.x * 2.0 - 1.0, 1.0 - normalized.y * 2.0);\n\
         \x20   var out: VsOut;\n\
         \x20   out.position = vec4<f32>(ndc, 0.0, 1.0);\n\
         \x20   out.world = world_pos;\n\
         \x20   return out;\n\
         }}\n\
         \n\
         @fragment\n\
         fn fs_main(in: VsOut) -> @location(0) vec4<f32> {{\n\
         \x20   let x = in.world.x;\n\
         \x20   let y = in.world.y;\n\
         \x20   let _color = {call_expr};\n\
         \x20   let a = _color.a;\n\
         \x20   return vec4<f32>(_color.rgb * a, a);\n\
         }}\n"
    ))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::parse;

    fn first_plot_arg(ir: &Ir) -> &Ir {
        let stmts: &[Ir] = match ir {
            Ir::Block { items, .. } => items.as_slice(),
            single => std::slice::from_ref(single),
        };
        for stmt in stmts {
            if let Ir::Apply {
                callee: Callee::Builtin(BuiltinOp::Plot),
                args,
                ..
            } = stmt
            {
                return &args[0];
            }
        }
        panic!("no plot() in IR");
    }

    /// Literal arrays of 2-tuples evaluate cleanly — the minimum
    /// surface form covered by issue #28's acceptance examples.
    #[test]
    fn literal_array_of_tuples_evaluates() {
        let ir = parse("plot([(0, 0), (1, 1), (2, 0)])").unwrap();
        let arg = first_plot_arg(&ir);
        let verts = try_evaluate(&ir, arg).expect("literal verts evaluate");
        assert_eq!(verts.positions, vec![[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]]);
    }

    /// Identifier references resolve through preceding top-level
    /// bindings — `verts := …; plot(verts)` is the documented form
    /// the issue gives as its canonical example.
    #[test]
    fn binding_reference_resolves() {
        let ir = parse("verts := [(0, 0), (1, 2)]\nplot(verts)").unwrap();
        let arg = first_plot_arg(&ir);
        let verts = try_evaluate(&ir, arg).expect("named verts resolve");
        assert_eq!(verts.positions, vec![[0.0, 0.0], [1.0, 2.0]]);
    }

    /// Constant-foldable arithmetic inside the tuple positions still
    /// counts as a literal — `(-1, 2*3)` resolves without the
    /// interpreter. Anything more elaborate falls through and the
    /// caller routes the arg through the analytic plot path.
    #[test]
    fn constant_folding_inside_tuples() {
        let ir = parse("plot([(-1, 2*3), (1/2, 1+1)])").unwrap();
        let arg = first_plot_arg(&ir);
        let verts = try_evaluate(&ir, arg).expect("folded verts");
        assert_eq!(verts.positions, vec![[-1.0, 6.0], [0.5, 2.0]]);
    }

    /// Non-vertex shapes (analytic curve, surface, scalar) return
    /// `None`, signaling the caller should fall back. Without this
    /// guard, vertex routing would shadow every plot in the suite.
    #[test]
    fn analytic_shapes_return_none() {
        for src in [
            "plot(y = sin(x))",
            "plot(x*x + y*y)",
            "plot((x) |-> sin(x))",
            "plot([1, 2, 3])", // flat array, not vertices
        ] {
            let ir = parse(src).unwrap();
            let arg = first_plot_arg(&ir);
            assert!(
                try_evaluate(&ir, arg).is_none(),
                "expected None for {:?}",
                src
            );
        }
    }

    /// Hash equality on identical position lists is the basis for
    /// the "re-upload only on change" criterion in issue #28.
    #[test]
    fn identical_positions_hash_equal() {
        let a = VertexData::from_positions(vec![[0.0, 0.0], [1.0, 2.0]]);
        let b = VertexData::from_positions(vec![[0.0, 0.0], [1.0, 2.0]]);
        assert_eq!(a.hash, b.hash);
    }

    /// Different positions hash differently (modulo the ~2^-64 chance
    /// of collision, which we ignore). Without this, a real edit
    /// wouldn't trigger the re-upload it should.
    #[test]
    fn different_positions_hash_differ() {
        let a = VertexData::from_positions(vec![[0.0, 0.0]]);
        let b = VertexData::from_positions(vec![[0.0, 1.0]]);
        assert_ne!(a.hash, b.hash);
    }

    /// The canonical vertex+fragment WGSL must pass naga validation
    /// (the same validation wgpu runs at pipeline build time).
    /// Without this guard, a typo in the shader template surfaces as
    /// a runtime panic deep inside the renderer rather than as a
    /// failed test next to the source.
    #[test]
    fn vertex_plot_wgsl_validates() {
        let module = naga::front::wgsl::parse_str(VERTEX_PLOT_WGSL)
            .unwrap_or_else(|e| panic!("vertex plot WGSL parse error:\n{}", e));
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .unwrap_or_else(|e| panic!("vertex plot WGSL validation error:\n{}", e));
    }

    /// Validate the dynamically composed vertex+color WGSL the same
    /// way wgpu would at pipeline build time. Covers three shapes the
    /// user can write:
    ///
    /// - Implicit color tuple `(r, g, b, a)` — wrapped as a 0-param
    ///   lambda, captures axis vars (issue #46).
    /// - Explicit 1-arg color lambda `(x) ↦ (…)` — single param,
    ///   captures resolved against the param + outer axis vars.
    /// - Constants and arithmetic in the color tuple.
    ///
    /// Each must produce a self-contained shader naga accepts; a
    /// failure here would otherwise crash the renderer at pipeline
    /// creation with an opaque WGSL error.
    #[test]
    fn shader_with_color_validates_for_common_shapes() {
        let cases = [
            "plot([(0, 0), (1, 1)], (1, 0, 0, 1))",
            "plot([(0, 0), (1, 1)], (sin(x), cos(y), 0.5, 1))",
            "plot([(0, 0), (1, 1)], (x) |-> (sin(x), cos(x), 0.5, 1))",
        ];
        for src in cases {
            let ir = parse(src).unwrap();
            let stmts: &[Ir] = match &ir {
                Ir::Block { items, .. } => items.as_slice(),
                single => std::slice::from_ref(single),
            };
            let args = match &stmts[0] {
                Ir::Apply { args, .. } => args,
                _ => panic!("expected plot Apply"),
            };
            let wgsl = shader_with_color(&args[1])
                .unwrap_or_else(|e| panic!("shader_with_color({:?}) failed: {}", src, e));
            let module = naga::front::wgsl::parse_str(&wgsl).unwrap_or_else(|e| {
                panic!("naga parse error for {:?}:\n{}\nWGSL:\n{}", src, e, wgsl)
            });
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| {
                panic!(
                    "naga validation error for {:?}:\n{}\nWGSL:\n{}",
                    src, e, wgsl
                )
            });
            assert!(
                wgsl.contains("fn _plot_color_"),
                "expected lifted color fn in {:?}; got:\n{}",
                src,
                wgsl
            );
            assert!(
                wgsl.contains("let _color = _plot_color_"),
                "expected fragment shader to call the color fn for {:?}; got:\n{}",
                src,
                wgsl
            );
            assert!(
                wgsl.contains("@vertex"),
                "expected vertex stage for {:?}",
                src
            );
        }
    }
}
