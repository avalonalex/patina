//! Pass 3 — Tail Position Marking
//!
//! Annotates every `App` node with `is_tail: bool`. An application is in tail
//! position when its result is directly returned from the enclosing lambda with
//! no further processing.
//!
//! Rules (R7RS §3.5 — proper tail calls):
//! - The last expression in a lambda body is in tail position.
//! - The last expression of a `Begin` is in tail position when the `Begin`
//!   itself is in tail position.
//! - Both branches of an `If` are in tail position when the `If` is in tail
//!   position.
//! - `Define` value and `Set*` value are never in tail position.
//!
//! See VM_COMPILER.md §Pass 3.

use super::pass2_closure::{ClosedExpr, ClosedExprKind};
use patina_core::core_expr::Symbol;
use patina_core::error::SourceLocation;
use patina_core::tagged_value::TaggedValue;
use std::collections::HashSet;

// ─────────────────────────────────────────────────────────────────────────────
// TailedExpr — output of Pass 3
// ─────────────────────────────────────────────────────────────────────────────

/// A tail-position-annotated expression with optional source location.
#[derive(Debug, Clone)]
pub struct TailedExpr {
    pub kind: TailedExprKind,
    pub source: Option<SourceLocation>,
}

impl TailedExpr {
    fn with_source(kind: TailedExprKind, source: Option<SourceLocation>) -> Self {
        Self { kind, source }
    }
}

#[derive(Debug, Clone)]
pub enum TailedExprKind {
    Literal(TaggedValue),
    Quote(TaggedValue),
    Quasiquote(TaggedValue),

    LocalRef(Symbol),
    ClosureRef {
        name: Symbol,
        slot: u16,
    },
    GlobalRef(Symbol),

    /// Read value out of a `MutableCell` held in a local register.
    ReadLocalCell(Symbol),
    /// Read value out of a `MutableCell` held in a closure slot.
    ReadClosureCell {
        name: Symbol,
        slot: u16,
    },

    Lambda(Box<TailedLambda>),

    If {
        test: Box<TailedExpr>,
        then: Box<TailedExpr>,
        else_: Box<TailedExpr>,
    },

    SetLocal {
        var: Symbol,
        value: Box<TailedExpr>,
    },
    /// `set!` through a `MutableCell` held in a local register.
    WriteLocalCell {
        var: Symbol,
        value: Box<TailedExpr>,
    },
    /// `set!` through a `MutableCell` held in a closure slot.
    WriteClosureCell {
        slot: u16,
        value: Box<TailedExpr>,
    },
    SetGlobal {
        var: Symbol,
        value: Box<TailedExpr>,
    },

    Begin(Vec<TailedExpr>),
    Define {
        name: Symbol,
        value: Box<TailedExpr>,
    },

    App {
        func: Box<TailedExpr>,
        args: Vec<TailedExpr>,
        /// True when this call is in tail position.
        is_tail: bool,
    },

    Apply {
        func: Box<TailedExpr>,
        args: Vec<TailedExpr>,
        is_tail: bool,
    },
}

