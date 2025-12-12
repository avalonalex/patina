//! Procedure application logic
//!
//! This module handles the application of procedures to arguments, including:
//! - `apply()` - Apply a procedure to evaluated arguments
//! - `check_arity()` - Verify argument count matches procedure arity

use patina_runtime::environment::Environment;
use patina_runtime::value::{Arity, Procedure, Value};
use std::rc::Rc;

use super::Evaluator;
use super::error::EvalError;

impl Evaluator {
    /// Evaluate a list of already macro-expanded argument expressions
    ///
    /// Uses `eval_expanded()` to avoid re-expansion.
    pub(super) fn eval_arguments_expanded(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Vec<Value>, EvalError> {
        let mut result = Vec::new();
        let mut current = args.clone();

        while let Value::Pair(pair) = current {
            let borrowed = pair.borrow();
            result.push(self.eval_expanded(&borrowed.0, env)?);
            current = borrowed.1.clone();
        }

        Ok(result)
    }

    /// Apply a procedure to a vector of evaluated arguments
    ///
    /// The `in_tail_position` parameter enables tail call optimization for primitives.
    /// When true, primitives like call-with-values can return TailCallPrimitive to
    /// participate in the trampoline loop.
    pub(super) fn apply(
        &self,
        proc: Value,
        args: Vec<Value>,
        in_tail_position: bool,
    ) -> Result<super::EvalResult, EvalError> {
        // Debug trace entry
        if self.debug.is_enabled(super::debug::DebugStage::Apply) {
            let args_str = args
                .iter()
                .map(|v| format!("{}", v))
                .collect::<Vec<_>>()
                .join(" ");
            tracing::debug!(
                target: "patina_tree_walker::apply",
                indent = %self.debug.current_indent(),
                proc = %proc,
                args = %args_str,
                "Applying"
            );
            self.debug.indent();
        }

        let result = match proc {
            Value::Procedure(ref proc_box) => match proc_box.as_ref() {
                Procedure::Primitive { arity, .. } => {
                    self.check_arity(arity, args.len())?;
                    self.apply_primitive(proc_box.as_ref(), args, in_tail_position)
                }
                Procedure::Lambda {
                    params,
                    variadic,
                    body,
                    env,
                    binding_scope,
                } => {
                    // Use shared helper methods to prepare environment and evaluate body
                    let new_env =
                        self.prepare_lambda_env(params, variadic, args, env, *binding_scope)?;
                    self.eval_lambda_body(body, &new_env, in_tail_position)
                }
                Procedure::CpsLambda { .. } => {
                    // CPS lambdas invoked from direct mode: use the CPS evaluator
                    // to apply the procedure with a halt continuation.
                    use crate::eval::cps_eval::CpsEvaluator;

                    let cps_eval = CpsEvaluator::new(self);
                    let result = cps_eval.apply_from_direct(
                        Value::Procedure(proc_box.clone()),
                        args,
                    )?;

                    Ok(super::EvalResult::Value(result))
                }
            },
            Value::Continuation(_) => {
                // Continuations can only be invoked from within CPS mode
                // In direct mode, we can't properly invoke them
                return Err(EvalError::InternalError(
                    "Continuations can only be invoked in CPS evaluation mode. \
                     This continuation was captured in CPS mode but is being invoked in direct mode."
                        .to_string(),
                ));
            }
            Value::Parameter { values, converter } => {
                // Parameters can be called with 0 or 1 arguments:
                // (param)      => get current value (top of stack)
                // (param val)  => set value (replace top of stack after applying converter)
                match args.len() {
                    0 => {
                        // Get current value (top of stack)
                        let stack = values.borrow();
                        let current_value = stack.last().ok_or_else(|| {
                            EvalError::InvalidSyntax("parameter stack is empty".to_string())
                        })?;
                        Ok(super::EvalResult::Value(current_value.clone()))
                    }
                    1 => {
                        // Set value (replace top of stack after applying converter)
                        let new_val = if let Some(conv) = converter {
                            // Apply converter to new value
                            let result = self.apply(
                                *conv.clone(),
                                vec![args[0].clone()],
                                false, // converter call is never in tail position
                            )?;
                            match result {
                                super::EvalResult::Value(v) => v,
                                _ => {
                                    return Err(EvalError::InvalidSyntax(
                                        "parameter converter returned non-value".to_string(),
                                    ));
                                }
                            }
                        } else {
                            args[0].clone()
                        };

                        // Set the new value (replace top of stack)
                        let mut stack = values.borrow_mut();
                        if let Some(last) = stack.last_mut() {
                            *last = new_val;
                        } else {
                            return Err(EvalError::InvalidSyntax(
                                "parameter stack is empty".to_string(),
                            ));
                        }
                        Ok(super::EvalResult::Value(Value::Unspecified))
                    }
                    _ => Err(EvalError::WrongArity {
                        expected: "0 or 1".to_string(),
                        actual: args.len(),
                    }),
                }
            }
            _ => Err(EvalError::NotAProcedure(format!("{}", proc))),
        };

        // Debug trace exit
        if self.debug.is_enabled(super::debug::DebugStage::Apply) {
            self.debug.dedent();
            match &result {
                Ok(super::EvalResult::Value(val)) => {
                    tracing::debug!(
                        target: "patina_tree_walker::apply",
                        indent = %self.debug.current_indent(),
                        result = %val,
                        "=> value"
                    );
                }
                Ok(super::EvalResult::TailCall { expr, .. }) => {
                    tracing::debug!(
                        target: "patina_tree_walker::apply",
                        indent = %self.debug.current_indent(),
                        expr = %expr,
                        "=> tail call"
                    );
                }
                Ok(super::EvalResult::TailCallPrimitive { proc, args }) => {
                    let args_str = args
                        .iter()
                        .map(|v| format!("{}", v))
                        .collect::<Vec<_>>()
                        .join(" ");
                    tracing::debug!(
                        target: "patina_tree_walker::apply",
                        indent = %self.debug.current_indent(),
                        proc = %proc,
                        args = %args_str,
                        "=> tail call primitive"
                    );
                }
                Err(e) => {
                    tracing::debug!(
                        target: "patina_tree_walker::apply",
                        indent = %self.debug.current_indent(),
                        error = %e,
                        "=> error"
                    );
                }
            }
        }

        result
    }

    /// Check if the actual argument count matches the expected arity
    pub(super) fn check_arity(&self, arity: &Arity, actual: usize) -> Result<(), EvalError> {
        match arity {
            Arity::Exact(n) if *n != actual => Err(EvalError::WrongArity {
                expected: n.to_string(),
                actual,
            }),
            Arity::Min(n) if actual < *n => Err(EvalError::WrongArity {
                expected: format!("at least {}", n),
                actual,
            }),
            Arity::Range(min, max) if actual < *min || actual > *max => {
                Err(EvalError::WrongArity {
                    expected: format!("{}-{}", min, max),
                    actual,
                })
            }
            _ => Ok(()),
        }
    }
}
