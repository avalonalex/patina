//! Define special form implementation
//!
//! The `define` special form creates new bindings in the current environment.
//! It has two forms: variable definition and function definition (shorthand).
//!
//! # Syntax
//!
//! ```scheme
//! (define name value)              ; variable definition
//! (define (name params...) body)   ; function definition (shorthand)
//! ```
//!
//! # Variable Definition
//!
//! ```scheme
//! (define x 10)
//! (define y (+ 1 2))
//! ```
//!
//! The value expression is evaluated and bound to the name.
//!
//! # Function Definition (Shorthand)
//!
//! ```scheme
//! (define (square x) (* x x))
//! ```
//!
//! This is equivalent to:
//!
//! ```scheme
//! (define square (lambda (x) (* x x)))
//! ```
//!
//! Supports variadic parameters:
//!
//! ```scheme
//! (define (sum . args) (apply + args))
//! (define (format-str fmt . args) ...)
//! ```
//!
//! # Semantics
//!
//! - Creates a new binding in the current environment
//! - If binding already exists, it is replaced
//! - Returns an unspecified value
//! - Top-level defines create global bindings
//! - Internal defines create local bindings

use super::super::{EvalError, EvalResult, Evaluator};
use super::SpecialForm;
use patina_runtime::{Environment, Procedure, Value};
use std::rc::Rc;

/// Define special form
///
/// Handles both variable and function definitions.
pub struct DefineForm;

impl SpecialForm for DefineForm {
    fn name(&self) -> &'static str {
        "define"
    }

    fn help(&self) -> &'static str {
        "(define name value) creates a binding.\n\
         (define (name params...) body...) defines a function.\n\
         Example: (define x 10)\n\
         Example: (define (square x) (* x x))"
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        _in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        let (first, rest) = evaluator.extract_pair(args)?;

        match first {
            Value::Symbol(name) | Value::WrappedIdentifier { name, .. } => {
                // (define var value)
                let (value_expr, rest) = evaluator.extract_pair(&rest)?;
                if !matches!(rest, Value::Null) {
                    return Err(EvalError::InvalidSyntax(
                        "define expects 2 arguments".to_string(),
                    ));
                }
                let value = evaluator.eval_in_env(&value_expr, env)?;
                env.define(name.to_string(), value);
                Ok(EvalResult::Value(Value::Unspecified))
            }
            Value::Pair(_) => {
                // (define (name params...) body...)
                // Shorthand for (define name (lambda (params...) body...))

                // Extract name from (name params...)
                let (name_val, params_rest) = evaluator.extract_pair(&first)?;
                let name = match name_val {
                    Value::Symbol(s) => s,
                    _ => {
                        return Err(EvalError::InvalidSyntax(
                            "define: function name must be a symbol".to_string(),
                        ));
                    }
                };

                // Parse the parameters
                let (params, variadic) = evaluator.parse_lambda_params(&params_rest)?;

                // rest contains the body expressions
                let body = evaluator.collect_list_items(&rest)?;

                if body.is_empty() {
                    return Err(EvalError::InvalidSyntax(
                        "define: function body cannot be empty".to_string(),
                    ));
                }

                // Create the lambda
                let lambda = Value::Procedure(Procedure::Lambda {
                    params,
                    variadic,
                    body,
                    env: env.clone(),
                });

                // Define it
                env.define(name.to_string(), lambda);
                Ok(EvalResult::Value(Value::Unspecified))
            }
            _ => Err(EvalError::InvalidSyntax(
                "define expects a symbol or list".to_string(),
            )),
        }
    }

    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
        match args {
            Value::Pair(pair1) => {
                // First argument exists (name or (name params...))
                let pair1_borrowed = pair1.borrow();
                match &pair1_borrowed.0 {
                    Value::Symbol(_) | Value::WrappedIdentifier { .. } => {
                        // Variable definition: (define name value)
                        // Check for exactly 2 arguments
                        match &pair1_borrowed.1 {
                            Value::Pair(pair2) => {
                                if matches!(pair2.borrow().1, Value::Null) {
                                    Ok(())
                                } else {
                                    Err(EvalError::InvalidSyntax(
                                        "define expects 2 arguments".to_string(),
                                    ))
                                }
                            }
                            _ => Err(EvalError::InvalidSyntax(
                                "define expects 2 arguments".to_string(),
                            )),
                        }
                    }
                    Value::Pair(_) => {
                        // Function definition: (define (name params...) body...)
                        // Need at least body
                        if matches!(pair1_borrowed.1, Value::Null) {
                            Err(EvalError::InvalidSyntax(
                                "define: function body cannot be empty".to_string(),
                            ))
                        } else {
                            Ok(())
                        }
                    }
                    _ => Err(EvalError::InvalidSyntax(
                        "define expects a symbol or list".to_string(),
                    )),
                }
            }
            _ => Err(EvalError::InvalidSyntax(
                "define expects at least 2 arguments".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_define_form_name() {
        let form = DefineForm;
        assert_eq!(form.name(), "define");
    }

    #[test]
    fn test_define_form_help() {
        let form = DefineForm;
        assert!(form.help().contains("define"));
        assert!(form.help().contains("binding"));
        assert!(!form.help().is_empty());
    }
}
