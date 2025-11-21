//! Quote special form implementation
//!
//! The `quote` special form returns its argument without evaluating it.
//! This is fundamental for treating code as data in Scheme.
//!
//! # Syntax
//!
//! ```scheme
//! (quote datum)
//! 'datum  ; shorthand
//! ```
//!
//! # Examples
//!
//! ```scheme
//! (quote x)           ; => x (symbol, not its value)
//! (quote (+ 1 2))     ; => (+ 1 2) (list, not evaluated)
//! '(a b c)            ; => (a b c)
//! ```

use super::super::{EvalError, EvalResult, Evaluator};
use super::SpecialForm;
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// Quote special form
///
/// Returns its argument without evaluating it.
pub struct QuoteForm;

impl SpecialForm for QuoteForm {
    fn name(&self) -> &'static str {
        "quote"
    }

    fn help(&self) -> &'static str {
        "(quote datum) returns datum without evaluating it.\n\
         Shorthand: 'datum\n\
         Example: (quote (+ 1 2)) => (+ 1 2)"
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        _env: &Rc<Environment>,
        _in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        // Extract the single argument
        let (quoted, rest) = evaluator.extract_pair(args)?;

        // Ensure exactly one argument
        if !matches!(rest, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "quote expects exactly one argument".to_string(),
            ));
        }

        // Return the quoted expression without evaluating
        Ok(EvalResult::Value(quoted))
    }

    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
        // Check that we have exactly one argument
        match args {
            Value::Pair(pair) => {
                if !matches!(pair.borrow().1, Value::Null) {
                    Err(EvalError::InvalidSyntax(
                        "quote expects exactly one argument".to_string(),
                    ))
                } else {
                    Ok(())
                }
            }
            _ => Err(EvalError::InvalidSyntax(
                "quote expects exactly one argument".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quote_form_name() {
        let form = QuoteForm;
        assert_eq!(form.name(), "quote");
    }

    #[test]
    fn test_quote_form_help() {
        let form = QuoteForm;
        assert!(form.help().contains("quote"));
        assert!(!form.help().is_empty());
    }
}
