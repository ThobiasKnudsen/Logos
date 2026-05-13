//! Type checker for Logos IR.
//!
//! Walks an `Ir` tree producing a `Type` for the program (or a `TypeError`
//! with a span). Inference everywhere — there are no surface type
//! annotations to write. Bindings carry their inferred type forward in a
//! scoped environment that the checker maintains as it walks.
//!
//! Per-builtin behavior is driven by `BuiltinOp::signatures()` (in
//! `signatures` below) so adding a new operator means adding one row to a
//! table, not editing a giant `match`. Each builtin can have multiple
//! signatures — that's how `+` works on `(Num, Num) → Num` and
//! `(VecN, VecN) → VecN` without special-casing the operator.
//!
//! Wired into `lang::compile` and the notebook's plot / whole-cell compile
//! paths via `lang::type_check`, which formats the resulting `TypeError`
//! into a `Line N, Col M:` message using `format_error_at`.

use std::collections::HashMap;

use super::ir::{BuiltinOp, Callee, Ir, Span, Type};

/// A type-checking failure with the source span of the offending node.
#[derive(Debug, Clone)]
pub struct TypeError {
    pub message: String,
    pub span: Span,
}

impl TypeError {
    fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

/// Lexically scoped variable + function environment.
///
/// Variables and functions are kept in separate maps so a name can refer to
/// either without the lookup order mattering. Child scopes share a parent
/// pointer; lookups walk up the chain.
struct TypeEnv<'p> {
    vars: HashMap<String, Type>,
    funcs: HashMap<String, (Vec<Type>, Type)>,
    parent: Option<&'p TypeEnv<'p>>,
}

impl<'p> TypeEnv<'p> {
    fn root() -> Self {
        Self {
            vars: HashMap::new(),
            funcs: HashMap::new(),
            parent: None,
        }
    }

    fn child<'c>(&'c self) -> TypeEnv<'c> {
        TypeEnv {
            vars: HashMap::new(),
            funcs: HashMap::new(),
            parent: Some(self),
        }
    }

    fn get_var(&self, name: &str) -> Option<&Type> {
        self.vars
            .get(name)
            .or_else(|| self.parent.and_then(|p| p.get_var(name)))
    }

    fn get_func(&self, name: &str) -> Option<&(Vec<Type>, Type)> {
        self.funcs
            .get(name)
            .or_else(|| self.parent.and_then(|p| p.get_func(name)))
    }
}

/// Type-check an IR program. Returns the type of the program's value
/// (typically the type of the last expression in the top-level block).
///
/// Today this clones and calls `check_mut` so existing read-only callers can
/// keep their `&Ir` shape. `lower::annotate_types` shares the same engine
/// via `check_mut` so every `Apply.result_ty` ends up populated as a side
/// effect of inference.
pub fn check(ir: &Ir) -> Result<Type, TypeError> {
    let mut owned = ir.clone();
    check_mut(&mut owned)
}

/// Mutating variant of `check`. Walks `ir` and writes `result_ty` on every
/// `Apply` node as inference proceeds. `lower::annotate_types` is the
/// production caller; `check` is a clone-and-discard wrapper kept for the
/// `Result<Type, _>`-only callers (e.g. `lang::type_check`).
pub(crate) fn check_mut(ir: &mut Ir) -> Result<Type, TypeError> {
    let mut env = TypeEnv::root();
    seed_axis_vars(&mut env);
    infer(ir, &mut env)
}

/// Seed the root scope with the always-available axis variables. These are
/// `f32` world coordinates in the fragment shader; `t` is the time uniform.
/// Without this, every cell that uses `x`, `y`, or `t` fails to type-check.
fn seed_axis_vars(env: &mut TypeEnv) {
    env.vars.insert("x".to_string(), Type::Num);
    env.vars.insert("y".to_string(), Type::Num);
    env.vars.insert("z".to_string(), Type::Num);
    env.vars.insert("t".to_string(), Type::Num);
}

