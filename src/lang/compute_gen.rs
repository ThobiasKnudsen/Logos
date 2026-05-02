use std::collections::HashSet;

use super::ir::Ir;
use super::interpreter::ParallelForRequest;

/// Workgroup size used by generated compute shaders. Must match the dispatch
/// divisor in `render::compute_pipeline::dispatch`.
pub const WORKGROUP_SIZE: u32 = 64;

/// Generate a WGSL compute shader from a parallel for request.
///
/// The generated shader has:
/// - A uniform buffer with `len` and scalar constants
/// - Read-only storage buffers for arrays that are only read
/// - Read-write storage buffers for arrays that are written to
/// - A @compute @workgroup_size(WORKGROUP_SIZE) entry point
pub fn generate(request: &ParallelForRequest) -> Result<String, String> {
    let mut shader = String::new();

    // Params uniform: length + scalar constants
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
    shader.push_str(&format!("    let {} = gid.x;\n", request.var_name));
    shader.push_str(&format!(
        "    if ({} >= params.len) {{ return; }}\n",
        request.var_name
    ));

    // Emit body statements
    let mut declared: HashSet<String> = HashSet::new();
    emit_body_statements(
        &request.body,
        &request.var_name,
        request,
        &mut shader,
        &mut declared,
    )?;

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
    loop_var: &str,
    request: &ParallelForRequest,
    shader: &mut String,
    declared: &mut HashSet<String>,
) -> Result<(), String> {
    match node {
        Ir::Block { items: stmts, .. } => {
            for stmt in stmts {
                emit_body_statements(stmt, loop_var, request, shader, declared)?;
            }
        }
        Ir::IndexAssign {
            array,
            index,
            value,
            ..
        } => {
            let idx = emit_compute_index(index, loop_var, request)?;
            let val = emit_compute_expr(value, loop_var, request, declared)?;
            if let Ir::Identifier { name, .. } = array.as_ref() {
                shader.push_str(&format!("    arr_{}[{}] = {};\n", name, idx, val));
            } else {
                return Err("IndexAssign target must be an identifier".to_string());
            }
        }
        Ir::Binding { name, value, .. } => {
            let val = emit_compute_expr(value, loop_var, request, declared)?;
            if declared.contains(name) {
                shader.push_str(&format!("    {} = {};\n", name, val));
            } else {
                shader.push_str(&format!("    var {} = {};\n", name, val));
                declared.insert(name.clone());
            }
        }
        _ => {
            // Expression statement — emit as-is (rare)
            let val = emit_compute_expr(node, loop_var, request, declared)?;
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

fn emit_compute_expr(
    node: &Ir,
    loop_var: &str,
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
            if name == loop_var {
                Ok(format!("f32({})", name))
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
            let idx = emit_compute_index(index, loop_var, request)?;
            if let Ir::Identifier { name, .. } = array.as_ref() {
                if is_array(name, request) {
                    return Ok(format!("arr_{}[{}]", name, idx));
                }
            }
            Err("Index access only supported on known arrays".to_string())
        }

        Ir::Apply { name, args, .. } => {
            let arg_strs: Vec<String> = args
                .iter()
                .map(|a| emit_compute_expr(a, loop_var, request, declared))
                .collect::<Result<_, _>>()?;

            match name.as_str() {
                "add" => Ok(format!("({} + {})", arg_strs[0], arg_strs[1])),
                "sub" => Ok(format!("({} - {})", arg_strs[0], arg_strs[1])),
                "mul" => Ok(format!("({} * {})", arg_strs[0], arg_strs[1])),
                "div" => Ok(format!("({} / {})", arg_strs[0], arg_strs[1])),
                "mod" => Ok(format!("({} % {})", arg_strs[0], arg_strs[1])),
                "pow" => Ok(format!("pow({}, {})", arg_strs[0], arg_strs[1])),

                "eq" => Ok(format!(
                    "select(0.0, 1.0, ({} == {}))",
                    arg_strs[0], arg_strs[1]
                )),
                "neq" => Ok(format!(
                    "select(0.0, 1.0, ({} != {}))",
                    arg_strs[0], arg_strs[1]
                )),
                "lt" => Ok(format!(
                    "select(0.0, 1.0, ({} < {}))",
                    arg_strs[0], arg_strs[1]
                )),
                "gt" => Ok(format!(
                    "select(0.0, 1.0, ({} > {}))",
                    arg_strs[0], arg_strs[1]
                )),
                "lte" => Ok(format!(
                    "select(0.0, 1.0, ({} <= {}))",
                    arg_strs[0], arg_strs[1]
                )),
                "gte" => Ok(format!(
                    "select(0.0, 1.0, ({} >= {}))",
                    arg_strs[0], arg_strs[1]
                )),

                "neg" => Ok(format!("(-{})", arg_strs[0])),

                "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh" | "cosh" | "tanh"
                | "log" | "log2" | "exp" | "exp2" | "floor" | "ceil" | "round" | "fract"
                | "abs" | "sign" | "sqrt" => Ok(format!("{}({})", name, arg_strs[0])),
                "log10" => Ok(format!("(log2({}) / log2(10.0))", arg_strs[0])),

                "min" | "max" => Ok(format!("{}({}, {})", name, arg_strs[0], arg_strs[1])),
                "step" => Ok(format!("step({}, {})", arg_strs[0], arg_strs[1])),

                "clamp" => Ok(format!(
                    "clamp({}, {}, {})",
                    arg_strs[0], arg_strs[1], arg_strs[2]
                )),
                "mix" => Ok(format!(
                    "mix({}, {}, {})",
                    arg_strs[0], arg_strs[1], arg_strs[2]
                )),
                "smoothstep" => Ok(format!(
                    "smoothstep({}, {}, {})",
                    arg_strs[0], arg_strs[1], arg_strs[2]
                )),

                "f32" | "f64" => Ok(arg_strs[0].clone()),
                "i32" => Ok(format!("f32(i32({}))", arg_strs[0])),

                _ => Err(format!("Unsupported function '{}' in compute shader", name)),
            }
        }

        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            let cond = emit_compute_expr(condition, loop_var, request, declared)?;
            let then_val = emit_compute_expr(then_branch, loop_var, request, declared)?;
            let else_val = if let Some(eb) = else_branch {
                emit_compute_expr(eb, loop_var, request, declared)?
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
fn emit_compute_index(
    node: &Ir,
    loop_var: &str,
    request: &ParallelForRequest,
) -> Result<String, String> {
    match node {
        Ir::Identifier { name, .. } if name == loop_var => Ok(name.clone()),
        _ => {
            let declared = HashSet::new();
            let expr = emit_compute_expr(node, loop_var, request, &declared)?;
            Ok(format!("u32({})", expr))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(
        body: Ir,
        readwrite: Vec<(&str, Vec<f64>)>,
        readonly: Vec<(&str, Vec<f64>)>,
        scalars: Vec<(&str, f64)>,
    ) -> ParallelForRequest {
        ParallelForRequest {
            var_name: "i".to_string(),
            range_start: 0,
            range_end: 4,
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
