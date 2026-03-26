use std::collections::HashMap;

use super::ast::AstNode;

const MAX_LOOP_ITERATIONS: usize = 10_000;

// ---------------------------------------------------------------------------
// Value
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Value {
    F64(f64),
    Bool(bool),
    Array(Vec<f64>),
    Void,
}

impl Value {
    pub fn as_f64(&self) -> Result<f64, String> {
        match self {
            Value::F64(n) => Ok(*n),
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            other => Err(format!("Expected number, got {:?}", other)),
        }
    }

    pub fn as_bool(&self) -> Result<bool, String> {
        match self {
            Value::Bool(b) => Ok(*b),
            Value::F64(n) => Ok(*n != 0.0),
            other => Err(format!("Expected bool, got {:?}", other)),
        }
    }

    pub fn as_array(&self) -> Result<&Vec<f64>, String> {
        match self {
            Value::Array(a) => Ok(a),
            other => Err(format!("Expected array, got {:?}", other)),
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::F64(n) => {
                if *n == n.floor() && n.abs() < 1e15 {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{}", n)
                }
            }
            Value::Bool(b) => write!(f, "{}", b),
            Value::Array(a) => {
                write!(f, "[")?;
                for (i, v) in a.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    if *v == v.floor() && v.abs() < 1e15 {
                        write!(f, "{}", *v as i64)?;
                    } else {
                        write!(f, "{}", v)?;
                    }
                }
                write!(f, "]")
            }
            Value::Void => write!(f, "()"),
        }
    }
}

// ---------------------------------------------------------------------------
// Function definition (stored in env)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct FuncDef {
    params: Vec<String>,
    body: AstNode,
}

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Env {
    vars: HashMap<String, Value>,
    funcs: HashMap<String, FuncDef>,
}

impl Env {
    fn new() -> Self {
        Self {
            vars: HashMap::new(),
            funcs: HashMap::new(),
        }
    }

