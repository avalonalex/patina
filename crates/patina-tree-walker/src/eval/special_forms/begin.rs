//! Begin special form implementation
//!
//! The `begin` special form evaluates a sequence of expressions in order
//! and returns the value of the last expression.
//!
//! # Syntax
//!
//! ```scheme
//! (begin expr1 expr2 ... exprN)
//! ```
//!
//! # Semantics
//!
//! - All expressions are evaluated in order from left to right
//! - The value of the last expression is returned
//! - If there are no expressions, the result is unspecified
//! - Side effects from all expressions occur
//!
//! # Tail Call Optimization
//!
//! Only the last expression is in tail position (if the `begin` itself is
//! in tail position). All other expressions are evaluated in non-tail position.
//!
//! # Examples
//!
//! ```scheme
//! (begin (display "hello\n") (+ 1 2))  ; prints "hello", returns 3
//! (begin 1 2 3)                         ; => 3
//! (begin)                               ; => unspecified
//! ```

use super::super::{EvalError, EvalResult, Evaluator};
use super::SpecialForm;
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// Begin special form
///
/// Evaluates a sequence of expressions sequentially with tail call optimization
/// for the last expression.
pub struct BeginForm;

impl SpecialForm for BeginForm {
    fn name(&self) -> &'static str {
        "begin"
    }

    fn help(&self) -> &'static str {
        "(begin expr1 expr2 ... exprN) evaluates expressions sequentially and returns \
         the value of the last expression.\n\
         Example: (begin (display \"hello\") (+ 1 2))"
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        let mut current = args.clone();
        let mut result = Value::Unspecified;

        // Evaluate all expressions except the last
        while let Value::Pair(pair) = current.clone() {
            let (car, cdr) = {
                let borrowed = pair.borrow();
                (borrowed.0.clone(), borrowed.1.clone())
            };
            if matches!(cdr, Value::Null) {
                // Last expression - in tail position if begin is
                if in_tail_position {
                    return Ok(EvalResult::TailCall {
                        expr: car,
                        env: env.clone(),
                    });
                } else {
                    result = evaluator.eval_in_env(&car, env)?;
                    break;
                }
            } else {
                // Not last expression - NOT in tail position
                result = evaluator.eval_in_env(&car, env)?;
                current = cdr;
            }
        }

        Ok(EvalResult::Value(result))
    }

    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
        // Begin accepts any number of arguments (including zero)
        // Just verify it's a proper list
        match args {
            Value::Null => Ok(()),
            Value::Pair(_) => Ok(()), // Simplified: if it's a pair, it's valid enough
            _ => Err(EvalError::InvalidSyntax(
                "begin expects a proper list of expressions".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_begin_form_name() {
        let form = BeginForm;
        assert_eq!(form.name(), "begin");
    }

    #[test]
    fn test_begin_form_help() {
        let form = BeginForm;
        assert!(form.help().contains("begin"));
        assert!(form.help().contains("sequentially"));
        assert!(!form.help().is_empty());
    }
}
