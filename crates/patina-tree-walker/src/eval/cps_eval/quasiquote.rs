//! Quasiquote Evaluator
//!
//! This module provides quasiquote evaluation for the CPS evaluator.
//! The main entry point is `eval_quasiquote_in_env()` which handles
//! quasiquote templates with proper nesting of quasiquote/unquote.

use super::eval_cps;
use crate::eval::error::EvalError;
use patina_frontend::Desugarer;
use patina_runtime::environment::Environment;
use patina_runtime::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate a Value expression using CPS evaluation
///
/// This is used by quasiquote to evaluate unquote expressions with full
/// continuation support. The expression is desugared and evaluated via CPS.
fn eval_value_via_cps(
    evaluator: &crate::eval::Evaluator,
    expr: &Value,
    env: &Rc<Environment>,
) -> Result<Value, EvalError> {
    let desugarer = Desugarer::with_env(env.clone());
    let core_expr = desugarer.desugar(expr).map_err(|e| {
        EvalError::InternalError(format!("quasiquote: failed to desugar unquote: {}", e))
    })?;
    eval_cps(&core_expr, env.clone(), evaluator)
}

/// Implementation of quasiquote with depth tracking
///
/// The depth parameter tracks nesting level:
/// - depth 0: at current quasiquote level (unquotes are active)
/// - depth > 0: inside nested quasiquote (unquotes become quoted)
fn eval_quasiquote_impl(
    evaluator: &crate::eval::Evaluator,
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
        | Value::Complex(_)
        | Value::Character(_)
        | Value::String(_)
        | Value::Bytevector(_)
        | Value::Unspecified => Ok(expr.clone()),

        // Symbols and null: quote them (return as-is)
        // Identifiers (from macro expansion) are converted to Symbols for consistency
        Value::Symbol(_) | Value::Null => Ok(expr.clone()),
        Value::Identifier(id) => Ok(Value::Symbol(id.name.clone())),

        // Vectors: convert to list, process, convert back
        Value::Vector(vec) => {
            let list = vector_to_list(&vec.borrow());
            let processed = eval_quasiquote_impl(evaluator, &list, env, depth)?;
            list_to_vector(&processed)
        }

        // Pairs: the interesting case
        Value::Pair(pair) => {
            let borrowed = pair.borrow();
            let car = &borrowed.0;
            let cdr = &borrowed.1;

            // Extract symbol name from either Symbol or Identifier
            // After macro expansion, special forms may be Identifiers with scopes
            let sym_name = match car {
                Value::Symbol(s) => Some(s.as_ref()),
                Value::Identifier(id) => Some(id.name.as_ref()),
                _ => None,
            };

            // Check if car is a symbol/identifier that requires special handling
            if let Some(name) = sym_name {
                match name {
                    // Nested quasiquote: increment depth
                    "quasiquote" => {
                        let (inner, rest) = extract_pair(cdr)?;
                        if !matches!(rest, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "quasiquote expects exactly one argument".to_string(),
                            ));
                        }
                        let processed = eval_quasiquote_impl(evaluator, &inner, env, depth + 1)?;
                        Ok(list_from_vec(vec![Value::symbol("quasiquote"), processed]))
                    }

                    // Unquote: evaluate if at depth 0, otherwise decrement depth
                    "unquote" => {
                        let (inner, rest) = extract_pair(cdr)?;
                        if !matches!(rest, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "unquote expects exactly one argument".to_string(),
                            ));
                        }

                        if depth == 0 {
                            // At quasiquote level: evaluate the unquoted expression via CPS
                            eval_value_via_cps(evaluator, &inner, env)
                        } else {
                            // Inside nested quasiquote: preserve unquote, decrement depth
                            let processed =
                                eval_quasiquote_impl(evaluator, &inner, env, depth - 1)?;
                            Ok(list_from_vec(vec![Value::symbol("unquote"), processed]))
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
                            let (inner, rest) = extract_pair(cdr)?;
                            if !matches!(rest, Value::Null) {
                                return Err(EvalError::InvalidSyntax(
                                    "unquote-splicing expects exactly one argument".to_string(),
                                ));
                            }
                            let processed =
                                eval_quasiquote_impl(evaluator, &inner, env, depth - 1)?;
                            Ok(list_from_vec(vec![
                                Value::symbol("unquote-splicing"),
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
    evaluator: &crate::eval::Evaluator,
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
        match current {
            Value::Null => break,
            Value::Pair(ref pair) => {
                let (car, cdr) = {
                    let borrowed = pair.borrow();
                    (borrowed.0.clone(), borrowed.1.clone())
                };

                // Check if this element is (unquote-splicing ...)
                if depth == 0
                    && let Value::Pair(ref inner_pair) = car
                {
                    let (inner_car, inner_cdr) = {
                        let b = inner_pair.borrow();
                        (b.0.clone(), b.1.clone())
                    };
                    if is_named(&inner_car, "unquote-splicing") {
                        // Evaluate the splicing expression via CPS
                        let (splice_expr, rest) = extract_pair(&inner_cdr)?;
                        if !matches!(rest, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "unquote-splicing expects exactly one argument".to_string(),
                            ));
                        }

                        let splice_result = eval_value_via_cps(evaluator, &splice_expr, env)?;

                        // Must be a list
                        if !is_list(&splice_result) {
                            return Err(EvalError::InvalidSyntax(
                                "unquote-splicing result must be a list".to_string(),
                            ));
                        }

                        // Append all elements from the spliced list
                        let mut splice_current = splice_result;
                        while let Value::Pair(ref sp) = splice_current {
                            let (sc, sn) = {
                                let b = sp.borrow();
                                (b.0.clone(), b.1.clone())
                            };
                            elements.push(sc);
                            splice_current = sn;
                        }

                        // After splicing, check if CDR is an unquote form for improper list tail
                        if let Value::Pair(ref cdr_pair) = cdr {
                            let (cdr_car, cdr_cdr) = {
                                let b = cdr_pair.borrow();
                                (b.0.clone(), b.1.clone())
                            };
                            if is_named(&cdr_car, "unquote") {
                                // Evaluate the unquote expression as the tail via CPS
                                let (unquote_expr, rest) = extract_pair(&cdr_cdr)?;
                                if !matches!(rest, Value::Null) {
                                    return Err(EvalError::InvalidSyntax(
                                        "unquote expects exactly one argument".to_string(),
                                    ));
                                }
                                tail = eval_value_via_cps(evaluator, &unquote_expr, env)?;
                                break;
                            }
                        }

                        current = cdr;
                        continue;
                    }
                }

                // Check if CDR is an unquote form (for improper lists like (a . ,x))
                if depth == 0
                    && let Value::Pair(ref cdr_pair) = cdr
                {
                    let (cdr_car, cdr_cdr) = {
                        let b = cdr_pair.borrow();
                        (b.0.clone(), b.1.clone())
                    };
                    if is_named(&cdr_car, "unquote") {
                        // This is an improper list: (... car . ,expr)
                        // Process car normally, then evaluate the unquote as tail
                        let processed_car = eval_quasiquote_impl(evaluator, &car, env, depth)?;
                        elements.push(processed_car);

                        // Evaluate the unquote expression via CPS
                        let (unquote_expr, rest) = extract_pair(&cdr_cdr)?;
                        if !matches!(rest, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "unquote expects exactly one argument".to_string(),
                            ));
                        }
                        tail = eval_value_via_cps(evaluator, &unquote_expr, env)?;
                        break;
                    }
                }

                // Regular element: process recursively
                let processed = eval_quasiquote_impl(evaluator, &car, env, depth)?;
                elements.push(processed);
                current = cdr;
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
            result = Value::Pair(Rc::new(RefCell::new((elem.clone(), result))));
        }
        Ok(result)
    }
}

