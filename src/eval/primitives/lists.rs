//! List and pair primitive operations (R7RS Section 6.4)
//!
//! Implements pair construction, list manipulation, and search operations.
//!
//! INSTRUCTIONS: Move from primitives.rs:
//! - primitive_cons()
//! - primitive_car()
//! - primitive_cdr()
//! - primitive_length()
//! - primitive_append()
//! - primitive_reverse()
//! - primitive_list_ref()
//! - primitive_list_tail()
//! - primitive_memq()
//! - primitive_memv()
//! - primitive_member()
//! - primitive_assq()
//! - primitive_assv()
//! - primitive_assoc()

use super::super::error::EvalError;
use super::super::Evaluator;
use super::equality::{values_eq, values_equal, values_eqv};
use crate::value::Value;
use std::rc::Rc;

pub(super) fn cons(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "cons")?;
    Ok(Value::Pair(Rc::new((args[0].clone(), args[1].clone()))))
}

pub(super) fn car(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "car")?;
    match &args[0] {
        Value::Pair(pair) => Ok(pair.0.clone()),
        _ => Err(EvalError::TypeError("car expects a pair".to_string())),
    }
}

pub(super) fn cdr(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "cdr")?;
    match &args[0] {
        Value::Pair(pair) => Ok(pair.1.clone()),
        _ => Err(EvalError::TypeError("cdr expects a pair".to_string())),
    }
}

pub(super) fn length(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "length")?;
    let items = evaluator.list_to_vec(args[0].clone(), "length")?;
    Ok(Value::Integer(items.len() as i64))
}

pub(super) fn append(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Ok(Value::Null);
    }

    if args.len() == 1 {
        return Ok(args[0].clone());
    }

    let mut result_items = Vec::new();
    for (i, list) in args.iter().enumerate() {
        if i == args.len() - 1 {
            if result_items.is_empty() {
                return Ok(list.clone());
            }
            let mut result = list.clone();
            for item in result_items.into_iter().rev() {
                result = Value::Pair(Rc::new((item, result)));
            }
            return Ok(result);
        }

        let mut current = list.clone();
        while let Value::Pair(pair) = current {
            result_items.push(pair.0.clone());
            current = pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::TypeError(format!(
                "append: argument {} must be a proper list",
                i + 1
            )));
        }
    }

    Ok(evaluator.list_from_vec(result_items))
}

pub(super) fn reverse(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "reverse")?;
    let mut items = evaluator.list_to_vec(args[0].clone(), "reverse")?;
    items.reverse();
    Ok(evaluator.list_from_vec(items))
}

pub(super) fn list_ref(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "list-ref")?;

    let k = match &args[1] {
        Value::Integer(n) if *n >= 0 => *n as usize,
        Value::Integer(_) => {
            return Err(EvalError::TypeError(
                "list-ref: index must be non-negative".to_string(),
            ))
        }
        _ => {
            return Err(EvalError::TypeError(
                "list-ref: index must be an integer".to_string(),
            ))
        }
    };

    let items = evaluator.list_to_vec(args[0].clone(), "list-ref")?;
    items
        .get(k)
        .cloned()
        .ok_or_else(|| EvalError::TypeError("list-ref: index out of bounds".to_string()))
}

pub(super) fn list_tail(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "list-tail")?;

    let k = match &args[1] {
        Value::Integer(n) if *n >= 0 => *n as usize,
        Value::Integer(_) => {
            return Err(EvalError::TypeError(
                "list-tail: index must be non-negative".to_string(),
            ))
        }
        _ => {
            return Err(EvalError::TypeError(
                "list-tail: index must be an integer".to_string(),
            ))
        }
    };

    let items = evaluator.list_to_vec(args[0].clone(), "list-tail")?;
    if k > items.len() {
        return Err(EvalError::TypeError(
            "list-tail: index out of bounds".to_string(),
        ));
    }
    Ok(evaluator.list_from_vec(items[k..].to_vec()))
}

pub(super) fn memq(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "memq")?;

    let obj = &args[0];
    let mut current = args[1].clone();

    while let Value::Pair(pair) = current {
        if values_eq(obj, &pair.0) {
            return Ok(Value::Pair(pair));
        }
        current = pair.1.clone();
    }

    Ok(Value::Boolean(false))
}

pub(super) fn memv(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "memv")?;

    let obj = &args[0];
    let mut current = args[1].clone();

    while let Value::Pair(pair) = current {
        if values_eqv(obj, &pair.0) {
            return Ok(Value::Pair(pair));
        }
        current = pair.1.clone();
    }

    Ok(Value::Boolean(false))
}

pub(super) fn member(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "2 or 3".to_string(),
            actual: args.len(),
        });
    }

    let obj = &args[0];
    let mut current = args[1].clone();
    let compare_proc = args.get(2);

    while let Value::Pair(pair) = current.clone() {
        let matches = if let Some(proc) = compare_proc {
            // Custom comparison not in tail position
            let result =
                match evaluator.apply(proc.clone(), vec![obj.clone(), pair.0.clone()], false)? {
                    super::super::EvalResult::Value(v) => v,
                    _ => {
                        return Err(EvalError::InternalError(
                            "Unexpected tail call in member comparison".to_string(),
                        ))
                    }
                };
            result.is_truthy()
        } else {
            values_equal(obj, &pair.0)?
        };

        if matches {
            return Ok(Value::Pair(pair));
        }
        current = pair.1.clone();
    }

    Ok(Value::Boolean(false))
}

pub(super) fn assq(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "assq")?;

    let obj = &args[0];
    let mut current = args[1].clone();

    while let Value::Pair(pair) = current {
        if let Value::Pair(entry) = &pair.0 {
            if values_eq(obj, &entry.0) {
                return Ok(pair.0.clone());
            }
        }
        current = pair.1.clone();
    }

    Ok(Value::Boolean(false))
}

pub(super) fn assv(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "assv")?;

    let obj = &args[0];
    let mut current = args[1].clone();

    while let Value::Pair(pair) = current {
        if let Value::Pair(entry) = &pair.0 {
            if values_eqv(obj, &entry.0) {
                return Ok(pair.0.clone());
            }
        }
        current = pair.1.clone();
    }

    Ok(Value::Boolean(false))
}

pub(super) fn assoc(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "2 or 3".to_string(),
            actual: args.len(),
        });
    }

    let obj = &args[0];
    let mut current = args[1].clone();
    let compare_proc = args.get(2);

    while let Value::Pair(pair) = current.clone() {
        if let Value::Pair(entry) = &pair.0 {
            let matches = if let Some(proc) = compare_proc {
                // Custom comparison not in tail position
                let result = match evaluator.apply(
                    proc.clone(),
                    vec![obj.clone(), entry.0.clone()],
                    false,
                )? {
                    super::super::EvalResult::Value(v) => v,
                    _ => {
                        return Err(EvalError::InternalError(
                            "Unexpected tail call in assoc comparison".to_string(),
                        ))
                    }
                };
                result.is_truthy()
            } else {
                values_equal(obj, &entry.0)?
            };

            if matches {
                return Ok(pair.0.clone());
            }
        }
        current = pair.1.clone();
    }

    Ok(Value::Boolean(false))
}
