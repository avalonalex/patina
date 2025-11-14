//! If special form implementation
//!
//! The `if` special form provides conditional evaluation.
//! It evaluates a test expression and then evaluates either the consequent
//! or alternate branch based on the result.
//!
//! # Syntax
//!
//! ```scheme
//! (if test consequent [alternate])
//! ```
//!
//! # Semantics
//!
//! - The test expression is evaluated first
//! - If the test is true (anything other than #f), the consequent is evaluated
//! - If the test is false (#f), the alternate is evaluated (if present)
//! - If the test is false and no alternate is provided, the result is unspecified
//! - Only one branch is evaluated (the other is never evaluated)
//!
//! # Tail Call Optimization
//!
//! Both the consequent and alternate branches are in tail position if the `if`
//! expression itself is in tail position. This enables proper tail call optimization.
//!
//! # Examples
//!
//! ```scheme
//! (if #t 1 2)              ; => 1
//! (if #f 1 2)              ; => 2
//! (if #t 1)                ; => 1
//! (if #f 1)                ; => unspecified
//! (if (> 3 2) 'yes 'no)    ; => yes
//! ```

use super::super::{EvalError, EvalResult, Evaluator};
use super::SpecialForm;
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// If special form
///
/// Provides conditional evaluation with tail call optimization support.
pub struct IfForm;

impl SpecialForm for IfForm {
    fn name(&self) -> &'static str {
        "if"
    }

    fn help(&self) -> &'static str {
        "(if test consequent [alternate]) evaluates test and returns consequent if true, \
         alternate if false.\n\
         If alternate is omitted and test is false, result is unspecified.\n\
         Example: (if (> x 0) 'positive 'non-positive)"
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        // Extract test, consequent, and optional alternate
        let (condition, rest) = evaluator.extract_pair(args)?;
        let (then_branch, rest) = evaluator.extract_pair(&rest)?;

        let else_branch = match rest {
            Value::Null => Value::Unspecified,
            Value::Pair(pair) => {
                if !matches!(pair.1, Value::Null) {
                    return Err(EvalError::InvalidSyntax(
                        "if expects 2 or 3 arguments".to_string(),
                    ));
                }
                pair.0.clone()
            }
            _ => {
                return Err(EvalError::InvalidSyntax(
                    "Malformed if expression".to_string(),
                ));
            }
        };

        // Evaluate condition (NOT in tail position)
        let test_result = evaluator.eval_in_env(&condition, env)?;

        // Both branches ARE in tail position if the if itself is
        let branch = if test_result.is_truthy() {
            then_branch
        } else {
            else_branch
        };

        if in_tail_position {
            Ok(EvalResult::TailCall {
                expr: branch,
                env: env.clone(),
            })
        } else {
            evaluator.eval_in_env(&branch, env).map(EvalResult::Value)
        }
    }

    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
        // Check that we have 2 or 3 arguments
        match args {
            Value::Pair(pair1) => {
                // First argument (test) exists
                match &pair1.1 {
                    Value::Pair(pair2) => {
                        // Second argument (consequent) exists
                        match &pair2.1 {
                            Value::Null => Ok(()), // 2 arguments - valid
                            Value::Pair(pair3) => {
                                // Third argument (alternate) exists
                                if matches!(pair3.1, Value::Null) {
                                    Ok(()) // 3 arguments - valid
                                } else {
                                    Err(EvalError::InvalidSyntax(
                                        "if expects 2 or 3 arguments".to_string(),
                                    ))
                                }
                            }
                            _ => Err(EvalError::InvalidSyntax(
                                "Malformed if expression".to_string(),
                            )),
                        }
                    }
                    _ => Err(EvalError::InvalidSyntax(
                        "if expects at least 2 arguments".to_string(),
                    )),
                }
            }
            _ => Err(EvalError::InvalidSyntax(
                "if expects at least 2 arguments".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_if_form_name() {
        let form = IfForm;
        assert_eq!(form.name(), "if");
    }

    #[test]
    fn test_if_form_help() {
        let form = IfForm;
        assert!(form.help().contains("if"));
        assert!(form.help().contains("test"));
        assert!(!form.help().is_empty());
    }
}
