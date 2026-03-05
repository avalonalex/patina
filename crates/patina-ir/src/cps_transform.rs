//! CPS Transformation - Convert CoreExpr to CpsExpr
//!
//! This module implements the CPS (Continuation-Passing Style) transformation
//! that converts direct-style `CoreExpr` into continuation-passing `CpsExpr`.
//!
//! # Algorithm
//!
//! The transformation uses the standard approach from Appel's "Compiling with
//! Continuations" with some modifications for Scheme semantics.
//!
//! ## Key Concepts
//!
//! - **Meta-continuation**: During transformation, we track "what to do with the
//!   result" as a Rust closure, not a CPS expression. This produces cleaner output.
//!
//! - **Administrative redexes**: The naive CPS transform produces many unnecessary
//!   `let-cont` bindings. We avoid these by using smart constructors.
//!
//! - **Tail position**: Expressions in tail position use the current continuation
//!   directly rather than creating a new one.
//!
//! # Example
//!
//! ```text
//! ;; Direct style
//! (+ (f x) (g y))
//!
//! ;; CPS transformed
//! (let-cont ((k1 (v1)
//!              (let-cont ((k2 (v2)
//!                          ((prim +) v1 v2 k)))
//!                (g y k2))))
//!   (f x k1))
//! ```
//!
//! Here `k` is the continuation passed in from the enclosing context.

use patina_core::TaggedValue;
use patina_core::core_expr::CoreExprKind;
use patina_core::cps_expr::{ContVar, CpsExpr, CpsExprKind, CpsParam};
use patina_core::error::SourceLocation;
use patina_core::scope::ScopeSet;
use patina_core::{CoreExpr, Formals, Symbol};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// CPS Transformer state
pub struct CpsTransformer {
    /// Counter for generating unique names
    gensym_counter: AtomicU64,
}

impl Default for CpsTransformer {
    fn default() -> Self {
        Self::new()
    }
}

impl CpsTransformer {
    pub fn new() -> Self {
        Self {
            gensym_counter: AtomicU64::new(0),
        }
    }

    /// Generate a unique symbol
    fn gensym(&self, prefix: &str) -> Symbol {
        let n = self.gensym_counter.fetch_add(1, Ordering::SeqCst);
        format!("{}_{}", prefix, n).into()
    }

    /// Generate a unique continuation variable
    fn gensym_cont(&self) -> ContVar {
        self.gensym("k")
    }

    /// Generate a unique value variable
    fn gensym_var(&self) -> Symbol {
        self.gensym("v")
    }

    /// Transform a top-level expression
    ///
    /// The result is wrapped in a `Halt` to represent program termination.
    pub fn transform_toplevel(&self, expr: &CoreExpr) -> CpsExpr {
        // Top-level continuation just halts with the value
        let halt_cont = self.gensym_cont();
        let result_var = self.gensym_var();

        // Create: (let-cont ((halt_k (result) (halt result))) <transformed-expr>)
        CpsExpr::new(CpsExprKind::LetCont {
            name: halt_cont.clone(),
            param: result_var.clone(),
            cont_body: CpsExpr::rc(CpsExprKind::Halt(CpsExpr::rc(CpsExprKind::Var {
                name: result_var,
                scopes: ScopeSet::new(),
            }))),
            body: Rc::new(self.transform(expr, &halt_cont)),
        })
    }

