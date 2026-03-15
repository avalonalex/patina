//! Pass 2 — Closure Conversion
//!
//! Transforms `CoreExpr` into `ClosedExpr` by making all free-variable
//! captures explicit and boxing mutable captured variables.
//!
//! ## Capture list
//!
//! Every `ClosedLambda` carries `capture_list: Vec<Symbol>` — the variables
//! that must be snapshotted when `MakeClosure` runs. Slot `i` in the closure's
//! `free_vars` array corresponds to `capture_list[i]`.
//!
//! ## MutableCell boxing
//!
//! A variable needs `MutableCell` boxing when it is **both**:
//! 1. Mutated by `set!` anywhere reachable from its binding lambda, **and**
//! 2. Captured by a nested lambda (so the cell pointer — not the value — is
//!    what gets snapshotted).
//!
//! Boxing protocol:
//! - **Binding site** (lambda param or top-level define): emit `AllocCell(name)`
//!   to heap-allocate the cell and store the initial value.
//! - **Read**: emit `ReadCell(LocalRef(name))` or `ReadCell(ClosureRef(slot))`.
//! - **Write (`set!`)**: emit `WriteCell(LocalRef|ClosureRef, value)`.
//!
//! Variables that are mutated but *not* captured use plain register assignment.
//!
//! See VM_COMPILER.md §Pass 2.

use super::pass1_analysis::{AnalysisInfo, NodeId};
use patina_core::core_expr::{CoreExpr, CoreExprKind, Formals, Symbol};
use patina_core::error::SourceLocation;
use patina_core::tagged_value::TaggedValue;
use std::collections::HashSet;

// ─────────────────────────────────────────────────────────────────────────────
// ClosedExpr — output IR of Pass 2
// ─────────────────────────────────────────────────────────────────────────────

/// A closure-converted expression with optional source location.
#[derive(Debug, Clone)]
pub struct ClosedExpr {
    pub kind: ClosedExprKind,
    pub source: Option<SourceLocation>,
}

impl ClosedExpr {
    fn with_source(kind: ClosedExprKind, source: Option<SourceLocation>) -> Self {
        Self { kind, source }
    }
}

#[derive(Debug, Clone)]
pub enum ClosedExprKind {
    Literal(TaggedValue),
    Quote(TaggedValue),
    Quasiquote(TaggedValue),

    /// Plain read of a local parameter register.
    LocalRef(Symbol),
    /// Plain read through a closure slot (non-boxed).
    ClosureRef {
        name: Symbol,
        slot: u16,
    },
    /// Plain global lookup.
    GlobalRef(Symbol),

    /// Read value out of a `MutableCell` held in a local register.
    ReadLocalCell(Symbol),
    /// Read value out of a `MutableCell` held in a closure slot.
    ReadClosureCell {
        name: Symbol,
        slot: u16,
    },

    /// Closure-converted lambda.
    Lambda(Box<ClosedLambda>),

    If {
        test: Box<ClosedExpr>,
        then: Box<ClosedExpr>,
        else_: Box<ClosedExpr>,
    },

    /// `set!` on a plain local (non-boxed, non-captured).
    SetLocal {
        var: Symbol,
        value: Box<ClosedExpr>,
    },
    /// `set!` through a `MutableCell` held in a local register.
    WriteLocalCell {
        var: Symbol,
        value: Box<ClosedExpr>,
    },
    /// `set!` through a `MutableCell` held in a closure slot.
    WriteClosureCell {
        slot: u16,
        value: Box<ClosedExpr>,
    },
    /// `set!` on a global.
    SetGlobal {
        var: Symbol,
        value: Box<ClosedExpr>,
    },

    Begin(Vec<ClosedExpr>),
    Define {
        name: Symbol,
        value: Box<ClosedExpr>,
    },

    App {
        func: Box<ClosedExpr>,
        args: Vec<ClosedExpr>,
    },
    Apply {
        func: Box<ClosedExpr>,
        args: Vec<ClosedExpr>,
    },
}