    fn child(&self) -> Self {
        Self {
            vars: self.vars.clone(),
            funcs: self.funcs.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// GPU callback: called when a parallel for is encountered
// ---------------------------------------------------------------------------

/// Information the interpreter passes to the GPU dispatch layer.
pub struct ParallelForRequest {
    pub var_name: String,
    pub range_start: usize,
    pub range_end: usize,
    pub body: AstNode,
    /// Arrays that are written to (read-write storage buffers).
    pub readwrite_arrays: Vec<(String, Vec<f64>)>,
    /// Arrays that are only read (read-only storage buffers).
    pub readonly_arrays: Vec<(String, Vec<f64>)>,
    /// Scalar constants referenced in the body.
    pub scalars: Vec<(String, f64)>,
}

/// Trait for GPU dispatch — the interpreter calls this for parallel for.
/// Returns updated (name, data) pairs for each read-write array.
pub trait GpuDispatch {
    fn dispatch(&self, request: &ParallelForRequest) -> Result<Vec<(String, Vec<f64>)>, String>;
}

/// CPU fallback: runs the parallel for on CPU (no GPU needed).
pub struct CpuFallback;

impl GpuDispatch for CpuFallback {
    fn dispatch(&self, request: &ParallelForRequest) -> Result<Vec<(String, Vec<f64>)>, String> {
        // Build environment with all arrays and scalars
        let mut env = Env::new();
        for (name, data) in &request.readwrite_arrays {
            env.vars.insert(name.clone(), Value::Array(data.clone()));
        }
        for (name, data) in &request.readonly_arrays {
            env.vars.insert(name.clone(), Value::Array(data.clone()));
        }
        for (name, val) in &request.scalars {
            env.vars.insert(name.clone(), Value::F64(*val));
        }

        // Execute body for each index (sequentially on CPU)
        for i in request.range_start..request.range_end {
            env.vars.insert(request.var_name.clone(), Value::F64(i as f64));
            eval_node(&request.body, &mut env, &CpuFallback)?;
        }

        // Collect updated read-write arrays
        let mut results = Vec::new();
        for (name, _) in &request.readwrite_arrays {
            if let Some(Value::Array(data)) = env.vars.get(name) {
                results.push((name.clone(), data.clone()));
            }
        }
        Ok(results)
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Evaluate an AST, returning the final value.
/// `gpu` provides GPU dispatch for parallel for; use `CpuFallback` if no GPU.
pub fn eval(ast: &AstNode, gpu: &dyn GpuDispatch) -> Result<Value, String> {
    let mut env = Env::new();
    eval_node(ast, &mut env, gpu)
}

// ---------------------------------------------------------------------------
// Core evaluator
// ---------------------------------------------------------------------------

fn eval_node(node: &AstNode, env: &mut Env, gpu: &dyn GpuDispatch) -> Result<Value, String> {
    match node {
        AstNode::Number(n) => Ok(Value::F64(*n)),
        AstNode::BoolLit(b) => Ok(Value::Bool(*b)),

        AstNode::Identifier(name) => {
            env.vars.get(name).cloned()
                .ok_or_else(|| format!("Undefined variable: {}", name))
        }

        AstNode::ArrayLiteral(elems) => {
            let mut arr = Vec::with_capacity(elems.len());
            for e in elems {
                arr.push(eval_node(e, env, gpu)?.as_f64()?);
            }
            Ok(Value::Array(arr))
        }

        AstNode::IndexAccess { array, index } => {
            let arr = eval_node(array, env, gpu)?;
            let idx = eval_node(index, env, gpu)?.as_f64()? as usize;
            match arr {
                Value::Array(ref a) => {
                    a.get(idx).copied()
                        .map(Value::F64)
                        .ok_or_else(|| format!("Index {} out of bounds (len {})", idx, a.len()))
                }
                _ => Err("Cannot index into non-array".to_string()),
            }
        }

        AstNode::Range { start, end } => {
            // Ranges aren't values themselves — they're only valid inside parallel for.
            // If we reach here, the user wrote a range outside that context.
            let s = eval_node(start, env, gpu)?.as_f64()?;
            let e = eval_node(end, env, gpu)?.as_f64()?;
            // Return as array of indices for convenience
            let len = (e - s) as usize;
            let arr: Vec<f64> = (0..len).map(|i| s + i as f64).collect();
            Ok(Value::Array(arr))
        }

        AstNode::Binding { name, value } => {
            let val = eval_node(value, env, gpu)?;
            env.vars.insert(name.clone(), val);
            Ok(Value::Void)
        }

        AstNode::TupleBinding { names, value } => {
            let val = eval_node(value, env, gpu)?;
            match val {
                Value::Array(ref a) if a.len() == names.len() => {
                    for (name, &v) in names.iter().zip(a.iter()) {
                        env.vars.insert(name.clone(), Value::F64(v));
                    }
                }
                _ => return Err(format!(
                    "Tuple binding expects array of length {}, got {:?}",
                    names.len(), val
                )),
            }
            Ok(Value::Void)
        }

        AstNode::Block(stmts) => {
            let mut last = Value::Void;
            for stmt in stmts {
                last = eval_node(stmt, env, gpu)?;
            }
            Ok(last)
        }

        AstNode::Tuple(elems) => {
            let mut arr = Vec::with_capacity(elems.len());
            for e in elems {
                arr.push(eval_node(e, env, gpu)?.as_f64()?);
            }
            Ok(Value::Array(arr))
        }

        AstNode::FunctionDef { name, params, body } => {
            env.funcs.insert(name.clone(), FuncDef {
                params: params.clone(),
                body: *body.clone(),
            });
            Ok(Value::Void)
        }

        AstNode::IfExpr { condition, then_branch, else_branch } => {
            let cond = eval_node(condition, env, gpu)?.as_bool()?;
            if cond {
                eval_node(then_branch, env, gpu)
            } else if let Some(eb) = else_branch {
                eval_node(eb, env, gpu)
            } else {
                Ok(Value::Void)
            }
        }

        AstNode::ForLoop { var, range, body } => {
            let (start, end) = match range.as_ref() {
                AstNode::Range { start, end } => {
                    let s = eval_node(start, env, gpu)?.as_f64()? as usize;
                    let e = eval_node(end, env, gpu)?.as_f64()? as usize;
                    (s, e)
                }
                _ => return Err("for loop range must be start..end".to_string()),
            };
            let mut last = Value::Void;
            for i in start..end {
                env.vars.insert(var.clone(), Value::F64(i as f64));
                last = eval_node(body, env, gpu)?;
            }
            Ok(last)
        }

        AstNode::WhileLoop { condition, body } => {
            for _ in 0..MAX_LOOP_ITERATIONS {
                let cond = eval_node(condition, env, gpu)?.as_bool()?;
                if !cond { break; }
                eval_node(body, env, gpu)?;
            }
            Ok(Value::Void)
        }

        AstNode::PropertyAccess { object, property } => {
            let val = eval_node(object, env, gpu)?;
            match (&val, property.as_str()) {
                (Value::Array(a), "len") => Ok(Value::F64(a.len() as f64)),
                _ => Err(format!("Unknown property .{} on {:?}", property, val)),
            }
        }

        AstNode::Apply { name, args } => eval_apply(name, args, env, gpu),

        AstNode::IndexAssign { array, index, value } => {
            let idx = eval_node(index, env, gpu)?.as_f64()? as usize;
            let val = eval_node(value, env, gpu)?.as_f64()?;
            if let AstNode::Identifier(name) = array.as_ref() {
                if let Some(Value::Array(ref mut arr)) = env.vars.get_mut(name) {
                    if idx < arr.len() {
                        arr[idx] = val;
                        return Ok(Value::Void);
                    } else {
                        return Err(format!("Index {} out of bounds (len {})", idx, arr.len()));
                    }
                }
                return Err(format!("'{}' is not an array", name));
            }
            Err("Index assignment target must be an array variable".to_string())
        }

        AstNode::ParallelFor { var, range, body } => {
            eval_parallel_for(var, range, body, env, gpu)
        }
    }
}

// ---------------------------------------------------------------------------
// Apply (operators + builtins + user functions)
// ---------------------------------------------------------------------------

fn eval_apply(
    name: &str,
    args: &[AstNode],
    env: &mut Env,
    gpu: &dyn GpuDispatch,
) -> Result<Value, String> {
    // Check user-defined functions first
    if let Some(func) = env.funcs.get(name).cloned() {
        if args.len() != func.params.len() {
            return Err(format!(
                "Function '{}' expects {} args, got {}",
                name, func.params.len(), args.len()
            ));
        }
        let mut child = env.child();
        for (param, arg_node) in func.params.iter().zip(args.iter()) {
            let val = eval_node(arg_node, env, gpu)?;
            child.vars.insert(param.clone(), val);
        }
        return eval_node(&func.body, &mut child, gpu);
    }

    // Evaluate arguments
    let vals: Vec<Value> = args.iter()
        .map(|a| eval_node(a, env, gpu))
        .collect::<Result<_, _>>()?;

    match name {
        // Binary arithmetic
        "add" => bin_f64(&vals, |a, b| a + b),
        "sub" => bin_f64(&vals, |a, b| a - b),
        "mul" => bin_f64(&vals, |a, b| a * b),
        "div" => bin_f64(&vals, |a, b| a / b),
        "mod" => bin_f64(&vals, |a, b| a % b),
        "pow" => bin_f64(&vals, |a, b| a.powf(b)),

        // Comparison
        "eq" => bin_cmp(&vals, |a, b| (a - b).abs() < 1e-10),
        "neq" => bin_cmp(&vals, |a, b| (a - b).abs() >= 1e-10),
        "lt" => bin_cmp(&vals, |a, b| a < b),
        "gt" => bin_cmp(&vals, |a, b| a > b),
        "lte" => bin_cmp(&vals, |a, b| a <= b),
        "gte" => bin_cmp(&vals, |a, b| a >= b),

        // Logical
        "and" => Ok(Value::Bool(vals[0].as_bool()? && vals[1].as_bool()?)),
        "or" => Ok(Value::Bool(vals[0].as_bool()? || vals[1].as_bool()?)),
        "not" => Ok(Value::Bool(!vals[0].as_bool()?)),

        // Unary
        "neg" => Ok(Value::F64(-vals[0].as_f64()?)),

        // Math builtins (1 arg)
        "sin" => un_f64(&vals, f64::sin),
        "cos" => un_f64(&vals, f64::cos),
        "tan" => un_f64(&vals, f64::tan),
        "asin" => un_f64(&vals, f64::asin),
        "acos" => un_f64(&vals, f64::acos),
        "atan" => un_f64(&vals, f64::atan),
        "sinh" => un_f64(&vals, f64::sinh),
        "cosh" => un_f64(&vals, f64::cosh),
        "tanh" => un_f64(&vals, f64::tanh),
        "log" => un_f64(&vals, f64::ln),
        "log2" => un_f64(&vals, f64::log2),
        "log10" => un_f64(&vals, f64::log10),
        "exp" => un_f64(&vals, f64::exp),
        "exp2" => un_f64(&vals, f64::exp2),
        "floor" => un_f64(&vals, f64::floor),
        "ceil" => un_f64(&vals, f64::ceil),
        "round" => un_f64(&vals, f64::round),
        "fract" => un_f64(&vals, f64::fract),
        "abs" => un_f64(&vals, f64::abs),
        "sign" => un_f64(&vals, f64::signum),
        "sqrt" => un_f64(&vals, f64::sqrt),

        // 2-arg builtins
        "min" => bin_f64(&vals, f64::min),
        "max" => bin_f64(&vals, f64::max),
        "step" => bin_f64(&vals, |edge, x| if x < edge { 0.0 } else { 1.0 }),

        // 3-arg builtins
        "clamp" => {
            let (x, lo, hi) = (vals[0].as_f64()?, vals[1].as_f64()?, vals[2].as_f64()?);
            Ok(Value::F64(x.clamp(lo, hi)))
        }
        "mix" => {
            let (a, b, t) = (vals[0].as_f64()?, vals[1].as_f64()?, vals[2].as_f64()?);
            Ok(Value::F64(a * (1.0 - t) + b * t))
        }
        "smoothstep" => {
            let (edge0, edge1, x) = (vals[0].as_f64()?, vals[1].as_f64()?, vals[2].as_f64()?);
            let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
            Ok(Value::F64(t * t * (3.0 - 2.0 * t)))
        }

        // Array builtins
        "len" => {
            match &vals[0] {
                Value::Array(a) => Ok(Value::F64(a.len() as f64)),
                _ => Err("len() expects an array".to_string()),
            }
        }

        // Type cast
        "f32" | "f64" => Ok(Value::F64(vals[0].as_f64()?)),
        "i32" => Ok(Value::F64((vals[0].as_f64()? as i32) as f64)),

        _ => Err(format!("Unknown function: {}", name)),
    }
}

fn un_f64(vals: &[Value], f: fn(f64) -> f64) -> Result<Value, String> {
    Ok(Value::F64(f(vals[0].as_f64()?)))
}

fn bin_f64(vals: &[Value], f: fn(f64, f64) -> f64) -> Result<Value, String> {
    Ok(Value::F64(f(vals[0].as_f64()?, vals[1].as_f64()?)))
}

fn bin_cmp(vals: &[Value], f: fn(f64, f64) -> bool) -> Result<Value, String> {
    Ok(Value::Bool(f(vals[0].as_f64()?, vals[1].as_f64()?)))
}

// ---------------------------------------------------------------------------
// Parallel for
// ---------------------------------------------------------------------------

fn eval_parallel_for(
    var: &str,
    range_node: &AstNode,
    body: &AstNode,
    env: &mut Env,
    gpu: &dyn GpuDispatch,
) -> Result<Value, String> {
    // Evaluate range
    let (start, end) = match range_node {
        AstNode::Range { start, end } => {
            let s = eval_node(start, env, gpu)?.as_f64()? as usize;
            let e = eval_node(end, env, gpu)?.as_f64()? as usize;
            (s, e)
        }
        _ => {
            let val = eval_node(range_node, env, gpu)?;
            match val {
                Value::Array(a) => (0, a.len()),
                _ => return Err("parallel for range must be start..end".to_string()),
            }
        }
    };

    // Find which arrays are written to (IndexAssign targets)
    let mut written_names = Vec::new();
    collect_written_arrays(body, &mut written_names);

    // Find all referenced identifiers
    let mut referenced = Vec::new();
    collect_references(body, &mut referenced);

    // Partition into readwrite, readonly, and scalars
    let mut readwrite_arrays = Vec::new();
    let mut readonly_arrays = Vec::new();
    let mut scalars = Vec::new();

    for name in &referenced {
        if name == var { continue; }
        if let Some(val) = env.vars.get(name) {
            match val {
                Value::Array(a) => {
                    if written_names.contains(name) {
                        readwrite_arrays.push((name.clone(), a.clone()));
                    } else {
                        readonly_arrays.push((name.clone(), a.clone()));
                    }
                }
                Value::F64(n) => scalars.push((name.clone(), *n)),
                Value::Bool(b) => scalars.push((name.clone(), if *b { 1.0 } else { 0.0 })),
                Value::Void => {}
            }
        }
    }

    let last_written = written_names.last().cloned();

    let request = ParallelForRequest {
        var_name: var.to_string(),
        range_start: start,
        range_end: end,
        body: body.clone(),
        readwrite_arrays,
        readonly_arrays,
        scalars,
    };

    let updated = gpu.dispatch(&request)?;

    // Apply updated arrays back to the environment
    for (name, data) in &updated {
        env.vars.insert(name.clone(), Value::Array(data.clone()));
    }

    // Return the last written array
    if let Some(name) = last_written {
        env.vars.get(&name).cloned().ok_or_else(|| format!("Array '{}' not found", name))
    } else {
        Ok(Value::Void)
    }
}

/// Collect array names that are targets of IndexAssign in the body.
fn collect_written_arrays(node: &AstNode, names: &mut Vec<String>) {
    match node {
        AstNode::IndexAssign { array, .. } => {
            if let AstNode::Identifier(name) = array.as_ref() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        AstNode::Block(stmts) => {
            for s in stmts { collect_written_arrays(s, names); }
        }
        _ => {}
    }
}

/// Collect all identifier names referenced in an AST node.
fn collect_references(node: &AstNode, refs: &mut Vec<String>) {
    match node {
        AstNode::Identifier(name) => {
            if !refs.contains(name) {
                refs.push(name.clone());
            }
        }
        AstNode::Apply { args, .. } => {
            for arg in args { collect_references(arg, refs); }
        }
        AstNode::IndexAccess { array, index } => {
            collect_references(array, refs);
            collect_references(index, refs);
        }
        AstNode::IndexAssign { array, index, value } => {
            collect_references(array, refs);
            collect_references(index, refs);
            collect_references(value, refs);
        }
        AstNode::Block(stmts) => {
            for s in stmts { collect_references(s, refs); }
        }
        AstNode::IfExpr { condition, then_branch, else_branch } => {
            collect_references(condition, refs);
            collect_references(then_branch, refs);
            if let Some(eb) = else_branch { collect_references(eb, refs); }
        }
        AstNode::Binding { value, .. } => collect_references(value, refs),
        AstNode::ArrayLiteral(elems) => {
            for e in elems { collect_references(e, refs); }
        }
        AstNode::Range { start, end } => {
            collect_references(start, refs);
            collect_references(end, refs);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::lexer::Lexer;
    use crate::lang::parser::Parser;

    fn run(source: &str) -> Value {
        let mut lex = Lexer::new(source);
        let tokens = lex.tokenize().unwrap();
        let mut parser = Parser::new(tokens, source.to_string());
        let ast = parser.parse().unwrap();
        eval(&ast, &CpuFallback).unwrap()
    }

    fn run_f64(source: &str) -> f64 {
        run(source).as_f64().unwrap()
    }

    #[test]
    fn test_number() {
        assert_eq!(run_f64("42"), 42.0);
    }

    #[test]
    fn test_arithmetic() {
        assert_eq!(run_f64("3 + 4 * 2"), 11.0);
        assert_eq!(run_f64("(3 + 4) * 2"), 14.0);
        assert_eq!(run_f64("10 / 3"), 10.0 / 3.0);
    }

    #[test]
    fn test_binding() {
        assert_eq!(run_f64("a := 5\na + 1"), 6.0);
    }

    #[test]
    fn test_function_def() {
        assert_eq!(run_f64("double(x) := x * 2\ndouble(21)"), 42.0);
    }

    #[test]
    fn test_if_expr() {
        assert_eq!(run_f64("if (3 > 2) 10 else 20"), 10.0);
        assert_eq!(run_f64("if (3 < 2) 10 else 20"), 20.0);
    }

    #[test]
    fn test_builtins() {
        assert!((run_f64("sin(0)") - 0.0).abs() < 1e-10);
        assert!((run_f64("cos(0)") - 1.0).abs() < 1e-10);
        assert!((run_f64("sqrt(4)") - 2.0).abs() < 1e-10);
        assert_eq!(run_f64("abs(-5)"), 5.0);
        assert_eq!(run_f64("min(3, 7)"), 3.0);
        assert_eq!(run_f64("max(3, 7)"), 7.0);
        assert_eq!(run_f64("clamp(10, 0, 5)"), 5.0);
    }

    #[test]
    fn test_array_literal() {
        let val = run("[1, 2, 3]");
        assert!(matches!(val, Value::Array(ref a) if a == &[1.0, 2.0, 3.0]));
    }

    #[test]
    fn test_index_access() {
        assert_eq!(run_f64("a := [10, 20, 30]\na[1]"), 20.0);
    }

    #[test]
    fn test_len() {
        assert_eq!(run_f64("a := [1, 2, 3, 4]\nlen(a)"), 4.0);
    }

    #[test]
    fn test_parallel_for_inplace() {
        let val = run(
            "data := [1, 2, 3, 4]\n\
             parallel for i in 0..4 ( data[i] := data[i] * 2 )\n\
             data"
        );
        match val {
            Value::Array(a) => assert_eq!(a, vec![2.0, 4.0, 6.0, 8.0]),
            _ => panic!("Expected array, got {:?}", val),
        }
    }

    #[test]
    fn test_parallel_for_with_len() {
        let val = run(
            "data := [10, 20, 30]\n\
             parallel for i in 0..len(data) ( data[i] := data[i] + 1 )\n\
             data"
        );
        match val {
            Value::Array(a) => assert_eq!(a, vec![11.0, 21.0, 31.0]),
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parallel_for_with_scalar() {
        let val = run(
            "data := [1, 2, 3]\nscale := 10\n\
             parallel for i in 0..3 ( data[i] := data[i] * scale )\n\
             data"
        );
        match val {
            Value::Array(a) => assert_eq!(a, vec![10.0, 20.0, 30.0]),
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parallel_for_returns_last_written() {
        // parallel for returns the last written array
        let val = run(
            "data := [1, 2, 3]\n\
             doubled := parallel for i in 0..3 ( data[i] := data[i] * 2 )\n\
             doubled"
        );
        match val {
            Value::Array(a) => assert_eq!(a, vec![2.0, 4.0, 6.0]),
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parallel_for_write_different_array() {
        let val = run(
            "a := [1, 2, 3]\n\
             b := [0, 0, 0]\n\
             parallel for i in 0..3 ( b[i] := a[i] + 10 )\n\
             b"
        );
        match val {
            Value::Array(a) => assert_eq!(a, vec![11.0, 12.0, 13.0]),
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_chained_parallel_for() {
        let val = run(
            "data := [1, 2, 3]\n\
             parallel for i in 0..3 ( data[i] := data[i] * 2 )\n\
             parallel for i in 0..3 ( data[i] := data[i] + 1 )\n\
             data"
        );
        match val {
            Value::Array(a) => assert_eq!(a, vec![3.0, 5.0, 7.0]),
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parallel_for_multi_statement() {
        let val = run(
            "data := [1, 2, 3, 4]\n\
             parallel for i in 0..4 (\n\
                 temp := data[i] * 2\n\
                 data[i] := temp + 1\n\
             )\n\
             data"
        );
        match val {
            Value::Array(a) => assert_eq!(a, vec![3.0, 5.0, 7.0, 9.0]),
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_for_loop_accumulate() {
        assert_eq!(
            run_f64("acc := 0\nfor i in 0..5 ( acc := acc + i )\nacc"),
            10.0
        );
    }

    #[test]
    fn test_for_loop_build_array() {
        let val = run(
            "data := [0, 0, 0, 0, 0]\n\
             for i in 0..5 ( data[i] := i * i )\n\
             data"
        );
        match val {
            Value::Array(a) => assert_eq!(a, vec![0.0, 1.0, 4.0, 9.0, 16.0]),
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_for_then_parallel_for() {
        let val = run(
            "data := [0, 0, 0]\n\
             for i in 0..3 ( data[i] := (i + 1) * 100 )\n\
             parallel for i in 0..3 ( data[i] := data[i] * 2 )\n\
             data"
        );
        match val {
            Value::Array(a) => assert_eq!(a, vec![200.0, 400.0, 600.0]),
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_display_value() {
        assert_eq!(format!("{}", Value::F64(42.0)), "42");
        assert_eq!(format!("{}", Value::F64(3.14)), "3.14");
        assert_eq!(format!("{}", Value::Bool(true)), "true");
        assert_eq!(format!("{}", Value::Array(vec![1.0, 2.5, 3.0])), "[1, 2.5, 3]");
    }
}
