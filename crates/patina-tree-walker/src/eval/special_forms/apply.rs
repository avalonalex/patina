//! Apply special form implementation
//!
//! The `apply` special form applies a procedure to a list of arguments.
//!
//! # Syntax
//!
//! ```scheme
//! (apply proc arg1 ... argN list)
//! ```
//!
//! # Semantics
//!
//! - Evaluates all arguments
//! - First argument must be a procedure
//! - Last argument must be a list
//! - Middle arguments (if any) are prepended to the list
//! - The procedure is applied to the combined arguments
//!
//! # Examples
//!
//! ```scheme
//! (apply + '(1 2 3))           ; => 6
//! (apply + 1 2 '(3 4))         ; => 10
//! (apply list 'a '(b c))       ; => (a b c)
//! (apply (lambda (x y) (+ x y)) '(3 4))  ; => 7
//! ```
//!
//! # Tail Call Optimization
//!
//! The apply form can participate in tail call optimization when it is
//! in tail position itself.

use super::super::{EvalError, EvalResult, Evaluator};
use super::SpecialForm;
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// Apply special form
///
/// Applies a procedure to a list of arguments.
pub struct ApplyForm;

impl SpecialForm for ApplyForm {
    fn name(&self) -> &'static str {
        "apply"
    }

    fn help(&self) -> &'static str {
        "(apply proc arg1 ... argN list) applies proc to the combined arguments.\n\
         The last argument must be a list.\n\
         Example: (apply + '(1 2 3)) => 6\n\
         Example: (apply + 1 2 '(3 4)) => 10"
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        // Collect all arguments
        let arg_exprs = evaluator.collect_list_items(args)?;

        if arg_exprs.len() < 2 {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: arg_exprs.len(),
            });
        }

        // First argument is the procedure
        let proc = evaluator.eval_in_env(&arg_exprs[0], env)?;

        // Middle arguments (if any) are regular arguments
        let mut final_args = Vec::new();
        for arg in arg_exprs.iter().take(arg_exprs.len() - 1).skip(1) {
            final_args.push(evaluator.eval_in_env(arg, env)?);
        }

        // Last argument must be a list
        let last_arg = evaluator.eval_in_env(&arg_exprs[arg_exprs.len() - 1], env)?;

        // Convert the last argument (list) to a vector and append to final_args
        let mut current = last_arg.clone();
        while let Value::Pair(pair) = current {
            let borrowed = pair.borrow();
            final_args.push(borrowed.0.clone());
            current = borrowed.1.clone();
        }

        // Check that the last argument was a proper list
        if !matches!(current, Value::Null) {
            return Err(EvalError::TypeError(
                "apply: last argument must be a proper list".to_string(),
            ));
        }

        // Now apply the procedure to the combined arguments
        evaluator.apply(proc, final_args, in_tail_position)
    }

    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
        // Check that we have at least 2 arguments
        match args {
            Value::Pair(pair1) => {
                // First argument (procedure) exists
                match &pair1.borrow().1 {
                    Value::Pair(_) => Ok(()), // At least 2 arguments
                    _ => Err(EvalError::InvalidSyntax(
                        "apply expects at least 2 arguments".to_string(),
                    )),
                }
            }
            _ => Err(EvalError::InvalidSyntax(
                "apply expects at least 2 arguments".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_form_name() {
        let form = ApplyForm;
        assert_eq!(form.name(), "apply");
    }

    #[test]
    fn test_apply_form_help() {
        let form = ApplyForm;
        assert!(form.help().contains("apply"));
        assert!(form.help().contains("proc"));
        assert!(!form.help().is_empty());
    }
}