/// A closure-converted lambda.
#[derive(Debug, Clone)]
pub struct ClosedLambda {
    /// Parameters in declaration order.
    pub params: Vec<Symbol>,
    /// Whether the last parameter is a variadic rest parameter.
    pub rest_param: bool,
    /// Variables captured from the enclosing scope, in slot order.
    /// Slot `i` → `capture_list[i]`.
    pub capture_list: Vec<Symbol>,
    /// Parameters (and internal defines) that must be boxed (wrapped in `MutableCell`)
    /// on entry because they are both mutated and captured.
    pub boxed_params: HashSet<Symbol>,
    /// Body after closure conversion.
    pub body: Vec<ClosedExpr>,
    /// Monotonic id assigned by Pass 1.
    pub node_id: NodeId,
    /// Names introduced by internal `define` forms, treated as local bindings.
    pub internal_defines: Vec<Symbol>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Variable resolution
// ─────────────────────────────────────────────────────────────────────────────

/// Where a variable currently lives, and whether it is boxed.
#[derive(Debug, Clone)]
enum VarLoc {
    /// Plain local register (not boxed).
    Local,
    /// Local register holding a `MutableCell`.
    LocalBoxed,
    /// Closure slot (not boxed).
    Closure(u16),
    /// Closure slot holding a `MutableCell`.
    ClosureBoxed(u16),
    /// Global.
    Global,
}

struct Ctx<'a> {
    analysis: &'a AnalysisInfo,
    counter: u32,
    scopes: Vec<Vec<(Symbol, VarLoc)>>,
}

impl<'a> Ctx<'a> {
    fn new(analysis: &'a AnalysisInfo) -> Self {
        Self {
            analysis,
            counter: 0,
            scopes: vec![],
        }
    }

    fn next_node_id(&mut self) -> NodeId {
        let id = NodeId(self.counter);
        self.counter += 1;
        id
    }

