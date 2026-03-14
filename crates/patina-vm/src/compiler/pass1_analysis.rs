//! Pass 1 — Analysis
//!
//! Two sub-passes combined into one tree walk:
//!
//! **1a. NodeId assignment** — assigns a monotonic `NodeId` to every `Lambda`
//! node (pre-order). This id is the key into `AnalysisInfo` maps so later
//! passes can look up per-lambda metadata without re-walking the tree.
//!
//! **1b. Free-variable analysis + mutation detection** — for each lambda:
//! - `free_vars`: variables referenced but not bound by that lambda (in stable
//!   insertion order, deduped).
//! - `mutated_bindings`: variables in *this* lambda's own scope that are
//!   targeted by `set!` (including from nested lambdas). Pass 2 uses this to
//!   decide which bindings need `MutableCell` boxing.
//!
//! Output: `AnalysisInfo` consumed by Pass 2.
//!
//! See VM_COMPILER.md §Pass 1.

use patina_core::core_expr::{CoreExpr, CoreExprKind, Formals, Symbol};
use std::collections::{HashMap, HashSet};

/// Stable id for a `Lambda` node. Assigned in pre-order during Pass 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

/// Per-lambda analysis results.
#[derive(Debug, Clone, Default)]
pub struct LambdaInfo {
    /// Free variables of this lambda in stable (first-reference) order.
    /// Does not include free vars of nested lambdas — those are separate entries.
    pub free_vars: Vec<Symbol>,
    /// Variables bound in *this* lambda's scope that are mutated by `set!`
    /// anywhere reachable from this lambda body (including nested lambdas).
    pub mutated_bindings: HashSet<Symbol>,
    /// Names introduced by internal `define` forms in this lambda's body.
    /// These behave like `letrec*` bindings scoped to the lambda body.
    pub internal_defines: Vec<Symbol>,
}

