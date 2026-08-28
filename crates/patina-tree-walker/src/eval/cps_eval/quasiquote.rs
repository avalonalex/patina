//! Quasiquote Evaluator
//!
//! This module provides quasiquote evaluation for the CPS evaluator.
//! The main entry point is `eval_quasiquote_in_env()` which handles
//! quasiquote templates with proper nesting of quasiquote/unquote.
//!
//! This implementation works directly with TaggedValue and the shared heap.

use super::eval_cps;
use crate::eval::error::EvalError;
use patina_core::tagged_value::TaggedValue;
use patina_frontend::Desugarer;
use patina_runtime::environment::Environment;
use std::rc::Rc;

/// Evaluate a TaggedValue expression using CPS evaluation
///
/// This is used by quasiquote to evaluate unquote expressions with full
/// continuation support. The expression is desugared and evaluated via CPS.
fn eval_tagged_via_cps(
    evaluator: &crate::eval::Evaluator,
    expr: TaggedValue,
    env: &Rc<Environment>,
) -> Result<TaggedValue, EvalError> {
    let heap = evaluator.global_env.heap();
    let desugarer = Desugarer::with_env(env.clone());
    // A failure here is the program's, not the interpreter's: `,if` names a
    // syntactic keyword, and #89 made the desugarer say so. `InternalError`
    // mis-stated that as an interpreter bug and hid the diagnostic behind its
    // own wrapper — see the VM's `quasiquote_expand::desugar_tagged`, which
    // used to panic outright on the same input.
    let core_expr = desugarer.desugar_tagged(expr, heap).map_err(|e| {
        EvalError::InvalidSyntax(match e {
            patina_frontend::DesugarError::InvalidSyntax(message) => message,
            other => other.to_string(),
        })
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
    expr: TaggedValue,
    env: &Rc<Environment>,
    depth: i32,
) -> Result<TaggedValue, EvalError> {
    let heap = evaluator.global_env.heap();

    // Handle immediate values that are self-evaluating
    if expr.is_fixnum() || expr.is_boolean() || expr.is_char() || expr.is_null() {
        return Ok(expr);
    }

    // Check for symbols - return as-is (quoted)
    if heap.borrow().is_symbol(expr) {
        return Ok(expr);
    }

    // Check for identifiers - convert to symbol for consistency in quoted context
    if heap.borrow().is_identifier(expr) {
        let name_owned: Option<String> = heap
            .borrow()
            .get_symbol_or_identifier_name(expr)
            .map(String::from);
        if let Some(name) = name_owned {
            let sym = heap.borrow_mut().intern_symbol(&name);
            return Ok(sym);
        }
        return Ok(expr);
    }

    // Check for strings and bytevectors - return as-is
    if expr.is_string() || heap.borrow().is_bytevector(expr) {
        return Ok(expr);
    }

    // Handle pairs (the interesting case)
    if expr.is_pair() {
        let (car, cdr) = get_pair_parts(evaluator, expr)?;

        // Check if car is a special symbol - convert to owned String to avoid borrow issues
        let sym_name: Option<String> = heap
            .borrow()
            .get_symbol_or_identifier_name(car)
            .map(String::from);

        if let Some(name) = sym_name.as_deref() {
            match name {
                // Nested quasiquote: increment depth
                "quasiquote" => {
                    let (inner, rest) = extract_pair_tagged(evaluator, cdr)?;
                    if !rest.is_null() {
                        return Err(EvalError::InvalidSyntax(
                            "quasiquote expects exactly one argument".to_string(),
                        ));
                    }
                    let processed = eval_quasiquote_impl(evaluator, inner, env, depth + 1)?;
                    return Ok(make_list_2(evaluator, "quasiquote", processed));
                }

                // Unquote: evaluate if at depth 0, otherwise decrement depth
                "unquote" => {
                    let (inner, rest) = extract_pair_tagged(evaluator, cdr)?;
                    if !rest.is_null() {
                        return Err(EvalError::InvalidSyntax(
                            "unquote expects exactly one argument".to_string(),
                        ));
                    }

                    if depth == 0 {
                        // At quasiquote level: evaluate the unquoted expression via CPS
                        return eval_tagged_via_cps(evaluator, inner, env);
                    } else {
                        // Inside nested quasiquote: preserve unquote, decrement depth
                        let processed = eval_quasiquote_impl(evaluator, inner, env, depth - 1)?;
                        return Ok(make_list_2(evaluator, "unquote", processed));
                    }
                }

                // Unquote-splicing: can't appear at top level of quasiquote
                "unquote-splicing" => {
                    if depth == 0 {
                        return Err(EvalError::InvalidSyntax(
                            "unquote-splicing not in list context".to_string(),
                        ));
                    } else {
                        // Inside nested quasiquote: preserve, decrement depth
                        let (inner, rest) = extract_pair_tagged(evaluator, cdr)?;
                        if !rest.is_null() {
                            return Err(EvalError::InvalidSyntax(
                                "unquote-splicing expects exactly one argument".to_string(),
                            ));
                        }
                        let processed = eval_quasiquote_impl(evaluator, inner, env, depth - 1)?;
                        return Ok(make_list_2(evaluator, "unquote-splicing", processed));
                    }
                }

                _ => {
                    // Regular symbol: process as normal pair
                    return process_quasiquote_pair(evaluator, expr, env, depth);
                }
            }
        } else {
            // Non-symbol car: process as normal pair
            return process_quasiquote_pair(evaluator, expr, env, depth);
        }
    }

    // Check for vectors - convert to list, process, convert back
    if expr.is_vector() {
        let list = vector_to_list_tagged(evaluator, expr)?;
        let processed = eval_quasiquote_impl(evaluator, list, env, depth)?;
        return list_to_vector_tagged(evaluator, processed);
    }

    // Other types: return as-is
    Ok(expr)
}

/// Process a regular pair in quasiquote context
///
/// This handles the case where we have a list that might contain unquote-splicing
fn process_quasiquote_pair(
    evaluator: &crate::eval::Evaluator,
    expr: TaggedValue,
    env: &Rc<Environment>,
    depth: i32,
) -> Result<TaggedValue, EvalError> {
    let heap = evaluator.global_env.heap();

    // Collect elements and handle splicing
    let mut elements: Vec<TaggedValue> = Vec::new();
    let mut current = expr;
    let mut tail = TaggedValue::NULL; // For improper lists

    // Walk the list
    loop {
        if current.is_null() {
            break;
        }

        if !current.is_pair() {
            // Improper list (dotted pair with non-list tail)
            tail = eval_quasiquote_impl(evaluator, current, env, depth)?;
            break;
        }

        let (car, cdr) = get_pair_parts(evaluator, current)?;

        // Check if this element is (unquote-splicing ...)
        if depth == 0 && car.is_pair() {
            let (inner_car, inner_cdr) = get_pair_parts(evaluator, car)?;

            if heap.borrow().is_named(inner_car, "unquote-splicing") {
                // Evaluate the splicing expression via CPS
                let (splice_expr, rest) = extract_pair_tagged(evaluator, inner_cdr)?;
                if !rest.is_null() {
                    return Err(EvalError::InvalidSyntax(
                        "unquote-splicing expects exactly one argument".to_string(),
                    ));
                }

                let splice_result = eval_tagged_via_cps(evaluator, splice_expr, env)?;

                // Must be a list
                if !is_list_tagged(evaluator, splice_result) {
                    return Err(EvalError::InvalidSyntax(
                        "unquote-splicing result must be a list".to_string(),
                    ));
                }

                // Append all elements from the spliced list
                let mut splice_current = splice_result;
                while !splice_current.is_null() {
                    if splice_current.is_pair() {
                        let (sc, sn) = get_pair_parts(evaluator, splice_current)?;
                        elements.push(sc);
                        splice_current = sn;
                    } else {
                        break;
                    }
                }

                // Check if CDR is an unquote form for improper list tail
                if cdr.is_pair() {
                    let (cdr_car, cdr_cdr) = get_pair_parts(evaluator, cdr)?;
                    if heap.borrow().is_named(cdr_car, "unquote") {
                        let (unquote_expr, rest) = extract_pair_tagged(evaluator, cdr_cdr)?;
                        if !rest.is_null() {
                            return Err(EvalError::InvalidSyntax(
                                "unquote expects exactly one argument".to_string(),
                            ));
                        }
                        tail = eval_tagged_via_cps(evaluator, unquote_expr, env)?;
                        break;
                    }
                }

                current = cdr;
                continue;
            }
        }

        // Check if CDR is an unquote form (for improper lists like (a . ,x))
        if depth == 0 && cdr.is_pair() {
            let (cdr_car, cdr_cdr) = get_pair_parts(evaluator, cdr)?;
            if heap.borrow().is_named(cdr_car, "unquote") {
                // This is an improper list: (... car . ,expr)
                // Process car normally, then evaluate the unquote as tail
                let processed_car = eval_quasiquote_impl(evaluator, car, env, depth)?;
                elements.push(processed_car);

                // Evaluate the unquote expression via CPS
                let (unquote_expr, rest) = extract_pair_tagged(evaluator, cdr_cdr)?;
                if !rest.is_null() {
                    return Err(EvalError::InvalidSyntax(
                        "unquote expects exactly one argument".to_string(),
                    ));
                }
                tail = eval_tagged_via_cps(evaluator, unquote_expr, env)?;
                break;
            }
        }

        // Regular element: process recursively
        let processed = eval_quasiquote_impl(evaluator, car, env, depth)?;
        elements.push(processed);
        current = cdr;
    }

    // Reconstruct the list using heap-allocated pairs
    let heap = evaluator.global_env.heap();
    let result = heap.borrow_mut().list_from_iter_with_tail(elements, tail);
    Ok(result)
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Get car and cdr from a pair (either native heap pair or boxed pair)
fn get_pair_parts(
    evaluator: &crate::eval::Evaluator,
    pair: TaggedValue,
) -> Result<(TaggedValue, TaggedValue), EvalError> {
    let heap = evaluator.global_env.heap();

    // Use heap's try_pair which handles both native and boxed pairs
    if let Some((car, cdr)) = heap.borrow().try_pair(pair) {
        return Ok((car, cdr));
    }

    // Check if it's null (not a pair)
    if pair.is_null() {
        return Err(EvalError::InvalidSyntax(
            "Expected pair, got null".to_string(),
        ));
    }

    Err(EvalError::InvalidSyntax("Expected pair".to_string()))
}

/// Extract car and cdr from a pair as TaggedValue
fn extract_pair_tagged(
    evaluator: &crate::eval::Evaluator,
    value: TaggedValue,
) -> Result<(TaggedValue, TaggedValue), EvalError> {
    get_pair_parts(evaluator, value)
}

/// Check if a value is a proper list
fn is_list_tagged(evaluator: &crate::eval::Evaluator, val: TaggedValue) -> bool {
    let heap = evaluator.global_env.heap();
    let mut current = val;

    loop {
        if current.is_null() {
            return true;
        }
        if current.is_pair() {
            let heap_ref = heap.borrow();
            current = heap_ref.cdr(current);
            continue;
        }
        return false;
    }
}

/// Make a 2-element list with a symbol as first element
fn make_list_2(
    evaluator: &crate::eval::Evaluator,
    sym_name: &str,
    second: TaggedValue,
) -> TaggedValue {
    let heap = evaluator.global_env.heap();
    let mut heap_ref = heap.borrow_mut();
    let sym = heap_ref.intern_symbol(sym_name);
    let inner = heap_ref.alloc_pair(second, TaggedValue::NULL);
    heap_ref.alloc_pair(sym, inner)
}

/// Convert vector to list (TaggedValue version)
fn vector_to_list_tagged(
    evaluator: &crate::eval::Evaluator,
    vec_tv: TaggedValue,
) -> Result<TaggedValue, EvalError> {
    let heap = evaluator.global_env.heap();

    // Handle native vectors (from tagged path)
    if vec_tv.is_vector() {
        let borrowed = heap.borrow();
        let len = borrowed.vector_len(vec_tv);
        let elements: Vec<TaggedValue> = (0..len).map(|i| borrowed.vector_ref(vec_tv, i)).collect();
        drop(borrowed);
        return Ok(heap.borrow_mut().list_from_iter(elements));
    }

    // Handle vectors
    let elements = heap
        .borrow()
        .try_vector_to_vec(vec_tv)
        .ok_or_else(|| EvalError::InvalidSyntax("Expected vector".to_string()))?;

    Ok(heap.borrow_mut().list_from_iter(elements))
}

/// Convert list to vector (TaggedValue version)
fn list_to_vector_tagged(
    evaluator: &crate::eval::Evaluator,
    list: TaggedValue,
) -> Result<TaggedValue, EvalError> {
    let heap = evaluator.global_env.heap();
    let mut elements: Vec<TaggedValue> = Vec::new();
    let mut current = list;

    loop {
        if current.is_null() {
            break;
        }
        if current.is_pair() {
            let (car, cdr) = get_pair_parts(evaluator, current)?;
            elements.push(car);
            current = cdr;
        } else {
            return Err(EvalError::InvalidSyntax(
                "Cannot convert improper list to vector".to_string(),
            ));
        }
    }

    // Allocate native vector on heap
    Ok(heap.borrow_mut().alloc_vector(elements))
}

// =============================================================================
// Public API for CPS Evaluator
// =============================================================================

/// Evaluate a quasiquote template in the given environment
///
/// This is the public entry point for the CPS evaluator. Takes and returns
/// TaggedValue directly to avoid unnecessary conversions.
///
/// Unquote expressions within quasiquote are evaluated via CPS for full
/// continuation support.
pub fn eval_quasiquote_in_env(
    evaluator: &crate::eval::Evaluator,
    template: TaggedValue,
    env: &Rc<Environment>,
) -> Result<TaggedValue, EvalError> {
    eval_quasiquote_impl(evaluator, template, env, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_core::TaggedValue;
    use patina_ir::{CoreExpr, CoreExprKind, Formals, ScopedParam};
    use patina_runtime::ScopeSet;

    #[test]
    fn test_eval_literal() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::new(CoreExprKind::Literal(TaggedValue::fixnum(42)));

        let result = eval_cps(&expr, env, &evaluator).unwrap();
        assert_eq!(result, TaggedValue::fixnum(42));
    }

    #[test]
    fn test_eval_quote() {
        let evaluator = crate::eval::Evaluator::new();
        let env = evaluator.global_env.clone();
        let heap = evaluator.global_env.heap();
        let sym = heap.borrow_mut().intern_symbol("x");
        let expr = CoreExpr::new(CoreExprKind::Quote(sym));

        let result = eval_cps(&expr, env, &evaluator).unwrap();
        assert!(heap.borrow().is_symbol(result));
    }

    #[test]
    fn test_eval_variable() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        env.define("x".to_string(), TaggedValue::fixnum(42));

        let expr = CoreExpr::new(CoreExprKind::Var {
            name: Rc::from("x"),
            scopes: ScopeSet::new(),
        });
        let result = eval_cps(&expr, env, &evaluator).unwrap();
        assert_eq!(result, TaggedValue::fixnum(42));
    }

    #[test]
    fn test_eval_variable_unbound() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::new(CoreExprKind::Var {
            name: Rc::from("undefined"),
            scopes: ScopeSet::new(),
        });

        let result = eval_cps(&expr, env, &evaluator);
        assert!(result.is_err());
    }

    #[test]
    fn test_eval_if_true() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::new(CoreExprKind::If {
            test: Rc::new(CoreExpr::new(CoreExprKind::Literal(TaggedValue::TRUE))),
            then: Rc::new(CoreExpr::new(CoreExprKind::Literal(TaggedValue::fixnum(1)))),
            else_: Rc::new(CoreExpr::new(CoreExprKind::Literal(TaggedValue::fixnum(2)))),
        });

        let result = eval_cps(&expr, env, &evaluator).unwrap();
        assert_eq!(result, TaggedValue::fixnum(1));
    }

    #[test]
    fn test_eval_if_false() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::new(CoreExprKind::If {
            test: Rc::new(CoreExpr::new(CoreExprKind::Literal(TaggedValue::FALSE))),
            then: Rc::new(CoreExpr::new(CoreExprKind::Literal(TaggedValue::fixnum(1)))),
            else_: Rc::new(CoreExpr::new(CoreExprKind::Literal(TaggedValue::fixnum(2)))),
        });

        let result = eval_cps(&expr, env, &evaluator).unwrap();
        assert_eq!(result, TaggedValue::fixnum(2));
    }

    #[test]
    fn test_eval_define() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::new(CoreExprKind::Define {
            name: Rc::from("x"),
            scopes: ScopeSet::new(),
            value: Rc::new(CoreExpr::new(CoreExprKind::Literal(TaggedValue::fixnum(
                42,
            )))),
        });

        let result = eval_cps(&expr, env.clone(), &evaluator).unwrap();
        assert_eq!(result, TaggedValue::UNSPECIFIED);

        let x_val = env.get(&Rc::from("x")).unwrap();
        assert_eq!(x_val, TaggedValue::fixnum(42));
    }

    #[test]
    fn test_eval_set() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        env.define("x".to_string(), TaggedValue::fixnum(1));

        let expr = CoreExpr::new(CoreExprKind::Set {
            var: Rc::from("x"),
            scopes: ScopeSet::new(),
            value: Rc::new(CoreExpr::new(CoreExprKind::Literal(TaggedValue::fixnum(
                42,
            )))),
        });

        let result = eval_cps(&expr, env.clone(), &evaluator).unwrap();
        assert_eq!(result, TaggedValue::UNSPECIFIED);

        let x_val = env.get(&Rc::from("x")).unwrap();
        assert_eq!(x_val, TaggedValue::fixnum(42));
    }

    #[test]
    fn test_eval_begin() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::new(CoreExprKind::Begin(vec![
            CoreExpr::new(CoreExprKind::Literal(TaggedValue::fixnum(1))),
            CoreExpr::new(CoreExprKind::Literal(TaggedValue::fixnum(2))),
            CoreExpr::new(CoreExprKind::Literal(TaggedValue::fixnum(3))),
        ]));

        let result = eval_cps(&expr, env, &evaluator).unwrap();
        assert_eq!(result, TaggedValue::fixnum(3));
    }

    #[test]
    fn test_eval_lambda() {
        let evaluator = crate::eval::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::new(CoreExprKind::Lambda {
            params: Formals::Fixed(vec![ScopedParam::simple(Rc::from("x"))]),
            body: vec![CoreExpr::new(CoreExprKind::Var {
                name: Rc::from("x"),
                scopes: ScopeSet::new(),
            })],
            binding_scopes: std::rc::Rc::new(patina_core::ScopeSet::new()),
        });

        let result = eval_cps(&expr, env, &evaluator).unwrap();
        assert!(
            evaluator
                .global_env
                .heap()
                .borrow()
                .get_procedure(result)
                .is_some()
        );
    }
}