    fn lookup(&self, name: &Symbol) -> VarLoc {
        for scope in self.scopes.iter().rev() {
            for (n, loc) in scope.iter().rev() {
                if n == name {
                    return loc.clone();
                }
            }
        }
        VarLoc::Global
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pass entry
// ─────────────────────────────────────────────────────────────────────────────

pub struct Pass2Closure;

impl Pass2Closure {
    pub fn run(expr: &CoreExpr, analysis: &AnalysisInfo) -> ClosedExpr {
        let mut ctx = Ctx::new(analysis);
        convert(expr, &mut ctx)
    }
}

fn convert(expr: &CoreExpr, ctx: &mut Ctx<'_>) -> ClosedExpr {
    let source = expr.source.clone();
    let kind = match &expr.kind {
        CoreExprKind::Literal(v) => ClosedExprKind::Literal(*v),
        CoreExprKind::Quote(v) => ClosedExprKind::Quote(*v),
        CoreExprKind::Quasiquote(v) => ClosedExprKind::Quasiquote(*v),

        CoreExprKind::Var { name, .. } => match ctx.lookup(name) {
            VarLoc::Local => ClosedExprKind::LocalRef(name.clone()),
            VarLoc::LocalBoxed => ClosedExprKind::ReadLocalCell(name.clone()),
            VarLoc::Closure(slot) => ClosedExprKind::ClosureRef {
                name: name.clone(),
                slot,
            },
            VarLoc::ClosureBoxed(slot) => ClosedExprKind::ReadClosureCell {
                name: name.clone(),
                slot,
            },
            VarLoc::Global => ClosedExprKind::GlobalRef(name.clone()),
        },

        CoreExprKind::Set { var, value, .. } => {
            let converted_val = convert(value, ctx);
            match ctx.lookup(var) {
                VarLoc::Local => ClosedExprKind::SetLocal {
                    var: var.clone(),
                    value: Box::new(converted_val),
                },
                VarLoc::LocalBoxed => ClosedExprKind::WriteLocalCell {
                    var: var.clone(),
                    value: Box::new(converted_val),
                },
                VarLoc::Closure(_) => ClosedExprKind::SetGlobal {
                    // Non-boxed closure slot that is set! — treat as global
                    // (shouldn't happen after analysis, but safe fallback).
                    var: var.clone(),
                    value: Box::new(converted_val),
                },
                VarLoc::ClosureBoxed(slot) => ClosedExprKind::WriteClosureCell {
                    slot,
                    value: Box::new(converted_val),
                },
                VarLoc::Global => ClosedExprKind::SetGlobal {
                    var: var.clone(),
                    value: Box::new(converted_val),
                },
            }
        }

        CoreExprKind::Lambda { params, body, .. } => {
            let node_id = ctx.next_node_id();

            let lambda_info = ctx.analysis.lambdas.get(&node_id);

            // Capture list from Pass 1, filtered to only variables that are
            // actually in an enclosing local/closure scope. Variables that
            // resolve to Global are looked up dynamically via LoadGlobal and
            // must NOT be snapshotted into the closure's free-var array
            // (they may not be defined yet at closure creation time).
            let raw_free_vars: Vec<Symbol> =
                lambda_info.map(|i| i.free_vars.clone()).unwrap_or_default();
            let capture_list: Vec<Symbol> = raw_free_vars
                .into_iter()
                .filter(|name| !matches!(ctx.lookup(name), VarLoc::Global))
                .collect();

            // Which captures are boxed: captured AND mutated anywhere.
            let all_mutated = &ctx.analysis.all_mutated;

            // Build closure slot locs: slot `i` → Closure(i) or ClosureBoxed(i).
            let closure_locs: Vec<(Symbol, VarLoc)> = capture_list
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let loc = if all_mutated.contains(name) {
                        VarLoc::ClosureBoxed(i as u16)
                    } else {
                        VarLoc::Closure(i as u16)
                    };
                    (name.clone(), loc)
                })
                .collect();

            let (param_names, rest_param) = flatten_formals(params);

            // Internal defines from Pass 1.
            let internal_defines: Vec<Symbol> = lambda_info
                .map(|i| i.internal_defines.clone())
                .unwrap_or_default();

            // Params that need boxing: mutated AND captured by some nested lambda.
            // We use `all_mutated` as an over-approximation (safe: only costs a box).
            let mutated_params: HashSet<Symbol> = lambda_info
                .map(|i| i.mutated_bindings.clone())
                .unwrap_or_default();
            // Box any param or internal define in `mutated_bindings`.
            let boxed_params: HashSet<Symbol> = param_names
                .iter()
                .chain(internal_defines.iter())
                .filter(|n| mutated_params.contains(*n))
                .cloned()
                .collect();

            // Build local locs: param → Local or LocalBoxed.
            let mut local_locs: Vec<(Symbol, VarLoc)> = param_names
                .iter()
                .map(|n| {
                    let loc = if boxed_params.contains(n) {
                        VarLoc::LocalBoxed
                    } else {
                        VarLoc::Local
                    };
                    (n.clone(), loc)
                })
                .collect();

            // Add internal defines as local bindings too.
            for name in &internal_defines {
                let loc = if boxed_params.contains(name) {
                    VarLoc::LocalBoxed
                } else {
                    VarLoc::Local
                };
                local_locs.push((name.clone(), loc));
            }

            // Scope chain: closure slots (outer) then params + internal defines (inner, shadow).
            ctx.scopes.push(closure_locs);
            ctx.scopes.push(local_locs);
            let converted_body: Vec<ClosedExpr> = body.iter().map(|e| convert(e, ctx)).collect();
            ctx.scopes.pop();
            ctx.scopes.pop();

            ClosedExprKind::Lambda(Box::new(ClosedLambda {
                params: param_names,
                rest_param,
                capture_list,
                boxed_params,
                body: converted_body,
                node_id,
                internal_defines,
            }))
        }

        CoreExprKind::If { test, then, else_ } => ClosedExprKind::If {
            test: Box::new(convert(test, ctx)),
            then: Box::new(convert(then, ctx)),
            else_: Box::new(convert(else_, ctx)),
        },

        CoreExprKind::Begin(exprs) => {
            ClosedExprKind::Begin(exprs.iter().map(|e| convert(e, ctx)).collect())
        }

        CoreExprKind::Define { name, value } => {
            let converted_val = convert(value, ctx);
            match ctx.lookup(name) {
                VarLoc::Local => ClosedExprKind::SetLocal {
                    var: name.clone(),
                    value: Box::new(converted_val),
                },
                VarLoc::LocalBoxed => ClosedExprKind::WriteLocalCell {
                    var: name.clone(),
                    value: Box::new(converted_val),
                },
                _ => ClosedExprKind::Define {
                    name: name.clone(),
                    value: Box::new(converted_val),
                },
            }
        }

        CoreExprKind::App { func, args } => ClosedExprKind::App {
            func: Box::new(convert(func, ctx)),
            args: args.iter().map(|a| convert(a, ctx)).collect(),
        },

        CoreExprKind::Apply { func, args } => ClosedExprKind::Apply {
            func: Box::new(convert(func, ctx)),
            args: args.iter().map(|a| convert(a, ctx)).collect(),
        },

        CoreExprKind::Import { .. } | CoreExprKind::Expand { .. } => {
            ClosedExprKind::Literal(TaggedValue::UNSPECIFIED)
        }
    };
    ClosedExpr::with_source(kind, source)
}

