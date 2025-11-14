//! Quasiquote special form implementation
//!
//! The `quasiquote` special form provides template construction with selective evaluation.
//!
//! # Syntax
//!
//! ```scheme
//! (quasiquote template)
//! `template  ; shorthand
//! ```
//!
//! # Features
//!
//! ## Basic Quasiquoting
//! Like quote, but allows selective evaluation:
//! ```scheme
//! `(a b c)  ; => (a b c)
//! ```
//!
//! ## Unquote (,)
//! Evaluates an expression within the template:
//! ```scheme
//! `(a ,(+ 1 2) c)  ; => (a 3 c)
//! ```
//!
//! ## Unquote-Splicing (,@)
//! Splices a list into the template:
//! ```scheme
//! `(a ,@(list 1 2) b)  ; => (a 1 2 b)
//! ```
//!
//! ## Nested Quasiquoting
//! Quasiquotes can be nested with proper depth tracking:
//! ```scheme
//! ``(a ,,x)  ; evaluates x once, produces `(a ,value-of-x)
//! ```
//!
//! # Semantics
//!
//! - Depth tracking for nested quasiquotes
//! - Unquote at depth 0 evaluates, at depth > 0 decrements
//! - Unquote-splicing only in list context
//! - Supports improper lists and vectors

use super::super::{EvalError, EvalResult, Evaluator};
use super::SpecialForm;
use patina_runtime::{Environment, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Quasiquote special form
///
/// Provides template construction with selective evaluation.
pub struct QuasiquoteForm;

impl SpecialForm for QuasiquoteForm {
    fn name(&self) -> &'static str {
        "quasiquote"
    }

    fn help(&self) -> &'static str {
        "(quasiquote template) or `template constructs templates with selective evaluation.\n\
         Use , (unquote) to evaluate within template.\n\
         Use ,@ (unquote-splicing) to splice lists.\n\
         Example: `(a ,(+ 1 2) ,@(list 3 4)) => (a 3 3 4)"
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        _in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        let (template, rest) = evaluator.extract_pair(args)?;
        if !matches!(rest, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "quasiquote expects exactly one argument".to_string(),
            ));
        }
        Ok(EvalResult::Value(eval_quasiquote_impl(
            evaluator, &template, env, 0,
        )?))
    }

    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
        // Check that we have exactly one argument
        match args {
            Value::Pair(pair) => {
                if matches!(pair.1, Value::Null) {
                    Ok(())
                } else {
                    Err(EvalError::InvalidSyntax(
                        "quasiquote expects exactly one argument".to_string(),
                    ))
                }
            }
            _ => Err(EvalError::InvalidSyntax(
                "quasiquote expects exactly one argument".to_string(),
            )),
        }
    }
}