    /// Transform an expression with a given continuation
    ///
    /// The continuation `k` will receive the result of evaluating `expr`.
    pub fn transform(&self, expr: &CoreExpr, k: &ContVar) -> CpsExpr {
        match &expr.kind {
            // ==================== Trivial expressions ====================
            // These don't need CPS transformation - just pass to continuation
            CoreExprKind::Literal(v) => CpsExpr::new(CpsExprKind::Continue {
                cont: k.clone(),
                value: CpsExpr::rc(CpsExprKind::Literal(*v)),
            }),

            CoreExprKind::Var { name, scopes } => CpsExpr::new(CpsExprKind::Continue {
                cont: k.clone(),
                value: CpsExpr::rc(CpsExprKind::Var {
                    name: name.clone(),
                    scopes: scopes.clone(),
                }),
            }),

            CoreExprKind::Quote(v) => CpsExpr::new(CpsExprKind::Continue {
                cont: k.clone(),
                value: CpsExpr::rc(CpsExprKind::Literal(*v)),
            }),

            CoreExprKind::Lambda {
                params,
                body,
                binding_scope,
            } => {
                // Transform: (lambda (x y) body)
                // Into: (k (lambda (x y k') body'))
                // where body' is body transformed with continuation k'

                let cont_param = self.gensym_cont();
                let cps_body = self.transform_body(body, &cont_param);

                let (cps_params, cps_variadic) = self.transform_formals(params);

                CpsExpr::new(CpsExprKind::Continue {
                    cont: k.clone(),
                    value: CpsExpr::rc(CpsExprKind::Lambda {
                        params: cps_params,
                        variadic: cps_variadic,
                        cont_param,
                        body: Rc::new(cps_body),
                        // Preserve binding_scope for hygiene
                        binding_scope: *binding_scope,
                    }),
                })
            }

            // ==================== Serious expressions ====================
            CoreExprKind::If { test, then, else_ } => {
                // Transform: (if test then else)
                // Into: transform test, then branch on result

                // If test is trivial, we can inline it
                if self.is_trivial(test) {
                    let cps_test = self.transform_trivial(test);
                    let cps_then = self.transform(then, k);
                    let cps_else = self.transform(else_, k);

                    CpsExpr::new(CpsExprKind::If {
                        test: Rc::new(cps_test),
                        consequent: Rc::new(cps_then),
                        alternate: Rc::new(cps_else),
                    })
                } else {
                    // Need to evaluate test first
                    let test_var = self.gensym_var();
                    let test_cont = self.gensym_cont();

                    let cps_then = self.transform(then, k);
                    let cps_else = self.transform(else_, k);

                    let if_expr = CpsExpr::new(CpsExprKind::If {
                        test: CpsExpr::rc(CpsExprKind::Var {
                            name: test_var.clone(),
                            scopes: ScopeSet::new(),
                        }),
                        consequent: Rc::new(cps_then),
                        alternate: Rc::new(cps_else),
                    });

                    CpsExpr::new(CpsExprKind::LetCont {
                        name: test_cont.clone(),
                        param: test_var,
                        cont_body: Rc::new(if_expr),
                        body: Rc::new(self.transform(test, &test_cont)),
                    })
                }
            }

            CoreExprKind::Begin(exprs) => self.transform_sequence(exprs, k),

            CoreExprKind::App { func, args } => {
                // Check if this is a call/cc application
                if self.is_callcc(func) && args.len() == 1 {
                    return self.transform_callcc(&args[0], k);
                }
                self.transform_app(func, args, k, expr.source.clone())
            }

            CoreExprKind::Apply { func, args } => {
                // Apply is like App but last arg is a list to splice
                // For now, transform similarly and let runtime handle it
                self.transform_apply(func, args, k, expr.source.clone())
            }

            CoreExprKind::Set { var, scopes, value } => {
                // Transform: (set! var value)
                // Into: evaluate value, do set!, continue with unspecified

                if self.is_trivial(value) {
                    let cps_value = self.transform_trivial(value);
                    CpsExpr::new(CpsExprKind::Set {
                        var: var.clone(),
                        scopes: scopes.clone(),
                        value: Rc::new(cps_value),
                        cont: CpsExpr::rc(CpsExprKind::Continue {
                            cont: k.clone(),
                            value: CpsExpr::rc(CpsExprKind::Literal(TaggedValue::UNSPECIFIED)),
                        }),
                    })
                } else {
                    let val_var = self.gensym_var();
                    let val_cont = self.gensym_cont();

                    let set_expr = CpsExpr::new(CpsExprKind::Set {
                        var: var.clone(),
                        scopes: scopes.clone(),
                        value: CpsExpr::rc(CpsExprKind::Var {
                            name: val_var.clone(),
                            scopes: ScopeSet::new(),
                        }),
                        cont: CpsExpr::rc(CpsExprKind::Continue {
                            cont: k.clone(),
                            value: CpsExpr::rc(CpsExprKind::Literal(TaggedValue::UNSPECIFIED)),
                        }),
                    });

                    CpsExpr::new(CpsExprKind::LetCont {
                        name: val_cont.clone(),
                        param: val_var,
                        cont_body: Rc::new(set_expr),
                        body: Rc::new(self.transform(value, &val_cont)),
                    })
                }
            }

            CoreExprKind::Define { name, value } => {
                // Transform: (define name value)
                // Into: evaluate value, do define, continue with unspecified

                if self.is_trivial(value) {
                    let cps_value = self.transform_trivial(value);
                    CpsExpr::new(CpsExprKind::Define {
                        name: name.clone(),
                        value: Rc::new(cps_value),
                        cont: CpsExpr::rc(CpsExprKind::Continue {
                            cont: k.clone(),
                            value: CpsExpr::rc(CpsExprKind::Literal(TaggedValue::UNSPECIFIED)),
                        }),
                    })
                } else {
                    let val_var = self.gensym_var();
                    let val_cont = self.gensym_cont();

                    let def_expr = CpsExpr::new(CpsExprKind::Define {
                        name: name.clone(),
                        value: CpsExpr::rc(CpsExprKind::Var {
                            name: val_var.clone(),
                            scopes: ScopeSet::new(),
                        }),
                        cont: CpsExpr::rc(CpsExprKind::Continue {
                            cont: k.clone(),
                            value: CpsExpr::rc(CpsExprKind::Literal(TaggedValue::UNSPECIFIED)),
                        }),
                    });

                    CpsExpr::new(CpsExprKind::LetCont {
                        name: val_cont.clone(),
                        param: val_var,
                        cont_body: Rc::new(def_expr),
                        body: Rc::new(self.transform(value, &val_cont)),
                    })
                }
            }

            // Quasiquote template - needs runtime evaluation for unquote/unquote-splicing
            CoreExprKind::Quasiquote(template) => {
                // Quasiquote is evaluated at runtime, so we pass the template
                // to the CPS evaluator which will process unquote/unquote-splicing
                CpsExpr::new(CpsExprKind::Quasiquote {
                    template: *template,
                    cont: k.clone(),
                })
            }

            CoreExprKind::Import { .. } => {
                // Import is a compile-time construct, shouldn't appear here
                panic!("Import should be handled before CPS transform")
            }

            // Note: Parameterize is now a macro using dynamic-wind (lib/scheme/base/parameters.scm)
            // It expands to regular code before CPS transform.
            CoreExprKind::Expand { .. } => {
                // Expand is a debugging form, skip CPS transform
                panic!("Expand should be handled before CPS transform")
            }
        }
    }