/// Core inference. Walks the node, mutates `env` for bindings/functions, and
/// returns the inferred type or a `TypeError`. Also writes the inferred type
/// into each `Apply.result_ty` as a side effect — `lower::annotate_types`
/// relies on this so backends can read types off the IR without a second walk.
fn infer(node: &mut Ir, env: &mut TypeEnv) -> Result<Type, TypeError> {
    match node {
        Ir::Number { .. } => Ok(Type::Num),
        Ir::BoolLit { .. } => Ok(Type::Bool),

        Ir::Identifier { name, span, .. } => env
            .get_var(name)
            .cloned()
            // Function names used as values (e.g. `N_integral(sq, 0, x, d)` —
            // `sq` referenced as a function pointer) aren't proper values yet.
            // Accept them as `Unknown` so type-checking still completes;
            // codegen separately rejects unrepresentable higher-order uses.
            .or_else(|| env.get_func(name).map(|_| Type::Unknown))
            .ok_or_else(|| TypeError::new(*span, format!("Undefined variable `{}`", name))),

        Ir::Apply {
            callee,
            args,
            span,
            result_ty,
        } => {
            let ty = infer_apply(callee, args, *span, env)?;
            *result_ty = Some(Box::new(ty.clone()));
            Ok(ty)
        }

        Ir::Tuple { items, .. } => {
            let item_types: Result<Vec<_>, _> =
                items.iter_mut().map(|i| infer(i, env)).collect();
            Ok(Type::Tuple(item_types?))
        }

        Ir::Binding {
            name,
            value,
            value_ty,
            ..
        } => {
            let ty = infer(value, env)?;
            *value_ty = Some(Box::new(ty.clone()));
            env.vars.insert(name.clone(), ty);
            // A binding statement contributes no value to its enclosing block.
            Ok(Type::Void)
        }

        Ir::TupleBinding { names, value, span } => {
            let ty = infer(value, env)?;
            match &ty {
                Type::Tuple(items) if items.len() == names.len() => {
                    for (n, t) in names.iter().zip(items.iter()) {
                        env.vars.insert(n.clone(), t.clone());
                    }
                    Ok(Type::Void)
                }
                Type::Tuple(items) => Err(TypeError::new(
                    *span,
                    format!(
                        "tuple binding expects {} names, RHS has {}",
                        names.len(),
                        items.len()
                    ),
                )),
                other => Err(TypeError::new(
                    *span,
                    format!(
                        "tuple binding RHS must be a tuple, got {}",
                        other.display()
                    ),
                )),
            }
        }

        Ir::Block { items, .. } => {
            let mut child = env.child();
            // Pre-register FunctionDef signatures so forward references inside
            // sibling function bodies resolve. After lowering, synthesized
            // helper functions are *prepended* to the block and reference
            // user-defined originals declared later in the same block; without
            // this pre-pass, the synthesized body's type-check fails on the
            // forward call. Each entry uses `Type::Unknown` as a placeholder
            // return type, then gets overwritten when the FunctionDef itself
            // is visited and its real return type is computed.
            for stmt in items.iter() {
                if let Ir::FunctionDef { name, params, .. } = stmt {
                    let param_tys = params.iter().map(|_| Type::Num).collect();
                    child
                        .funcs
                        .insert(name.clone(), (param_tys, Type::Unknown));
                }
            }
            let mut last = Type::Void;
            for stmt in items.iter_mut() {
                last = infer(stmt, &mut child)?;
            }
            Ok(last)
        }

        Ir::IfExpr {
            condition,
            then_branch,
            else_branch,
            span,
        } => {
            let cond_ty = infer(condition, env)?;
            // Numeric conditions are accepted as implicit-curve booleans
            // (matching the fragment-shader convention used by wgsl_gen).
            if !matches!(cond_ty, Type::Bool | Type::Num | Type::Unknown) {
                return Err(TypeError::new(
                    condition.span(),
                    format!(
                        "if condition must be Bool or Num, got {}",
                        cond_ty.display()
                    ),
                ));
            }
            let then_ty = infer(then_branch, env)?;
            let else_ty = match else_branch {
                Some(e) => infer(e, env)?,
                None => Type::Void,
            };
            unify_branches(&then_ty, &else_ty).ok_or_else(|| {
                TypeError::new(
                    *span,
                    format!(
                        "if branches have incompatible types: {} vs {}",
                        then_ty.display(),
                        else_ty.display()
                    ),
                )
            })
        }

        Ir::Lambda { params, body, .. } => {
            // Type-check the body in a child scope where each lambda param
            // is a fresh numeric. The lambda itself doesn't have a useful
            // value type — codegen lifts it to a synthetic function before
            // anything tries to use it — so we report `Unknown`.
            let mut child = env.child();
            for p in params.iter() {
                child.vars.insert(p.clone(), Type::Num);
            }
            let _body_ty = infer(body, &mut child)?;
            Ok(Type::Unknown)
        }

        Ir::FunctionDef {
            name, params, body, ..
        } => {
            // We don't yet have parameter type annotations, so each parameter
            // is treated as `Num` for now — the dominant case for math cells.
            // When polymorphic user functions matter, this is where to widen
            // (e.g., infer from call sites or accept explicit annotations).
            let mut child = env.child();
            for p in params.iter() {
                child.vars.insert(p.clone(), Type::Num);
            }
            let body_ty = infer(body, &mut child)?;
            let param_tys: Vec<Type> = params.iter().map(|_| Type::Num).collect();
            env.funcs
                .insert(name.clone(), (param_tys, body_ty.clone()));
            Ok(Type::Void)
        }

        Ir::ForLoop {
            var, range, body, ..
        } => {
            let range_ty = infer(range, env)?;
            if !matches!(range_ty, Type::Range | Type::Unknown) {
                return Err(TypeError::new(
                    range.span(),
                    format!("for loop expects a Range, got {}", range_ty.display()),
                ));
            }
            let mut child = env.child();
            child.vars.insert(var.clone(), Type::Num);
            infer(body, &mut child)?;
            Ok(Type::Void)
        }

        Ir::WhileLoop {
            condition, body, ..
        } => {
            let cond_ty = infer(condition, env)?;
            if !matches!(cond_ty, Type::Bool | Type::Num | Type::Unknown) {
                return Err(TypeError::new(
                    condition.span(),
                    format!(
                        "while condition must be Bool or Num, got {}",
                        cond_ty.display()
                    ),
                ));
            }
            infer(body, env)?;
            Ok(Type::Void)
        }

        Ir::Range { start, end, .. } => {
            let s = infer(start, env)?;
            let e = infer(end, env)?;
            if !matches!(s, Type::Num | Type::Unknown) {
                return Err(TypeError::new(
                    start.span(),
                    format!("range start must be Num, got {}", s.display()),
                ));
            }
            if !matches!(e, Type::Num | Type::Unknown) {
                return Err(TypeError::new(
                    end.span(),
                    format!("range end must be Num, got {}", e.display()),
                ));
            }
            Ok(Type::Range)
        }

        Ir::ArrayLiteral { items, span } => {
            if items.is_empty() {
                // Empty array — element type is unknown until used.
                return Ok(Type::Array(Box::new(Type::Unknown)));
            }
            let span = *span;
            let (first_item, rest) = items.split_first_mut().unwrap();
            let first = infer(first_item, env)?;
            for item in rest.iter_mut() {
                let ty = infer(item, env)?;
                if ty != first {
                    return Err(TypeError::new(
                        span,
                        format!(
                            "array elements have mixed types: {} and {}",
                            first.display(),
                            ty.display()
                        ),
                    ));
                }
            }
            Ok(Type::Array(Box::new(first)))
        }

        Ir::IndexAccess { array, index, .. } => {
            let arr_ty = infer(array, env)?;
            let idx_ty = infer(index, env)?;
            if !matches!(idx_ty, Type::Num | Type::Unknown) {
                return Err(TypeError::new(
                    index.span(),
                    format!("array index must be Num, got {}", idx_ty.display()),
                ));
            }
            match arr_ty {
                Type::Array(elem) => Ok(*elem),
                Type::Unknown => Ok(Type::Unknown),
                other => Err(TypeError::new(
                    array.span(),
                    format!("cannot index a non-array value of type {}", other.display()),
                )),
            }
        }

        Ir::IndexAssign {
            array,
            index,
            value,
            ..
        } => {
            let arr_ty = infer(array, env)?;
            let idx_ty = infer(index, env)?;
            let val_ty = infer(value, env)?;
            if !matches!(idx_ty, Type::Num | Type::Unknown) {
                return Err(TypeError::new(
                    index.span(),
                    format!("array index must be Num, got {}", idx_ty.display()),
                ));
            }
            match arr_ty {
                Type::Array(elem) if *elem == val_ty || matches!(*elem, Type::Unknown) => {
                    Ok(Type::Void)
                }
                Type::Array(elem) => Err(TypeError::new(
                    value.span(),
                    format!(
                        "array element type {} doesn't match assigned value type {}",
                        elem.display(),
                        val_ty.display()
                    ),
                )),
                Type::Unknown => Ok(Type::Void),
                other => Err(TypeError::new(
                    array.span(),
                    format!(
                        "indexed assignment requires an Array, got {}",
                        other.display()
                    ),
                )),
            }
        }

        Ir::ParallelFor {
            var, range, body, ..
        } => {
            let range_ty = infer(range, env)?;
            if !matches!(range_ty, Type::Range | Type::Unknown) {
                return Err(TypeError::new(
                    range.span(),
                    format!(
                        "parallel for expects a Range, got {}",
                        range_ty.display()
                    ),
                ));
            }
            let mut child = env.child();
            child.vars.insert(var.clone(), Type::Num);
            infer(body, &mut child)?;
            Ok(Type::Void)
        }

        Ir::PropertyAccess {
            object,
            property,
            span,
        } => {
            // Today only axis property access is meaningful (`x.min`, `y.res`,
            // etc.), and they all produce `Num`. Anything else is rejected so
            // typos don't slip through.
            let prop_span = *span;
            let _ = infer(object, env)?;
            match property.as_str() {
                "min" | "max" | "res" => Ok(Type::Num),
                other => Err(TypeError::new(
                    prop_span,
                    format!("unknown property `.{}`", other),
                )),
            }
        }
    }
}

