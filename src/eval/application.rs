//! Procedure application logic
//!
//! This module handles the application of procedures to arguments, including:
//! - `apply()` - Apply a procedure to evaluated arguments
//! - `eval_arguments()` - Evaluate a list of argument expressions
//! - `check_arity()` - Verify argument count matches procedure arity

use crate::env::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

use super::error::EvalError;
use super::Evaluator;

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
    pub(super) fn apply(&self, proc: Value, args: Vec<Value>) -> Result<Value, EvalError> {
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
            Value::Procedure(Procedure::Primitive { name, arity }) => {
                self.check_arity(&arity, args.len())?;
                self.apply_primitive(name, args)
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
                let mut result = Value::Unspecified;
                for expr in body {
                    result = self.eval_in_env(&expr, &new_env)?;
                }

                Ok(result)
            }
            _ => Err(EvalError::NotAProcedure(format!("{}", proc))),
        };

        // Debug trace exit
        if self.debug.is_enabled(super::debug::DebugStage::Apply) {
            self.debug.dedent();
            match &result {
                Ok(val) => eprintln!("[APPLY]{} => {}", self.debug.current_indent(), val),
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
