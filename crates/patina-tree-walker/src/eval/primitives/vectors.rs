//! Vector primitive operations (R7RS Section 6.8)
//!
//! Implements vector operations with:
//! - O(1) random access
//! - Mutable vectors via vector-set!
//! - Conversion operations with lists and strings
//!
//! All functions extracted from primitives.rs

use super::super::Evaluator;
use super::super::error::EvalError;
use patina_runtime::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

pub(super) fn make_vector(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_range(&args, 1, 2, "make-vector")?;

    let k = match &args[0] {
        Value::Integer(n) if *n >= 0 => *n as usize,
        Value::Integer(n) => {
            return Err(EvalError::TypeError(format!(
                "make-vector length must be non-negative, got {}",
                n
            )));
        }
        _ => {
            return Err(EvalError::TypeError(
                "make-vector expects integer length".to_string(),
            ));
        }
    };

    let fill = if args.len() == 2 {
        args[1].clone()
    } else {
        Value::Unspecified
    };

    let elements = vec![fill; k];
    Ok(Value::Vector(Rc::new(RefCell::new(elements))))
}

pub(super) fn vector(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // vector accepts any number of arguments (0 or more), so no arity check needed
    Ok(Value::Vector(Rc::new(RefCell::new(args))))
}

pub(super) fn vector_length(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "vector-length")?;
    match &args[0] {
        Value::Vector(v) => Ok(Value::Integer(v.borrow().len() as i64)),
        _ => Err(EvalError::TypeError(
            "vector-length expects a vector".to_string(),
        )),
    }
}

pub(super) fn vector_ref(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "vector-ref")?;

    match (&args[0], &args[1]) {
        (Value::Vector(v), Value::Integer(k)) => {
            if *k < 0 {
                return Err(EvalError::IndexOutOfBounds(format!(
                    "vector-ref index must be non-negative, got {}",
                    k
                )));
            }

            let vec = v.borrow();
            let idx = *k as usize;
            vec.get(idx).cloned().ok_or_else(|| {
                EvalError::IndexOutOfBounds(format!(
                    "vector-ref index {} out of bounds for vector of length {}",
                    k,
                    vec.len()
                ))
            })
        }
        _ => Err(EvalError::TypeError(
            "vector-ref expects vector and integer".to_string(),
        )),
    }
}

pub(super) fn vector_set(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 3, "vector-set!")?;

    match (&args[0], &args[1]) {
        (Value::Vector(v), Value::Integer(k)) => {
            if *k < 0 {
                return Err(EvalError::IndexOutOfBounds(format!(
                    "vector-set! index must be non-negative, got {}",
                    k
                )));
            }

            let mut vec = v.borrow_mut();
            let idx = *k as usize;
            if idx >= vec.len() {
                return Err(EvalError::IndexOutOfBounds(format!(
                    "vector-set! index {} out of bounds for vector of length {}",
                    k,
                    vec.len()
                )));
            }

            vec[idx] = args[2].clone();
            Ok(Value::Unspecified)
        }
        _ => Err(EvalError::TypeError(
            "vector-set! expects vector, integer, and value".to_string(),
        )),
    }
}

pub(super) fn vector_to_list(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "vector->list expects 1 to 3 arguments".to_string(),
            actual: args.len(),
        });
    }

    let vec = match &args[0] {
        Value::Vector(v) => v.borrow().clone(),
        _ => {
            return Err(EvalError::TypeError(
                "vector->list expects a vector".to_string(),
            ));
        }
    };

    let start = if args.len() >= 2 {
        match &args[1] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            Value::Integer(n) => {
                return Err(EvalError::IndexOutOfBounds(format!(
                    "vector->list start index must be non-negative, got {}",
                    n
                )));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "vector->list start index must be an integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() >= 3 {
        match &args[2] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            Value::Integer(n) => {
                return Err(EvalError::IndexOutOfBounds(format!(
                    "vector->list end index must be non-negative, got {}",
                    n
                )));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "vector->list end index must be an integer".to_string(),
                ));
            }
        }
    } else {
        vec.len()
    };

    if start > end || end > vec.len() {
        return Err(EvalError::IndexOutOfBounds(
            "vector->list indices out of bounds".to_string(),
        ));
    }

    Ok(evaluator.list_from_vec(vec[start..end].to_vec()))
}