fn flatten_formals(params: &Formals) -> (Vec<Symbol>, bool) {
    match params {
        Formals::Fixed(ps) => (ps.iter().map(|p| p.name.clone()).collect(), false),
        Formals::Variadic(p) => (vec![p.name.clone()], true),
        Formals::Mixed { fixed, rest } => {
            let mut names: Vec<Symbol> = fixed.iter().map(|p| p.name.clone()).collect();
            names.push(rest.name.clone());
            (names, true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::pass1_analysis::Pass1Analysis;
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
    fn literal_converts() {
        let expr = lit(7);
        let info = Pass1Analysis::run(&expr);
        let closed = Pass2Closure::run(&expr, &info);
        assert!(matches!(closed.kind, ClosedExprKind::Literal(_)));
    }

    #[test]
    fn local_ref_resolved() {
        let expr = lambda(vec!["x"], vec![var("x")]);
        let info = Pass1Analysis::run(&expr);
        let closed = Pass2Closure::run(&expr, &info);
        let ClosedExprKind::Lambda(lam) = closed.kind else {
            panic!("expected Lambda")
        };
        assert!(matches!(&lam.body[0].kind, ClosedExprKind::LocalRef(n) if n.as_ref() == "x"));
        assert!(lam.capture_list.is_empty());
    }

    #[test]
    fn unbound_free_var_stays_global_not_captured() {
        let expr = lambda(vec!["x"], vec![var("y")]);
        let info = Pass1Analysis::run(&expr);
        let closed = Pass2Closure::run(&expr, &info);
        let ClosedExprKind::Lambda(lam) = closed.kind else {
            panic!("expected Lambda")
        };
        assert!(
            lam.capture_list.is_empty(),
            "global var must not be captured"
        );
        assert!(matches!(&lam.body[0].kind, ClosedExprKind::GlobalRef(n) if n.as_ref() == "y"));
    }

    #[test]
    fn unresolved_var_becomes_global_ref() {
        let info = Pass1Analysis::run(&var("foo"));
        let closed = Pass2Closure::run(&var("foo"), &info);
        assert!(matches!(closed.kind, ClosedExprKind::GlobalRef(n) if n.as_ref() == "foo"));
    }

    #[test]
    fn nested_lambda_capture_list() {
        let inner = lambda(vec!["y"], vec![var("x")]);
        let outer = lambda(vec!["x"], vec![inner]);
        let info = Pass1Analysis::run(&outer);
        let closed = Pass2Closure::run(&outer, &info);
        let ClosedExprKind::Lambda(outer_lam) = closed.kind else {
            panic!("expected Lambda (outer)")
        };
        assert!(outer_lam.capture_list.is_empty());
        let ClosedExprKind::Lambda(inner_lam) = &outer_lam.body[0].kind else {
            panic!("expected Lambda (inner)")
        };
        assert!(inner_lam.capture_list.contains(&Rc::<str>::from("x")));
        assert!(matches!(
            &inner_lam.body[0].kind,
            ClosedExprKind::ClosureRef { .. }
        ));
    }

    #[test]
    fn mutated_captured_param_is_boxed() {
        let set_x = CoreExpr::new(CoreExprKind::Set {
            var: Rc::from("x"),
            scopes: Default::default(),
            value: Rc::new(lit(1)),
        });
        let inner = lambda(vec![], vec![var("x")]);
        let outer = lambda(vec!["x"], vec![set_x, inner]);
        let info = Pass1Analysis::run(&outer);
        let closed = Pass2Closure::run(&outer, &info);
        let ClosedExprKind::Lambda(outer_lam) = closed.kind else {
            panic!("expected Lambda")
        };
        assert!(outer_lam.boxed_params.contains(&Rc::<str>::from("x")));
        assert!(matches!(
            &outer_lam.body[0].kind,
            ClosedExprKind::WriteLocalCell { .. }
        ));
    }

    #[test]
    fn triple_nested_capture_through_middle() {
        let set_x = CoreExpr::new(CoreExprKind::Set {
            var: Rc::from("x"),
            scopes: Default::default(),
            value: Rc::new(lit(1)),
        });
        let inner = lambda(vec![], vec![set_x]);
        let mid = lambda(vec![], vec![inner]);
        let outer = lambda(vec!["x"], vec![mid]);

        let info = Pass1Analysis::run(&outer);
        let closed = Pass2Closure::run(&outer, &info);

        let ClosedExprKind::Lambda(outer_lam) = &closed.kind else {
            panic!("expected Lambda");
        };
        assert!(
            outer_lam.boxed_params.contains(&Rc::<str>::from("x")),
            "x should be in outer's boxed_params"
        );

        let ClosedExprKind::Lambda(mid_lam) = &outer_lam.body[0].kind else {
            panic!("expected Lambda for mid, got {:?}", outer_lam.body[0]);
        };
        assert!(
            !mid_lam.capture_list.is_empty(),
            "mid should capture x, but capture_list is empty"
        );
        assert_eq!(
            mid_lam.capture_list[0].as_ref(),
            "x",
            "mid should capture x"
        );
    }

    #[test]
    fn body_thunk_with_app_captures_indirect_free_var() {
        let set_x = CoreExpr::new(CoreExprKind::Set {
            var: Rc::from("x"),
            scopes: Default::default(),
            value: Rc::new(lit(1)),
        });
        let lam1 = lambda(vec![], vec![lit(0)]);
        let lam2 = lambda(vec![], vec![set_x]);
        let lam3 = lambda(vec![], vec![var("x")]);
        let app = CoreExpr::new(CoreExprKind::App {
            func: Rc::new(var("f")),
            args: vec![lam1, lam2, lam3],
        });
        let body_thunk = lambda(vec![], vec![app]);
        let outer = lambda(vec!["x"], vec![body_thunk]);

        let info = Pass1Analysis::run(&outer);
        let closed = Pass2Closure::run(&outer, &info);

        let ClosedExprKind::Lambda(outer_lam) = &closed.kind else {
            panic!("expected outer Lambda");
        };
        assert!(
            outer_lam.boxed_params.contains(&Rc::<str>::from("x")),
            "x should be in outer's boxed_params"
        );

        let ClosedExprKind::Lambda(body_lam) = &outer_lam.body[0].kind else {
            panic!("expected body_thunk Lambda, got {:?}", outer_lam.body[0]);
        };
        assert!(
            body_lam.capture_list.contains(&Rc::<str>::from("x")),
            "body_thunk should capture x (has capture_list: {:?})",
            body_lam.capture_list
        );
    }
}