#[derive(Debug, Clone)]
pub struct TailedLambda {
    pub params: Vec<Symbol>,
    pub rest_param: bool,
    pub capture_list: Vec<Symbol>,
    /// Parameters (and internal defines) that must be wrapped in `MutableCell` on entry.
    pub boxed_params: HashSet<Symbol>,
    pub body: Vec<TailedExpr>,
    pub node_id: super::pass1_analysis::NodeId,
    /// Names introduced by internal `define` forms, treated as local bindings.
    pub internal_defines: Vec<Symbol>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Pass entry
// ─────────────────────────────────────────────────────────────────────────────

pub struct Pass3Tail;

impl Pass3Tail {
    pub fn run(expr: &ClosedExpr) -> TailedExpr {
        // Top-level expressions are not in tail position (result discarded or
        // used by `Define`). Lambda bodies handle their own tail context.
        mark(expr, false)
    }
}

fn mark(expr: &ClosedExpr, tail: bool) -> TailedExpr {
    let source = expr.source.clone();
    let kind = match &expr.kind {
        ClosedExprKind::Literal(v) => TailedExprKind::Literal(*v),
        ClosedExprKind::Quote(v) => TailedExprKind::Quote(*v),
        ClosedExprKind::Quasiquote(v) => TailedExprKind::Quasiquote(*v),
        ClosedExprKind::LocalRef(s) => TailedExprKind::LocalRef(s.clone()),
        ClosedExprKind::ClosureRef { name, slot } => TailedExprKind::ClosureRef {
            name: name.clone(),
            slot: *slot,
        },
        ClosedExprKind::GlobalRef(s) => TailedExprKind::GlobalRef(s.clone()),

        ClosedExprKind::ReadLocalCell(s) => TailedExprKind::ReadLocalCell(s.clone()),
        ClosedExprKind::ReadClosureCell { name, slot } => TailedExprKind::ReadClosureCell {
            name: name.clone(),
            slot: *slot,
        },

        ClosedExprKind::Lambda(lam) => {
            // Each lambda body is its own tail context — the last expr is tail.
            let body_len = lam.body.len();
            let body: Vec<TailedExpr> = lam
                .body
                .iter()
                .enumerate()
                .map(|(i, e)| mark(e, i == body_len - 1))
                .collect();
            TailedExprKind::Lambda(Box::new(TailedLambda {
                params: lam.params.clone(),
                rest_param: lam.rest_param,
                capture_list: lam.capture_list.clone(),
                boxed_params: lam.boxed_params.clone(),
                body,
                node_id: lam.node_id,
                internal_defines: lam.internal_defines.clone(),
            }))
        }

        ClosedExprKind::If { test, then, else_ } => TailedExprKind::If {
            test: Box::new(mark(test, false)),
            then: Box::new(mark(then, tail)),
            else_: Box::new(mark(else_, tail)),
        },

        ClosedExprKind::SetLocal { var, value } => TailedExprKind::SetLocal {
            var: var.clone(),
            value: Box::new(mark(value, false)),
        },
        ClosedExprKind::WriteLocalCell { var, value } => TailedExprKind::WriteLocalCell {
            var: var.clone(),
            value: Box::new(mark(value, false)),
        },
        ClosedExprKind::WriteClosureCell { slot, value } => TailedExprKind::WriteClosureCell {
            slot: *slot,
            value: Box::new(mark(value, false)),
        },
        ClosedExprKind::SetGlobal { var, value } => TailedExprKind::SetGlobal {
            var: var.clone(),
            value: Box::new(mark(value, false)),
        },

        ClosedExprKind::Begin(exprs) => {
            let n = exprs.len();
            TailedExprKind::Begin(
                exprs
                    .iter()
                    .enumerate()
                    .map(|(i, e)| mark(e, tail && i == n - 1))
                    .collect(),
            )
        }

        ClosedExprKind::Define { name, value } => TailedExprKind::Define {
            name: name.clone(),
            value: Box::new(mark(value, false)),
        },

        ClosedExprKind::App { func, args } => TailedExprKind::App {
            func: Box::new(mark(func, false)),
            args: args.iter().map(|a| mark(a, false)).collect(),
            is_tail: tail,
        },

        ClosedExprKind::Apply { func, args } => TailedExprKind::Apply {
            func: Box::new(mark(func, false)),
            args: args.iter().map(|a| mark(a, false)).collect(),
            is_tail: tail,
        },
    };
    TailedExpr::with_source(kind, source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::pass1_analysis::Pass1Analysis;
    use crate::compiler::pass2_closure::Pass2Closure;
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

    fn app(func: CoreExpr, args: Vec<CoreExpr>) -> CoreExpr {
        CoreExpr::new(CoreExprKind::App {
            func: Rc::new(func),
            args,
        })
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
            binding_scopes: Default::default(),
        })
    }

    fn pipeline(expr: &CoreExpr) -> TailedExpr {
        let info = Pass1Analysis::run(expr);
        let closed = Pass2Closure::run(expr, &info);
        Pass3Tail::run(&closed)
    }

    #[test]
    fn tail_call_in_lambda_body() {
        let expr = lambda(vec![], vec![app(var("f"), vec![lit(1)])]);
        let tailed = pipeline(&expr);
        let TailedExprKind::Lambda(lam) = tailed.kind else {
            panic!("expected Lambda");
        };
        let TailedExprKind::App { is_tail, .. } = &lam.body[0].kind else {
            panic!("expected App");
        };
        assert!(*is_tail);
    }

    #[test]
    fn non_tail_call_in_begin() {
        let expr = lambda(
            vec![],
            vec![CoreExpr::new(CoreExprKind::Begin(vec![
                app(var("f"), vec![lit(1)]),
                app(var("g"), vec![lit(2)]),
            ]))],
        );
        let tailed = pipeline(&expr);
        let TailedExprKind::Lambda(lam) = tailed.kind else {
            panic!("expected Lambda");
        };
        let TailedExprKind::Begin(exprs) = &lam.body[0].kind else {
            panic!("expected Begin");
        };
        let TailedExprKind::App { is_tail: t0, .. } = &exprs[0].kind else {
            panic!("expected App at exprs[0]");
        };
        let TailedExprKind::App { is_tail: t1, .. } = &exprs[1].kind else {
            panic!("expected App at exprs[1]");
        };
        assert!(!t0);
        assert!(*t1);
    }

    #[test]
    fn if_branches_inherit_tail() {
        let if_expr = CoreExpr::new(CoreExprKind::If {
            test: Rc::new(CoreExpr::new(CoreExprKind::Literal(TaggedValue::TRUE))),
            then: Rc::new(app(var("f"), vec![])),
            else_: Rc::new(app(var("g"), vec![])),
        });
        let expr = lambda(vec![], vec![if_expr]);
        let tailed = pipeline(&expr);
        let TailedExprKind::Lambda(lam) = tailed.kind else {
            panic!("expected Lambda");
        };
        let TailedExprKind::If { then, else_, .. } = &lam.body[0].kind else {
            panic!("expected If");
        };
        let TailedExprKind::App { is_tail: t1, .. } = &then.kind else {
            panic!("expected App in then-branch");
        };
        let TailedExprKind::App { is_tail: t2, .. } = &else_.kind else {
            panic!("expected App in else-branch");
        };
        assert!(*t1);
        assert!(*t2);
    }
}