    /// Check if an expression is trivial (no control effects)
    fn is_trivial(&self, expr: &CoreExpr) -> bool {
        matches!(
            &expr.kind,
            CoreExprKind::Literal(_)
                | CoreExprKind::Var { .. }
                | CoreExprKind::Quote(_)
                | CoreExprKind::Lambda { .. }
        )
    }

    /// Check if an expression is a call/cc reference
    fn is_callcc(&self, expr: &CoreExpr) -> bool {
        match &expr.kind {
            CoreExprKind::Var { name, .. } => {
                name.as_ref() == "call/cc" || name.as_ref() == "call-with-current-continuation"
            }
            _ => false,
        }
    }

    /// Transform a call/cc application into CpsExpr::CallCC
    ///
    /// (call/cc proc) transforms to CallCC { proc: <proc>, cont: k }
    fn transform_callcc(&self, proc: &CoreExpr, k: &ContVar) -> CpsExpr {
        if self.is_trivial(proc) {
            // Proc is trivial - can inline
            let cps_proc = self.transform_trivial(proc);
            CpsExpr::new(CpsExprKind::CallCC {
                proc: Rc::new(cps_proc),
                cont: k.clone(),
            })
        } else {
            // Proc is not trivial - need to evaluate it first
            let proc_var = self.gensym("proc");
            let proc_cont = self.gensym_cont();

            let callcc_expr = CpsExpr::new(CpsExprKind::CallCC {
                proc: CpsExpr::rc(CpsExprKind::Var {
                    name: proc_var.clone(),
                    scopes: ScopeSet::new(),
                }),
                cont: k.clone(),
            });

            CpsExpr::new(CpsExprKind::LetCont {
                name: proc_cont.clone(),
                param: proc_var,
                cont_body: Rc::new(callcc_expr),
                body: Rc::new(self.transform(proc, &proc_cont)),
            })
        }
    }