/// Infer the type of an `Apply` node. Builtins go through `signatures` for
/// overload resolution; user calls look up the function table.
fn infer_apply(
    callee: &Callee,
    args: &mut [Ir],
    span: Span,
    env: &mut TypeEnv,
) -> Result<Type, TypeError> {
    let arg_types: Result<Vec<_>, _> = args.iter_mut().map(|a| infer(a, env)).collect();
    let arg_types = arg_types?;

    match callee {
        Callee::Builtin(op) => match resolve_builtin(*op, &arg_types) {
            Ok(ret) => Ok(ret),
            Err(BuiltinResolveErr::WrongArity { expected, got }) => Err(TypeError::new(
                span,
                format!(
                    "`{}` expects {} arg{}, got {}",
                    op.name(),
                    expected,
                    if expected == 1 { "" } else { "s" },
                    got
                ),
            )),
            Err(BuiltinResolveErr::NoMatch) => {
                // If an arithmetic op was applied to a tuple, point at the
                // tuple itself rather than printing a generic overload-
                // resolution failure. Don't speculate about intent — just
                // state the mismatch.
                let is_arith = matches!(
                    op,
                    BuiltinOp::Add
                        | BuiltinOp::Sub
                        | BuiltinOp::Mul
                        | BuiltinOp::Div
                        | BuiltinOp::Mod
                        | BuiltinOp::Pow
                        | BuiltinOp::Neg
                );
                if is_arith {
                    if let Some(idx) = arg_types.iter().position(|t| matches!(t, Type::Tuple(_))) {
                        let n = match &arg_types[idx] {
                            Type::Tuple(items) => items.len(),
                            _ => 0,
                        };
                        return Err(TypeError::new(
                            args[idx].span(),
                            format!(
                                "expected a scalar, found a {}-element tuple",
                                n
                            ),
                        ));
                    }
                }
                let arg_str: Vec<String> = arg_types.iter().map(|t| t.display()).collect();
                Err(TypeError::new(
                    span,
                    format!(
                        "`{}` has no overload matching ({})",
                        op.name(),
                        arg_str.join(", ")
                    ),
                ))
            }
        },
        Callee::User(name) => {
            // Function-typed parameters (e.g. `N_integral(f, x0, x1, d)` where
            // `f` is called as `f(i*d)` inside the body) land as variables in
            // the env, not as functions — the inference pass hasn't carried
            // their arity through yet. Accept the call with an unknown
            // signature so higher-order user functions still type-check.
            let (params, ret) = match env.get_func(name).cloned() {
                Some(sig) => sig,
                None if env.get_var(name).is_some() => {
                    return Ok(Type::Unknown);
                }
                None => {
                    return Err(TypeError::new(
                        span,
                        format!("Undefined function `{}`", name),
                    ));
                }
            };
            if params.len() != arg_types.len() {
                return Err(TypeError::new(
                    span,
                    format!(
                        "`{}` expects {} arg{}, got {}",
                        name,
                        params.len(),
                        if params.len() == 1 { "" } else { "s" },
                        arg_types.len()
                    ),
                ));
            }
            for (i, (expected, actual)) in params.iter().zip(arg_types.iter()).enumerate() {
                if expected != actual && !matches!(actual, Type::Unknown) {
                    return Err(TypeError::new(
                        args[i].span(),
                        format!(
                            "argument {} of `{}`: expected {}, got {}",
                            i + 1,
                            name,
                            expected.display(),
                            actual.display()
                        ),
                    ));
                }
            }
            Ok(ret)
        }
    }
}

