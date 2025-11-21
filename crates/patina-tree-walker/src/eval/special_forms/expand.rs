//! Expand special form implementation
//!
//! The `expand` special form shows macro expansion without evaluating the result.
//! This is useful for debugging macros and understanding macro transformations.
//!
//! # Non-Standard Extension
//!
//! **NOTE:** This is NOT part of the R7RS specification. It's a Patina extension
//! for development and debugging, similar to `expand` in Chez Scheme and Racket.
//!
//! TODO: Consider moving this to a library like `(patina debug)` or `(patina expand)`
//! rather than being a built-in special form. This would make the core evaluator
//! more strictly R7RS compliant while still providing this useful debugging tool
//! through the library system.
//!
//! # Syntax
//!
//! ```scheme
//! (expand expr)
//! ```
//!
//! # Examples
//!
//! ```scheme
//! (expand '(let ((x 1)) x))
//! ; => ((lambda (x) x) 1)
//!
//! (expand '(cond (else 42)))
//! ; => 42
//! ```
//!
//! # Usage
//!
//! The expression is typically quoted to prevent evaluation:
//! ```scheme
//! (expand '(macro-call args...))
//! ```

use super::super::{EvalError, EvalResult, Evaluator};
use super::SpecialForm;
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// Expand special form
///
/// Shows the macro expansion of an expression without evaluating the result.
/// Similar to Chez Scheme's `(expand)` and Racket's `(expand)`.
pub struct ExpandForm;

impl SpecialForm for ExpandForm {
    fn name(&self) -> &'static str {
        "expand"
    }

    fn help(&self) -> &'static str {
        "(expand expr) shows the macro expansion of expr without evaluating the result.\n\
         Useful for debugging macros.\n\
         Example: (expand '(let ((x 1)) x)) => ((lambda (x) x) 1)"
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        _in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        // Extract the single argument
        let (expr_arg, rest) = evaluator.extract_pair(args)?;

        // Ensure exactly one argument
        if !matches!(rest, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "expand expects exactly one argument".to_string(),
            ));
        }

        // Evaluate the argument to unwrap quotes
        // (expand '(do ...)) evaluates '(do ...) to get (do ...)
        let expr = evaluator.eval_in_env(&expr_arg, env)?;

        // Check if expr is a macro call
        if let Value::Pair(p) = &expr {
            let p_ref = p.borrow();
            let car = &p_ref.0;
            if let Value::Symbol(sym) = car {
                // Check if this symbol is bound to a macro
                if let Some(Value::Macro { data, .. }) = env.get(sym) {
                    let compiled_macro = data
                        .downcast_ref::<patina_frontend::macro_expander::CompiledMacro>()
                        .ok_or_else(|| {
                            EvalError::InternalError("Invalid macro data".to_string())
                        })?;

                    // Expand the macro using the MacroExpander trait interface
                    use patina_frontend::macro_expander::{CompiledMacroExpander, MacroExpander};
                    let expander = CompiledMacroExpander::new(compiled_macro.clone());
                    let expanded = expander.expand(&expr, env).map_err(EvalError::from)?;
                    return Ok(EvalResult::Value(expanded));
                }
            }
        }

        // If not a macro, return the expression as-is
        Ok(EvalResult::Value(expr))
    }

    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
        // Check that we have exactly one argument
        match args {
            Value::Pair(pair) => {
                if !matches!(pair.borrow().1, Value::Null) {
                    Err(EvalError::InvalidSyntax(
                        "expand expects exactly one argument".to_string(),
                    ))
                } else {
                    Ok(())
                }
            }
            _ => Err(EvalError::InvalidSyntax(
                "expand expects exactly one argument".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_form_name() {
        let form = ExpandForm;
        assert_eq!(form.name(), "expand");
    }

    #[test]
    fn test_expand_form_help() {
        let form = ExpandForm;
        assert!(form.help().contains("expand"));
        assert!(!form.help().is_empty());
    }
}
