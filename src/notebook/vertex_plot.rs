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
    // Map [0, 1] to NDC [-1, 1]; flip y so world y-up matches screen.
    let ndc = vec2<f32>(normalized.x * 2.0 - 1.0, normalized.y * 2.0 - 1.0);
    return vec4<f32>(ndc, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    let a = u.primary_color.a;
    return vec4<f32>(u.primary_color.rgb * a, a);
}
"#;

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
}
