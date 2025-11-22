//! Higher-order function primitives (R7RS Section 6.4)
//!
//! Implements:
//! - `map` - Apply function to lists/vectors element-wise
//! - `for-each` - Iterate over lists/vectors for side effects
//!
//! INSTRUCTIONS: Move from primitives.rs:
//! - primitive_map()
//! - primitive_for_each()

use super::super::Evaluator;
use super::super::error::EvalError;
use patina_runtime::value::Value;

pub(super) fn map(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 1".to_string(),
            actual: 0,
        });
    }

    let proc = &args[0];
    let lists = &args[1..];

    if lists.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: 1,
        });
    }

    // Initialize list cursors - track current position in each list
    let mut cursors: Vec<Value> = lists.to_vec();
    let mut results = Vec::new();

    // Iterate in parallel through all lists
    // Stop when ANY list is exhausted (reaches Null)
    // This handles circular lists correctly - we stop when the shortest non-circular list ends
    loop {
        // Check if any list is exhausted
        let mut all_have_elements = true;
        for cursor in &cursors {
            if matches!(cursor, Value::Null) {
                all_have_elements = false;
                break;
            }
        }

        if !all_have_elements {
            break;
        }

        // Collect elements from current position in each list
        let mut proc_args = Vec::new();
        for (i, cursor) in cursors.iter().enumerate() {
            match cursor {
                Value::Pair(pair) => {
                    let borrowed = pair.borrow();
                    proc_args.push(borrowed.0.clone());
                }
                Value::Null => {
                    // Already checked above, but handle for safety
                    break;
                }
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "map: argument {} must be a proper list (found improper list)",
                        i + 2 // +1 for proc, +1 for 1-indexed
                    )));
                }
            }
        }

        // Apply procedure to collected elements
        let result = match evaluator.apply(proc.clone(), proc_args, false)? {
            super::super::EvalResult::Value(v) => v,
            _ => {
                return Err(EvalError::InternalError(
                    "Unexpected tail call in map".to_string(),
                ));
            }
        };
        results.push(result);

        // Advance all cursors to next element
        for cursor in cursors.iter_mut() {
            if let Value::Pair(pair) = cursor {
                let next = {
                    let borrowed = pair.borrow();
                    borrowed.1.clone()
                }; // borrowed is dropped here
                *cursor = next;
            }
        }
    }

    Ok(evaluator.list_from_vec(results))
}

pub(super) fn for_each(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 1".to_string(),
            actual: 0,
        });
    }

    let proc = &args[0];
    let lists = &args[1..];

    if lists.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: 1,
        });
    }

    // Initialize list cursors - track current position in each list
    let mut cursors: Vec<Value> = lists.to_vec();

    // Iterate in parallel through all lists
    // Stop when ANY list is exhausted (reaches Null)
    // This handles circular lists correctly - we stop when the shortest non-circular list ends
    loop {
        // Check if any list is exhausted
        let mut all_have_elements = true;
        for cursor in &cursors {
            if matches!(cursor, Value::Null) {
                all_have_elements = false;
                break;
            }
        }

        if !all_have_elements {
            break;
        }

        // Collect elements from current position in each list
        let mut proc_args = Vec::new();
        for (i, cursor) in cursors.iter().enumerate() {
            match cursor {
                Value::Pair(pair) => {
                    let borrowed = pair.borrow();
                    proc_args.push(borrowed.0.clone());
                }
                Value::Null => {
                    // Already checked above, but handle for safety
                    break;
                }
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "for-each: argument {} must be a proper list (found improper list)",
                        i + 2 // +1 for proc, +1 for 1-indexed
                    )));
                }
            }
        }

        // Apply procedure to collected elements (for side effects)
        match evaluator.apply(proc.clone(), proc_args, false)? {
            super::super::EvalResult::Value(_) => {}
            _ => {
                return Err(EvalError::InternalError(
                    "Unexpected tail call in for-each".to_string(),
                ));
            }
        }

        // Advance all cursors to next element
        for cursor in cursors.iter_mut() {
            if let Value::Pair(pair) = cursor {
                let next = {
                    let borrowed = pair.borrow();
                    borrowed.1.clone()
                }; // borrowed is dropped here
                *cursor = next;
            }
        }
    }

    Ok(Value::Unspecified)
}
pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Map - apply procedure to each element
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "map",
        Arity::Min(2),
        "Applies proc element-wise to the elements of the lists and returns a list of the results.",
        |eval, args, _tail| map(eval, args).map(EvalResult::Value),
    ));

    // For-each - apply procedure for side effects
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "for-each",
        Arity::Min(2),
        "Applies proc element-wise to the elements of the lists for side effects.",
        |eval, args, _tail| for_each(eval, args).map(EvalResult::Value),
    ));
}
