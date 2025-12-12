//! CPS Evaluator - Evaluates CPS (Continuation-Passing Style) expressions
//!
//! This module implements evaluation of CpsExpr, which is the IR used for
//! implementing first-class continuations (call/cc) and delimited continuations
//! (shift/reset, prompt/control).
//!
//! # Architecture
//!
//! ```text
//! CoreExpr → [CPS Transform] → CpsExpr → [THIS MODULE] → Value
//! ```
//!
//! The CPS evaluator handles the following expression forms:
//!
//! **Trivial expressions** (evaluate immediately):
//! - `Literal` - Self-evaluating values
//! - `Var` - Variable references
//! - `ContRef` - Continuation variable references
//! - `Lambda` - CPS lambda (takes continuation parameter)
//!
//! **Serious expressions** (use continuations):
//! - `LetVal` - Bind trivial value and continue
//! - `LetCont` - Define local continuation
//! - `App` - Application with explicit continuation
//! - `Continue` - Invoke continuation with value
//! - `If` - Conditional
//! - `Set` - Mutation
//! - `Define` - Definition
//!
//! **Control operators**:
//! - `CallCC` - Capture current continuation
//! - `Prompt` - Establish delimiter for shift/reset
//! - `Control` - Capture delimited continuation
//! - `Abort` - Abort to prompt
//!
//! - `PrimOp` - Primitive operations
//! - `Halt` - Program termination
//!
//! # Continuation Representation
//!
//! Continuations are represented as `Value::Continuation(CpsContinuation)` which
//! stores the continuation body, parameter name, captured environment, and
//! dynamic-wind state.
//!
//! # Trampoline Architecture
//!
//! The evaluator uses a fully-trampolined design to avoid Rust stack overflow:
//! - All evaluation returns a `StepResult` enum indicating either a final value
//!   or the next expression to evaluate
//! - A single loop in `eval()` processes steps iteratively
//! - No recursive Rust calls during normal CPS evaluation
//!
//! # Module Organization
//!
//! - `mod.rs` - CpsEvaluator struct, entry points, trampoline loop
//! - `types.rs` - ContValue, StepResult, PromptFrame, ExceptionHandler
//! - `environment.rs` - Variable lookup, closure creation
//! - `step.rs` - eval_one_step (main dispatch)
//! - `application.rs` - apply_cps_step (procedure application)
//! - `continuation.rs` - Continuation capture/restore, reification
//! - `wind.rs` - Dynamic-wind handling, promise forcing
//! - `exceptions.rs` - Exception routing through CPS handlers
//! - `quasiquote.rs` - Quasiquote template evaluation

mod application;
mod continuation;
mod environment;
mod exceptions;
pub mod quasiquote;
mod step;
mod types;
mod wind;

use crate::eval::error::EvalError;
use patina_core::Environment;
use patina_core::cps_expr::CpsExpr;
use patina_core::value::Value;
use std::collections::HashMap;
use std::rc::Rc;

use types::{ContValue, StepResult, take_pending_escape};

/// CPS Evaluator state
///
/// Manages the evaluation of CpsExpr with support for:
/// - Continuation environment (mapping ContVar to ContValue)
/// - Prompt stack for delimited continuations
/// - Dynamic-wind state
pub struct CpsEvaluator<'a> {
    /// The tree-walker evaluator for primitive operations
    evaluator: &'a super::Evaluator,
}

impl<'a> CpsEvaluator<'a> {
    /// Create a new CPS evaluator
    pub fn new(evaluator: &'a super::Evaluator) -> Self {
        Self { evaluator }
    }

    // apply_from_direct is defined in wind.rs

    /// Evaluate a CPS expression to a final value
    ///
    /// This is the main entry point for CPS evaluation. The expression
    /// should be produced by CpsTransformer::transform_toplevel() which
    /// wraps the result in a Halt continuation.
    ///
    /// Uses a trampoline pattern to avoid Rust stack overflow on deep recursion.
    /// All CPS evaluation steps are processed iteratively in a single loop.
    pub fn eval(&self, expr: &CpsExpr) -> Result<Value, EvalError> {
        self.eval_in_env(expr, self.evaluator.global_env.clone())
    }

