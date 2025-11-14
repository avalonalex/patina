//! Lambda special form implementation
//!
//! The `lambda` special form creates anonymous procedures (functions).
//!
//! # Syntax
//!
//! ```scheme
//! (lambda params body ...)
//! ```
//!
//! # Parameter Forms
//!
//! ## Fixed Parameters
//! ```scheme
//! (lambda (x y z) (+ x y z))
//! ```
//!
//! ## Variadic (Rest Parameters)
//! ```scheme
//! (lambda args args)              ; all args go to 'args' list
//! (lambda (x y . rest) ...)       ; x, y are required, rest is a list
//! ```
//!
//! ## No Parameters
//! ```scheme
//! (lambda () 42)
//! ```
//!
//! # Semantics
//!
//! - Does NOT evaluate the body expressions
//! - Captures the current environment (lexical closure)
//! - Returns a procedure value
//! - Body is evaluated when the procedure is called
//! - Last body expression is in tail position
//!
//! # Closures
//!
//! Lambda creates lexical closures that capture their defining environment:
//!
//! ```scheme
//! (define make-counter
//!   (lambda (n)
//!     (lambda () (set! n (+ n 1)) n)))
//!
//! (define counter (make-counter 0))
//! (counter)  ; => 1
//! (counter)  ; => 2
//! ```
//!
//! # Examples
//!
//! ```scheme
//! ((lambda (x) (* x x)) 5)                    ; => 25
//! ((lambda (x y) (+ x y)) 3 4)                ; => 7
//! ((lambda args (apply + args)) 1 2 3)        ; => 6
//! ((lambda (op . args) (apply op args)) + 1 2 3)  ; => 6
//! ```

use super::super::{EvalError, EvalResult, Evaluator};
use super::SpecialForm;
use patina_runtime::{Environment, Procedure, Value};
use std::rc::Rc;

/// Lambda special form
///
/// Creates anonymous procedures with lexical closures.
pub struct LambdaForm;

impl SpecialForm for LambdaForm {
    fn name(&self) -> &'static str {
        "lambda"
    }

    fn help(&self) -> &'static str {
        "(lambda params body ...) creates an anonymous procedure.\n\
         Parameters can be:\n\
         - (x y z) - fixed parameters\n\
         - args - single variadic parameter\n\
         - (x y . rest) - fixed + variadic\n\
         Example: (lambda (x) (* x x))\n\
         Example: (lambda (x . rest) (cons x rest))"
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        _in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        let (params_expr, rest) = evaluator.extract_pair(args)?;

        // Parse parameters
        let (params, variadic) = evaluator.parse_lambda_params(&params_expr)?;

        // Collect body expressions
        let body = evaluator.collect_list_items(&rest)?;

        if body.is_empty() {
            return Err(EvalError::InvalidSyntax(
                "lambda requires at least one body expression".to_string(),
            ));
        }

        Ok(EvalResult::Value(Value::Procedure(Procedure::Lambda {
            params,
            variadic,
            body,
            env: env.clone(),
        })))
    }

    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
        match args {
            Value::Pair(pair) => {
                // First argument is parameters (can be symbol, null, or list)
                // Second and beyond are body expressions (must have at least one)
                if matches!(pair.1, Value::Null) {
                    Err(EvalError::InvalidSyntax(
                        "lambda requires at least one body expression".to_string(),
                    ))
                } else {
                    Ok(())
                }
            }
            _ => Err(EvalError::InvalidSyntax(
                "lambda expects at least 2 arguments (params and body)".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lambda_form_name() {
        let form = LambdaForm;
        assert_eq!(form.name(), "lambda");
    }

    #[test]
    fn test_lambda_form_help() {
        let form = LambdaForm;
        assert!(form.help().contains("lambda"));
        assert!(form.help().contains("procedure"));
        assert!(!form.help().is_empty());
    }
}
