//! Case-lambda special form implementation
//!
//! The `case-lambda` special form creates procedures that dispatch based on
//! the number of arguments provided.
//!
//! # Syntax
//!
//! ```scheme
//! (case-lambda
//!   clause ...)
//! ```
//!
//! Each clause has the form `(params body ...)` where params follows the same
//! syntax as lambda params.
//!
//! # Examples
//!
//! ```scheme
//! (define any-arity
//!   (case-lambda
//!     (() 'zero)
//!     ((x) x)
//!     ((x y) (cons x y))
//!     ((x y z) (list x y z))
//!     (args (cons 'many args))))
//!
//! (any-arity)        ; => zero
//! (any-arity 1)      ; => 1
//! (any-arity 1 2)    ; => (1 . 2)
//! (any-arity 1 2 3)  ; => (1 2 3)
//! (any-arity 1 2 3 4) ; => (many 1 2 3 4)
//! ```
//!
//! # Semantics
//!
//! - Each clause is tried in order
//! - The first clause whose params match the argument count is selected
//! - If no clause matches, an error is raised
//! - Fixed params `(x y z)` match exactly that many arguments
//! - Variadic params `args` match any number of arguments
//! - Mixed params `(x y . rest)` match that many or more arguments
//!
//! # R7RS Compliance
//!
//! This implements the R7RS `case-lambda` from `(scheme case-lambda)`.

use super::super::{EvalError, EvalResult, Evaluator};
use super::SpecialForm;
use patina_runtime::{Environment, Procedure, Value};
use std::rc::Rc;

/// Case-lambda special form
///
/// Creates procedures that dispatch based on argument count.
pub struct CaseLambdaForm;

impl SpecialForm for CaseLambdaForm {
    fn name(&self) -> &'static str {
        "case-lambda"
    }

    fn help(&self) -> &'static str {
        "(case-lambda clause ...) creates a procedure that dispatches on argument count.\n\
         Each clause has the form (params body ...).\n\
         Example: (case-lambda\n\
                    (() 'zero)\n\
                    ((x) x)\n\
                    ((x y) (cons x y)))"
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        _in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        // Collect all clauses
        let clauses = evaluator.collect_list_items(args)?;

        if clauses.is_empty() {
            return Err(EvalError::InvalidSyntax(
                "case-lambda requires at least one clause".to_string(),
            ));
        }

        // Parse each clause into (params, variadic, body)
        let mut parsed_clauses = Vec::new();

        for clause in clauses {
            // Each clause should be (params body ...)
            match clause {
                Value::Pair(pair) => {
                    let pair_ref = pair.borrow();
                    let params_expr = pair_ref.0.clone();
                    let rest = pair_ref.1.clone();
                    drop(pair_ref);

                    // Parse parameters
                    let (params, variadic) = evaluator.parse_lambda_params(&params_expr)?;

                    // Collect body expressions
                    let body = evaluator.collect_list_items(&rest)?;

                    if body.is_empty() {
                        return Err(EvalError::InvalidSyntax(
                            "case-lambda clause requires at least one body expression".to_string(),
                        ));
                    }

                    parsed_clauses.push((params, variadic, body));
                }
                _ => {
                    return Err(EvalError::InvalidSyntax(
                        "case-lambda clause must be a list (params body ...)".to_string(),
                    ));
                }
            }
        }

        // Create CaseLambda procedure
        Ok(EvalResult::Value(Value::Procedure(Procedure::CaseLambda {
            clauses: parsed_clauses,
            env: env.clone(),
        })))
    }

    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
        match args {
            Value::Null => Err(EvalError::InvalidSyntax(
                "case-lambda requires at least one clause".to_string(),
            )),
            Value::Pair(_) => Ok(()),
            _ => Err(EvalError::InvalidSyntax(
                "case-lambda expects a list of clauses".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_case_lambda_form_name() {
        let form = CaseLambdaForm;
        assert_eq!(form.name(), "case-lambda");
    }

    #[test]
    fn test_case_lambda_form_help() {
        let form = CaseLambdaForm;
        assert!(form.help().contains("case-lambda"));
        assert!(form.help().contains("dispatch"));
        assert!(!form.help().is_empty());
    }
}