pub(super) fn list_to_vector(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "list->vector")?;
    let items = evaluator.list_to_vec(args[0].clone(), "list->vector")?;
    Ok(Value::Vector(Rc::new(RefCell::new(items))))
}

pub(super) fn vector_to_string(
    _evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "vector->string expects 1 to 3 arguments".to_string(),
            actual: args.len(),
        });
    }

    let vec = match &args[0] {
        Value::Vector(v) => v.borrow().clone(),
        _ => {
            return Err(EvalError::TypeError(
                "vector->string expects a vector".to_string(),
            ));
        }
    };

    let start = if args.len() >= 2 {
        match &args[1] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "start index must be a non-negative integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() >= 3 {
        match &args[2] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "end index must be a non-negative integer".to_string(),
                ));
            }
        }
    } else {
        vec.len()
    };

    if start > end || end > vec.len() {
        return Err(EvalError::IndexOutOfBounds(
            "indices out of bounds".to_string(),
        ));
    }

    let mut chars = Vec::new();
    for val in &vec[start..end] {
        match val {
            Value::Character(c) => chars.push(*c),
            _ => {
                return Err(EvalError::TypeError(
                    "vector->string requires vector of characters".to_string(),
                ));
            }
        }
    }

    Ok(Value::String(Rc::new(RefCell::new(
        chars.into_iter().collect(),
    ))))
}

pub(super) fn string_to_vector(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_range(&args, 1, 3, "string->vector")?;

    let s = match &args[0] {
        Value::String(s) => s.borrow().clone(),
        _ => {
            return Err(EvalError::TypeError(
                "string->vector expects a string".to_string(),
            ));
        }
    };

    let chars: Vec<char> = s.chars().collect();

    let start = if args.len() >= 2 {
        match &args[1] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "start index must be a non-negative integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() >= 3 {
        match &args[2] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "end index must be a non-negative integer".to_string(),
                ));
            }
        }
    } else {
        chars.len()
    };

    if start > end || end > chars.len() {
        return Err(EvalError::IndexOutOfBounds(
            "indices out of bounds".to_string(),
        ));
    }

    let elements: Vec<Value> = chars[start..end]
        .iter()
        .map(|c| Value::Character(*c))
        .collect();
    Ok(Value::Vector(Rc::new(RefCell::new(elements))))
}

pub(super) fn vector_copy(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_range(&args, 1, 3, "vector-copy")?;

    let vec = match &args[0] {
        Value::Vector(v) => v.borrow().clone(),
        _ => {
            return Err(EvalError::TypeError(
                "vector-copy expects a vector".to_string(),
            ));
        }
    };

    let start = if args.len() >= 2 {
        match &args[1] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "start index must be a non-negative integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() >= 3 {
        match &args[2] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "end index must be a non-negative integer".to_string(),
                ));
            }
        }
    } else {
        vec.len()
    };

    if start > end || end > vec.len() {
        return Err(EvalError::IndexOutOfBounds(
            "indices out of bounds".to_string(),
        ));
    }

    Ok(Value::Vector(Rc::new(RefCell::new(
        vec[start..end].to_vec(),
    ))))
}

pub(super) fn vector_copy_bang(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_range(&args, 3, 5, "vector-copy!")?;

    let to = match &args[0] {
        Value::Vector(v) => v,
        _ => {
            return Err(EvalError::TypeError(
                "first argument must be a vector".to_string(),
            ));
        }
    };

    let at = match &args[1] {
        Value::Integer(n) if *n >= 0 => *n as usize,
        _ => {
            return Err(EvalError::TypeError(
                "at index must be a non-negative integer".to_string(),
            ));
        }
    };

    let from_vec = match &args[2] {
        Value::Vector(v) => v.borrow().clone(),
        _ => {
            return Err(EvalError::TypeError(
                "from argument must be a vector".to_string(),
            ));
        }
    };

    let start = if args.len() >= 4 {
        match &args[3] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "start index must be a non-negative integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() >= 5 {
        match &args[4] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "end index must be a non-negative integer".to_string(),
                ));
            }
        }
    } else {
        from_vec.len()
    };

    if start > end || end > from_vec.len() {
        return Err(EvalError::IndexOutOfBounds(
            "from indices out of bounds".to_string(),
        ));
    }

    let mut to_vec = to.borrow_mut();
    let copy_len = end - start;

    if at > to_vec.len() {
        return Err(EvalError::IndexOutOfBounds(
            "at index out of bounds".to_string(),
        ));
    }

    if to_vec.len() - at < copy_len {
        return Err(EvalError::IndexOutOfBounds(
            "not enough space in destination vector".to_string(),
        ));
    }

    // Copy elements - handle overlapping case by cloning
    let slice_to_copy: Vec<Value> = from_vec[start..end].to_vec();
    for (i, val) in slice_to_copy.into_iter().enumerate() {
        to_vec[at + i] = val;
    }

    Ok(Value::Unspecified)
}

