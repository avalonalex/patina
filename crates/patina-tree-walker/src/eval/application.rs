//! Procedure application logic
//!
//! This module handles the application of procedures to arguments, including:
//! - `apply()` - Apply a procedure to evaluated arguments
//! - `eval_arguments()` - Evaluate a list of argument expressions
//! - `check_arity()` - Verify argument count matches procedure arity

use patina_runtime::environment::Environment;
use patina_runtime::value::{Arity, Procedure, Value};
use std::rc::Rc;

use super::Evaluator;
use super::error::EvalError;

impl Evaluator {
    /// Evaluate a list of argument expressions
    pub(super) fn eval_arguments(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Vec<Value>, EvalError> {
        let mut result = Vec::new();
        let mut current = args.clone();

        while let Value::Pair(pair) = current {
            let borrowed = pair.borrow();
            result.push(self.eval_in_env(&borrowed.0, env)?);
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
            eprintln!(
                "[APPLY]{} Applying: {} to ({})",
                self.debug.current_indent(),
                proc,
                args_str
            );
            self.debug.indent();
        }

        let result = match proc {
            Value::Procedure(ref procedure @ Procedure::Primitive { ref arity, .. }) => {
                self.check_arity(arity, args.len())?;
                self.apply_primitive(procedure, args, in_tail_position)
            }
            Value::Procedure(Procedure::Lambda {
                params,
                variadic,
                body,
                env,
            }) => {
                // Use shared helper methods to prepare environment and evaluate body
                let new_env = self.prepare_lambda_env(&params, &variadic, args, &env)?;
                self.eval_lambda_body(&body, &new_env, in_tail_position)
            }
            Value::Procedure(Procedure::CaseLambda { clauses, env }) => {
                // Try each clause in order to find one that matches the argument count
                for (params, variadic, body) in clauses {
                    let matches = if variadic.is_some() {
                        // Variadic clause: need at least as many args as fixed params
                        args.len() >= params.len()
                    } else {
                        // Fixed arity clause: need exact number of args
                        args.len() == params.len()
                    };

                    if matches {
                        // Found a matching clause - use shared helper methods
                        let new_env = self.prepare_lambda_env(&params, &variadic, args, &env)?;
                        return self.eval_lambda_body(&body, &new_env, in_tail_position);
                    }
                }

                // No matching clause found
                Err(EvalError::WrongArity {
                    expected: format!("case-lambda: no clause matches {} arguments", args.len()),
                    actual: args.len(),
                })
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
                    eprintln!("[APPLY]{} => {}", self.debug.current_indent(), val)
                }
                Ok(super::EvalResult::TailCall { expr, .. }) => {
                    eprintln!(
                        "[APPLY]{} => TAIL CALL: {}",
                        self.debug.current_indent(),
                        expr
                    )
                }
                Ok(super::EvalResult::TailCallPrimitive { proc, args }) => {
                    let args_str = args
                        .iter()
                        .map(|v| format!("{}", v))
                        .collect::<Vec<_>>()
                        .join(" ");
                    eprintln!(
                        "[APPLY]{} => TAIL CALL PRIMITIVE: {} ({})",
                        self.debug.current_indent(),
                        proc,
                        args_str
                    )
                }
                Err(e) => eprintln!("[APPLY]{} => ERROR: {}", self.debug.current_indent(), e),
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