// ---------------------------------------------------------------------------
// Per-builtin signatures (overload resolution)
// ---------------------------------------------------------------------------

/// Why a builtin couldn't be resolved against the actual argument types.
enum BuiltinResolveErr {
    WrongArity { expected: usize, got: usize },
    NoMatch,
}

/// Pick the right return type for a builtin given the actual argument types.
///
/// Each builtin defines its allowed (input pattern → output) shapes inline.
/// New builtins or new overloads slot in by adding a row — no checker
/// changes needed.
fn resolve_builtin(op: BuiltinOp, args: &[Type]) -> Result<Type, BuiltinResolveErr> {
    use BuiltinOp::*;

    // Helpers for the common patterns.
    let unary_num = |args: &[Type]| -> Result<Type, BuiltinResolveErr> {
        match args {
            [Type::Num] | [Type::Unknown] => Ok(Type::Num),
            [_] => Err(BuiltinResolveErr::NoMatch),
            _ => Err(BuiltinResolveErr::WrongArity {
                expected: 1,
                got: args.len(),
            }),
        }
    };
    let binary_num = |args: &[Type]| -> Result<Type, BuiltinResolveErr> {
        match args {
            [Type::Num, Type::Num] => Ok(Type::Num),
            [Type::Unknown, _] | [_, Type::Unknown] => Ok(Type::Num),
            [_, _] => Err(BuiltinResolveErr::NoMatch),
            _ => Err(BuiltinResolveErr::WrongArity {
                expected: 2,
                got: args.len(),
            }),
        }
    };
    let ternary_num = |args: &[Type]| -> Result<Type, BuiltinResolveErr> {
        match args {
            [Type::Num, Type::Num, Type::Num] => Ok(Type::Num),
            [Type::Unknown, _, _] | [_, Type::Unknown, _] | [_, _, Type::Unknown] => Ok(Type::Num),
            [_, _, _] => Err(BuiltinResolveErr::NoMatch),
            _ => Err(BuiltinResolveErr::WrongArity {
                expected: 3,
                got: args.len(),
            }),
        }
    };
    let comparison = |args: &[Type]| -> Result<Type, BuiltinResolveErr> {
        match args {
            [Type::Num, Type::Num] => Ok(Type::Bool),
            [Type::Unknown, _] | [_, Type::Unknown] => Ok(Type::Bool),
            [_, _] => Err(BuiltinResolveErr::NoMatch),
            _ => Err(BuiltinResolveErr::WrongArity {
                expected: 2,
                got: args.len(),
            }),
        }
    };
    let binary_bool = |args: &[Type]| -> Result<Type, BuiltinResolveErr> {
        match args {
            [Type::Bool, Type::Bool] => Ok(Type::Bool),
            // Allow numeric operands as implicit-curve bools.
            [Type::Num | Type::Bool | Type::Unknown, Type::Num | Type::Bool | Type::Unknown] => {
                Ok(Type::Bool)
            }
            _ => Err(BuiltinResolveErr::WrongArity {
                expected: 2,
                got: args.len(),
            }),
        }
    };

    match op {
        // Arithmetic — Num,Num → Num. (Vec overloads to land when the first
        // backend that needs them does.)
        Add | Sub | Mul | Div | Mod | Pow => binary_num(args),
        Neg => unary_num(args),

        // Comparison
        Eq | Neq | Lt | Gt | Lte | Gte => comparison(args),

        // Logical
        And | Or => binary_bool(args),
        Not => match args {
            [Type::Bool] | [Type::Num] | [Type::Unknown] => Ok(Type::Bool),
            [_] => Err(BuiltinResolveErr::NoMatch),
            _ => Err(BuiltinResolveErr::WrongArity {
                expected: 1,
                got: args.len(),
            }),
        },

        // Unary math
        Sin | Cos | Tan | Asin | Acos | Sinh | Cosh | Tanh | Log | Log2 | Log10 | Exp
        | Exp2 | Sqrt | Abs | Sign | Floor | Ceil | Round | Fract => unary_num(args),

        // atan: 1 arg → atan, 2 args → atan2
        Atan => match args.len() {
            1 => unary_num(args),
            2 => binary_num(args),
            n => Err(BuiltinResolveErr::WrongArity {
                expected: 1,
                got: n,
            }),
        },

        // Binary math
        Min | Max | Step => binary_num(args),

        // Ternary math
        Clamp | Mix | Smoothstep => ternary_num(args),

        // Vector math
        Length => match args {
            [t] if t.is_vec() || matches!(t, Type::Unknown) => Ok(Type::Num),
            [_] => Err(BuiltinResolveErr::NoMatch),
            _ => Err(BuiltinResolveErr::WrongArity {
                expected: 1,
                got: args.len(),
            }),
        },
        Normalize => match args {
            [t] if t.is_vec() => Ok(t.clone()),
            [Type::Unknown] => Ok(Type::Unknown),
            [_] => Err(BuiltinResolveErr::NoMatch),
            _ => Err(BuiltinResolveErr::WrongArity {
                expected: 1,
                got: args.len(),
            }),
        },
        Dot => match args {
            [a, b] if a.is_vec() && a == b => Ok(Type::Num),
            [Type::Unknown, _] | [_, Type::Unknown] => Ok(Type::Num),
            [_, _] => Err(BuiltinResolveErr::NoMatch),
            _ => Err(BuiltinResolveErr::WrongArity {
                expected: 2,
                got: args.len(),
            }),
        },
        Cross => match args {
            [Type::Vec3, Type::Vec3] => Ok(Type::Vec3),
            [Type::Unknown, _] | [_, Type::Unknown] => Ok(Type::Vec3),
            [_, _] => Err(BuiltinResolveErr::NoMatch),
            _ => Err(BuiltinResolveErr::WrongArity {
                expected: 2,
                got: args.len(),
            }),
        },

        // Constructors — accept any number of Num components, output the
        // matching width. (WGSL also allows shorter arg counts that broadcast
        // a single scalar; we match WGSL's relaxed rules here.)
        Vec2 => check_all_num(args).map(|_| Type::Vec2),
        Vec3 => check_all_num(args).map(|_| Type::Vec3),
        Vec4 => check_all_num(args).map(|_| Type::Vec4),

        // Casts — Num in, Num out (precision is a backend concern)
        F32 | F64 | I32 => unary_num(args),

        // Array
        Len => match args {
            [Type::Array(_)] | [Type::Unknown] => Ok(Type::Num),
            [_] => Err(BuiltinResolveErr::NoMatch),
            _ => Err(BuiltinResolveErr::WrongArity {
                expected: 1,
                got: args.len(),
            }),
        },

        // Actions: print(x) returns x's type; plot(_) returns Void.
        Print => match args {
            [t] => Ok(t.clone()),
            _ => Err(BuiltinResolveErr::WrongArity {
                expected: 1,
                got: args.len(),
            }),
        },
        Plot => match args {
            [_] => Ok(Type::Void),
            _ => Err(BuiltinResolveErr::WrongArity {
                expected: 1,
                got: args.len(),
            }),
        },
    }
}