pub(super) fn vector_append(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // vector-append accepts any number of arguments (0 or more), so no arity check needed
    let mut result = Vec::new();

    for arg in args {
        match arg {
            Value::Vector(v) => {
                result.extend(v.borrow().clone());
            }
            _ => {
                return Err(EvalError::TypeError(
                    "vector-append expects only vectors".to_string(),
                ));
            }
        }
    }

    Ok(Value::Vector(Rc::new(RefCell::new(result))))
}

pub(super) fn vector_fill(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_range(&args, 2, 4, "vector-fill!")?;

    let vec = match &args[0] {
        Value::Vector(v) => v,
        _ => {
            return Err(EvalError::TypeError(
                "first argument must be a vector".to_string(),
            ));
        }
    };

    let fill = args[1].clone();

    let start = if args.len() >= 3 {
        match &args[2] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "start index must be a non-negative integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let mut vec_mut = vec.borrow_mut();
    let end = if args.len() >= 4 {
        match &args[3] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "end index must be a non-negative integer".to_string(),
                ));
            }
        }
    } else {
        vec_mut.len()
    };

    if start > end || end > vec_mut.len() {
        return Err(EvalError::IndexOutOfBounds(
            "indices out of bounds".to_string(),
        ));
    }

    for i in start..end {
        vec_mut[i] = fill.clone();
    }

    Ok(Value::Unspecified)
}

pub(super) fn vector_map(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "vector-map")?;

    let proc = &args[0];
    let vectors: Vec<_> = args[1..]
        .iter()
        .map(|v| match v {
            Value::Vector(vec) => Ok(vec.borrow().clone()),
            _ => Err(EvalError::TypeError(
                "vector-map expects vectors".to_string(),
            )),
        })
        .collect::<Result<_, _>>()?;

    if vectors.is_empty() {
        return Ok(Value::Vector(Rc::new(RefCell::new(Vec::new()))));
    }

    let min_len = vectors.iter().map(|v| v.len()).min().unwrap_or(0);
    let mut result = Vec::new();

    for i in 0..min_len {
        let proc_args: Vec<Value> = vectors.iter().map(|v| v[i].clone()).collect();
        // Procedure calls within vector-map are not in tail position
        let val = match evaluator.apply(proc.clone(), proc_args, false)? {
            super::super::EvalResult::Value(v) => v,
            _ => {
                return Err(EvalError::InternalError(
                    "Unexpected tail call in vector-map".to_string(),
                ));
            }
        };
        result.push(val);
    }

    Ok(Value::Vector(Rc::new(RefCell::new(result))))
}

pub(super) fn vector_for_each(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "vector-for-each")?;

    let proc = &args[0];
    let vectors: Vec<_> = args[1..]
        .iter()
        .map(|v| match v {
            Value::Vector(vec) => Ok(vec.borrow().clone()),
            _ => Err(EvalError::TypeError(
                "vector-for-each expects vectors".to_string(),
            )),
        })
        .collect::<Result<_, _>>()?;

    if vectors.is_empty() {
        return Ok(Value::Unspecified);
    }

    let min_len = vectors.iter().map(|v| v.len()).min().unwrap_or(0);

    for i in 0..min_len {
        let proc_args: Vec<Value> = vectors.iter().map(|v| v[i].clone()).collect();
        // Procedure calls within vector-for-each are not in tail position
        match evaluator.apply(proc.clone(), proc_args, false)? {
            super::super::EvalResult::Value(_) => {}
            _ => {
                return Err(EvalError::InternalError(
                    "Unexpected tail call in vector-for-each".to_string(),
                ));
            }
        }
    }

    Ok(Value::Unspecified)
}
