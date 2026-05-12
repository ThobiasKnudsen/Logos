//! Lowering pass: enrich a freshly-parsed `Ir` with the analysis information
//! every backend (interpreter, wgsl_gen, future cranelift JIT) needs.
//!
//! The IR coming out of the parser is essentially an AST — names are strings,
//! types are unknown, higher-order calls are not yet monomorphized, and
//! lambdas still appear as `Ir::Lambda` nodes. Backends used to each re-derive
//! all of this independently; this module is the single shared pipeline.
//!
//! The pass enriches the same `Ir` in place (no separate `LoweredIr` type).
//! Future phases will add `Option`-typed annotation fields per variant that
//! parser-produced trees leave `None` and the lowering pass populates.
//!
//! ## Sub-passes (run in order)
//!
//! 1. `hoist_anonymous_blocks` — synthesize top-level bindings for imperative
//!    blocks that appear in expression position (e.g. `plot(y = (sum:=0; …))`).
//! 2. `lift_lambdas` — replace each `Ir::Lambda` with a synthetic
//!    `FunctionDef` plus an `Identifier` referring to it.
//! 3. `specialize_higher_order_calls` — monomorphize each HOF call so backends
//!    see concrete function references instead of function-typed parameters.
//! 4. `resolve_names` — *future*: populate `Resolution` on every `Identifier`.
//! 5. `annotate_types` — *future*: populate `result_ty` on every expression.

use std::collections::{HashMap, HashSet};

use super::ir::{Callee, Ir};

/// Top-level entry point for the lowering pass.
///
/// Currently runs the three relocated pre-passes. Future phases will add
/// `resolve_names` and `annotate_types` sub-passes.
pub fn lower(ir: Ir) -> Result<Ir, String> {
    Ok(pre_passes(ir))
}

/// Run the three syntactic pre-passes that normalize the AST shape before
/// any backend consumes it: hoist anonymous blocks, lift lambdas, specialize
/// higher-order calls. After this, the IR contains no `Ir::Lambda` nodes
/// and no calls to user functions whose first-class function arguments
/// could have been monomorphized.
pub fn pre_passes(ast: Ir) -> Ir {
    let ast = if needs_anon_hoisting(&ast) {
        hoist_anonymous_blocks(&ast)
    } else {
        ast
    };
    let ast = lift_lambdas(&ast).unwrap_or(ast);
    specialize_higher_order_calls(&ast).unwrap_or(ast)
}

// ---------------------------------------------------------------------------
// Pass 1: hoist anonymous imperative blocks
// ---------------------------------------------------------------------------

/// True if `ast` contains at least one anonymous imperative block (a Block
/// with bindings/loops appearing somewhere other than as a binding's value or
/// a function/loop body). Cheap check used to skip cloning when there's
/// nothing to hoist.
pub(crate) fn needs_anon_hoisting(ast: &Ir) -> bool {
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

/// True if a Block's stmt list contains any imperative statements
/// (bindings, loops). Used by `wgsl_gen` codegen too, hence `pub(crate)`.
pub(crate) fn has_imperative_stmt(stmts: &[Ir]) -> bool {
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

// ---------------------------------------------------------------------------
// Pass 2: lift lambdas into synthetic FunctionDefs
// ---------------------------------------------------------------------------

/// Replace every `Ir::Lambda` with a synthetic `Ir::FunctionDef` whose name
/// is `_lambda_N`, and an `Ir::Identifier` referring to that name. Returns
/// the rewritten AST (or `None` if no lambdas were present).
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

// ---------------------------------------------------------------------------
// Pass 3: specialize higher-order function calls
// ---------------------------------------------------------------------------

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

pub(crate) fn collect_owned_function_defs(
    node: &Ir,
    out: &mut HashMap<String, (Vec<String>, Ir)>,
) {
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
pub(crate) fn compute_hof_indices(
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