fn check_all_num(args: &[Type]) -> Result<(), BuiltinResolveErr> {
    if args.is_empty() {
        return Err(BuiltinResolveErr::WrongArity {
            expected: 1,
            got: 0,
        });
    }
    for a in args {
        if !matches!(a, Type::Num | Type::Unknown) {
            return Err(BuiltinResolveErr::NoMatch);
        }
    }
    Ok(())
}

/// Pick a single type for the value of an `if` whose branches infer to
/// `then_ty` and `else_ty`. Identical types unify trivially; an `Unknown`
/// on either side defers to the other; nothing else combines.
fn unify_branches(then_ty: &Type, else_ty: &Type) -> Option<Type> {
    if then_ty == else_ty {
        return Some(then_ty.clone());
    }
    match (then_ty, else_ty) {
        (Type::Unknown, t) | (t, Type::Unknown) => Some(t.clone()),
        // A missing else branch produces Void — accept that as "this if
        // is used as a statement, the value is whatever the then-branch is."
        (t, Type::Void) | (Type::Void, t) => Some(t.clone()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::parse;

    fn check_str(source: &str) -> Result<Type, TypeError> {
        let ir = parse(source).expect("parse failed");
        check(&ir)
    }

    #[test]
    fn number_literal_is_num() {
        assert_eq!(check_str("3").unwrap(), Type::Num);
    }

    #[test]
    fn bool_literal_is_bool() {
        assert_eq!(check_str("true").unwrap(), Type::Bool);
    }

    #[test]
    fn axis_var_x_is_num() {
        assert_eq!(check_str("x").unwrap(), Type::Num);
    }

    #[test]
    fn arithmetic_is_num() {
        assert_eq!(check_str("x + y * 2").unwrap(), Type::Num);
        assert_eq!(check_str("x ^ 2").unwrap(), Type::Num);
        assert_eq!(check_str("-x").unwrap(), Type::Num);
    }

    #[test]
    fn arithmetic_on_tuple_reports_tuple_arity() {
        // Whenever a tuple lands as an arithmetic operand the message should
        // describe the actual mismatch (scalar expected, n-tuple found) and
        // not speculate about what operator the user might have meant.
        let err = check_str("x / (1, 2)").unwrap_err();
        assert!(
            err.message.contains("expected a scalar"),
            "got: {}",
            err.message
        );
        assert!(
            err.message.contains("2-element tuple"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn comparison_is_bool() {
        assert_eq!(check_str("x = 0").unwrap(), Type::Bool);
        assert_eq!(check_str("x < y").unwrap(), Type::Bool);
        assert_eq!(check_str("x >= y").unwrap(), Type::Bool);
    }

    #[test]
    fn logical_is_bool() {
        assert_eq!(check_str("x > 0 and y > 0").unwrap(), Type::Bool);
        assert_eq!(check_str("not (x = 0)").unwrap(), Type::Bool);
    }

    #[test]
    fn math_builtins_are_num() {
        assert_eq!(check_str("sin(x)").unwrap(), Type::Num);
        assert_eq!(check_str("sqrt(x*x + y*y)").unwrap(), Type::Num);
        assert_eq!(check_str("clamp(x, 0, 1)").unwrap(), Type::Num);
        assert_eq!(check_str("min(x, y)").unwrap(), Type::Num);
    }

    #[test]
    fn binding_propagates_type() {
        // r := sqrt(x*x + y*y) ; r — r should be Num.
        assert_eq!(
            check_str("r := sqrt(x*x + y*y)\nr").unwrap(),
            Type::Num
        );
    }

    #[test]
    fn binding_to_bool_propagates() {
        assert_eq!(check_str("f := x = y\nf").unwrap(), Type::Bool);
    }

    #[test]
    fn user_function_inferred() {
        let ty = check_str("dist(a, b) := sqrt(a*a + b*b)\ndist(x, y)").unwrap();
        assert_eq!(ty, Type::Num);
    }

    #[test]
    fn user_function_arity_mismatch_errors() {
        let err = check_str("f(a, b) := a + b\nf(1)").unwrap_err();
        assert!(err.message.contains("expects 2 args, got 1"), "got: {}", err.message);
    }

    #[test]
    fn undefined_var_errors() {
        let err = check_str("nope").unwrap_err();
        assert!(err.message.contains("Undefined variable `nope`"));
    }

    #[test]
    fn vector_constructor_typed() {
        assert_eq!(check_str("vec3(1, 2, 3)").unwrap(), Type::Vec3);
    }

    #[test]
    fn dot_of_two_vec3_is_num() {
        assert_eq!(
            check_str("dot(vec3(1, 0, 0), vec3(0, 1, 0))").unwrap(),
            Type::Num
        );
    }

    #[test]
    fn length_of_vec_is_num() {
        assert_eq!(check_str("length(vec2(3, 4))").unwrap(), Type::Num);
    }

    #[test]
    fn cross_of_vec3_is_vec3() {
        assert_eq!(
            check_str("cross(vec3(1, 0, 0), vec3(0, 1, 0))").unwrap(),
            Type::Vec3
        );
    }

    #[test]
    fn cross_rejects_non_vec3() {
        let err = check_str("cross(vec2(1, 0), vec2(0, 1))").unwrap_err();
        assert!(err.message.contains("no overload"), "got: {}", err.message);
    }

    #[test]
    fn if_branches_must_unify() {
        // both branches Num — fine.
        assert_eq!(check_str("if (x > 0) 1 else -1").unwrap(), Type::Num);
        // mismatched branches — error.
        let err = check_str("if (x > 0) 1 else true").unwrap_err();
        assert!(err.message.contains("incompatible"), "got: {}", err.message);
    }

    #[test]
    fn if_without_else_is_then_type() {
        assert_eq!(check_str("if (x > 0) 1").unwrap(), Type::Num);
    }

    #[test]
    fn for_loop_requires_range() {
        // Valid: range start..end.
        assert!(check_str("for i in 0..10 (\n  i\n)").is_ok());
    }

    #[test]
    fn array_literal_typed() {
        assert_eq!(
            check_str("[1, 2, 3]").unwrap(),
            Type::Array(Box::new(Type::Num))
        );
    }

    #[test]
    fn mixed_array_errors() {
        let err = check_str("[1, true]").unwrap_err();
        assert!(err.message.contains("mixed types"), "got: {}", err.message);
    }

    #[test]
    fn index_access_unwraps_array() {
        assert_eq!(check_str("a := [1, 2, 3]\na[0]").unwrap(), Type::Num);
    }

    #[test]
    fn print_passes_through() {
        assert_eq!(check_str("print(x + y)").unwrap(), Type::Num);
        assert_eq!(check_str("print(x = 0)").unwrap(), Type::Bool);
    }

    #[test]
    fn plot_is_void() {
        assert_eq!(check_str("plot(x*x + y*y)").unwrap(), Type::Void);
    }

    #[test]
    fn property_access_returns_num() {
        assert_eq!(check_str("x.min").unwrap(), Type::Num);
        assert_eq!(check_str("y.max").unwrap(), Type::Num);
    }

    #[test]
    fn unknown_property_errors() {
        let err = check_str("x.bogus").unwrap_err();
        assert!(err.message.contains("unknown property"));
    }
}
