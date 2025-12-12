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

use patina_core::cps_expr::{ContVar, CpsExpr, CpsParam};
use patina_core::scope::ScopeSet;
use patina_core::value::Value;
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
        CpsExpr::LetCont {
            name: halt_cont.clone(),
            param: result_var.clone(),
            cont_body: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Var {
                name: result_var,
                scopes: ScopeSet::new(),
            }))),
            body: Rc::new(self.transform(expr, &halt_cont)),
        }
    }

    /// Transform an expression with a given continuation
    ///
    /// The continuation `k` will receive the result of evaluating `expr`.
    pub fn transform(&self, expr: &CoreExpr, k: &ContVar) -> CpsExpr {
        match expr {
            // ==================== Trivial expressions ====================
            // These don't need CPS transformation - just pass to continuation
            CoreExpr::Literal(v) => CpsExpr::Continue {
                cont: k.clone(),
                value: Rc::new(CpsExpr::Literal(v.clone())),
            },

            CoreExpr::Var { name, scopes } => CpsExpr::Continue {
                cont: k.clone(),
                value: Rc::new(CpsExpr::Var {
                    name: name.clone(),
                    scopes: scopes.clone(),
                }),
            },

            CoreExpr::Quote(v) => CpsExpr::Continue {
                cont: k.clone(),
                value: Rc::new(CpsExpr::Literal(v.clone())),
            },

            CoreExpr::Lambda {
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

                CpsExpr::Continue {
                    cont: k.clone(),
                    value: Rc::new(CpsExpr::Lambda {
                        params: cps_params,
                        variadic: cps_variadic,
                        cont_param,
                        body: Rc::new(cps_body),
                        // Preserve binding_scope for hygiene
                        binding_scope: *binding_scope,
                    }),
                }
            }

            // ==================== Serious expressions ====================
            CoreExpr::If { test, then, else_ } => {
                // Transform: (if test then else)
                // Into: transform test, then branch on result

                // If test is trivial, we can inline it
                if self.is_trivial(test) {
                    let cps_test = self.transform_trivial(test);
                    let cps_then = self.transform(then, k);
                    let cps_else = self.transform(else_, k);

                    CpsExpr::If {
                        test: Rc::new(cps_test),
                        consequent: Rc::new(cps_then),
                        alternate: Rc::new(cps_else),
                    }
                } else {
                    // Need to evaluate test first
                    let test_var = self.gensym_var();
                    let test_cont = self.gensym_cont();

                    let cps_then = self.transform(then, k);
                    let cps_else = self.transform(else_, k);

                    let if_expr = CpsExpr::If {
                        test: Rc::new(CpsExpr::Var {
                            name: test_var.clone(),
                            scopes: ScopeSet::new(),
                        }),
                        consequent: Rc::new(cps_then),
                        alternate: Rc::new(cps_else),
                    };

                    CpsExpr::LetCont {
                        name: test_cont.clone(),
                        param: test_var,
                        cont_body: Rc::new(if_expr),
                        body: Rc::new(self.transform(test, &test_cont)),
                    }
                }
            }

            CoreExpr::Begin(exprs) => self.transform_sequence(exprs, k),

            CoreExpr::App { func, args } => {
                // Check if this is a call/cc application
                if self.is_callcc(func) && args.len() == 1 {
                    return self.transform_callcc(&args[0], k);
                }
                self.transform_app(func, args, k)
            }

            CoreExpr::Apply { func, args } => {
                // Apply is like App but last arg is a list to splice
                // For now, transform similarly and let runtime handle it
                // TODO: Handle apply specially in CPS evaluator
                self.transform_apply(func, args, k)
            }

            CoreExpr::Set { var, scopes, value } => {
                // Transform: (set! var value)
                // Into: evaluate value, do set!, continue with unspecified

                if self.is_trivial(value) {
                    let cps_value = self.transform_trivial(value);
                    CpsExpr::Set {
                        var: var.clone(),
                        scopes: scopes.clone(),
                        value: Rc::new(cps_value),
                        cont: Rc::new(CpsExpr::Continue {
                            cont: k.clone(),
                            value: Rc::new(CpsExpr::Literal(Rc::new(Value::Unspecified))),
                        }),
                    }
                } else {
                    let val_var = self.gensym_var();
                    let val_cont = self.gensym_cont();

                    let set_expr = CpsExpr::Set {
                        var: var.clone(),
                        scopes: scopes.clone(),
                        value: Rc::new(CpsExpr::Var {
                            name: val_var.clone(),
                            scopes: ScopeSet::new(),
                        }),
                        cont: Rc::new(CpsExpr::Continue {
                            cont: k.clone(),
                            value: Rc::new(CpsExpr::Literal(Rc::new(Value::Unspecified))),
                        }),
                    };

                    CpsExpr::LetCont {
                        name: val_cont.clone(),
                        param: val_var,
                        cont_body: Rc::new(set_expr),
                        body: Rc::new(self.transform(value, &val_cont)),
                    }
                }
            }

            CoreExpr::Define { name, value } => {
                // Transform: (define name value)
                // Into: evaluate value, do define, continue with unspecified

                if self.is_trivial(value) {
                    let cps_value = self.transform_trivial(value);
                    CpsExpr::Define {
                        name: name.clone(),
                        value: Rc::new(cps_value),
                        cont: Rc::new(CpsExpr::Continue {
                            cont: k.clone(),
                            value: Rc::new(CpsExpr::Literal(Rc::new(Value::Unspecified))),
                        }),
                    }
                } else {
                    let val_var = self.gensym_var();
                    let val_cont = self.gensym_cont();

                    let def_expr = CpsExpr::Define {
                        name: name.clone(),
                        value: Rc::new(CpsExpr::Var {
                            name: val_var.clone(),
                            scopes: ScopeSet::new(),
                        }),
                        cont: Rc::new(CpsExpr::Continue {
                            cont: k.clone(),
                            value: Rc::new(CpsExpr::Literal(Rc::new(Value::Unspecified))),
                        }),
                    };

                    CpsExpr::LetCont {
                        name: val_cont.clone(),
                        param: val_var,
                        cont_body: Rc::new(def_expr),
                        body: Rc::new(self.transform(value, &val_cont)),
                    }
                }
            }

            // Quasiquote template - needs runtime evaluation for unquote/unquote-splicing
            CoreExpr::Quasiquote(template) => {
                // Quasiquote is evaluated at runtime, so we pass the template
                // to the CPS evaluator which will process unquote/unquote-splicing
                CpsExpr::Quasiquote {
                    template: template.clone(),
                    cont: k.clone(),
                }
            }

            CoreExpr::Import { .. } => {
                // Import is a compile-time construct, shouldn't appear here
                panic!("Import should be handled before CPS transform")
            }

            CoreExpr::Parameterize { bindings, body } => {
                self.transform_parameterize(bindings, body, k)
            }

            CoreExpr::Expand { .. } => {
                // Expand is a debugging form, skip CPS transform
                panic!("Expand should be handled before CPS transform")
            }
        }
    }

    /// Check if an expression is trivial (no control effects)
    fn is_trivial(&self, expr: &CoreExpr) -> bool {
        matches!(
            expr,
            CoreExpr::Literal(_)
                | CoreExpr::Var { .. }
                | CoreExpr::Quote(_)
                | CoreExpr::Lambda { .. }
        )
    }

    /// Check if an expression is a call/cc reference
    fn is_callcc(&self, expr: &CoreExpr) -> bool {
        match expr {
            CoreExpr::Var { name, .. } => {
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
            CpsExpr::CallCC {
                proc: Rc::new(cps_proc),
                cont: k.clone(),
            }
        } else {
            // Proc is not trivial - need to evaluate it first
            let proc_var = self.gensym("proc");
            let proc_cont = self.gensym_cont();

            let callcc_expr = CpsExpr::CallCC {
                proc: Rc::new(CpsExpr::Var {
                    name: proc_var.clone(),
                    scopes: ScopeSet::new(),
                }),
                cont: k.clone(),
            };

            CpsExpr::LetCont {
                name: proc_cont.clone(),
                param: proc_var,
                cont_body: Rc::new(callcc_expr),
                body: Rc::new(self.transform(proc, &proc_cont)),
            }
        }
    }

    /// Transform a trivial expression (must be trivial!)
    fn transform_trivial(&self, expr: &CoreExpr) -> CpsExpr {
        match expr {
            CoreExpr::Literal(v) => CpsExpr::Literal(v.clone()),
            CoreExpr::Var { name, scopes } => CpsExpr::Var {
                name: name.clone(),
                scopes: scopes.clone(),
            },
            CoreExpr::Quote(v) => CpsExpr::Literal(v.clone()),
            CoreExpr::Lambda {
                params,
                body,
                binding_scope,
            } => {
                let cont_param = self.gensym_cont();
                let cps_body = self.transform_body(body, &cont_param);
                let (cps_params, cps_variadic) = self.transform_formals(params);

                CpsExpr::Lambda {
                    params: cps_params,
                    variadic: cps_variadic,
                    cont_param,
                    body: Rc::new(cps_body),
                    binding_scope: *binding_scope,
                }
            }
            _ => panic!(
                "transform_trivial called on non-trivial expression: {:?}",
                expr.kind()
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
            CpsExpr::Continue {
                cont: k.clone(),
                value: Rc::new(CpsExpr::Literal(Rc::new(Value::Unspecified))),
            }
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

            CpsExpr::LetCont {
                name: rest_cont.clone(),
                param: ignore_var,
                cont_body: Rc::new(rest_expr),
                body: Rc::new(self.transform(first, &rest_cont)),
            }
        }
    }

    /// Transform function application
    fn transform_app(&self, func: &CoreExpr, args: &[CoreExpr], k: &ContVar) -> CpsExpr {
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
        let func_var = CpsExpr::Var {
            name: names[0].clone(),
            scopes: ScopeSet::new(),
        };

        let arg_vars: Vec<CpsExpr> = names[1..]
            .iter()
            .map(|n| CpsExpr::Var {
                name: n.clone(),
                scopes: ScopeSet::new(),
            })
            .collect();

        let app = CpsExpr::App {
            func: Rc::new(func_var),
            args: arg_vars,
            cont: k.clone(),
        };

        // Build let-cont chain from inside out
        self.transform_args_then(&all_exprs, &names, app)
    }

    /// Transform apply
    fn transform_apply(&self, func: &CoreExpr, args: &[CoreExpr], k: &ContVar) -> CpsExpr {
        // Transform apply like app but generate CpsExpr::Apply
        // This preserves the "flatten last arg" semantics for the CPS evaluator

        // Generate names for all expressions
        let func_name = self.gensym_var();
        let arg_names: Vec<Symbol> = args.iter().map(|_| self.gensym_var()).collect();

        // The final apply expression
        let apply_expr = CpsExpr::Apply {
            func: Rc::new(CpsExpr::Var {
                name: func_name.clone(),
                scopes: ScopeSet::new(),
            }),
            args: arg_names
                .iter()
                .map(|n| CpsExpr::Var {
                    name: n.clone(),
                    scopes: ScopeSet::new(),
                })
                .collect(),
            cont: k.clone(),
        };

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

        // Work backwards to build nested let-conts
        let mut result = final_expr;

        for (expr, name) in exprs.iter().zip(names.iter()).rev() {
            if self.is_trivial(expr) {
                // Trivial - just let-bind it
                let cps_value = self.transform_trivial(expr);
                result = CpsExpr::LetVal {
                    name: name.clone(),
                    value: Rc::new(cps_value),
                    body: Rc::new(result),
                };
            } else {
                // Non-trivial - need continuation
                let cont_name = self.gensym_cont();
                let transformed = self.transform(expr, &cont_name);

                result = CpsExpr::LetCont {
                    name: cont_name,
                    param: name.clone(),
                    cont_body: Rc::new(result),
                    body: Rc::new(transformed),
                };
            }
        }

        result
    }

    /// Transform parameterize
    ///
    /// Transforms (parameterize ((p1 v1) (p2 v2) ...) body ...) into CPS.
    /// This generates a CpsExpr::Parameterize that the CPS evaluator handles
    /// by pushing values onto parameter stacks, evaluating body, then popping.
    fn transform_parameterize(
        &self,
        bindings: &[(CoreExpr, CoreExpr)],
        body: &[CoreExpr],
        k: &ContVar,
    ) -> CpsExpr {
        // Transform the body with the same continuation k
        // The body will eventually invoke k with its result
        let body_cps = self.transform_sequence(body, k);

        // Transform each binding pair (param expr, value expr)
        let mut cps_bindings = Vec::new();
        for (param_expr, value_expr) in bindings {
            // Both param and value should be transformed as trivial expressions
            // (they're evaluated before the body)
            let param_cps = self.transform_trivial(param_expr);
            let value_cps = self.transform_trivial(value_expr);
            cps_bindings.push((Rc::new(param_cps), Rc::new(value_cps)));
        }

        // Generate parameterize CPS expression
        // The evaluator will:
        // 1. Evaluate param and value expressions
        // 2. Push values onto parameter stacks
        // 3. Replace k in cont_env with cleanup continuation
        // 4. Evaluate body (which will eventually invoke k)
        // 5. Cleanup continuation pops values and invokes original k
        CpsExpr::Parameterize {
            bindings: cps_bindings,
            body: Rc::new(body_cps),
            cont: k.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_literal(n: i64) -> CoreExpr {
        CoreExpr::Literal(Rc::new(Value::Integer(n)))
    }

    fn make_var(name: &str) -> CoreExpr {
        CoreExpr::Var {
            name: name.into(),
            scopes: ScopeSet::new(),
        }
    }

    #[test]
    fn test_transform_literal() {
        let transformer = CpsTransformer::new();
        let expr = make_literal(42);
        let cps = transformer.transform_toplevel(&expr);

        // Should produce: (let-cont ((k_0 (v_1) (halt v_1))) (k_0 42))
        println!("CPS: {}", cps);
        assert!(matches!(cps, CpsExpr::LetCont { .. }));
    }

    #[test]
    fn test_transform_variable() {
        let transformer = CpsTransformer::new();
        let expr = make_var("x");
        let cps = transformer.transform_toplevel(&expr);

        println!("CPS: {}", cps);
        assert!(matches!(cps, CpsExpr::LetCont { .. }));
    }

    #[test]
    fn test_transform_if() {
        let transformer = CpsTransformer::new();

        // (if #t 1 2)
        let expr = CoreExpr::If {
            test: Rc::new(CoreExpr::Literal(Rc::new(Value::Boolean(true)))),
            then: Rc::new(make_literal(1)),
            else_: Rc::new(make_literal(2)),
        };

        let cps = transformer.transform_toplevel(&expr);
        println!("CPS: {}", cps);

        // Should have an if in the output
        fn has_if(e: &CpsExpr) -> bool {
            match e {
                CpsExpr::If { .. } => true,
                CpsExpr::LetCont {
                    cont_body, body, ..
                } => has_if(cont_body) || has_if(body),
                CpsExpr::LetVal { body, .. } => has_if(body),
                _ => false,
            }
        }
        assert!(has_if(&cps));
    }

    #[test]
    fn test_transform_lambda() {
        let transformer = CpsTransformer::new();

        // (lambda (x) x)
        let expr = CoreExpr::Lambda {
            params: Formals::Fixed(vec![patina_core::ScopedParam::simple("x".into())]),
            body: vec![make_var("x")],
            binding_scope: None,
        };

        let cps = transformer.transform_toplevel(&expr);
        println!("CPS: {}", cps);

        // Should produce a lambda with cont parameter
        fn has_lambda(e: &CpsExpr) -> bool {
            match e {
                CpsExpr::Lambda { .. } => true,
                CpsExpr::Continue { value, .. } => has_lambda(value),
                CpsExpr::LetCont {
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
        let expr = CoreExpr::App {
            func: Rc::new(make_var("f")),
            args: vec![make_var("x")],
        };

        let cps = transformer.transform_toplevel(&expr);
        println!("CPS: {}", cps);

        // Should have an application
        fn has_app(e: &CpsExpr) -> bool {
            match e {
                CpsExpr::App { .. } => true,
                CpsExpr::LetCont {
                    cont_body, body, ..
                } => has_app(cont_body) || has_app(body),
                CpsExpr::LetVal { body, .. } => has_app(body),
                _ => false,
            }
        }
        assert!(has_app(&cps));
    }

    #[test]
    fn test_transform_begin() {
        let transformer = CpsTransformer::new();

        // (begin 1 2 3)
        let expr = CoreExpr::Begin(vec![make_literal(1), make_literal(2), make_literal(3)]);

        let cps = transformer.transform_toplevel(&expr);
        println!("CPS: {}", cps);

        // Should sequence the expressions
        assert!(matches!(cps, CpsExpr::LetCont { .. }));
    }

    #[test]
    fn test_transform_callcc() {
        let transformer = CpsTransformer::new();

        // (call/cc (lambda (k) 42))
        // This tests that call/cc is properly transformed
        // call/cc is represented as (call/cc proc), i.e., an App with call/cc as the function
        let lambda_body = make_literal(42);
        let lambda = CoreExpr::Lambda {
            params: Formals::Fixed(vec![patina_core::ScopedParam::simple("k".into())]),
            body: vec![lambda_body],
            binding_scope: None,
        };

        let expr = CoreExpr::App {
            func: Rc::new(make_var("call/cc")),
            args: vec![lambda],
        };

        let cps = transformer.transform_toplevel(&expr);
        println!("CPS for call/cc: {}", cps);

        // Should produce a CpsExpr::CallCC
        fn has_callcc(e: &CpsExpr) -> bool {
            match e {
                CpsExpr::CallCC { .. } => true,
                CpsExpr::LetCont {
                    cont_body, body, ..
                } => has_callcc(cont_body) || has_callcc(body),
                CpsExpr::LetVal { body, .. } => has_callcc(body),
                _ => false,
            }
        }
        assert!(has_callcc(&cps), "CPS should contain CallCC node");
    }

    #[test]
    fn test_transform_callcc_with_escape() {
        let transformer = CpsTransformer::new();

        // (call/cc (lambda (exit) (exit 99)))
        // The continuation 'exit' is invoked, which should escape
        let exit_call = CoreExpr::App {
            func: Rc::new(make_var("exit")),
            args: vec![make_literal(99)],
        };
        let lambda = CoreExpr::Lambda {
            params: Formals::Fixed(vec![patina_core::ScopedParam::simple("exit".into())]),
            body: vec![exit_call],
            binding_scope: None,
        };

        let expr = CoreExpr::App {
            func: Rc::new(make_var("call/cc")),
            args: vec![lambda],
        };

        let cps = transformer.transform_toplevel(&expr);
        println!("CPS for call/cc with escape: {}", cps);

        // The transformation should be valid (no panic)
        // The structure should include CallCC
        fn has_callcc(e: &CpsExpr) -> bool {
            match e {
                CpsExpr::CallCC { .. } => true,
                CpsExpr::LetCont {
                    cont_body, body, ..
                } => has_callcc(cont_body) || has_callcc(body),
                CpsExpr::LetVal { body, .. } => has_callcc(body),
                _ => false,
            }
        }
        assert!(has_callcc(&cps));
    }

    #[test]
    fn test_transform_nested_callcc() {
        let transformer = CpsTransformer::new();

        // (+ 1 (call/cc (lambda (k) (k 2))))
        // This tests call/cc in a non-tail position
        let k_call = CoreExpr::App {
            func: Rc::new(make_var("k")),
            args: vec![make_literal(2)],
        };
        let lambda = CoreExpr::Lambda {
            params: Formals::Fixed(vec![patina_core::ScopedParam::simple("k".into())]),
            body: vec![k_call],
            binding_scope: None,
        };
        let callcc = CoreExpr::App {
            func: Rc::new(make_var("call/cc")),
            args: vec![lambda],
        };

        // (+ 1 <callcc>) - represented as App with + variable
        let add = CoreExpr::App {
            func: Rc::new(make_var("+")),
            args: vec![make_literal(1), callcc],
        };

        let cps = transformer.transform_toplevel(&add);
        println!("CPS for nested call/cc: {}", cps);

        // Should produce valid CPS with CallCC
        fn has_callcc(e: &CpsExpr) -> bool {
            match e {
                CpsExpr::CallCC { .. } => true,
                CpsExpr::LetCont {
                    cont_body, body, ..
                } => has_callcc(cont_body) || has_callcc(body),
                CpsExpr::LetVal { body, .. } => has_callcc(body),
                CpsExpr::PrimOp { .. } => false,
                _ => false,
            }
        }
        assert!(has_callcc(&cps));
    }
}
