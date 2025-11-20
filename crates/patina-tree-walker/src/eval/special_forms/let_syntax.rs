//! Let-syntax and letrec-syntax special form implementations
//!
//! These special forms define locally-scoped macros.
//!
//! # Syntax
//!
//! ```scheme
//! (let-syntax ((name transformer) ...)
//!   body ...)
//!
//! (letrec-syntax ((name transformer) ...)
//!   body ...)
//! ```
//!
//! # Semantics
//!
//! - `let-syntax`: Each transformer is compiled in the outer environment.
//!   Macros cannot reference each other.
//! - `letrec-syntax`: All transformers are compiled in the new environment.
//!   Macros can reference each other (recursive definitions).
//!
//! # Examples
//!
//! ```scheme
//! ; let-syntax: local macro
//! (let-syntax ((when (syntax-rules ()
//!                      ((when test body ...)
//!                       (if test (begin body ...))))))
//!   (when #t (display "hello")))
//!
//! ; letrec-syntax: recursive macro
//! (letrec-syntax ((my-or (syntax-rules ()
//!                          ((my-or) #f)
//!                          ((my-or e) e)
//!                          ((my-or e1 e2 ...)
//!                           (let ((temp e1))
//!                             (if temp temp (my-or e2 ...)))))))
//!   (my-or #f #f 42))  ; => 42
//! ```

use super::super::{EvalError, EvalResult, Evaluator};
use super::SpecialForm;
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// Let-syntax special form
///
/// Defines locally-scoped macros. Macros cannot reference each other.
pub struct LetSyntaxForm;

impl SpecialForm for LetSyntaxForm {
    fn name(&self) -> &'static str {
        "let-syntax"
    }

    fn help(&self) -> &'static str {
        "(let-syntax ((name transformer) ...) body ...)\n\
         Defines locally-scoped macros. Macros cannot reference each other.\n\
         Example:\n\
         (let-syntax ((when (syntax-rules ()\n\
                              ((when test body ...)\n\
                               (if test (begin body ...))))))\n\
           (when #t (display \"hello\")))"
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        eval_let_syntax_impl(evaluator, args, env, in_tail_position, false)
    }

    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
        validate_let_syntax_syntax(args)
    }
}

/// Letrec-syntax special form
///
/// Defines locally-scoped macros. Macros can reference each other (recursive).
pub struct LetrecSyntaxForm;

impl SpecialForm for LetrecSyntaxForm {
    fn name(&self) -> &'static str {
        "letrec-syntax"
    }

    fn help(&self) -> &'static str {
        "(letrec-syntax ((name transformer) ...) body ...)\n\
         Defines locally-scoped macros. Macros can reference each other (recursive definitions).\n\
         Example:\n\
         (letrec-syntax ((my-or (syntax-rules ()\n\
                                  ((my-or) #f)\n\
                                  ((my-or e) e)\n\
                                  ((my-or e1 e2 ...)\n\
                                   (let ((temp e1))\n\
                                     (if temp temp (my-or e2 ...)))))))\n\
           (my-or #f #f 42))"
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        eval_let_syntax_impl(evaluator, args, env, in_tail_position, true)
    }

    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
        validate_let_syntax_syntax(args)
    }
}

