//! Parameterize special form implementation
//!
//! Dynamically rebinds parameters for the extent of the body.
//!
//! # Syntax
//!
//! ```scheme
//! (parameterize ((param1 value1) (param2 value2) ...)
//!   body ...)
//! ```
//!
//! # Semantics
//!
//! 1. Evaluates each param and value expression
//! 2. Saves current parameter values
//! 3. Sets parameters to new values
//! 4. Evaluates body in sequence
//! 5. Restores original parameter values
//!
//! # Examples
//!
//! ```scheme
//! (define radix (make-parameter 10))
//! (radix)  ; => 10
//!
//! (parameterize ((radix 2))
//!   (radix))  ; => 2
//!
//! (radix)  ; => 10 (restored)
//! ```

use super::super::{EvalError, EvalResult, Evaluator};
use super::SpecialForm;
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// Parameterize special form
///
/// Dynamically rebinds parameters for the extent of the body.
pub struct ParameterizeForm;

impl SpecialForm for ParameterizeForm {
    fn name(&self) -> &'static str {
        "parameterize"
    }

    fn help(&self) -> &'static str {
        "(parameterize ((param1 value1) ...) body ...)\n\
         Dynamically rebinds parameters for the extent of the body.\n\
         Example:\n\
         (define radix (make-parameter 10))\n\
         (parameterize ((radix 2))\n\
           (radix))  ; => 2\n\
         (radix)  ; => 10 (restored)"
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        _in_tail_position: bool, // TODO: see internal/PARAMETER_BUG.md
    ) -> Result<EvalResult, EvalError> {
        // Parse: (parameterize ((param value) ...) body ...)
        let (bindings, body) = evaluator.extract_pair(args)?;

        // Body must be non-empty
        if matches!(body, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "parameterize body cannot be empty".to_string(),
            ));
        }

        // Process bindings: evaluate params and values, push new values onto stack
        let mut params = Vec::new();

        let mut current = bindings.clone();
        while let Value::Pair(pair) = current {
            // Each binding should be (param value)
            let binding = &pair.0;
            let (param_expr, value_rest) = evaluator.extract_pair(binding)?;
            let (value_expr, rest) = evaluator.extract_pair(&value_rest)?;

            // Check no extra elements in binding
            if !matches!(rest, Value::Null) {
                return Err(EvalError::InvalidSyntax(
                    "Each parameterize binding must be (param value)".to_string(),
                ));
            }

            // Evaluate param expression
            let param = evaluator.eval_in_env(&param_expr, env)?;

            // Verify it's a parameter
            match &param {
                Value::Parameter { values, converter } => {
                    // Evaluate new value expression
                    let new_val = evaluator.eval_in_env(&value_expr, env)?;

                    // Apply converter if present
                    let converted_val = if let Some(conv) = converter {
                        let result =
                            evaluator.apply(*conv.clone(), vec![new_val.clone()], false)?;
                        match result {
                            EvalResult::Value(v) => v,
                            _ => {
                                return Err(EvalError::InvalidSyntax(
                                    "parameter converter returned non-value".to_string(),
                                ));
                            }
                        }
                    } else {
                        new_val.clone()
                    };

                    // Push new value onto parameter stack
                    values.borrow_mut().push(converted_val.clone());
                    params.push(param.clone());
                }
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "parameterize: expected parameter, got {}",
                        param.type_name()
                    )));
                }
            }

            current = pair.1.clone();
        }

        // Bindings must be a proper list
        if !matches!(current, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "parameterize bindings must be a proper list".to_string(),
            ));
        }

        // Evaluate body expressions in sequence
        let mut result = Value::Unspecified;
        let mut body_current = body;

        while let Value::Pair(pair) = &body_current {
            // TODO: Bad interaction between tail call optimization and parameterize
            // We don't use tail call optimization for parameterize because we need to
            // ensure the parameter stack is popped AFTER the body completes, not before.
            // If we did tail call optimization, we would pop the stack before returning
            // the TailCall result, causing the called procedure to see the old parameter value.
            //
            // The proper solution would be to implement dynamic-wind or use a similar
            // mechanism that ensures cleanup happens after tail calls complete.
            // For now, we sacrifice tail call optimization in parameterize bodies.
            result = evaluator.eval_in_env(&pair.0, env)?;
            body_current = pair.1.clone();
        }

        // Pop parameter values from stack (restore to previous state)
        for param in &params {
            if let Value::Parameter { values, .. } = param {
                values.borrow_mut().pop();
            }
        }

        Ok(EvalResult::Value(result))
    }

    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
        // Must have at least bindings and one body expression
        match args {
            Value::Pair(pair1) => {
                // First argument is bindings - must be a list
                if !matches!(pair1.0, Value::Pair(_) | Value::Null) {
                    return Err(EvalError::InvalidSyntax(
                        "parameterize bindings must be a list".to_string(),
                    ));
                }

                // Must have at least one body expression
                if matches!(pair1.1, Value::Null) {
                    return Err(EvalError::InvalidSyntax(
                        "parameterize body cannot be empty".to_string(),
                    ));
                }

                Ok(())
            }
            _ => Err(EvalError::InvalidSyntax(
                "Invalid syntax: expected bindings and body".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parameterize_form_name() {
        let form = ParameterizeForm;
        assert_eq!(form.name(), "parameterize");
    }

    #[test]
    fn test_parameterize_form_help() {
        let form = ParameterizeForm;
        assert!(form.help().contains("parameterize"));
        assert!(form.help().contains("Dynamically"));
        assert!(!form.help().is_empty());
    }
}
