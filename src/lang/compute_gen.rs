use std::collections::HashSet;

use super::ir::{BuiltinOp, Callee, Ir};
use super::interpreter::ParallelForRequest;

/// Workgroup size used by generated compute shaders. Must match the dispatch
/// divisor in `render::compute_pipeline::dispatch`.
pub const WORKGROUP_SIZE: u32 = 64;

/// Generate a WGSL compute shader from a parallel for request.
///
/// The generated shader has:
/// - A uniform buffer with `len` (total thread count) and scalar constants
/// - Read-only storage buffers for arrays that are only read
/// - Read-write storage buffers for arrays that are written to
/// - A @compute @workgroup_size(WORKGROUP_SIZE) entry point
///
/// For multi-dim iteration, the global invocation id is decomposed into a
/// multi-index using per-axis counts, then each loop variable is bound to
/// `start + index * delta` (as `f32`).
pub fn generate(request: &ParallelForRequest) -> Result<String, String> {
    if request.dims.is_empty() {
        return Err("parallel for has no iteration dimensions".to_string());
    }

    let mut shader = String::new();

    // Params uniform: total length + scalar constants
    shader.push_str("struct Params {\n");
    shader.push_str("    len: u32,\n");
    for (name, _) in &request.scalars {
        shader.push_str(&format!("    {}: f32,\n", name));
    }
    shader.push_str("};\n\n");

    shader.push_str("@group(0) @binding(0) var<uniform> params: Params;\n");

    // Read-only array storage buffers
    let mut binding_idx = 1u32;
    for (name, _) in &request.readonly_arrays {
        shader.push_str(&format!(
            "@group(0) @binding({}) var<storage, read> arr_{}: array<f32>;\n",
            binding_idx, name
        ));
        binding_idx += 1;
    }

    // Read-write array storage buffers
    for (name, _) in &request.readwrite_arrays {
        shader.push_str(&format!(
            "@group(0) @binding({}) var<storage, read_write> arr_{}: array<f32>;\n",
            binding_idx, name
        ));
        binding_idx += 1;
    }

    shader.push('\n');

    // Compute entry point
    shader.push_str(&format!("@compute @workgroup_size({})\n", WORKGROUP_SIZE));
    shader.push_str("fn main(@builtin(global_invocation_id) gid: vec3<u32>) {\n");
    shader.push_str("    if (gid.x >= params.len) { return; }\n");

    // Decompose gid.x → per-axis index. Innermost axis varies fastest,
    // matching the CPU fallback's iteration order via `iterate_dims_inner`.
    let counts: Vec<u32> = request.dims.iter().map(|d| d.count()).collect();
    if request.dims.len() == 1 {
        // 1-D fast path: the per-axis index *is* gid.x. For a unit-stride
        // zero-start range we can name it directly with the loop variable
        // (u32, cast to f32 on use in math expressions); array indexing
        // then uses the raw name with no cast. For non-unit-stride or
        // non-zero-start ranges, the variable holds the float value.
        let dim = &request.dims[0];
        if is_simple_dim(dim) {
            shader.push_str(&format!("    let {} = gid.x;\n", dim.var_name));
        } else {
            shader.push_str(&format!(
                "    let {} = {:.6} + f32(gid.x) * {:.6};\n",
                dim.var_name, dim.start, dim.delta
            ));
        }
    } else {
        // Multi-dim: peel innermost-first, then bind each loop variable.
        shader.push_str("    var _tid: u32 = gid.x;\n");
        let mut idx_decls: Vec<(String, String)> = Vec::new();
        for (i, dim) in request.dims.iter().enumerate().rev() {
            let idx_name = format!("_idx_{}", dim.var_name);
            if i == 0 {
                shader.push_str(&format!("    let {} = _tid;\n", idx_name));
            } else {
                let c = counts[i];
                shader.push_str(&format!("    let {} = _tid % {}u;\n", idx_name, c));
                shader.push_str(&format!("    _tid = _tid / {}u;\n", c));
            }
            idx_decls.push((dim.var_name.clone(), idx_name));
        }
        // Bind each loop variable.
        for (var_name, idx_name) in idx_decls.iter().rev() {
            let dim = request.dims.iter().find(|d| &d.var_name == var_name).unwrap();
            if is_simple_dim(dim) {
                shader.push_str(&format!("    let {} = {};\n", var_name, idx_name));
            } else {
                shader.push_str(&format!(
                    "    let {} = {:.6} + f32({}) * {:.6};\n",
                    var_name, dim.start, idx_name, dim.delta
                ));
            }
        }
    }

    // Emit body statements
    let mut declared: HashSet<String> = HashSet::new();
    emit_body_statements(&request.body, request, &mut shader, &mut declared)?;

    shader.push_str("}\n");

    Ok(shader)
}

