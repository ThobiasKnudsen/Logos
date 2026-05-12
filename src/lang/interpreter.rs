use std::collections::HashMap;
use std::rc::Rc;

use super::ir::{BuiltinOp, Callee, EvalImpl, Ir};

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
                    if i > 0 {
                        write!(f, ", ")?;
                    }
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
    body: Ir,
}

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

/// Environment — variable + function bindings.
///
/// Functions are stored behind `Rc<HashMap>` so that creating a child scope
/// doesn't deep-clone the function table. `Rc::make_mut` is used on insertion
/// to give copy-on-write semantics. Vars are still cloned because most
/// function bodies only read a handful of parent vars and inserting params
/// would otherwise mutate the parent. (For deeply nested or hot-path calls
/// this could be improved further with a parent-pointer scope chain.)
#[derive(Debug, Clone)]
pub struct Env {
    vars: HashMap<String, Value>,
    funcs: Rc<HashMap<String, FuncDef>>,
}

impl Env {
    fn new() -> Self {
        Self {
            vars: HashMap::new(),
            funcs: Rc::new(HashMap::new()),
        }
    }

    fn child(&self) -> Self {
        Self {
            vars: self.vars.clone(),
            funcs: Rc::clone(&self.funcs),
        }
    }

    fn insert_func(&mut self, name: String, def: FuncDef) {
        Rc::make_mut(&mut self.funcs).insert(name, def);
    }