    /// Transform a trivial expression (must be trivial!)
    fn transform_trivial(&self, expr: &CoreExpr) -> CpsExpr {
        match &expr.kind {
            CoreExprKind::Literal(v) => CpsExpr::new(CpsExprKind::Literal(*v)),
            CoreExprKind::Var { name, scopes } => CpsExpr::new(CpsExprKind::Var {
                name: name.clone(),
                scopes: scopes.clone(),
            }),
            CoreExprKind::Quote(v) => CpsExpr::new(CpsExprKind::Literal(*v)),
            CoreExprKind::Lambda {
                params,
                body,
                binding_scope,
            } => {
                let cont_param = self.gensym_cont();
                let cps_body = self.transform_body(body, &cont_param);
                let (cps_params, cps_variadic) = self.transform_formals(params);

                CpsExpr::new(CpsExprKind::Lambda {
                    params: cps_params,
                    variadic: cps_variadic,
                    cont_param,
                    body: Rc::new(cps_body),
                    binding_scope: *binding_scope,
                })
            }
            _ => panic!(
                "transform_trivial called on non-trivial expression: {:?}",
                expr.expr_kind()
            ),
        }
    }

    /// Transform lambda formals
    fn transform_formals(&self, formals: &Formals) -> (Vec<CpsParam>, Option<CpsParam>) {
        match formals {
            Formals::Fixed(params) => {
                let cps_params = params
                    .iter()
                    .map(|p| CpsParam::with_scopes(p.name.clone(), p.scopes.clone()))
                    .collect();
                (cps_params, None)
            }
            Formals::Variadic(param) => {
                let rest = CpsParam::with_scopes(param.name.clone(), param.scopes.clone());
                (vec![], Some(rest))
            }
            Formals::Mixed { fixed, rest } => {
                let cps_params = fixed
                    .iter()
                    .map(|p| CpsParam::with_scopes(p.name.clone(), p.scopes.clone()))
                    .collect();
                let rest_param = CpsParam::with_scopes(rest.name.clone(), rest.scopes.clone());
                (cps_params, Some(rest_param))
            }
        }
    }

    /// Transform a sequence of expressions (begin body)
    fn transform_body(&self, body: &[CoreExpr], k: &ContVar) -> CpsExpr {
        self.transform_sequence(body, k)
    }

    /// Transform a sequence of expressions
    fn transform_sequence(&self, exprs: &[CoreExpr], k: &ContVar) -> CpsExpr {
        if exprs.is_empty() {
            // Empty sequence returns unspecified
            CpsExpr::new(CpsExprKind::Continue {
                cont: k.clone(),
                value: CpsExpr::rc(CpsExprKind::Literal(TaggedValue::UNSPECIFIED)),
            })
        } else if exprs.len() == 1 {
            // Single expression - tail position
            self.transform(&exprs[0], k)
        } else {
            // Multiple expressions - evaluate first for effect, then continue
            let first = &exprs[0];
            let rest = &exprs[1..];

            // Create continuation for the rest
            let ignore_var = self.gensym("_");
            let rest_cont = self.gensym_cont();

            let rest_expr = self.transform_sequence(rest, k);

            CpsExpr::new(CpsExprKind::LetCont {
                name: rest_cont.clone(),
                param: ignore_var,
                cont_body: Rc::new(rest_expr),
                body: Rc::new(self.transform(first, &rest_cont)),
            })
        }
    }

