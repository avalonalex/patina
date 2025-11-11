//! Higher-order function primitives (R7RS Section 6.4)
//!
//! Implements:
//! - `map` - Apply function to lists/vectors element-wise
//! - `for-each` - Iterate over lists/vectors for side effects
//!
//! INSTRUCTIONS: Move from primitives.rs:
//! - primitive_map()
//! - primitive_for_each()

use super::super::error::EvalError;
use super::super::Evaluator;
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

    let mut list_vecs: Vec<Vec<Value>> = Vec::new();
    for list in lists {
        let mut items = Vec::new();
        let mut current = list.clone();

        while let Value::Pair(pair) = current {
            items.push(pair.0.clone());
            current = pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::TypeError(
                "map: argument must be a proper list".to_string(),
            ));
        }

        list_vecs.push(items);
    }

    let min_len = list_vecs.iter().map(|v| v.len()).min().unwrap_or(0);

    let mut results = Vec::new();
    for i in 0..min_len {
        let mut proc_args = Vec::new();
        for list_vec in &list_vecs {
            proc_args.push(list_vec[i].clone());
        }

        // Procedure calls within map are not in tail position
        let result = match evaluator.apply(proc.clone(), proc_args, false)? {
            super::super::EvalResult::Value(v) => v,
            _ => {
                return Err(EvalError::InternalError(
                    "Unexpected tail call in map".to_string(),
                ))
            }
        };
        results.push(result);
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

    let mut list_vecs: Vec<Vec<Value>> = Vec::new();
    for list in lists {
        let mut items = Vec::new();
        let mut current = list.clone();

        while let Value::Pair(pair) = current {
            items.push(pair.0.clone());
            current = pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::TypeError(
                "for-each: argument must be a proper list".to_string(),
            ));
        }

        list_vecs.push(items);
    }

    let min_len = list_vecs.iter().map(|v| v.len()).min().unwrap_or(0);

    for i in 0..min_len {
        let mut proc_args = Vec::new();
        for list_vec in &list_vecs {
            proc_args.push(list_vec[i].clone());
        }

        // Procedure calls within for-each are not in tail position
        match evaluator.apply(proc.clone(), proc_args, false)? {
            super::super::EvalResult::Value(_) => {}
            _ => {
                return Err(EvalError::InternalError(
                    "Unexpected tail call in for-each".to_string(),
                ))
            }
        }
    }

    Ok(Value::Unspecified)
}