/// The total number of bindings: params(1) + readonly + readwrite
pub fn binding_count(request: &ParallelForRequest) -> u32 {
    1 + request.readonly_arrays.len() as u32 + request.readwrite_arrays.len() as u32
}

// ---------------------------------------------------------------------------
// Statement emitter
// ---------------------------------------------------------------------------

fn emit_body_statements(
    node: &Ir,
    request: &ParallelForRequest,
    shader: &mut String,
    declared: &mut HashSet<String>,
) -> Result<(), String> {
    match node {
        Ir::Block { items: stmts, .. } => {
            for stmt in stmts {
                emit_body_statements(stmt, request, shader, declared)?;
            }
        }
        Ir::IndexAssign {
            array,
            index,
            value,
            ..
        } => {
            let idx = emit_compute_index(index, request)?;
            let val = emit_compute_expr(value, request, declared)?;
            if let Ir::Identifier { name, .. } = array.as_ref() {
                shader.push_str(&format!("    arr_{}[{}] = {};\n", name, idx, val));
            } else {
                return Err("IndexAssign target must be an identifier".to_string());
            }
        }
        Ir::Binding { name, value, .. } => {
            let val = emit_compute_expr(value, request, declared)?;
            if declared.contains(name) {
                shader.push_str(&format!("    {} = {};\n", name, val));
            } else {
                shader.push_str(&format!("    var {} = {};\n", name, val));
                declared.insert(name.clone());
            }
        }
        _ => {
            // Expression statement — emit as-is (rare)
            let val = emit_compute_expr(node, request, declared)?;
            shader.push_str(&format!("    _ = {};\n", val));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Expression emitter (IR → WGSL compute expression)
// ---------------------------------------------------------------------------

fn is_array(name: &str, request: &ParallelForRequest) -> bool {
    request.readonly_arrays.iter().any(|(n, _)| n == name)
        || request.readwrite_arrays.iter().any(|(n, _)| n == name)
}

/// A "simple" iteration axis is `0..n` with unit stride — the loop
/// variable is bound directly to the integer multi-index and needs a
/// `f32(...)` cast in math expressions.
fn is_simple_dim(dim: &super::interpreter::DimSpec) -> bool {
    dim.delta == 1.0 && dim.start == 0.0
}

fn loop_var_dim<'a>(
    name: &str,
    request: &'a ParallelForRequest,
) -> Option<&'a super::interpreter::DimSpec> {
    request.dims.iter().find(|d| d.var_name == name)
}

fn emit_compute_expr(
    node: &Ir,
    request: &ParallelForRequest,
    declared: &HashSet<String>,
) -> Result<String, String> {
    match node {
        Ir::Number { value: n, .. } => {
            if *n == n.floor() && n.abs() < 1e15 {
                Ok(format!("{:.1}", n))
            } else {
                Ok(format!("{}", n))
            }
        }

        Ir::BoolLit { value: b, .. } => Ok(format!("{}", b)),

        Ir::Identifier { name, .. } => {
            if let Some(dim) = loop_var_dim(name, request) {
                // Simple dim → u32, cast to f32 here for use in math
                // expressions. Non-simple dim → already an f32, use directly.
                if is_simple_dim(dim) {
                    Ok(format!("f32({})", name))
                } else {
                    Ok(name.clone())
                }
            } else if declared.contains(name) {
                Ok(name.clone())
            } else if request.scalars.iter().any(|(n, _)| n == name) {
                Ok(format!("params.{}", name))
            } else if is_array(name, request) {
                Err(format!("Array '{}' used without indexing", name))
            } else {
                Err(format!(
                    "Unknown variable '{}' in compute shader body",
                    name
                ))
            }
        }

        Ir::IndexAccess { array, index, .. } => {
            let idx = emit_compute_index(index, request)?;
            if let Ir::Identifier { name, .. } = array.as_ref() {
                if is_array(name, request) {
                    return Ok(format!("arr_{}[{}]", name, idx));
                }
            }
            Err("Index access only supported on known arrays".to_string())
        }

        Ir::Apply { callee, args, .. } => {
            let arg_strs: Vec<String> = args
                .iter()
                .map(|a| emit_compute_expr(a, request, declared))
                .collect::<Result<_, _>>()?;

            let op = match callee {
                Callee::Builtin(op) => *op,
                Callee::User(name) => {
                    return Err(format!("Unsupported function '{}' in compute shader", name));
                }
            };

            match op {
                BuiltinOp::Add => Ok(format!("({} + {})", arg_strs[0], arg_strs[1])),
                BuiltinOp::Sub => Ok(format!("({} - {})", arg_strs[0], arg_strs[1])),
                BuiltinOp::Mul => Ok(format!("({} * {})", arg_strs[0], arg_strs[1])),
                BuiltinOp::Div => Ok(format!("({} / {})", arg_strs[0], arg_strs[1])),
                BuiltinOp::Mod => Ok(format!("({} % {})", arg_strs[0], arg_strs[1])),
                BuiltinOp::Pow => Ok(format!("pow({}, {})", arg_strs[0], arg_strs[1])),

                BuiltinOp::Eq => Ok(format!(
                    "select(0.0, 1.0, ({} == {}))",
                    arg_strs[0], arg_strs[1]
                )),
                BuiltinOp::Neq => Ok(format!(
                    "select(0.0, 1.0, ({} != {}))",
                    arg_strs[0], arg_strs[1]
                )),
                BuiltinOp::Lt => Ok(format!(
                    "select(0.0, 1.0, ({} < {}))",
                    arg_strs[0], arg_strs[1]
                )),
                BuiltinOp::Gt => Ok(format!(
                    "select(0.0, 1.0, ({} > {}))",
                    arg_strs[0], arg_strs[1]
                )),
                BuiltinOp::Lte => Ok(format!(
                    "select(0.0, 1.0, ({} <= {}))",
                    arg_strs[0], arg_strs[1]
                )),
                BuiltinOp::Gte => Ok(format!(
                    "select(0.0, 1.0, ({} >= {}))",
                    arg_strs[0], arg_strs[1]
                )),

                BuiltinOp::Neg => Ok(format!("(-{})", arg_strs[0])),

                BuiltinOp::Sin | BuiltinOp::Cos | BuiltinOp::Tan
                | BuiltinOp::Asin | BuiltinOp::Acos | BuiltinOp::Atan
                | BuiltinOp::Sinh | BuiltinOp::Cosh | BuiltinOp::Tanh
                | BuiltinOp::Log | BuiltinOp::Log2 | BuiltinOp::Exp | BuiltinOp::Exp2
                | BuiltinOp::Floor | BuiltinOp::Ceil | BuiltinOp::Round | BuiltinOp::Fract
                | BuiltinOp::Abs | BuiltinOp::Sign | BuiltinOp::Sqrt => {
                    Ok(format!("{}({})", op.name(), arg_strs[0]))
                }
                BuiltinOp::Log10 => Ok(format!("(log2({}) / log2(10.0))", arg_strs[0])),

                BuiltinOp::Min | BuiltinOp::Max => {
                    Ok(format!("{}({}, {})", op.name(), arg_strs[0], arg_strs[1]))
                }
                BuiltinOp::Step => Ok(format!("step({}, {})", arg_strs[0], arg_strs[1])),

                BuiltinOp::Clamp => Ok(format!(
                    "clamp({}, {}, {})",
                    arg_strs[0], arg_strs[1], arg_strs[2]
                )),
                BuiltinOp::Mix => Ok(format!(
                    "mix({}, {}, {})",
                    arg_strs[0], arg_strs[1], arg_strs[2]
                )),
                BuiltinOp::Smoothstep => Ok(format!(
                    "smoothstep({}, {}, {})",
                    arg_strs[0], arg_strs[1], arg_strs[2]
                )),

                BuiltinOp::F32 | BuiltinOp::F64 => Ok(arg_strs[0].clone()),
                BuiltinOp::I32 => Ok(format!("f32(i32({}))", arg_strs[0])),

                BuiltinOp::And | BuiltinOp::Or | BuiltinOp::Not
                | BuiltinOp::Length | BuiltinOp::Normalize | BuiltinOp::Dot | BuiltinOp::Cross
                | BuiltinOp::Vec2 | BuiltinOp::Vec3 | BuiltinOp::Vec4
                | BuiltinOp::Len | BuiltinOp::Print | BuiltinOp::Plot => {
                    Err(format!("Unsupported builtin '{}' in compute shader", op.name()))
                }
            }
        }

        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            let cond = emit_compute_expr(condition, request, declared)?;
            let then_val = emit_compute_expr(then_branch, request, declared)?;
            let else_val = if let Some(eb) = else_branch {
                emit_compute_expr(eb, request, declared)?
            } else {
                "0.0".to_string()
            };
            Ok(format!(
                "select({}, {}, ({} != 0.0))",
                else_val, then_val, cond
            ))
        }

        _ => Err(format!(
            "Unsupported expression in compute shader: {:?}",
            node
        )),
    }
}