    /// Transform function application
    fn transform_app(
        &self,
        func: &CoreExpr,
        args: &[CoreExpr],
        k: &ContVar,
        source: Option<SourceLocation>,
    ) -> CpsExpr {
        // Collect all expressions to evaluate: func, then args
        let mut all_exprs: Vec<&CoreExpr> = vec![func];
        all_exprs.extend(args.iter());

        // Generate names for all results
        let names: Vec<Symbol> = (0..all_exprs.len())
            .map(|i| {
                if i == 0 {
                    self.gensym("f")
                } else {
                    self.gensym("a")
                }
            })
            .collect();

        // Build the application expression
        let func_var = CpsExpr::new(CpsExprKind::Var {
            name: names[0].clone(),
            scopes: ScopeSet::new(),
        });

        let arg_vars: Vec<CpsExpr> = names[1..]
            .iter()
            .map(|n| {
                CpsExpr::new(CpsExprKind::Var {
                    name: n.clone(),
                    scopes: ScopeSet::new(),
                })
            })
            .collect();

        let app = CpsExpr::with_opt_source(
            CpsExprKind::App {
                func: Rc::new(func_var),
                args: arg_vars,
                cont: k.clone(),
            },
            source,
        );

        // Build let-cont chain from inside out
        self.transform_args_then(&all_exprs, &names, app)
    }

    /// Transform apply
    fn transform_apply(
        &self,
        func: &CoreExpr,
        args: &[CoreExpr],
        k: &ContVar,
        source: Option<SourceLocation>,
    ) -> CpsExpr {
        // Transform apply like app but generate CpsExpr::Apply
        // This preserves the "flatten last arg" semantics for the CPS evaluator

        // Generate names for all expressions
        let func_name = self.gensym_var();
        let arg_names: Vec<Symbol> = args.iter().map(|_| self.gensym_var()).collect();

        // The final apply expression, stamping the call-site source
        let apply_expr = CpsExpr::with_opt_source(
            CpsExprKind::Apply {
                func: CpsExpr::rc(CpsExprKind::Var {
                    name: func_name.clone(),
                    scopes: ScopeSet::new(),
                }),
                args: arg_names
                    .iter()
                    .map(|n| {
                        CpsExpr::new(CpsExprKind::Var {
                            name: n.clone(),
                            scopes: ScopeSet::new(),
                        })
                    })
                    .collect(),
                cont: k.clone(),
            },
            source,
        );

        // Build up let-bindings for func and all args
        let all_exprs: Vec<&CoreExpr> = std::iter::once(func).chain(args.iter()).collect();
        let all_names: Vec<Symbol> = std::iter::once(func_name).chain(arg_names).collect();

        self.transform_args_then(&all_exprs, &all_names, apply_expr)
    }

    /// Transform arguments left-to-right, then execute final expression
    fn transform_args_then(
        &self,
        exprs: &[&CoreExpr],
        names: &[Symbol],
        final_expr: CpsExpr,
    ) -> CpsExpr {
        if exprs.is_empty() {
            return final_expr;
        }

        // Propagate the call-site source to LetVal wrappers so that errors
        // during argument evaluation carry the application's source location.
        let call_source = final_expr.source.clone();

        // Work backwards to build nested let-conts
        let mut result = final_expr;

        for (expr, name) in exprs.iter().zip(names.iter()).rev() {
            if self.is_trivial(expr) {
                // Trivial - just let-bind it
                let cps_value = self.transform_trivial(expr);
                result = CpsExpr::with_opt_source(
                    CpsExprKind::LetVal {
                        name: name.clone(),
                        value: Rc::new(cps_value),
                        body: Rc::new(result),
                    },
                    call_source.clone(),
                );
            } else {
                // Non-trivial - need continuation
                let cont_name = self.gensym_cont();
                let transformed = self.transform(expr, &cont_name);

                result = CpsExpr::new(CpsExprKind::LetCont {
                    name: cont_name,
                    param: name.clone(),
                    cont_body: Rc::new(result),
                    body: Rc::new(transformed),
                });
            }
        }

        result
    }

