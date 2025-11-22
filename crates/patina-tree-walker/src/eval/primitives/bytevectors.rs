//! Bytevector primitive operations (R7RS Section 6.9)
//!
//! Implements R7RS bytevector operations.
//! Bytevectors are vectors of exact integers in the range 0-255.

use super::super::Evaluator;
use super::super::error::EvalError;
use patina_runtime::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

/// (bytevector? obj) - Type predicate
pub(super) fn bytevector_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v, Value::Bytevector(_)))
}

/// (make-bytevector k [byte]) - Create bytevector of k bytes
pub(super) fn make_bytevector(
    _evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1 or 2".to_string(),
            actual: args.len(),
        });
    }

    let k = match &args[0] {
        Value::Integer(n) if *n >= 0 => *n as usize,
        Value::Integer(n) => {
            return Err(EvalError::TypeError(format!(
                "make-bytevector: length must be non-negative, got {}",
                n
            )));
        }
        _ => {
            return Err(EvalError::TypeError(
                "make-bytevector: first argument must be an integer".to_string(),
            ));
        }
    };

    let fill = if args.len() == 2 {
        match &args[1] {
            Value::Integer(n) if *n >= 0 && *n <= 255 => *n as u8,
            Value::Integer(n) => {
                return Err(EvalError::TypeError(format!(
                    "make-bytevector: byte must be in range 0-255, got {}",
                    n
                )));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "make-bytevector: fill value must be an integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    Ok(Value::Bytevector(Rc::new(RefCell::new(vec![fill; k]))))
}

/// (bytevector byte ...) - Construct bytevector from arguments
pub(super) fn bytevector(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    let mut bytes = Vec::with_capacity(args.len());

    for (i, arg) in args.iter().enumerate() {
        match arg {
            Value::Integer(n) if *n >= 0 && *n <= 255 => {
                bytes.push(*n as u8);
            }
            Value::Integer(n) => {
                return Err(EvalError::TypeError(format!(
                    "bytevector: argument {} must be in range 0-255, got {}",
                    i, n
                )));
            }
            _ => {
                return Err(EvalError::TypeError(format!(
                    "bytevector: argument {} must be an integer",
                    i
                )));
            }
        }
    }

    Ok(Value::Bytevector(Rc::new(RefCell::new(bytes))))
}

/// (bytevector-length bytevector) - Get length
pub(super) fn bytevector_length(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "bytevector-length")?;

    match &args[0] {
        Value::Bytevector(bv) => Ok(Value::Integer(bv.borrow().len() as i64)),
        _ => Err(EvalError::TypeError(
            "bytevector-length: argument must be a bytevector".to_string(),
        )),
    }
}

/// (bytevector-u8-ref bytevector k) - Get byte at index k
pub(super) fn bytevector_u8_ref(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "bytevector-u8-ref")?;

    let bv = match &args[0] {
        Value::Bytevector(bv) => bv,
        _ => {
            return Err(EvalError::TypeError(
                "bytevector-u8-ref: first argument must be a bytevector".to_string(),
            ));
        }
    };

    let bv_borrowed = bv.borrow();
    let index = match &args[1] {
        Value::Integer(n) if *n >= 0 && (*n as usize) < bv_borrowed.len() => *n as usize,
        Value::Integer(n) if *n >= 0 => {
            return Err(EvalError::TypeError(format!(
                "bytevector-u8-ref: index {} out of bounds (length {})",
                n,
                bv_borrowed.len()
            )));
        }
        Value::Integer(n) => {
            return Err(EvalError::TypeError(format!(
                "bytevector-u8-ref: index must be non-negative, got {}",
                n
            )));
        }
        _ => {
            return Err(EvalError::TypeError(
                "bytevector-u8-ref: second argument must be an integer".to_string(),
            ));
        }
    };

    Ok(Value::Integer(bv_borrowed[index] as i64))
}