/// Helper: extract pair into (car, cdr)
fn extract_pair(value: &Value) -> Result<(Value, Value), EvalError> {
    match value {
        Value::Pair(pair) => {
            let borrowed = pair.borrow();
            Ok((borrowed.0.clone(), borrowed.1.clone()))
        }
        _ => Err(EvalError::InvalidSyntax("Expected pair".to_string())),
    }
}

/// Helper: check if a value is a symbol/identifier with a given name
/// After macro expansion, symbols may become Identifiers with scopes
fn is_named(value: &Value, name: &str) -> bool {
    match value {
        Value::Symbol(s) => s.as_ref() == name,
        Value::Identifier(id) => id.name.as_ref() == name,
        _ => false,
    }
}

/// Helper: check if a value is a proper list
fn is_list(val: &Value) -> bool {
    let mut current = val.clone();
    loop {
        match current {
            Value::Null => return true,
            Value::Pair(ref pair) => {
                let next = pair.borrow().1.clone();
                current = next;
            }
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
    let mut current = list.clone();

    loop {
        match current {
            Value::Null => break,
            Value::Pair(ref pair) => {
                let (car, cdr) = {
                    let borrowed = pair.borrow();
                    (borrowed.0.clone(), borrowed.1.clone())
                };
                vec.push(car);
                current = cdr;
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
        result = Value::Pair(Rc::new(RefCell::new((item, result))));
    }
    result
}

// =============================================================================
// Public API for CPS Evaluator
// =============================================================================

/// Evaluate a quasiquote template in the given environment
///
/// This is a public wrapper around `eval_quasiquote_impl` for use by the CPS evaluator.
/// Unquote expressions within quasiquote are evaluated via CPS for full continuation
/// support.
pub fn eval_quasiquote_in_env(
    evaluator: &crate::eval::Evaluator,
    template: &Value,
    env: &Rc<Environment>,
) -> Result<Value, EvalError> {
    eval_quasiquote_impl(evaluator, template, env, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_ir::{CoreExpr, Formals, ScopedParam};
    use patina_runtime::ScopeSet;

    #[test]
    fn test_eval_literal() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::Literal(Rc::new(Value::Integer(42)));

        let result = eval_cps(&expr, env, &evaluator).unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_eval_quote() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::Quote(Rc::new(Value::symbol("x")));

        let result = eval_cps(&expr, env, &evaluator).unwrap();
        assert!(matches!(result, Value::Symbol(_)));
    }

    #[test]
    fn test_eval_variable() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        env.define("x".to_string(), Value::Integer(42));

        let expr = CoreExpr::Var {
            name: Rc::from("x"),
            scopes: ScopeSet::new(),
        };
        let result = eval_cps(&expr, env, &evaluator).unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_eval_variable_unbound() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::Var {
            name: Rc::from("undefined"),
            scopes: ScopeSet::new(),
        };

        let result = eval_cps(&expr, env, &evaluator);
        assert!(result.is_err());
    }

    #[test]
    fn test_eval_if_true() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::If {
            test: Rc::new(CoreExpr::Literal(Rc::new(Value::Boolean(true)))),
            then: Rc::new(CoreExpr::Literal(Rc::new(Value::Integer(1)))),
            else_: Rc::new(CoreExpr::Literal(Rc::new(Value::Integer(2)))),
        };

        let result = eval_cps(&expr, env, &evaluator).unwrap();
        assert!(matches!(result, Value::Integer(1)));
    }

    #[test]
    fn test_eval_if_false() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::If {
            test: Rc::new(CoreExpr::Literal(Rc::new(Value::Boolean(false)))),
            then: Rc::new(CoreExpr::Literal(Rc::new(Value::Integer(1)))),
            else_: Rc::new(CoreExpr::Literal(Rc::new(Value::Integer(2)))),
        };

        let result = eval_cps(&expr, env, &evaluator).unwrap();
        assert!(matches!(result, Value::Integer(2)));
    }

    #[test]
    fn test_eval_define() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::Define {
            name: Rc::from("x"),
            value: Rc::new(CoreExpr::Literal(Rc::new(Value::Integer(42)))),
        };

        let result = eval_cps(&expr, env.clone(), &evaluator).unwrap();
        assert!(matches!(result, Value::Unspecified));

        // Check that variable was defined
        let x_val = env.get(&Rc::from("x")).unwrap();
        assert!(matches!(x_val, Value::Integer(42)));
    }

    #[test]
    fn test_eval_set() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        env.define("x".to_string(), Value::Integer(1));

        let expr = CoreExpr::Set {
            var: Rc::from("x"),
            scopes: ScopeSet::new(),
            value: Rc::new(CoreExpr::Literal(Rc::new(Value::Integer(42)))),
        };

        let result = eval_cps(&expr, env.clone(), &evaluator).unwrap();
        assert!(matches!(result, Value::Unspecified));

        // Check that variable was updated
        let x_val = env.get(&Rc::from("x")).unwrap();
        assert!(matches!(x_val, Value::Integer(42)));
    }

    #[test]
    fn test_eval_begin() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::Begin(vec![
            CoreExpr::Literal(Rc::new(Value::Integer(1))),
            CoreExpr::Literal(Rc::new(Value::Integer(2))),
            CoreExpr::Literal(Rc::new(Value::Integer(3))),
        ]);

        let result = eval_cps(&expr, env, &evaluator).unwrap();
        // Should return last value
        assert!(matches!(result, Value::Integer(3)));
    }

    #[test]
    fn test_eval_lambda() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::Lambda {
            params: Formals::Fixed(vec![ScopedParam::simple(Rc::from("x"))]),
            body: vec![CoreExpr::Var {
                name: Rc::from("x"),
                scopes: ScopeSet::new(),
            }],
            binding_scope: None,
        };

        let result = eval_cps(&expr, env, &evaluator).unwrap();
        // Should return a procedure (CpsLambda now)
        assert!(matches!(result, Value::Procedure(_)));
    }
}