/// Emit an index expression — must produce a u32 for array indexing.
/// When the index is exactly a "simple" loop variable (unit stride,
/// start 0), the variable is already u32 so we can use it directly. Any
/// other expression gets wrapped in `u32(...)`.
fn emit_compute_index(node: &Ir, request: &ParallelForRequest) -> Result<String, String> {
    if let Ir::Identifier { name, .. } = node {
        if let Some(dim) = loop_var_dim(name, request) {
            if is_simple_dim(dim) {
                return Ok(name.clone());
            }
        }
    }
    let declared = HashSet::new();
    let expr = emit_compute_expr(node, request, &declared)?;
    Ok(format!("u32({})", expr))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use crate::lang::interpreter::DimSpec;

    fn make_request(
        body: Ir,
        readwrite: Vec<(&str, Vec<f64>)>,
        readonly: Vec<(&str, Vec<f64>)>,
        scalars: Vec<(&str, f64)>,
    ) -> ParallelForRequest {
        ParallelForRequest {
            dims: vec![DimSpec {
                var_name: "i".to_string(),
                start: 0.0,
                end: 4.0,
                delta: 1.0,
            }],
            body,
            readwrite_arrays: readwrite
                .into_iter()
                .map(|(n, d)| (n.to_string(), d))
                .collect(),
            readonly_arrays: readonly
                .into_iter()
                .map(|(n, d)| (n.to_string(), d))
                .collect(),
            scalars: scalars
                .into_iter()
                .map(|(n, v)| (n.to_string(), v))
                .collect(),
        }
    }

    fn parse_body(source: &str) -> Ir {
        let mut lex = crate::lang::lexer::Lexer::new(source);
        let tokens = lex.tokenize().unwrap();
        let mut parser = crate::lang::parser::Parser::new(tokens, source.to_string());
        parser.parse().unwrap()
    }

    fn validate_wgsl(wgsl: &str) -> Result<(), String> {
        let module = naga::front::wgsl::parse_str(wgsl)
            .map_err(|e| format!("naga parse error: {}\n\n--- WGSL ---\n{}", e, wgsl))?;
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .map_err(|e| format!("naga validation error: {}\n\n--- WGSL ---\n{}", e, wgsl))?;
        Ok(())
    }

    #[test]
    fn test_inplace_multiply() {
        let body = parse_body("data[i] := data[i] * 2");
        let req = make_request(body, vec![("data", vec![1.0; 4])], vec![], vec![]);
        let wgsl = generate(&req).unwrap();
        validate_wgsl(&wgsl).unwrap();
        assert!(wgsl.contains("read_write"));
        assert!(wgsl.contains("arr_data[i] ="));
    }

    #[test]
    fn test_with_scalar() {
        let body = parse_body("data[i] := data[i] * scale");
        let req = make_request(
            body,
            vec![("data", vec![1.0; 4])],
            vec![],
            vec![("scale", 10.0)],
        );
        let wgsl = generate(&req).unwrap();
        validate_wgsl(&wgsl).unwrap();
        assert!(wgsl.contains("params.scale"));
    }

    #[test]
    fn test_read_write_separate() {
        // c[i] := a[i] + b[i]  — a,b readonly, c readwrite
        let body = parse_body("c[i] := a[i] + b[i]");
        let req = make_request(
            body,
            vec![("c", vec![0.0; 4])],
            vec![("a", vec![1.0; 4]), ("b", vec![2.0; 4])],
            vec![],
        );
        let wgsl = generate(&req).unwrap();
        validate_wgsl(&wgsl).unwrap();
        assert!(wgsl.contains("var<storage, read> arr_a"));
        assert!(wgsl.contains("var<storage, read> arr_b"));
        assert!(wgsl.contains("var<storage, read_write> arr_c"));
    }

    #[test]
    fn test_multi_statement() {
        let body = parse_body("temp := data[i] * 2\ndata[i] := temp + 1");
        let req = make_request(body, vec![("data", vec![1.0; 4])], vec![], vec![]);
        let wgsl = generate(&req).unwrap();
        validate_wgsl(&wgsl).unwrap();
        assert!(wgsl.contains("var temp ="));
        assert!(wgsl.contains("arr_data[i] ="));
    }

    #[test]
    fn test_math_builtins() {
        let body = parse_body("data[i] := sin(data[i]) + sqrt(data[i])");
        let req = make_request(body, vec![("data", vec![1.0; 4])], vec![], vec![]);
        let wgsl = generate(&req).unwrap();
        validate_wgsl(&wgsl).unwrap();
        assert!(wgsl.contains("sin("));
        assert!(wgsl.contains("sqrt("));
    }
}
