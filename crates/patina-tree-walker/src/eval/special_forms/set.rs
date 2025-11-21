//! Set! special form implementation
//!
//! The `set!` special form mutates existing bindings.
//!
//! # Syntax
//!
//! ```scheme
//! (set! name value)
//! ```
//!
//! # Semantics
//!
//! - Evaluates the value expression
//! - Assigns the result to the existing variable
//! - The variable must already be defined
//! - Returns an unspecified value
//! - Raises an error if the variable is undefined
//!
//! # Examples
//!
//! ```scheme
//! (define x 10)
//! (set! x 20)    ; x is now 20
//! (set! y 10)    ; error: y is undefined
//! ```
//!
//! # Difference from Define
//!
//! - `define` creates a new binding (or replaces if already exists)
//! - `set!` only mutates existing bindings (error if undefined)

use super::super::{EvalError, EvalResult, Evaluator};
use super::SpecialForm;
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// Set! special form
///
/// Mutates existing variable bindings.
pub struct SetForm;

impl SpecialForm for SetForm {
    fn name(&self) -> &'static str {
        "set!"
    }

    fn help(&self) -> &'static str {
        "(set! name value) assigns value to an existing variable.\n\
         The variable must already be defined.\n\
         Example: (set! x 20)"
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        _in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        let (var, rest) = evaluator.extract_pair(args)?;
        let (value_expr, rest) = evaluator.extract_pair(&rest)?;

        if !matches!(rest, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "set! expects 2 arguments".to_string(),
            ));
        }

        if let Value::Symbol(name) = var {
            let value = evaluator.eval_in_env(&value_expr, env)?;
            env.set(&name, value)
                .map_err(EvalError::UndefinedVariable)?;
            Ok(EvalResult::Value(Value::Unspecified))
        } else {
            Err(EvalError::InvalidSyntax(
                "set! expects a symbol".to_string(),
            ))
        }
    }

    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
        // Check that we have exactly 2 arguments
        match args {
            Value::Pair(pair1) => {
                let pair1_ref = pair1.borrow();
                // First argument must be a symbol
                if !matches!(pair1_ref.0, Value::Symbol(_)) {
                    return Err(EvalError::InvalidSyntax(
                        "set! expects a symbol".to_string(),
                    ));
                }

                // Second argument exists
                match &pair1_ref.1 {
                    Value::Pair(pair2) => {
                        if matches!(pair2.borrow().1, Value::Null) {
                            Ok(())
                        } else {
                            Err(EvalError::InvalidSyntax(
                                "set! expects 2 arguments".to_string(),
                            ))
                        }
                    }
                    _ => Err(EvalError::InvalidSyntax(
                        "set! expects 2 arguments".to_string(),
                    )),
                }
            }
            _ => Err(EvalError::InvalidSyntax(
                "set! expects 2 arguments".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_form_name() {
        let form = SetForm;
        assert_eq!(form.name(), "set!");
    }

    #[test]
    fn test_set_form_help() {
        let form = SetForm;
        assert!(form.help().contains("set!"));
        assert!(form.help().contains("assigns"));
        assert!(!form.help().is_empty());
    }
}
