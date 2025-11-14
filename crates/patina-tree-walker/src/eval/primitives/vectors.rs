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

    let vec = match &args[0] {
        Value::Vector(v) => v,
        _ => {
            return Err(EvalError::TypeError(
                "vector-ref expects vector and integer".to_string(),
            ));
        }
    };

    // Extract index, supporting Integer, BigInteger, and Rational (if it's an exact integer)
    let idx = match &args[1] {
        Value::Integer(k) => {
            if *k < 0 {
                return Err(EvalError::IndexOutOfBounds(format!(
                    "vector-ref index must be non-negative, got {}",
                    k
                )));
            }
            *k as usize
        }
        Value::BigInteger(k) => {
            use num_traits::ToPrimitive;
            if k.sign() == num_bigint::Sign::Minus {
                return Err(EvalError::IndexOutOfBounds(
                    "vector-ref index must be non-negative".to_string(),
                ));
            }
            match k.to_usize() {
                Some(idx) => idx,
                None => {
                    return Err(EvalError::IndexOutOfBounds(
                        "vector-ref index too large".to_string(),
                    ));
                }
            }
        }
        Value::Rational(r) => {
            use num_bigint::BigInt;
            use num_traits::ToPrimitive;
            // Check if it's an exact integer (denominator == 1)
            if r.denom() != &BigInt::from(1) {
                return Err(EvalError::TypeError(
                    "vector-ref index must be an integer".to_string(),
                ));
            }
            if r.numer().sign() == num_bigint::Sign::Minus {
                return Err(EvalError::IndexOutOfBounds(
                    "vector-ref index must be non-negative".to_string(),
                ));
            }
            match r.numer().to_usize() {
                Some(idx) => idx,
                None => {
                    return Err(EvalError::IndexOutOfBounds(
                        "vector-ref index too large".to_string(),
                    ));
                }
            }
        }
        _ => {
            return Err(EvalError::TypeError(
                "vector-ref expects vector and integer".to_string(),
            ));
        }
    };

    let borrowed_vec = vec.borrow();
    borrowed_vec.get(idx).cloned().ok_or_else(|| {
        EvalError::IndexOutOfBounds(format!(
            "vector-ref index {} out of bounds for vector of length {}",
            idx,
            borrowed_vec.len()
        ))
    })
}