/// Implementation of quasiquote with depth tracking
///
/// The depth parameter tracks nesting level:
/// - depth 0: at current quasiquote level (unquotes are active)
/// - depth > 0: inside nested quasiquote (unquotes become quoted)
fn eval_quasiquote_impl(
    evaluator: &Evaluator,
    expr: &Value,
    env: &Rc<Environment>,
    depth: i32,
) -> Result<Value, EvalError> {
    match expr {
        // Self-evaluating values: return as-is
        Value::Boolean(_)
        | Value::Integer(_)
        | Value::BigInteger(_)
        | Value::Rational(_)
        | Value::Real(_)
        | Value::Complex(_, _)
        | Value::Character(_)
        | Value::String(_)
        | Value::Bytevector(_)
        | Value::Unspecified => Ok(expr.clone()),

        // Symbols and null: quote them (return as-is)
        Value::Symbol(_) | Value::Null => Ok(expr.clone()),

        // Vectors: convert to list, process, convert back
        Value::Vector(vec) => {
            let list = vector_to_list(&vec.borrow());
            let processed = eval_quasiquote_impl(evaluator, &list, env, depth)?;
            list_to_vector(&processed)
        }

        // Pairs: the interesting case
        Value::Pair(pair) => {
            let car = &pair.0;
            let cdr = &pair.1;

            // Check if car is a symbol that requires special handling
            if let Value::Symbol(sym) = car {
                match sym.as_ref() {
                    // Nested quasiquote: increment depth
                    "quasiquote" => {
                        let (inner, rest) = evaluator.extract_pair(cdr)?;
                        if !matches!(rest, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "quasiquote expects exactly one argument".to_string(),
                            ));
                        }
                        let processed = eval_quasiquote_impl(evaluator, &inner, env, depth + 1)?;
                        Ok(list_from_vec(vec![
                            Value::Symbol(Rc::from("quasiquote")),
                            processed,
                        ]))
                    }

                    // Unquote: evaluate if at depth 0, otherwise decrement depth
                    "unquote" => {
                        let (inner, rest) = evaluator.extract_pair(cdr)?;
                        if !matches!(rest, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "unquote expects exactly one argument".to_string(),
                            ));
                        }

                        if depth == 0 {
                            // At quasiquote level: evaluate the unquoted expression
                            evaluator.eval_in_env(&inner, env)
                        } else {
                            // Inside nested quasiquote: preserve unquote, decrement depth
                            let processed =
                                eval_quasiquote_impl(evaluator, &inner, env, depth - 1)?;
                            Ok(list_from_vec(vec![
                                Value::Symbol(Rc::from("unquote")),
                                processed,
                            ]))
                        }
                    }

                    // Unquote-splicing: can't appear at top level of quasiquote
                    "unquote-splicing" => {
                        if depth == 0 {
                            Err(EvalError::InvalidSyntax(
                                "unquote-splicing not in list context".to_string(),
                            ))
                        } else {
                            // Inside nested quasiquote: preserve, decrement depth
                            let (inner, rest) = evaluator.extract_pair(cdr)?;
                            if !matches!(rest, Value::Null) {
                                return Err(EvalError::InvalidSyntax(
                                    "unquote-splicing expects exactly one argument".to_string(),
                                ));
                            }
                            let processed =
                                eval_quasiquote_impl(evaluator, &inner, env, depth - 1)?;
                            Ok(list_from_vec(vec![
                                Value::Symbol(Rc::from("unquote-splicing")),
                                processed,
                            ]))
                        }
                    }

                    _ => {
                        // Regular symbol: process as normal pair
                        process_quasiquote_pair(evaluator, expr, env, depth)
                    }
                }
            } else {
                // Non-symbol car: process as normal pair
                process_quasiquote_pair(evaluator, expr, env, depth)
            }
        }

        // Other types: return as-is
        _ => Ok(expr.clone()),
    }
}