    fn get_func(&self, name: &str) -> Option<&FuncDef> {
        self.funcs.get(name)
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
    pub body: Ir,
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
            env.vars
                .insert(request.var_name.clone(), Value::F64(i as f64));
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

/// Evaluate an IR subtree, returning the final value.
/// `gpu` provides GPU dispatch for parallel for; use `CpuFallback` if no GPU.
pub fn eval(ast: &Ir, gpu: &dyn GpuDispatch) -> Result<Value, String> {
    let mut env = Env::new();
    eval_node(ast, &mut env, gpu)
}

// ---------------------------------------------------------------------------
// Core evaluator
// ---------------------------------------------------------------------------

fn eval_node(node: &Ir, env: &mut Env, gpu: &dyn GpuDispatch) -> Result<Value, String> {
    match node {
        Ir::Number { value, .. } => Ok(Value::F64(*value)),
        Ir::BoolLit { value, .. } => Ok(Value::Bool(*value)),

        Ir::Identifier { name, .. } => env
            .vars
            .get(name)
            .cloned()
            .ok_or_else(|| format!("Undefined variable: {}", name)),

        Ir::ArrayLiteral { items: elems, .. } => {
            let mut arr = Vec::with_capacity(elems.len());
            for e in elems {
                arr.push(eval_node(e, env, gpu)?.as_f64()?);
            }
            Ok(Value::Array(arr))
        }

        Ir::IndexAccess { array, index, .. } => {
            let arr = eval_node(array, env, gpu)?;
            let idx = eval_node(index, env, gpu)?.as_f64()? as usize;
            match arr {
                Value::Array(ref a) => a
                    .get(idx)
                    .copied()
                    .map(Value::F64)
                    .ok_or_else(|| format!("Index {} out of bounds (len {})", idx, a.len())),
                _ => Err("Cannot index into non-array".to_string()),
            }
        }

        Ir::Range { start, end, .. } => {
            // Ranges aren't values themselves — they're only valid inside parallel for.
            // If we reach here, the user wrote a range outside that context.
            let s = eval_node(start, env, gpu)?.as_f64()?;
            let e = eval_node(end, env, gpu)?.as_f64()?;
            // Return as array of indices for convenience
            let len = (e - s) as usize;
            let arr: Vec<f64> = (0..len).map(|i| s + i as f64).collect();
            Ok(Value::Array(arr))
        }

        Ir::Binding { name, value, .. } => {
            let val = eval_node(value, env, gpu)?;
            env.vars.insert(name.clone(), val);
            Ok(Value::Void)
        }

        Ir::TupleBinding { names, value, .. } => {
            let val = eval_node(value, env, gpu)?;
            match val {
                Value::Array(ref a) if a.len() == names.len() => {
                    for (name, &v) in names.iter().zip(a.iter()) {
                        env.vars.insert(name.clone(), Value::F64(v));
                    }
                }
                _ => {
                    return Err(format!(
                        "Tuple binding expects array of length {}, got {:?}",
                        names.len(),
                        val
                    ))
                }
            }
            Ok(Value::Void)
        }

        Ir::Block { items: stmts, .. } => {
            let mut last = Value::Void;
            for stmt in stmts {
                last = eval_node(stmt, env, gpu)?;
            }
            Ok(last)
        }

        Ir::Tuple { items: elems, .. } => {
            let mut arr = Vec::with_capacity(elems.len());
            for e in elems {
                arr.push(eval_node(e, env, gpu)?.as_f64()?);
            }
            Ok(Value::Array(arr))
        }

        Ir::FunctionDef {
            name, params, body, ..
        } => {
            env.insert_func(
                name.clone(),
                FuncDef {
                    params: params.clone(),
                    body: *body.clone(),
                },
            );
            Ok(Value::Void)
        }

        Ir::Lambda { .. } => {
            // Lambdas only have meaning as HOF arguments and are lifted to
            // synthetic function defs by `lower::lift_lambdas` before any
            // backend sees the IR. If one reaches the interpreter, the
            // lowering pass missed it.
            Err(
                "internal: unlifted lambda reached the interpreter (should have been \
                 specialized into a synthetic function def by wgsl_gen)"
                    .to_string(),
            )
        }

        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            let cond = eval_node(condition, env, gpu)?.as_bool()?;
            if cond {
                eval_node(then_branch, env, gpu)
            } else if let Some(eb) = else_branch {
                eval_node(eb, env, gpu)
            } else {
                Ok(Value::Void)
            }
        }

        Ir::ForLoop {
            var, range, body, ..
        } => {
            let (start, end) = match range.as_ref() {
                Ir::Range { start, end, .. } => {
                    let s_f = eval_node(start, env, gpu)?.as_f64()?;
                    let e_f = eval_node(end, env, gpu)?.as_f64()?;
                    if s_f < 0.0 || e_f < 0.0 {
                        return Err(format!(
                            "for loop range bounds must be non-negative ({}..{})",
                            s_f, e_f
                        ));
                    }
                    if e_f < s_f {
                        return Err(format!("for loop range end < start ({}..{})", s_f, e_f));
                    }
                    (s_f as usize, e_f as usize)
                }
                _ => return Err("for loop range must be start..end".to_string()),
            };
            if end - start > MAX_LOOP_ITERATIONS {
                return Err(format!(
                    "for loop range too large ({} iterations, max {})",
                    end - start,
                    MAX_LOOP_ITERATIONS
                ));
            }
            let mut last = Value::Void;
            for i in start..end {
                env.vars.insert(var.clone(), Value::F64(i as f64));
                last = eval_node(body, env, gpu)?;
            }
            Ok(last)
        }

        Ir::WhileLoop {
            condition, body, ..
        } => {
            let mut iters = 0usize;
            loop {
                let cond = eval_node(condition, env, gpu)?.as_bool()?;
                if !cond {
                    break;
                }
                eval_node(body, env, gpu)?;
                iters += 1;
                if iters >= MAX_LOOP_ITERATIONS {
                    return Err(format!(
                        "while loop exceeded {} iterations (possible infinite loop)",
                        MAX_LOOP_ITERATIONS
                    ));
                }
            }
            Ok(Value::Void)
        }

        Ir::PropertyAccess {
            object, property, ..
        } => {
            let val = eval_node(object, env, gpu)?;
            match (&val, property.as_str()) {
                (Value::Array(a), "len") => Ok(Value::F64(a.len() as f64)),
                _ => Err(format!("Unknown property .{} on {:?}", property, val)),
            }
        }

        Ir::Apply { callee, args, .. } => eval_apply(callee, args, env, gpu),

        Ir::IndexAssign {
            array,
            index,
            value,
            ..
        } => {
            let idx = eval_node(index, env, gpu)?.as_f64()? as usize;
            let val = eval_node(value, env, gpu)?.as_f64()?;
            if let Ir::Identifier { name, .. } = array.as_ref() {
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

        Ir::ParallelFor {
            var, range, body, ..
        } => eval_parallel_for(var, range, body, env, gpu),
    }
}

// ---------------------------------------------------------------------------
// Apply (operators + builtins + user functions)
// ---------------------------------------------------------------------------

fn eval_apply(
    callee: &Callee,
    args: &[Ir],
    env: &mut Env,
    gpu: &dyn GpuDispatch,
) -> Result<Value, String> {
    let op = match callee {
        Callee::Builtin(op) => *op,
        Callee::User(name) => {
            // Check user-defined functions
            if let Some(func) = env.get_func(name).cloned() {
                if args.len() != func.params.len() {
                    return Err(format!(
                        "Function '{}' expects {} args, got {}",
                        name,
                        func.params.len(),
                        args.len()
                    ));
                }
                let mut child = env.child();
                for (param, arg_node) in func.params.iter().zip(args.iter()) {
                    let val = eval_node(arg_node, env, gpu)?;
                    child.vars.insert(param.clone(), val);
                }
                return eval_node(&func.body, &mut child, gpu);
            }
            return Err(format!("Unknown function: {}", name));
        }
    };

    // Evaluate arguments
    let vals: Vec<Value> = args
        .iter()
        .map(|a| eval_node(a, env, gpu))
        .collect::<Result<_, _>>()?;

    // Generic shapes — match the per-op classification in ir.rs.
    match op.eval_impl() {
        EvalImpl::UnaryF(f) => {
            return Ok(Value::F64(f(vals[0].as_f64()?)));
        }
        EvalImpl::BinaryF(f) => {
            return Ok(Value::F64(f(vals[0].as_f64()?, vals[1].as_f64()?)));
        }
        EvalImpl::CmpF(f) => {
            return Ok(Value::Bool(f(vals[0].as_f64()?, vals[1].as_f64()?)));
        }
        EvalImpl::Custom => {}
    }

    match op {
        BuiltinOp::Div => {
            let (a, b) = (vals[0].as_f64()?, vals[1].as_f64()?);
            if b == 0.0 {
                return Err("division by zero".to_string());
            }
            Ok(Value::F64(a / b))
        }
        BuiltinOp::Mod => {
            let (a, b) = (vals[0].as_f64()?, vals[1].as_f64()?);
            if b == 0.0 {
                return Err("modulo by zero".to_string());
            }
            Ok(Value::F64(a % b))
        }

        BuiltinOp::And => Ok(Value::Bool(vals[0].as_bool()? && vals[1].as_bool()?)),
        BuiltinOp::Or => Ok(Value::Bool(vals[0].as_bool()? || vals[1].as_bool()?)),
        BuiltinOp::Not => Ok(Value::Bool(!vals[0].as_bool()?)),

        BuiltinOp::Clamp => {
            let (x, lo, hi) = (vals[0].as_f64()?, vals[1].as_f64()?, vals[2].as_f64()?);
            Ok(Value::F64(x.clamp(lo, hi)))
        }
        BuiltinOp::Mix => {
            let (a, b, t) = (vals[0].as_f64()?, vals[1].as_f64()?, vals[2].as_f64()?);
            Ok(Value::F64(a * (1.0 - t) + b * t))
        }
        BuiltinOp::Smoothstep => {
            let (edge0, edge1, x) = (vals[0].as_f64()?, vals[1].as_f64()?, vals[2].as_f64()?);
            let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
            Ok(Value::F64(t * t * (3.0 - 2.0 * t)))
        }

        BuiltinOp::Len => match &vals[0] {
            Value::Array(a) => Ok(Value::F64(a.len() as f64)),
            _ => Err("len() expects an array".to_string()),
        },

        BuiltinOp::F32 | BuiltinOp::F64 => Ok(Value::F64(vals[0].as_f64()?)),
        BuiltinOp::I32 => Ok(Value::F64((vals[0].as_f64()? as i32) as f64)),

        BuiltinOp::Print => {
            if vals.len() == 1 {
                Ok(vals.into_iter().next().unwrap())
            } else {
                Err("print() expects 1 argument".to_string())
            }
        }
        BuiltinOp::Plot => Ok(Value::Void),

        // Vector ops are not yet supported by the CPU interpreter.
        BuiltinOp::Length | BuiltinOp::Normalize | BuiltinOp::Dot | BuiltinOp::Cross
        | BuiltinOp::Vec2 | BuiltinOp::Vec3 | BuiltinOp::Vec4 => {
            Err(format!("Builtin '{}' not supported in interpreter", op.name()))
        }

        // Every other op carries a non-Custom EvalImpl and is handled above.
        _ => unreachable!("op {:?} returned Custom eval_impl but has no explicit arm", op),
    }
}

// ---------------------------------------------------------------------------
// Parallel for
// ---------------------------------------------------------------------------

fn eval_parallel_for(
    var: &str,
    range_node: &Ir,
    body: &Ir,
    env: &mut Env,
    gpu: &dyn GpuDispatch,
) -> Result<Value, String> {
    // Evaluate range
    let (start, end) = match range_node {
        Ir::Range { start, end, .. } => {
            let s_f = eval_node(start, env, gpu)?.as_f64()?;
            let e_f = eval_node(end, env, gpu)?.as_f64()?;
            if s_f < 0.0 || e_f < 0.0 {
                return Err(format!(
                    "parallel for range bounds must be non-negative ({}..{})",
                    s_f, e_f
                ));
            }
            if e_f < s_f {
                return Err(format!("parallel for range end < start ({}..{})", s_f, e_f));
            }
            (s_f as usize, e_f as usize)
        }
        _ => {
            let val = eval_node(range_node, env, gpu)?;
            match val {
                Value::Array(a) => (0, a.len()),
                _ => return Err("parallel for range must be start..end".to_string()),
            }
        }
    };

    // Find which arrays are written to (IndexAssign targets). Vec preserves
    // insertion order (used below for `last_written`); HashSet does dedup
    // checks in O(1).
    let mut written_names = Vec::new();
    let mut written_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    collect_written_arrays(body, &mut written_names, &mut written_set);

    // Find all referenced identifiers
    let mut referenced = Vec::new();
    let mut referenced_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    collect_references(body, &mut referenced, &mut referenced_set);

    // Partition into readwrite, readonly, and scalars
    let mut readwrite_arrays = Vec::new();
    let mut readonly_arrays = Vec::new();
    let mut scalars = Vec::new();

    for name in &referenced {
        if name == var {
            continue;
        }
        if let Some(val) = env.vars.get(name) {
            match val {
                Value::Array(a) => {
                    if written_set.contains(name) {
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
        env.vars
            .get(&name)
            .cloned()
            .ok_or_else(|| format!("Array '{}' not found", name))
    } else {
        Ok(Value::Void)
    }
}

/// Collect array names that are targets of IndexAssign in the body.
/// `names` preserves insertion order; `seen` provides O(1) dedup.
fn collect_written_arrays(
    node: &Ir,
    names: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    match node {
        Ir::IndexAssign { array, .. } => {
            if let Ir::Identifier { name, .. } = array.as_ref() {
                if seen.insert(name.clone()) {
                    names.push(name.clone());
                }
            }
        }
        Ir::Block { items: stmts, .. } => {
            for s in stmts {
                collect_written_arrays(s, names, seen);
            }
        }
        _ => {}
    }
}

/// Collect all identifier names referenced in an IR node.
/// `refs` preserves insertion order; `seen` provides O(1) dedup.
fn collect_references(
    node: &Ir,
    refs: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    match node {
        Ir::Identifier { name, .. } => {
            if seen.insert(name.clone()) {
                refs.push(name.clone());
            }
        }
        Ir::Apply { args, .. } => {
            for arg in args {
                collect_references(arg, refs, seen);
            }
        }
        Ir::IndexAccess { array, index, .. } => {
            collect_references(array, refs, seen);
            collect_references(index, refs, seen);
        }
        Ir::IndexAssign {
            array,
            index,
            value,
            ..
        } => {
            collect_references(array, refs, seen);
            collect_references(index, refs, seen);
            collect_references(value, refs, seen);
        }
        Ir::Block { items: stmts, .. } => {
            for s in stmts {
                collect_references(s, refs, seen);
            }
        }
        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            collect_references(condition, refs, seen);
            collect_references(then_branch, refs, seen);
            if let Some(eb) = else_branch {
                collect_references(eb, refs, seen);
            }
        }
        Ir::Binding { value, .. } => collect_references(value, refs, seen),
        Ir::ArrayLiteral { items: elems, .. } => {
            for e in elems {
                collect_references(e, refs, seen);
            }
        }
        Ir::Range { start, end, .. } => {
            collect_references(start, refs, seen);
            collect_references(end, refs, seen);
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
        let val = run("data := [1, 2, 3, 4]\n\
             parallel for i in 0..4 ( data[i] := data[i] * 2 )\n\
             data");
        match val {
            Value::Array(a) => assert_eq!(a, vec![2.0, 4.0, 6.0, 8.0]),
            _ => panic!("Expected array, got {:?}", val),
        }
    }

    #[test]
    fn test_parallel_for_with_len() {
        let val = run("data := [10, 20, 30]\n\
             parallel for i in 0..len(data) ( data[i] := data[i] + 1 )\n\
             data");
        match val {
            Value::Array(a) => assert_eq!(a, vec![11.0, 21.0, 31.0]),
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parallel_for_with_scalar() {
        let val = run("data := [1, 2, 3]\nscale := 10\n\
             parallel for i in 0..3 ( data[i] := data[i] * scale )\n\
             data");
        match val {
            Value::Array(a) => assert_eq!(a, vec![10.0, 20.0, 30.0]),
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parallel_for_returns_last_written() {
        // parallel for returns the last written array
        let val = run("data := [1, 2, 3]\n\
             doubled := parallel for i in 0..3 ( data[i] := data[i] * 2 )\n\
             doubled");
        match val {
            Value::Array(a) => assert_eq!(a, vec![2.0, 4.0, 6.0]),
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parallel_for_write_different_array() {
        let val = run("a := [1, 2, 3]\n\
             b := [0, 0, 0]\n\
             parallel for i in 0..3 ( b[i] := a[i] + 10 )\n\
             b");
        match val {
            Value::Array(a) => assert_eq!(a, vec![11.0, 12.0, 13.0]),
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_chained_parallel_for() {
        let val = run("data := [1, 2, 3]\n\
             parallel for i in 0..3 ( data[i] := data[i] * 2 )\n\
             parallel for i in 0..3 ( data[i] := data[i] + 1 )\n\
             data");
        match val {
            Value::Array(a) => assert_eq!(a, vec![3.0, 5.0, 7.0]),
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parallel_for_multi_statement() {
        let val = run("data := [1, 2, 3, 4]\n\
             parallel for i in 0..4 (\n\
                 temp := data[i] * 2\n\
                 data[i] := temp + 1\n\
             )\n\
             data");
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
        let val = run("data := [0, 0, 0, 0, 0]\n\
             for i in 0..5 ( data[i] := i * i )\n\
             data");
        match val {
            Value::Array(a) => assert_eq!(a, vec![0.0, 1.0, 4.0, 9.0, 16.0]),
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_for_then_parallel_for() {
        let val = run("data := [0, 0, 0]\n\
             for i in 0..3 ( data[i] := (i + 1) * 100 )\n\
             parallel for i in 0..3 ( data[i] := data[i] * 2 )\n\
             data");
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
        assert_eq!(
            format!("{}", Value::Array(vec![1.0, 2.5, 3.0])),
            "[1, 2.5, 3]"
        );
    }

    // -----------------------------------------------------------------------
    // Error-path tests
    // -----------------------------------------------------------------------

    fn try_run(source: &str) -> Result<Value, String> {
        let mut lex = Lexer::new(source);
        let tokens = lex.tokenize()?;
        let mut parser = Parser::new(tokens, source.to_string());
        let ast = parser.parse()?;
        eval(&ast, &CpuFallback)
    }

    #[test]
    fn err_division_by_zero() {
        let err = try_run("1 / 0").unwrap_err();
        assert!(err.contains("zero"), "got: {}", err);
    }

    #[test]
    fn err_modulo_by_zero() {
        let err = try_run("mod(5, 0)").unwrap_err();
        assert!(err.contains("zero"), "got: {}", err);
    }

    #[test]
    fn err_undefined_variable() {
        let err = try_run("nonexistent_var").unwrap_err();
        assert!(err.contains("Undefined"), "got: {}", err);
        assert!(err.contains("nonexistent_var"), "got: {}", err);
    }

    #[test]
    fn err_index_out_of_bounds_read() {
        let err = try_run("a := [1, 2, 3]\na[5]").unwrap_err();
        assert!(err.contains("out of bounds"), "got: {}", err);
    }

    #[test]
    fn err_index_out_of_bounds_write() {
        let err = try_run("a := [1, 2, 3]\nfor i in 0..1 ( a[10] := 99 )").unwrap_err();
        assert!(err.contains("out of bounds"), "got: {}", err);
    }

    #[test]
    fn err_for_loop_negative_range() {
        let err = try_run("for i in 0..(-3) ( i )").unwrap_err();
        assert!(
            err.contains("non-negative") || err.contains("end < start"),
            "got: {}",
            err
        );
    }

    #[test]
    fn err_for_loop_inverted_range() {
        let err = try_run("for i in 5..2 ( i )").unwrap_err();
        assert!(err.contains("end < start"), "got: {}", err);
    }

    #[test]
    fn err_for_loop_too_large() {
        let err = try_run("for i in 0..1000000 ( i )").unwrap_err();
        assert!(err.contains("too large"), "got: {}", err);
    }

    #[test]
    fn err_unknown_function() {
        let err = try_run("totally_made_up(3)").unwrap_err();
        assert!(
            err.contains("Unknown") || err.contains("Undefined"),
            "got: {}",
            err
        );
    }

    #[test]
    fn err_function_arity_mismatch() {
        let err = try_run("f(x, y) := x + y\nf(1)").unwrap_err();
        assert!(
            err.contains("expects") && err.contains("got"),
            "got: {}",
            err
        );
    }

    #[test]
    fn err_index_into_non_array() {
        // `q` (not an axis var) so it can be bound; `5` is a scalar so [0] is invalid.
        let err = try_run("q := 5\nq[0]").unwrap_err();
        assert!(
            err.contains("non-array") || err.contains("not an array"),
            "got: {}",
            err
        );
    }
}