/// (bytevector-u8-set! bytevector k byte) - Set byte at index k
pub(super) fn bytevector_u8_set(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 3, "bytevector-u8-set!")?;

    let bv = match &args[0] {
        Value::Bytevector(bv) => bv,
        _ => {
            return Err(EvalError::TypeError(
                "bytevector-u8-set!: first argument must be a bytevector".to_string(),
            ));
        }
    };

    let byte = match &args[2] {
        Value::Integer(n) if *n >= 0 && *n <= 255 => *n as u8,
        Value::Integer(n) => {
            return Err(EvalError::TypeError(format!(
                "bytevector-u8-set!: byte must be in range 0-255, got {}",
                n
            )));
        }
        _ => {
            return Err(EvalError::TypeError(
                "bytevector-u8-set!: third argument must be an integer".to_string(),
            ));
        }
    };

    let mut bv_borrowed = bv.borrow_mut();
    let index = match &args[1] {
        Value::Integer(n) if *n >= 0 && (*n as usize) < bv_borrowed.len() => *n as usize,
        Value::Integer(n) if *n >= 0 => {
            return Err(EvalError::TypeError(format!(
                "bytevector-u8-set!: index {} out of bounds (length {})",
                n,
                bv_borrowed.len()
            )));
        }
        Value::Integer(n) => {
            return Err(EvalError::TypeError(format!(
                "bytevector-u8-set!: index must be non-negative, got {}",
                n
            )));
        }
        _ => {
            return Err(EvalError::TypeError(
                "bytevector-u8-set!: second argument must be an integer".to_string(),
            ));
        }
    };

    bv_borrowed[index] = byte;
    Ok(Value::Unspecified)
}

/// (bytevector-copy bytevector [start [end]]) - Copy bytevector
pub(super) fn bytevector_copy(
    _evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

    let bv = match &args[0] {
        Value::Bytevector(bv) => bv,
        _ => {
            return Err(EvalError::TypeError(
                "bytevector-copy: first argument must be a bytevector".to_string(),
            ));
        }
    };

    let bv_borrowed = bv.borrow();
    let start = if args.len() >= 2 {
        match &args[1] {
            Value::Integer(n) if *n >= 0 && (*n as usize) <= bv_borrowed.len() => *n as usize,
            Value::Integer(n) => {
                return Err(EvalError::TypeError(format!(
                    "bytevector-copy: start index out of bounds: {}",
                    n
                )));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "bytevector-copy: start must be an integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() >= 3 {
        match &args[2] {
            Value::Integer(n) if *n >= start as i64 && (*n as usize) <= bv_borrowed.len() => {
                *n as usize
            }
            Value::Integer(n) => {
                return Err(EvalError::TypeError(format!(
                    "bytevector-copy: end index out of bounds or less than start: {}",
                    n
                )));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "bytevector-copy: end must be an integer".to_string(),
                ));
            }
        }
    } else {
        bv_borrowed.len()
    };

    let copied = bv_borrowed[start..end].to_vec();
    Ok(Value::Bytevector(Rc::new(RefCell::new(copied))))
}

/// (bytevector-copy! to at from [start [end]]) - Copy bytes from one bytevector to another
pub(super) fn bytevector_copy_mut(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_range(&args, 3, 5, "bytevector-copy!")?;

    // Extract destination bytevector
    let to_bv = match &args[0] {
        Value::Bytevector(bv) => bv,
        _ => {
            return Err(EvalError::TypeError(
                "bytevector-copy!: first argument must be a bytevector".to_string(),
            ));
        }
    };

    // Extract destination index
    let at = match &args[1] {
        Value::Integer(n) if *n >= 0 => *n as usize,
        Value::Integer(n) => {
            return Err(EvalError::TypeError(format!(
                "bytevector-copy!: 'at' must be non-negative, got {}",
                n
            )));
        }
        _ => {
            return Err(EvalError::TypeError(
                "bytevector-copy!: second argument must be an integer".to_string(),
            ));
        }
    };

    // Extract source bytevector
    let from_bv = match &args[2] {
        Value::Bytevector(bv) => bv,
        _ => {
            return Err(EvalError::TypeError(
                "bytevector-copy!: third argument must be a bytevector".to_string(),
            ));
        }
    };

    // Determine start and end indices
    let (start, end) = {
        let from_borrowed = from_bv.borrow();
        let start = if args.len() >= 4 {
            match &args[3] {
                Value::Integer(n) if *n >= 0 && (*n as usize) <= from_borrowed.len() => *n as usize,
                Value::Integer(n) => {
                    return Err(EvalError::TypeError(format!(
                        "bytevector-copy!: start index out of bounds: {}",
                        n
                    )));
                }
                _ => {
                    return Err(EvalError::TypeError(
                        "bytevector-copy!: start must be an integer".to_string(),
                    ));
                }
            }
        } else {
            0
        };

        let end = if args.len() >= 5 {
            match &args[4] {
                Value::Integer(n) if *n >= start as i64 && (*n as usize) <= from_borrowed.len() => {
                    *n as usize
                }
                Value::Integer(n) => {
                    return Err(EvalError::TypeError(format!(
                        "bytevector-copy!: end index out of bounds or less than start: {}",
                        n
                    )));
                }
                _ => {
                    return Err(EvalError::TypeError(
                        "bytevector-copy!: end must be an integer".to_string(),
                    ));
                }
            }
        } else {
            from_borrowed.len()
        };
        (start, end)
    }; // from_borrowed is dropped here

    // Copy data to temporary buffer if needed (especially if from and to are the same)
    let temp_data = {
        let from_borrowed = from_bv.borrow();
        from_borrowed[start..end].to_vec()
    }; // from_borrowed is dropped here

    // Now we can safely borrow to_bv mutably
    let mut to_borrowed = to_bv.borrow_mut();

    // Validate that copy will fit in destination
    let copy_len = temp_data.len();
    if at + copy_len > to_borrowed.len() {
        return Err(EvalError::TypeError(format!(
            "bytevector-copy!: not enough space in destination (need {}, have {})",
            at + copy_len,
            to_borrowed.len()
        )));
    }

    // Perform the copy from temp buffer
    to_borrowed[at..at + copy_len].copy_from_slice(&temp_data);

    Ok(Value::Unspecified)
}