/// Process a regular pair in quasiquote context
///
/// This handles the case where we have a list that might contain unquote-splicing
fn process_quasiquote_pair(
    evaluator: &Evaluator,
    expr: &Value,
    env: &Rc<Environment>,
    depth: i32,
) -> Result<Value, EvalError> {
    // Convert to vector for easier manipulation
    let mut elements = Vec::new();
    let mut current = expr.clone();
    let mut tail = Value::Null; // For improper lists

    // Walk the list
    loop {
        match &current {
            Value::Null => break,
            Value::Pair(pair) => {
                let car = &pair.0;
                let cdr = &pair.1;

                // Check if this element is (unquote-splicing ...)
                if depth == 0
                    && let Value::Pair(inner_pair) = car
                    && let Value::Symbol(sym) = &inner_pair.0
                    && sym.as_ref() == "unquote-splicing"
                {
                    // Evaluate the splicing expression
                    let (splice_expr, rest) = evaluator.extract_pair(&inner_pair.1)?;
                    if !matches!(rest, Value::Null) {
                        return Err(EvalError::InvalidSyntax(
                            "unquote-splicing expects exactly one argument".to_string(),
                        ));
                    }

                    let splice_result = evaluator.eval_in_env(&splice_expr, env)?;

                    // Must be a list
                    if !is_list(&splice_result) {
                        return Err(EvalError::InvalidSyntax(
                            "unquote-splicing result must be a list".to_string(),
                        ));
                    }

                    // Append all elements from the spliced list
                    let mut splice_current = splice_result;
                    while let Value::Pair(splice_pair) = splice_current {
                        elements.push(splice_pair.0.clone());
                        splice_current = splice_pair.1.clone();
                    }

                    // After splicing, check if CDR is an unquote form for improper list tail
                    if let Value::Pair(cdr_pair) = cdr
                        && let Value::Symbol(sym) = &cdr_pair.0
                        && sym.as_ref() == "unquote"
                    {
                        // Evaluate the unquote expression as the tail
                        let (unquote_expr, rest) = evaluator.extract_pair(&cdr_pair.1)?;
                        if !matches!(rest, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "unquote expects exactly one argument".to_string(),
                            ));
                        }
                        tail = evaluator.eval_in_env(&unquote_expr, env)?;
                        break;
                    }

                    current = cdr.clone();
                    continue;
                }

                // Check if CDR is an unquote form (for improper lists like (a . ,x))
                if depth == 0
                    && let Value::Pair(cdr_pair) = cdr
                    && let Value::Symbol(sym) = &cdr_pair.0
                    && sym.as_ref() == "unquote"
                {
                    // This is an improper list: (... car . ,expr)
                    // Process car normally, then evaluate the unquote as tail
                    let processed_car = eval_quasiquote_impl(evaluator, car, env, depth)?;
                    elements.push(processed_car);

                    // Evaluate the unquote expression
                    let (unquote_expr, rest) = evaluator.extract_pair(&cdr_pair.1)?;
                    if !matches!(rest, Value::Null) {
                        return Err(EvalError::InvalidSyntax(
                            "unquote expects exactly one argument".to_string(),
                        ));
                    }
                    tail = evaluator.eval_in_env(&unquote_expr, env)?;
                    break;
                }

                // Regular element: process recursively
                let processed = eval_quasiquote_impl(evaluator, car, env, depth)?;
                elements.push(processed);
                current = cdr.clone();
            }
            _ => {
                // Improper list (dotted pair with non-list tail)
                tail = eval_quasiquote_impl(evaluator, &current, env, depth)?;
                break;
            }
        }
    }

    // Reconstruct the list
    if matches!(tail, Value::Null) {
        Ok(list_from_vec(elements))
    } else {
        // Improper list
        let mut result = tail;
        for elem in elements.iter().rev() {
            result = Value::Pair(Rc::new((elem.clone(), result)));
        }
        Ok(result)
    }
}

/// Helper: check if a value is a proper list
fn is_list(val: &Value) -> bool {
    let mut current = val;
    loop {
        match current {
            Value::Null => return true,
            Value::Pair(pair) => current = &pair.1,
            _ => return false,
        }
    }
}

/// Helper: convert vector to list
fn vector_to_list(vec: &[Value]) -> Value {
    list_from_vec(vec.to_vec())
}

/// Helper: convert list to vector
fn list_to_vector(list: &Value) -> Result<Value, EvalError> {
    let mut vec = Vec::new();
    let mut current = list;

    loop {
        match current {
            Value::Null => break,
            Value::Pair(pair) => {
                vec.push(pair.0.clone());
                current = &pair.1;
            }
            _ => {
                return Err(EvalError::InvalidSyntax(
                    "Cannot convert improper list to vector".to_string(),
                ));
            }
        }
    }

    Ok(Value::Vector(Rc::new(RefCell::new(vec))))
}

/// Helper: construct list from vector
fn list_from_vec(vec: Vec<Value>) -> Value {
    let mut result = Value::Null;
    for item in vec.into_iter().rev() {
        result = Value::Pair(Rc::new((item, result)));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quasiquote_form_name() {
        let form = QuasiquoteForm;
        assert_eq!(form.name(), "quasiquote");
    }

    #[test]
    fn test_quasiquote_form_help() {
        let form = QuasiquoteForm;
        assert!(form.help().contains("quasiquote"));
        assert!(form.help().contains("template"));
        assert!(!form.help().is_empty());
    }
}