/// Common implementation for let-syntax and letrec-syntax
///
/// # Arguments
/// - `is_letrec`: true for letrec-syntax, false for let-syntax
fn eval_let_syntax_impl(
    evaluator: &Evaluator,
    args: &Value,
    env: &Rc<Environment>,
    in_tail_position: bool,
    is_letrec: bool,
) -> Result<EvalResult, EvalError> {
    // Parse: (let-syntax ((name transformer) ...) body ...)
    let (bindings, rest) = evaluator.extract_pair(args)?;

    // Body must be non-empty
    if matches!(rest, Value::Null) {
        return Err(EvalError::InvalidSyntax(format!(
            "{} body cannot be empty",
            if is_letrec {
                "letrec-syntax"
            } else {
                "let-syntax"
            }
        )));
    }

    // Create new environment for the body
    let new_env = if is_letrec {
        // For letrec-syntax, create new environment first so macros can reference each other
        Rc::new(Environment::with_parent(env.clone()))
    } else {
        // For let-syntax, we'll create it after compiling macros
        env.clone()
    };

    // Parse and compile each macro binding
    let mut current = bindings.clone();
    let mut macro_bindings = Vec::new();

    while let Value::Pair(pair) = current {
        // Each binding should be (name transformer)
        let binding = &pair.0;
        let (name_expr, trans_rest) = evaluator.extract_pair(binding)?;
        let (transformer_expr, trans_rest2) = evaluator.extract_pair(&trans_rest)?;

        // Check no extra elements in binding
        if !matches!(trans_rest2, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "Each macro binding must be (name transformer)".to_string(),
            ));
        }

        // Name must be a symbol
        let name = match name_expr {
            Value::Symbol(s) => s,
            _ => {
                return Err(EvalError::InvalidSyntax(
                    "Macro name must be a symbol".to_string(),
                ));
            }
        };

        // Compile the transformer
        // Note: compile_syntax_rules doesn't use the environment parameter currently,
        // so the distinction between let-syntax and letrec-syntax is handled by
        // when we create the environment and add the bindings.
        let compiled_macro = evaluator.compile_syntax_rules(&transformer_expr, name.clone())?;

        macro_bindings.push((name.clone(), compiled_macro));

        current = pair.1.clone();
    }

    // Bindings must be a proper list
    if !matches!(current, Value::Null) {
        return Err(EvalError::InvalidSyntax(
            "Macro bindings must be a proper list".to_string(),
        ));
    }

    // Create the environment with macro bindings
    let body_env = if is_letrec {
        // Already created new_env, just add bindings
        for (name, compiled_macro) in macro_bindings {
            let macro_value = Value::Macro {
                name: name.clone(),
                data: Rc::new(compiled_macro),
            };
            new_env.define(name.to_string(), macro_value);
        }
        new_env
    } else {
        // Create new environment and add bindings
        let env_for_body = Rc::new(Environment::with_parent(env.clone()));
        for (name, compiled_macro) in macro_bindings {
            let macro_value = Value::Macro {
                name: name.clone(),
                data: Rc::new(compiled_macro),
            };
            env_for_body.define(name.to_string(), macro_value);
        }
        env_for_body
    };

    // Evaluate body expressions in sequence (like begin)
    // The last expression is in tail position if the let-syntax itself is
    let mut current = rest;
    let mut result = Value::Unspecified;

    while let Value::Pair(pair) = &current {
        if matches!(pair.1, Value::Null) {
            // Last expression - in tail position if let-syntax is
            if in_tail_position {
                return Ok(EvalResult::TailCall {
                    expr: pair.0.clone(),
                    env: body_env.clone(),
                });
            } else {
                result = evaluator.eval_in_env(&pair.0, &body_env)?;
                break;
            }
        } else {
            // Not last expression - NOT in tail position
            result = evaluator.eval_in_env(&pair.0, &body_env)?;
            current = pair.1.clone();
        }
    }

    Ok(EvalResult::Value(result))
}

/// Validate syntax for let-syntax / letrec-syntax
fn validate_let_syntax_syntax(args: &Value) -> Result<(), EvalError> {
    // Must have at least bindings and one body expression
    match args {
        Value::Pair(pair1) => {
            // First argument is bindings - must be a list
            if !matches!(pair1.0, Value::Pair(_) | Value::Null) {
                return Err(EvalError::InvalidSyntax(
                    "Macro bindings must be a list".to_string(),
                ));
            }

            // Must have at least one body expression
            if matches!(pair1.1, Value::Null) {
                return Err(EvalError::InvalidSyntax("Body cannot be empty".to_string()));
            }

            Ok(())
        }
        _ => Err(EvalError::InvalidSyntax(
            "Invalid syntax: expected bindings and body".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_let_syntax_form_name() {
        let form = LetSyntaxForm;
        assert_eq!(form.name(), "let-syntax");
    }

    #[test]
    fn test_letrec_syntax_form_name() {
        let form = LetrecSyntaxForm;
        assert_eq!(form.name(), "letrec-syntax");
    }

    #[test]
    fn test_let_syntax_form_help() {
        let form = LetSyntaxForm;
        assert!(form.help().contains("let-syntax"));
        assert!(form.help().contains("locally-scoped"));
        assert!(!form.help().is_empty());
    }

    #[test]
    fn test_letrec_syntax_form_help() {
        let form = LetrecSyntaxForm;
        assert!(form.help().contains("letrec-syntax"));
        assert!(form.help().contains("recursive"));
        assert!(!form.help().is_empty());
    }
}