/// (bytevector-append bytevector ...) - Concatenate bytevectors
pub(super) fn bytevector_append(
    _evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    let mut result = Vec::new();

    for (i, arg) in args.iter().enumerate() {
        match arg {
            Value::Bytevector(bv) => {
                result.extend_from_slice(&bv.borrow());
            }
            _ => {
                return Err(EvalError::TypeError(format!(
                    "bytevector-append: argument {} must be a bytevector",
                    i
                )));
            }
        }
    }

    Ok(Value::Bytevector(Rc::new(RefCell::new(result))))
}

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // bytevector? - Type predicate
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "bytevector?",
        Arity::Exact(1),
        "Returns #t if obj is a bytevector.",
        |eval, args, _tail| bytevector_p(eval, args).map(EvalResult::Value),
    ));

    // make-bytevector - Create bytevector
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "make-bytevector",
        Arity::Range(1, 2),
        "Returns a newly allocated bytevector of k bytes, optionally filled with byte.",
        |eval, args, _tail| make_bytevector(eval, args).map(EvalResult::Value),
    ));

    // bytevector - Construct from arguments
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "bytevector",
        Arity::Min(0),
        "Returns a newly allocated bytevector whose elements are the given arguments.",
        |eval, args, _tail| bytevector(eval, args).map(EvalResult::Value),
    ));

    // bytevector-length - Get length
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "bytevector-length",
        Arity::Exact(1),
        "Returns the length of bytevector in bytes.",
        |eval, args, _tail| bytevector_length(eval, args).map(EvalResult::Value),
    ));

    // bytevector-u8-ref - Get byte
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "bytevector-u8-ref",
        Arity::Exact(2),
        "Returns the kth byte of bytevector.",
        |eval, args, _tail| bytevector_u8_ref(eval, args).map(EvalResult::Value),
    ));

    // bytevector-u8-set! - Set byte (not yet functional)
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "bytevector-u8-set!",
        Arity::Exact(3),
        "Stores byte as the kth byte of bytevector.",
        |eval, args, _tail| bytevector_u8_set(eval, args).map(EvalResult::Value),
    ));

    // bytevector-copy - Copy bytevector
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "bytevector-copy",
        Arity::Range(1, 3),
        "Returns a newly allocated bytevector containing the bytes in bytevector between start and end.",
        |eval, args, _tail| bytevector_copy(eval, args).map(EvalResult::Value),
    ));

    // bytevector-copy! - Copy bytes
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "bytevector-copy!",
        Arity::Range(3, 5),
        "Copies the bytes of from between start and end to to, starting at at.",
        |eval, args, _tail| bytevector_copy_mut(eval, args).map(EvalResult::Value),
    ));

    // bytevector-append - Concatenate bytevectors
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "bytevector-append",
        Arity::Min(0),
        "Returns a newly allocated bytevector whose bytes are the concatenation of the bytes in the given bytevectors.",
        |eval, args, _tail| bytevector_append(eval, args).map(EvalResult::Value),
    ));
}