/// The complete output of Pass 1.
#[derive(Debug, Default)]
pub struct AnalysisInfo {
    /// Per-lambda results, keyed by `NodeId`.
    pub lambdas: HashMap<NodeId, LambdaInfo>,
    /// All `set!` targets in the entire expression (quick global lookup).
    pub all_mutated: HashSet<Symbol>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Implementation
// ─────────────────────────────────────────────────────────────────────────────

pub struct Pass1Analysis;

impl Pass1Analysis {
    pub fn run(expr: &CoreExpr) -> AnalysisInfo {
        let mut info = AnalysisInfo::default();
        let mut counter = 0u32;
        // Top level has no enclosing lambda scope — pass an empty binding set.
        collect(expr, &[], &mut counter, &mut info);
        info
    }
}

/// Recursively walk `expr`.
///
/// `outer_bindings`: the stack of binding sets for all enclosing lambdas,
/// innermost last. Each entry is `(node_id, bound_names)`.
///
/// Returns the set of free variables referenced in `expr` relative to
/// `outer_bindings` — i.e. variables not bound in any enclosing scope we track.
fn collect(
    expr: &CoreExpr,
    outer_bindings: &[(&NodeId, &HashSet<Symbol>)],
    counter: &mut u32,
    info: &mut AnalysisInfo,
) -> HashSet<Symbol> {
    match &expr.kind {
        CoreExprKind::Literal(_) | CoreExprKind::Quote(_) | CoreExprKind::Quasiquote(_) => {
            HashSet::new()
        }

        CoreExprKind::Var { name, .. } => {
            let mut refs = HashSet::new();
            refs.insert(name.clone());
            refs
        }

        CoreExprKind::Set { var, value, .. } => {
            info.all_mutated.insert(var.clone());
            // Record mutation in the innermost enclosing lambda that owns this binding.
            for (nid, bound) in outer_bindings.iter().rev() {
                if bound.contains(var) {
                    info.lambdas
                        .entry(**nid)
                        .or_default()
                        .mutated_bindings
                        .insert(var.clone());
                    break;
                }
            }
            let mut refs = collect(value, outer_bindings, counter, info);
            refs.insert(var.clone());
            refs
        }

        CoreExprKind::Lambda { params, body, .. } => {
            let node_id = NodeId(*counter);
            *counter += 1;

            // Collect the parameter names bound by this lambda.
            let mut bound: HashSet<Symbol> = formals_names(params).into_iter().collect();

            // Scan body for internal defines — these are `letrec*`-scoped bindings.
            let internal_defines = collect_internal_define_names(body);
            for name in &internal_defines {
                bound.insert(name.clone());
            }

            // Internal defines are assigned after initialization (letrec* semantics),
            // so mark them as mutated. This ensures they get MutableCell boxing when
            // captured by nested lambdas (including their own value expressions,
            // which is essential for recursive internal defines).
            for name in &internal_defines {
                info.all_mutated.insert(name.clone());
            }

            // Extend the outer scope chain with this lambda's bindings.
            let mut chain: Vec<(&NodeId, &HashSet<Symbol>)> = outer_bindings.to_vec();
            chain.push((&node_id, &bound));

            // We need owned storage for `bound` during the recursive walk.
            // Use a two-step approach: walk body, then register.
            let mut body_refs: HashSet<Symbol> = HashSet::new();
            for e in body {
                body_refs.extend(collect(e, &chain, counter, info));
            }

            // Free vars of this lambda = body refs minus its own bindings.
            let free: Vec<Symbol> = body_refs
                .iter()
                .filter(|v| !bound.contains(*v))
                .cloned()
                .collect();

            let entry = info.lambdas.entry(node_id).or_default();
            // Stable order: sort for determinism (avoids non-deterministic register layout).
            let mut free_sorted = free.clone();
            free_sorted.sort();
            entry.free_vars = free_sorted;
            entry.internal_defines = internal_defines.clone();
            // Also mark internal defines in this lambda's own mutated_bindings.
            for name in &internal_defines {
                entry.mutated_bindings.insert(name.clone());
            }

            // Return free vars to the caller (they may be free in outer lambdas too).
            free.into_iter().collect()
        }

        CoreExprKind::If { test, then, else_ } => {
            let mut refs = collect(test, outer_bindings, counter, info);
            refs.extend(collect(then, outer_bindings, counter, info));
            refs.extend(collect(else_, outer_bindings, counter, info));
            refs
        }

        CoreExprKind::Begin(exprs) => {
            let mut refs = HashSet::new();
            for e in exprs {
                refs.extend(collect(e, outer_bindings, counter, info));
            }
            refs
        }

        CoreExprKind::Define { name: _, value } => {
            // Top-level define: the name is not a free-var reference.
            collect(value, outer_bindings, counter, info)
        }

        CoreExprKind::App { func, args } => {
            let mut refs = collect(func, outer_bindings, counter, info);
            for a in args {
                refs.extend(collect(a, outer_bindings, counter, info));
            }
            refs
        }

        CoreExprKind::Apply { func, args } => {
            let mut refs = collect(func, outer_bindings, counter, info);
            for a in args {
                refs.extend(collect(a, outer_bindings, counter, info));
            }
            refs
        }

        CoreExprKind::Import { .. } | CoreExprKind::Expand { .. } => HashSet::new(),
    }
}

/// Scan the body of a lambda for internal `define` names.
/// Only considers direct children and one level of `Begin` wrapping.
fn collect_internal_define_names(body: &[CoreExpr]) -> Vec<Symbol> {
    let mut names = Vec::new();
    for expr in body {
        match &expr.kind {
            CoreExprKind::Define { name, .. } => names.push(name.clone()),
            CoreExprKind::Begin(exprs) => {
                for e in exprs {
                    if let CoreExprKind::Define { name, .. } = &e.kind {
                        names.push(name.clone());
                    }
                }
            }
            _ => {}
        }
    }
    names
}

/// Extract all parameter names from a `Formals`.
fn formals_names(params: &Formals) -> Vec<Symbol> {
    match params {
        Formals::Fixed(ps) => ps.iter().map(|p| p.name.clone()).collect(),
        Formals::Variadic(p) => vec![p.name.clone()],
        Formals::Mixed { fixed, rest } => {
            let mut names: Vec<Symbol> = fixed.iter().map(|p| p.name.clone()).collect();
            names.push(rest.name.clone());
            names
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_core::core_expr::{CoreExpr, CoreExprKind, Formals, ScopedParam};
    use patina_core::tagged_value::TaggedValue;
    use std::rc::Rc;

    fn var(name: &str) -> CoreExpr {
        CoreExpr::new(CoreExprKind::Var {
            name: Rc::from(name),
            scopes: Default::default(),
        })
    }

    fn lit(n: i64) -> CoreExpr {
        CoreExpr::new(CoreExprKind::Literal(TaggedValue::fixnum(n)))
    }

    fn lambda(params: Vec<&str>, body: Vec<CoreExpr>) -> CoreExpr {
        CoreExpr::new(CoreExprKind::Lambda {
            params: Formals::Fixed(
                params
                    .iter()
                    .map(|n| ScopedParam::simple(Rc::from(*n)))
                    .collect(),
            ),
            body,
            binding_scope: None,
        })
    }

    #[test]
    fn literal_has_no_free_vars() {
        let info = Pass1Analysis::run(&lit(42));
        assert!(info.lambdas.is_empty());
        assert!(info.all_mutated.is_empty());
    }

    #[test]
    fn lambda_with_no_captures() {
        // (lambda (x) x)  — x is bound, no free vars
        let expr = lambda(vec!["x"], vec![var("x")]);
        let info = Pass1Analysis::run(&expr);
        assert_eq!(info.lambdas.len(), 1);
        let entry = info.lambdas.values().next().unwrap();
        assert!(entry.free_vars.is_empty());
    }

    #[test]
    fn lambda_captures_free_var() {
        // (lambda (x) (+ x y))  — y is free
        let plus = var("+");
        let body = CoreExpr::new(CoreExprKind::App {
            func: Rc::new(plus),
            args: vec![var("x"), var("y")],
        });
        let expr = lambda(vec!["x"], vec![body]);
        let info = Pass1Analysis::run(&expr);
        let entry = info.lambdas.values().next().unwrap();
        assert!(entry.free_vars.contains(&Rc::from("y")));
        assert!(!entry.free_vars.contains(&Rc::from("x")));
        // + is also free (not a bound param)
        assert!(entry.free_vars.contains(&Rc::from("+")));
    }

    #[test]
    fn nested_lambda_free_vars() {
        // (lambda (x) (lambda (y) (+ x y)))
        // inner: x and + are free (y is bound)
        // outer: + is free (x is bound)
        let plus = var("+");
        let inner_app = CoreExpr::new(CoreExprKind::App {
            func: Rc::new(plus),
            args: vec![var("x"), var("y")],
        });
        let inner = lambda(vec!["y"], vec![inner_app]);
        let outer = lambda(vec!["x"], vec![inner]);
        let info = Pass1Analysis::run(&outer);
        assert_eq!(info.lambdas.len(), 2);
        // Find outer (NodeId 0) and inner (NodeId 1)
        let outer_info = &info.lambdas[&NodeId(0)];
        let inner_info = &info.lambdas[&NodeId(1)];
        // outer: only + is free
        assert!(outer_info.free_vars.contains(&Rc::from("+")));
        assert!(!outer_info.free_vars.contains(&Rc::from("x")));
        // inner: x and + are free
        assert!(inner_info.free_vars.contains(&Rc::from("x")));
        assert!(inner_info.free_vars.contains(&Rc::from("+")));
        assert!(!inner_info.free_vars.contains(&Rc::from("y")));
    }

    #[test]
    fn set_detected_as_mutated() {
        // (lambda (x) (set! x 1) x)
        let set_x = CoreExpr::new(CoreExprKind::Set {
            var: Rc::from("x"),
            scopes: Default::default(),
            value: Rc::new(lit(1)),
        });
        let expr = lambda(vec!["x"], vec![set_x, var("x")]);
        let info = Pass1Analysis::run(&expr);
        assert!(info.all_mutated.contains(&Rc::from("x")));
        let entry = info.lambdas.values().next().unwrap();
        assert!(entry.mutated_bindings.contains(&Rc::from("x")));
    }

    #[test]
    fn triple_nested_set_detected() {
        // (lambda (log) (lambda () (lambda () (set! log 1) log)))
        // log should be mutated and captured across three lambda levels.
        let set_log = CoreExpr::new(CoreExprKind::Set {
            var: Rc::from("log"),
            scopes: Default::default(),
            value: Rc::new(lit(1)),
        });
        let inner = lambda(vec![], vec![set_log, var("log")]);
        let mid = lambda(vec![], vec![inner]);
        let outer = lambda(vec!["log"], vec![mid]);
        let info = Pass1Analysis::run(&outer);

        assert!(info.all_mutated.contains(&Rc::from("log")));

        // outer = NodeId(0), mid = NodeId(1), inner = NodeId(2)
        let outer_info = &info.lambdas[&NodeId(0)];
        let mid_info = &info.lambdas[&NodeId(1)];
        let inner_info = &info.lambdas[&NodeId(2)];

        // outer should have log in mutated_bindings (it owns the binding)
        assert!(
            outer_info.mutated_bindings.contains(&Rc::from("log")),
            "outer should record log as mutated binding"
        );

        // mid captures log (it's free in mid)
        assert!(
            mid_info.free_vars.contains(&Rc::from("log")),
            "mid should have log as free var"
        );

        // inner captures log (it's free in inner)
        assert!(
            inner_info.free_vars.contains(&Rc::from("log")),
            "inner should have log as free var"
        );
    }
}
