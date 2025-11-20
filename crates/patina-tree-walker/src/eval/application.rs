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
            result.push(self.eval_in_env(&pair.0, env)?);
            current = pair.1.clone();
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
                // Check arity
                if variadic.is_some() {
                    // Variadic: need at least as many args as fixed params
                    if args.len() < params.len() {
                        return Err(EvalError::WrongArity {
                            expected: format!("at least {}", params.len()),
                            actual: args.len(),
                        });
                    }
                } else {
                    // Fixed arity: need exact number of args
                    if args.len() != params.len() {
                        return Err(EvalError::WrongArity {
                            expected: params.len().to_string(),
                            actual: args.len(),
                        });
                    }
                }

                // Create new environment with lambda's captured environment as parent
                let new_env = Rc::new(Environment::with_parent(env));

                // Bind fixed parameters
                for (param, arg) in params.iter().zip(args.iter()) {
                    new_env.define(param.clone(), arg.clone());
                }

                // Bind rest parameter if variadic
                if let Some(rest_param) = variadic {
                    // Collect remaining args into a list
                    let rest_args: Vec<Value> = args.into_iter().skip(params.len()).collect();
                    let rest_list = self.list_from_vec(rest_args);
                    new_env.define(rest_param, rest_list);
                }

                // Evaluate body expressions in sequence
                // If we're in tail position, the last expression of the lambda body
                // should be returned as a TailCall
                if in_tail_position && !body.is_empty() {
                    // Evaluate all but the last expression
                    for expr in &body[..body.len() - 1] {
                        self.eval_in_env(expr, &new_env)?;
                    }
                    // Last expression is in tail position
                    Ok(super::EvalResult::TailCall {
                        expr: body.last().unwrap().clone(),
                        env: new_env,
                    })
                } else {
                    // Not in tail position or empty body
                    let mut result = Value::Unspecified;
                    for expr in body {
                        result = self.eval_in_env(&expr, &new_env)?;
                    }
                    Ok(super::EvalResult::Value(result))
                }
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
                        // Found a matching clause - bind arguments and evaluate body
                        let new_env = Rc::new(Environment::with_parent(env.clone()));

                        // Bind fixed parameters
                        for (param, arg) in params.iter().zip(args.iter()) {
                            new_env.define(param.clone(), arg.clone());
                        }

                        // Bind rest parameter if variadic
                        if let Some(rest_param) = variadic {
                            // Collect remaining args into a list
                            let rest_args: Vec<Value> =
                                args.into_iter().skip(params.len()).collect();
                            let rest_list = self.list_from_vec(rest_args);
                            new_env.define(rest_param.clone(), rest_list);
                        }

                        // Evaluate body expressions in sequence
                        // If we're in tail position, the last expression of the lambda body
                        // should be returned as a TailCall
                        if in_tail_position && !body.is_empty() {
                            // Evaluate all but the last expression
                            for expr in &body[..body.len() - 1] {
                                self.eval_in_env(expr, &new_env)?;
                            }
                            // Last expression is in tail position
                            return Ok(super::EvalResult::TailCall {
                                expr: body.last().unwrap().clone(),
                                env: new_env,
                            });
                        } else {
                            // Not in tail position or empty body
                            let mut result = Value::Unspecified;
                            for expr in body {
                                result = self.eval_in_env(&expr, &new_env)?;
                            }
                            return Ok(super::EvalResult::Value(result));
                        }
                    }
                }

                // No matching clause found
                Err(EvalError::WrongArity {
                    expected: format!("case-lambda: no clause matches {} arguments", args.len()),
                    actual: args.len(),
                })
            }
            Value::Parameter { value, converter } => {
                // Parameters can be called with 0 or 1 arguments:
                // (param)      => get current value
                // (param val)  => set value (after applying converter)
                match args.len() {
                    0 => {
                        // Get current value
                        Ok(super::EvalResult::Value(value.borrow().clone()))
                    }
                    1 => {
                        // Set value (applying converter if present)
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

                        // Set the new value
                        *value.borrow_mut() = new_val;
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