    /// Evaluate a CPS expression in a specific environment
    ///
    /// Like `eval`, but allows specifying the environment to use.
    /// This is needed for library loading where definitions should
    /// go into the library's environment, not the global environment.
    pub fn eval_in_env(&self, expr: &CpsExpr, env: Rc<Environment>) -> Result<Value, EvalError> {
        let cont_env = HashMap::new();
        let prompt_stack = Vec::new();
        let dynamic_winds = Vec::new();
        let exception_handlers = Vec::new();

        // Debug: show the CPS expression
        let debug = std::env::var("CPS_DEBUG").is_ok();
        if debug {
            eprintln!("[CPS] Input expr: {}", expr);
        }

        // Start with the initial expression
        let mut current_step = match self.eval_one_step(
            expr,
            env,
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        ) {
            Ok(step) => step,
            Err(e) => {
                if debug {
                    eprintln!("[CPS] Error in initial eval_one_step: {}", e);
                }
                return Err(e);
            }
        };

        let mut step_count = 0;

        // Trampoline loop - process steps until we get a final value
        loop {
            step_count += 1;
            if debug && step_count <= 30 {
                eprintln!(
                    "[CPS] Step {}: {:?}",
                    step_count,
                    std::mem::discriminant(&current_step)
                );
            }
            // Process step, catching ContinuationEscape to handle escaped continuations
            let step_result = match current_step {
                StepResult::Done(value) => {
                    if debug {
                        eprintln!("[CPS] Done after {} steps: {}", step_count, value);
                    }
                    return Ok(value);
                }

                StepResult::Continue {
                    expr,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                } => self.eval_one_step(
                    &expr,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                ),

                StepResult::InvokeContinuation {
                    cont,
                    value,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                } => self.invoke_continuation_step(
                    cont,
                    value,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                ),

                StepResult::ApplyProc {
                    proc,
                    args,
                    cont,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                } => self.apply_cps_step(
                    proc,
                    args,
                    cont,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                ),
            };

            // Handle result, catching ContinuationEscape
            match step_result {
                Ok(step) => current_step = step,
                Err(EvalError::ContinuationEscape) => {
                    // A continuation escaped from apply_from_direct
                    if let Some((value, k)) = take_pending_escape() {
                        if debug {
                            eprintln!("[CPS] Continuation escape with value: {}", value);
                        }

                        // Check if this is a special DynamicWindCleanup continuation
                        let is_dw_cleanup = matches!(
                            k.body.as_ref(),
                            CpsExpr::Halt(inner) if matches!(
                                inner.as_ref(),
                                CpsExpr::Literal(v) if matches!(v.as_ref(), Value::Symbol(s) if s.as_ref() == "__dynamic_wind_cleanup__")
                            )
                        );

                        if is_dw_cleanup {
                            // Reconstruct the DynamicWindCleanup continuation
                            let new_cont = self.restore_dynamic_wind_cleanup(&k)?;
                            let restored_cont_env =
                                self.restore_cont_bindings(&k.captured_cont_bindings);
                            current_step = self.invoke_continuation_step(
                                new_cont,
                                value,
                                k.env.clone(),
                                restored_cont_env,
                                Vec::new(),
                                k.dynamic_winds.clone(),
                                Vec::new(),
                            )?;
                        } else {
                            // Normal continuation - resume with the escaped continuation
                            let restored_cont_env =
                                self.restore_cont_bindings(&k.captured_cont_bindings);
                            let new_cont = ContValue::Local {
                                param: k.param.clone(),
                                body: k.body.clone(),
                                env: k.env.clone(),
                                cont_env: restored_cont_env.clone(),
                            };
                            current_step = self.invoke_continuation_step(
                                new_cont,
                                value,
                                k.env.clone(),
                                restored_cont_env,
                                Vec::new(),
                                k.dynamic_winds.clone(),
                                Vec::new(),
                            )?;
                        }
                    } else {
                        return Err(EvalError::InternalError(
                            "ContinuationEscape without pending data".to_string(),
                        ));
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Evaluate a CoreExpr using CPS transformation
///
/// This is the main entry point for CPS-based evaluation. It:
/// 1. Transforms CoreExpr to CpsExpr using CpsTransformer
/// 2. Evaluates the CpsExpr using CpsEvaluator
///
/// This function should be used when call/cc or delimited continuations
/// are needed. For regular code, use `eval_core` instead for better performance.
///
/// # Arguments
/// * `expr` - The CoreExpr to evaluate
/// * `env` - The environment for variable lookup and definitions
/// * `evaluator` - The evaluator instance (wrapped in Rc for sharing)
///
/// # Returns
/// The result of evaluating the expression
pub fn eval_cps(
    expr: &patina_core::CoreExpr,
    env: Rc<Environment>,
    evaluator: &super::Evaluator,
) -> Result<Value, EvalError> {
    use patina_core::CoreExpr;
    use patina_ir::CpsTransformer;

    // Handle Import specially - it's a side-effect that modifies the environment
    // and doesn't need CPS transformation
    if let CoreExpr::Import { import_sets } = expr {
        for import_set_expr in import_sets {
            let import_set = patina_frontend::LibraryDefinition::parse_import_set(import_set_expr)
                .map_err(|e| EvalError::InvalidSyntax(format!("Invalid import set: {}", e)))?;
            evaluator.process_import_for_eval(&import_set, &env)?;
        }
        return Ok(Value::Unspecified);
    }

    // Transform CoreExpr to CpsExpr
    let transformer = CpsTransformer::new();
    let cps_expr = transformer.transform_toplevel(expr);

    // Create CPS evaluator and evaluate in the specified environment
    let cps_evaluator = CpsEvaluator::new(evaluator);
    cps_evaluator.eval_in_env(&cps_expr, env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_core::ScopeSet;
    use patina_core::cps_expr::CpsPrimitive;

    fn make_test_evaluator() -> super::super::Evaluator {
        super::super::Evaluator::new()
    }

    /// Helper to check if two Values are equal (since Value doesn't impl PartialEq)
    fn values_equal(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Integer(x), Value::Integer(y)) => x == y,
            (Value::Boolean(x), Value::Boolean(y)) => x == y,
            _ => false,
        }
    }

    #[test]
    fn test_cps_eval_literal() {
        let evaluator = make_test_evaluator();
        let cps_eval = CpsEvaluator::new(&evaluator);

        // CPS: (halt 42)
        let expr = CpsExpr::Halt(Rc::new(CpsExpr::Literal(Rc::new(Value::Integer(42)))));
        let result = cps_eval.eval(&expr).unwrap();
        assert!(values_equal(&result, &Value::Integer(42)));
    }

    #[test]
    fn test_cps_eval_variable() {
        let evaluator = make_test_evaluator();
        evaluator
            .global_env
            .define("x".to_string(), Value::Integer(10));
        let cps_eval = CpsEvaluator::new(&evaluator);

        // CPS: (halt x)
        let expr = CpsExpr::Halt(Rc::new(CpsExpr::Var {
            name: Rc::from("x"),
            scopes: ScopeSet::new(),
        }));
        let result = cps_eval.eval(&expr).unwrap();
        assert!(values_equal(&result, &Value::Integer(10)));
    }

    #[test]
    fn test_cps_eval_primop() {
        let evaluator = make_test_evaluator();
        let cps_eval = CpsEvaluator::new(&evaluator);

        // CPS: (let-cont ((k result) (halt result)) (+ 1 2 k))
        let halt_cont = CpsExpr::LetCont {
            name: Rc::from("k"),
            param: Rc::from("result"),
            cont_body: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Var {
                name: Rc::from("result"),
                scopes: ScopeSet::new(),
            }))),
            body: Rc::new(CpsExpr::PrimOp {
                op: CpsPrimitive::Add,
                args: vec![
                    CpsExpr::Literal(Rc::new(Value::Integer(1))),
                    CpsExpr::Literal(Rc::new(Value::Integer(2))),
                ],
                cont: Rc::from("k"),
            }),
        };

        let result = cps_eval.eval(&halt_cont).unwrap();
        assert!(values_equal(&result, &Value::Integer(3)));
    }
}