pub(super) fn vector_set(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 3, "vector-set!")?;

    let vec = match &args[0] {
        Value::Vector(v) => v,
        _ => {
            return Err(EvalError::TypeError(
                "vector-set! expects vector, integer, and value".to_string(),
            ));
        }
    };

    // Extract index, supporting Integer, BigInteger, and Rational (if it's an exact integer)
    let idx = match &args[1] {
        Value::Integer(k) => {
            if *k < 0 {
                return Err(EvalError::IndexOutOfBounds(format!(
                    "vector-set! index must be non-negative, got {}",
                    k
                )));
            }
            *k as usize
        }
        Value::BigInteger(k) => {
            use num_traits::ToPrimitive;
            if k.sign() == num_bigint::Sign::Minus {
                return Err(EvalError::IndexOutOfBounds(
                    "vector-set! index must be non-negative".to_string(),
                ));
            }
            match k.to_usize() {
                Some(idx) => idx,
                None => {
                    return Err(EvalError::IndexOutOfBounds(
                        "vector-set! index too large".to_string(),
                    ));
                }
            }
        }
        Value::Rational(r) => {
            use num_bigint::BigInt;
            use num_traits::ToPrimitive;
            // Check if it's an exact integer (denominator == 1)
            if r.denom() != &BigInt::from(1) {
                return Err(EvalError::TypeError(
                    "vector-set! index must be an integer".to_string(),
                ));
            }
            if r.numer().sign() == num_bigint::Sign::Minus {
                return Err(EvalError::IndexOutOfBounds(
                    "vector-set! index must be non-negative".to_string(),
                ));
            }
            match r.numer().to_usize() {
                Some(idx) => idx,
                None => {
                    return Err(EvalError::IndexOutOfBounds(
                        "vector-set! index too large".to_string(),
                    ));
                }
            }
        }
        _ => {
            return Err(EvalError::TypeError(
                "vector-set! expects vector, integer, and value".to_string(),
            ));
        }
    };

    let mut borrowed_vec = vec.borrow_mut();
    if idx >= borrowed_vec.len() {
        return Err(EvalError::IndexOutOfBounds(format!(
            "vector-set! index {} out of bounds for vector of length {}",
            idx,
            borrowed_vec.len()
        )));
    }

    borrowed_vec[idx] = args[2].clone();
    Ok(Value::Unspecified)
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

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Make-vector
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "make-vector",
        Arity::Range(1, 2),
        "Returns a newly allocated vector of k elements.",
        |eval, args, _tail| make_vector(eval, args).map(EvalResult::Value),
    ));

    // Vector constructor
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "vector",
        Arity::Min(0),
        "Returns a newly allocated vector whose elements contain the given arguments.",
        |eval, args, _tail| vector(eval, args).map(EvalResult::Value),
    ));

    // Vector-length
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "vector-length",
        Arity::Exact(1),
        "Returns the number of elements in vector.",
        |eval, args, _tail| vector_length(eval, args).map(EvalResult::Value),
    ));

    // Vector-ref
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "vector-ref",
        Arity::Exact(2),
        "Returns the contents of element k of vector.",
        |eval, args, _tail| vector_ref(eval, args).map(EvalResult::Value),
    ));

    // Vector-set!
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "vector-set!",
        Arity::Exact(3),
        "Stores obj in element k of vector.",
        |eval, args, _tail| vector_set(eval, args).map(EvalResult::Value),
    ));

    // Vector->list
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "vector->list",
        Arity::Range(1, 3),
        "Returns a newly allocated list of the objects contained in the elements of vector.",
        |eval, args, _tail| vector_to_list(eval, args).map(EvalResult::Value),
    ));

    // List->vector
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "list->vector",
        Arity::Exact(1),
        "Returns a newly allocated vector of the objects contained in the list.",
        |eval, args, _tail| list_to_vector(eval, args).map(EvalResult::Value),
    ));

    // Vector->string
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "vector->string",
        Arity::Range(1, 3),
        "Returns a newly allocated string of the characters contained in the elements of vector.",
        |eval, args, _tail| vector_to_string(eval, args).map(EvalResult::Value),
    ));

    // String->vector
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string->vector",
        Arity::Range(1, 3),
        "Returns a newly allocated vector of the characters contained in the string.",
        |eval, args, _tail| string_to_vector(eval, args).map(EvalResult::Value),
    ));

    // Vector-copy
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "vector-copy",
        Arity::Range(1, 3),
        "Returns a newly allocated copy of the elements of the given vector.",
        |eval, args, _tail| vector_copy(eval, args).map(EvalResult::Value),
    ));

    // Vector-copy!
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "vector-copy!",
        Arity::Range(3, 5),
        "Copies the elements of vector from at to into vector to, starting at at.",
        |eval, args, _tail| vector_copy_bang(eval, args).map(EvalResult::Value),
    ));

    // Vector-append
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "vector-append",
        Arity::Min(0),
        "Returns a newly allocated vector whose elements are the concatenation of the elements of the given vectors.",
        |eval, args, _tail| vector_append(eval, args).map(EvalResult::Value),
    ));

    // Vector-fill!
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "vector-fill!",
        Arity::Range(2, 4),
        "Stores fill in the elements of vector.",
        |eval, args, _tail| vector_fill(eval, args).map(EvalResult::Value),
    ));

    // Vector-map
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "vector-map",
        Arity::Min(2),
        "Returns a newly allocated vector of the results of applying proc element-wise to the elements of the vectors.",
        |eval, args, _tail| vector_map(eval, args).map(EvalResult::Value),
    ));

    // Vector-for-each
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "vector-for-each",
        Arity::Min(2),
        "Applies proc element-wise to the elements of the vectors for side effects.",
        |eval, args, _tail| vector_for_each(eval, args).map(EvalResult::Value),
    ));
}
