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

use super::pass2_closure::ClosedExpr;
use patina_core::core_expr::Symbol;
use patina_core::tagged_value::TaggedValue;

// ─────────────────────────────────────────────────────────────────────────────
// TailedExpr — output of Pass 3
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum TailedExpr {
    Literal(TaggedValue),
    Quote(TaggedValue),
    Quasiquote(TaggedValue),
    LocalRef(Symbol),
    ClosureRef {
        name: Symbol,
        slot: u16,
    },
    GlobalRef(Symbol),

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
    SetClosure {
        var: Symbol,
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
    pub body: Vec<TailedExpr>,
    pub node_id: super::pass1_analysis::NodeId,
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
    match expr {
        ClosedExpr::Literal(v) => TailedExpr::Literal(*v),
        ClosedExpr::Quote(v) => TailedExpr::Quote(*v),
        ClosedExpr::Quasiquote(v) => TailedExpr::Quasiquote(*v),
        ClosedExpr::LocalRef(s) => TailedExpr::LocalRef(s.clone()),
        ClosedExpr::ClosureRef { name, slot } => TailedExpr::ClosureRef {
            name: name.clone(),
            slot: *slot,
        },
        ClosedExpr::GlobalRef(s) => TailedExpr::GlobalRef(s.clone()),

        ClosedExpr::Lambda(lam) => {
            // Each lambda body is its own tail context — the last expr is tail.
            let body_len = lam.body.len();
            let body: Vec<TailedExpr> = lam
                .body
                .iter()
                .enumerate()
                .map(|(i, e)| mark(e, i == body_len - 1))
                .collect();
            TailedExpr::Lambda(Box::new(TailedLambda {
                params: lam.params.clone(),
                rest_param: lam.rest_param,
                capture_list: lam.capture_list.clone(),
                body,
                node_id: lam.node_id,
            }))
        }

        ClosedExpr::If { test, then, else_ } => TailedExpr::If {
            test: Box::new(mark(test, false)),
            then: Box::new(mark(then, tail)),
            else_: Box::new(mark(else_, tail)),
        },

        ClosedExpr::SetLocal { var, value } => TailedExpr::SetLocal {
            var: var.clone(),
            value: Box::new(mark(value, false)),
        },
        ClosedExpr::SetClosure { var, slot, value } => TailedExpr::SetClosure {
            var: var.clone(),
            slot: *slot,
            value: Box::new(mark(value, false)),
        },
        ClosedExpr::SetGlobal { var, value } => TailedExpr::SetGlobal {
            var: var.clone(),
            value: Box::new(mark(value, false)),
        },

        ClosedExpr::Begin(exprs) => {
            let n = exprs.len();
            TailedExpr::Begin(
                exprs
                    .iter()
                    .enumerate()
                    .map(|(i, e)| mark(e, tail && i == n - 1))
                    .collect(),
            )
        }

        ClosedExpr::Define { name, value } => TailedExpr::Define {
            name: name.clone(),
            value: Box::new(mark(value, false)),
        },

        ClosedExpr::App { func, args } => TailedExpr::App {
            func: Box::new(mark(func, false)),
            args: args.iter().map(|a| mark(a, false)).collect(),
            is_tail: tail,
        },

        ClosedExpr::Apply { func, args } => TailedExpr::Apply {
            func: Box::new(mark(func, false)),
            args: args.iter().map(|a| mark(a, false)).collect(),
            is_tail: tail,
        },
    }
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
            binding_scope: None,
        })
    }

    fn pipeline(expr: &CoreExpr) -> TailedExpr {
        let info = Pass1Analysis::run(expr);
        let closed = Pass2Closure::run(expr, &info);
        Pass3Tail::run(&closed)
    }

    #[test]
    fn tail_call_in_lambda_body() {
        // (lambda () (f 1))  — the app is in tail position
        let expr = lambda(vec![], vec![app(var("f"), vec![lit(1)])]);
        let tailed = pipeline(&expr);
        let TailedExpr::Lambda(lam) = tailed else {
            panic!("expected Lambda");
        };
        let TailedExpr::App { is_tail, .. } = &lam.body[0] else {
            panic!("expected App");
        };
        assert!(*is_tail);
    }

    #[test]
    fn non_tail_call_in_begin() {
        // (lambda () (begin (f 1) (g 2)))  — only last is tail
        let expr = lambda(
            vec![],
            vec![CoreExpr::new(CoreExprKind::Begin(vec![
                app(var("f"), vec![lit(1)]),
                app(var("g"), vec![lit(2)]),
            ]))],
        );
        let tailed = pipeline(&expr);
        let TailedExpr::Lambda(lam) = tailed else {
            panic!();
        };
        let TailedExpr::Begin(exprs) = &lam.body[0] else {
            panic!();
        };
        let TailedExpr::App { is_tail: t0, .. } = &exprs[0] else {
            panic!();
        };
        let TailedExpr::App { is_tail: t1, .. } = &exprs[1] else {
            panic!();
        };
        assert!(!t0);
        assert!(*t1);
    }

    #[test]
    fn if_branches_inherit_tail() {
        // (lambda () (if #t (f) (g)))  — both branches are tail
        let if_expr = CoreExpr::new(CoreExprKind::If {
            test: Rc::new(CoreExpr::new(CoreExprKind::Literal(TaggedValue::TRUE))),
            then: Rc::new(app(var("f"), vec![])),
            else_: Rc::new(app(var("g"), vec![])),
        });
        let expr = lambda(vec![], vec![if_expr]);
        let tailed = pipeline(&expr);
        let TailedExpr::Lambda(lam) = tailed else {
            panic!();
        };
        let TailedExpr::If { then, else_, .. } = &lam.body[0] else {
            panic!();
        };
        let TailedExpr::App { is_tail: t1, .. } = then.as_ref() else {
            panic!();
        };
        let TailedExpr::App { is_tail: t2, .. } = else_.as_ref() else {
            panic!();
        };
        assert!(*t1);
        assert!(*t2);
    }
}
