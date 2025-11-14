//! Define-syntax special form implementation
//!
//! The `define-syntax` special form defines macros using syntax-rules.
//!
//! # Syntax
//!
//! ```scheme
//! (define-syntax name
//!   (syntax-rules (literals ...)
//!     (pattern template) ...))
//! ```
//!
//! # Semantics
//!
//! - Compiles the syntax-rules transformer
//! - Creates a macro binding in the environment
//! - Returns an unspecified value
//!
//! # Examples
//!
//! ```scheme
//! (define-syntax when
//!   (syntax-rules ()
//!     ((when test body ...)
//!      (if test (begin body ...)))))
//!
//! (when #t (display "hello\n"))  ; => prints "hello"
//! ```
//!
//! # Hygiene
//!
//! Macros are hygienic - identifiers introduced by the macro don't
//! capture identifiers in the macro's usage context.

use super::super::{EvalError, EvalResult, Evaluator};
use super::SpecialForm;
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// Define-syntax special form
///
/// Defines macros using syntax-rules.
pub struct DefineSyntaxForm;

impl SpecialForm for DefineSyntaxForm {
    fn name(&self) -> &'static str {
        "define-syntax"
    }

    fn help(&self) -> &'static str {
        "(define-syntax name (syntax-rules (literals ...) (pattern template) ...))\n\
         Defines a hygienic macro.\n\
         Example:\n\
         (define-syntax when\n\
           (syntax-rules ()\n\
             ((when test body ...)\n\
              (if test (begin body ...)))))"
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        _in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        // Parse: (define-syntax name transformer)
        let (name_expr, rest) = evaluator.extract_pair(args)?;
        let (transformer_expr, rest2) = evaluator.extract_pair(&rest)?;

        // Check that there are no extra arguments
        if !matches!(rest2, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "define-syntax expects exactly 2 arguments".to_string(),
            ));
        }

        // Name must be a symbol
        let name = match name_expr {
            Value::Symbol(s) => s,
            _ => {
                return Err(EvalError::InvalidSyntax(
                    "define-syntax name must be a symbol".to_string(),
                ));
            }
        };

        // Transformer must be (syntax-rules (literals) rules...)
        // Use V2 compiler to compile the macro
        let compiled_macro = evaluator.compile_syntax_rules(&transformer_expr, name.clone())?;

        // Store the compiled macro
        let macro_value = Value::Macro {
            name: name.clone(),
            data: Rc::new(compiled_macro),
        };

        // Bind the macro in the environment
        env.define(name.to_string(), macro_value);

        Ok(EvalResult::Value(Value::Unspecified))
    }

    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
        // Check that we have exactly 2 arguments
        match args {
            Value::Pair(pair1) => {
                // First argument (name) must be a symbol
                if !matches!(pair1.0, Value::Symbol(_)) {
                    return Err(EvalError::InvalidSyntax(
                        "define-syntax name must be a symbol".to_string(),
                    ));
                }

                // Second argument (transformer) exists
                match &pair1.1 {
                    Value::Pair(pair2) => {
                        if matches!(pair2.1, Value::Null) {
                            Ok(())
                        } else {
                            Err(EvalError::InvalidSyntax(
                                "define-syntax expects exactly 2 arguments".to_string(),
                            ))
                        }
                    }
                    _ => Err(EvalError::InvalidSyntax(
                        "define-syntax expects exactly 2 arguments".to_string(),
                    )),
                }
            }
            _ => Err(EvalError::InvalidSyntax(
                "define-syntax expects exactly 2 arguments".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_define_syntax_form_name() {
        let form = DefineSyntaxForm;
        assert_eq!(form.name(), "define-syntax");
    }

    #[test]
    fn test_define_syntax_form_help() {
        let form = DefineSyntaxForm;
        assert!(form.help().contains("define-syntax"));
        assert!(form.help().contains("macro"));
        assert!(!form.help().is_empty());
    }
}