    // Note: transform_parameterize has been removed.
    // Parameterize is now a macro using dynamic-wind (lib/scheme/base/parameters.scm)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_literal(n: i64) -> CoreExpr {
        CoreExpr::new(CoreExprKind::Literal(TaggedValue::fixnum(n)))
    }

    fn make_var(name: &str) -> CoreExpr {
        CoreExpr::new(CoreExprKind::Var {
            name: name.into(),
            scopes: ScopeSet::new(),
        })
    }

    #[test]
    fn test_transform_literal() {
        let transformer = CpsTransformer::new();
        let expr = make_literal(42);
        let cps = transformer.transform_toplevel(&expr);

        // Should produce: (let-cont ((k_0 (v_1) (halt v_1))) (k_0 42))
        println!("CPS: {}", cps);
        assert!(matches!(cps.kind, CpsExprKind::LetCont { .. }));
    }

    #[test]
    fn test_transform_variable() {
        let transformer = CpsTransformer::new();
        let expr = make_var("x");
        let cps = transformer.transform_toplevel(&expr);

        println!("CPS: {}", cps);
        assert!(matches!(cps.kind, CpsExprKind::LetCont { .. }));
    }

    #[test]
    fn test_transform_if() {
        let transformer = CpsTransformer::new();

        // (if #t 1 2)
        let expr = CoreExpr::new(CoreExprKind::If {
            test: Rc::new(CoreExpr::new(CoreExprKind::Literal(TaggedValue::TRUE))),
            then: Rc::new(make_literal(1)),
            else_: Rc::new(make_literal(2)),
        });

        let cps = transformer.transform_toplevel(&expr);
        println!("CPS: {}", cps);

        // Should have an if in the output
        fn has_if(e: &CpsExpr) -> bool {
            match &e.kind {
                CpsExprKind::If { .. } => true,
                CpsExprKind::LetCont {
                    cont_body, body, ..
                } => has_if(cont_body) || has_if(body),
                CpsExprKind::LetVal { body, .. } => has_if(body),
                _ => false,
            }
        }
        assert!(has_if(&cps));
    }

    #[test]
    fn test_transform_lambda() {
        let transformer = CpsTransformer::new();

        // (lambda (x) x)
        let expr = CoreExpr::new(CoreExprKind::Lambda {
            params: Formals::Fixed(vec![patina_core::ScopedParam::simple("x".into())]),
            body: vec![make_var("x")],
            binding_scope: None,
        });

        let cps = transformer.transform_toplevel(&expr);
        println!("CPS: {}", cps);

        // Should produce a lambda with cont parameter
        fn has_lambda(e: &CpsExpr) -> bool {
            match &e.kind {
                CpsExprKind::Lambda { .. } => true,
                CpsExprKind::Continue { value, .. } => has_lambda(value),
                CpsExprKind::LetCont {
                    cont_body, body, ..
                } => has_lambda(cont_body) || has_lambda(body),
                _ => false,
            }
        }
        assert!(has_lambda(&cps));
    }

    #[test]
    fn test_transform_application() {
        let transformer = CpsTransformer::new();

        // (f x)
        let expr = CoreExpr::new(CoreExprKind::App {
            func: Rc::new(make_var("f")),
            args: vec![make_var("x")],
        });

        let cps = transformer.transform_toplevel(&expr);
        println!("CPS: {}", cps);

        // Should have an application
        fn has_app(e: &CpsExpr) -> bool {
            match &e.kind {
                CpsExprKind::App { .. } => true,
                CpsExprKind::LetCont {
                    cont_body, body, ..
                } => has_app(cont_body) || has_app(body),
                CpsExprKind::LetVal { body, .. } => has_app(body),
                _ => false,
            }
        }
        assert!(has_app(&cps));
    }

    #[test]
    fn test_transform_begin() {
        let transformer = CpsTransformer::new();

        // (begin 1 2 3)
        let expr = CoreExpr::new(CoreExprKind::Begin(vec![
            make_literal(1),
            make_literal(2),
            make_literal(3),
        ]));

        let cps = transformer.transform_toplevel(&expr);
        println!("CPS: {}", cps);

        // Should sequence the expressions
        assert!(matches!(cps.kind, CpsExprKind::LetCont { .. }));
    }

    #[test]
    fn test_transform_callcc() {
        let transformer = CpsTransformer::new();

        // (call/cc (lambda (k) 42))
        let lambda_body = make_literal(42);
        let lambda = CoreExpr::new(CoreExprKind::Lambda {
            params: Formals::Fixed(vec![patina_core::ScopedParam::simple("k".into())]),
            body: vec![lambda_body],
            binding_scope: None,
        });

        let expr = CoreExpr::new(CoreExprKind::App {
            func: Rc::new(make_var("call/cc")),
            args: vec![lambda],
        });

        let cps = transformer.transform_toplevel(&expr);
        println!("CPS for call/cc: {}", cps);

        // Should produce a CpsExprKind::CallCC
        fn has_callcc(e: &CpsExpr) -> bool {
            match &e.kind {
                CpsExprKind::CallCC { .. } => true,
                CpsExprKind::LetCont {
                    cont_body, body, ..
                } => has_callcc(cont_body) || has_callcc(body),
                CpsExprKind::LetVal { body, .. } => has_callcc(body),
                _ => false,
            }
        }
        assert!(has_callcc(&cps), "CPS should contain CallCC node");
    }

    #[test]
    fn test_transform_callcc_with_escape() {
        let transformer = CpsTransformer::new();

        // (call/cc (lambda (exit) (exit 99)))
        let exit_call = CoreExpr::new(CoreExprKind::App {
            func: Rc::new(make_var("exit")),
            args: vec![make_literal(99)],
        });
        let lambda = CoreExpr::new(CoreExprKind::Lambda {
            params: Formals::Fixed(vec![patina_core::ScopedParam::simple("exit".into())]),
            body: vec![exit_call],
            binding_scope: None,
        });

        let expr = CoreExpr::new(CoreExprKind::App {
            func: Rc::new(make_var("call/cc")),
            args: vec![lambda],
        });

        let cps = transformer.transform_toplevel(&expr);
        println!("CPS for call/cc with escape: {}", cps);

        fn has_callcc(e: &CpsExpr) -> bool {
            match &e.kind {
                CpsExprKind::CallCC { .. } => true,
                CpsExprKind::LetCont {
                    cont_body, body, ..
                } => has_callcc(cont_body) || has_callcc(body),
                CpsExprKind::LetVal { body, .. } => has_callcc(body),
                _ => false,
            }
        }
        assert!(has_callcc(&cps));
    }

    #[test]
    fn test_transform_nested_callcc() {
        let transformer = CpsTransformer::new();

        // (+ 1 (call/cc (lambda (k) (k 2))))
        let k_call = CoreExpr::new(CoreExprKind::App {
            func: Rc::new(make_var("k")),
            args: vec![make_literal(2)],
        });
        let lambda = CoreExpr::new(CoreExprKind::Lambda {
            params: Formals::Fixed(vec![patina_core::ScopedParam::simple("k".into())]),
            body: vec![k_call],
            binding_scope: None,
        });
        let callcc = CoreExpr::new(CoreExprKind::App {
            func: Rc::new(make_var("call/cc")),
            args: vec![lambda],
        });

        // (+ 1 <callcc>) - represented as App with + variable
        let add = CoreExpr::new(CoreExprKind::App {
            func: Rc::new(make_var("+")),
            args: vec![make_literal(1), callcc],
        });

        let cps = transformer.transform_toplevel(&add);
        println!("CPS for nested call/cc: {}", cps);

        fn has_callcc(e: &CpsExpr) -> bool {
            match &e.kind {
                CpsExprKind::CallCC { .. } => true,
                CpsExprKind::LetCont {
                    cont_body, body, ..
                } => has_callcc(cont_body) || has_callcc(body),
                CpsExprKind::LetVal { body, .. } => has_callcc(body),
                CpsExprKind::PrimOp { .. } => false,
                _ => false,
            }
        }
        assert!(has_callcc(&cps));
    }
}
