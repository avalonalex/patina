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

/// (utf8->string bytevector [start [end]]) - Decode UTF-8 bytevector to string
pub(super) fn utf8_to_string(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
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
                "utf8->string: first argument must be a bytevector".to_string(),
            ));
        }
    };

    let bv_borrowed = bv.borrow();
    let start = if args.len() >= 2 {
        match &args[1] {
            Value::Integer(n) if *n >= 0 && (*n as usize) <= bv_borrowed.len() => *n as usize,
            Value::Integer(n) => {
                return Err(EvalError::TypeError(format!(
                    "utf8->string: start index out of bounds: {}",
                    n
                )));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "utf8->string: start must be an integer".to_string(),
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
                    "utf8->string: end index out of bounds or less than start: {}",
                    n
                )));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "utf8->string: end must be an integer".to_string(),
                ));
            }
        }
    } else {
        bv_borrowed.len()
    };

    let bytes = &bv_borrowed[start..end];
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok(Value::String(Rc::new(RefCell::new(s.to_string())))),
        Err(e) => Err(EvalError::TypeError(format!(
            "utf8->string: invalid UTF-8 sequence at byte {}",
            e.valid_up_to()
        ))),
    }
}

/// (string->utf8 string [start [end]]) - Encode string as UTF-8 bytevector
pub(super) fn string_to_utf8(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

    let s = match &args[0] {
        Value::String(s) => s,
        _ => {
            return Err(EvalError::TypeError(
                "string->utf8: first argument must be a string".to_string(),
            ));
        }
    };

    let s_borrowed = s.borrow();
    let char_count = s_borrowed.chars().count();

    let start = if args.len() >= 2 {
        match &args[1] {
            Value::Integer(n) if *n >= 0 && (*n as usize) <= char_count => *n as usize,
            Value::Integer(n) => {
                return Err(EvalError::TypeError(format!(
                    "string->utf8: start index out of bounds: {}",
                    n
                )));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "string->utf8: start must be an integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() >= 3 {
        match &args[2] {
            Value::Integer(n) if *n >= start as i64 && (*n as usize) <= char_count => *n as usize,
            Value::Integer(n) => {
                return Err(EvalError::TypeError(format!(
                    "string->utf8: end index out of bounds or less than start: {}",
                    n
                )));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "string->utf8: end must be an integer".to_string(),
                ));
            }
        }
    } else {
        char_count
    };

    // Extract the substring by character indices and encode to UTF-8
    let substring: String = s_borrowed.chars().skip(start).take(end - start).collect();
    let bytes = substring.into_bytes();

    Ok(Value::Bytevector(Rc::new(RefCell::new(bytes))))
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

    // utf8->string - Decode UTF-8 bytevector to string
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "utf8->string",
        Arity::Range(1, 3),
        "Decodes the bytes of a bytevector between start and end as UTF-8 and returns the corresponding string.",
        |eval, args, _tail| utf8_to_string(eval, args).map(EvalResult::Value),
    ));

    // string->utf8 - Encode string as UTF-8 bytevector
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string->utf8",
        Arity::Range(1, 3),
        "Encodes the characters of a string between start and end as UTF-8 and returns the corresponding bytevector.",
        |eval, args, _tail| string_to_utf8(eval, args).map(EvalResult::Value),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Evaluator;

    fn make_evaluator() -> Evaluator {
        Evaluator::new()
    }

    fn make_bytevector(bytes: Vec<u8>) -> Value {
        Value::Bytevector(Rc::new(RefCell::new(bytes)))
    }

    fn make_string(s: &str) -> Value {
        Value::String(Rc::new(RefCell::new(s.to_string())))
    }

    // utf8->string tests

    #[test]
    fn test_utf8_to_string_basic() {
        let eval = make_evaluator();
        let bv = make_bytevector(vec![65, 66, 67]); // "ABC"
        let result = utf8_to_string(&eval, vec![bv]).unwrap();
        assert_eq!(result.to_string(), "\"ABC\"");
    }

    #[test]
    fn test_utf8_to_string_with_start() {
        let eval = make_evaluator();
        let bv = make_bytevector(vec![0, 65, 66, 67]); // skip first byte
        let result = utf8_to_string(&eval, vec![bv, Value::Integer(1)]).unwrap();
        assert_eq!(result.to_string(), "\"ABC\"");
    }

    #[test]
    fn test_utf8_to_string_with_start_and_end() {
        let eval = make_evaluator();
        let bv = make_bytevector(vec![0, 65, 66, 67, 0]); // extract middle
        let result = utf8_to_string(&eval, vec![bv, Value::Integer(1), Value::Integer(4)]).unwrap();
        assert_eq!(result.to_string(), "\"ABC\"");
    }

    #[test]
    fn test_utf8_to_string_unicode() {
        let eval = make_evaluator();
        // Greek lambda: U+03BB = 206 187 in UTF-8
        let bv = make_bytevector(vec![206, 187]);
        let result = utf8_to_string(&eval, vec![bv]).unwrap();
        assert_eq!(result.to_string(), "\"λ\"");
    }

    #[test]
    fn test_utf8_to_string_unicode_with_range() {
        let eval = make_evaluator();
        // Extract lambda from middle: 0 206 187 0
        let bv = make_bytevector(vec![0, 206, 187, 0]);
        let result = utf8_to_string(&eval, vec![bv, Value::Integer(1), Value::Integer(3)]).unwrap();
        assert_eq!(result.to_string(), "\"λ\"");
    }

    #[test]
    fn test_utf8_to_string_empty() {
        let eval = make_evaluator();
        let bv = make_bytevector(vec![]);
        let result = utf8_to_string(&eval, vec![bv]).unwrap();
        assert_eq!(result.to_string(), "\"\"");
    }

    #[test]
    fn test_utf8_to_string_invalid_utf8() {
        let eval = make_evaluator();
        // Invalid UTF-8 sequence: 0xFF is not valid
        let bv = make_bytevector(vec![0xFF, 0xFE]);
        let result = utf8_to_string(&eval, vec![bv]);
        assert!(result.is_err());
    }

    #[test]
    fn test_utf8_to_string_start_out_of_bounds() {
        let eval = make_evaluator();
        let bv = make_bytevector(vec![65, 66, 67]);
        let result = utf8_to_string(&eval, vec![bv, Value::Integer(10)]);
        assert!(result.is_err());
    }

    // string->utf8 tests

    #[test]
    fn test_string_to_utf8_basic() {
        let eval = make_evaluator();
        let s = make_string("ABC");
        let result = string_to_utf8(&eval, vec![s]).unwrap();
        match result {
            Value::Bytevector(bv) => {
                assert_eq!(*bv.borrow(), vec![65, 66, 67]);
            }
            _ => panic!("Expected bytevector"),
        }
    }

    #[test]
    fn test_string_to_utf8_with_start() {
        let eval = make_evaluator();
        let s = make_string("ABC");
        let result = string_to_utf8(&eval, vec![s, Value::Integer(1)]).unwrap();
        match result {
            Value::Bytevector(bv) => {
                assert_eq!(*bv.borrow(), vec![66, 67]); // "BC"
            }
            _ => panic!("Expected bytevector"),
        }
    }

    #[test]
    fn test_string_to_utf8_with_start_and_end() {
        let eval = make_evaluator();
        let s = make_string("ABC");
        let result = string_to_utf8(&eval, vec![s, Value::Integer(1), Value::Integer(2)]).unwrap();
        match result {
            Value::Bytevector(bv) => {
                assert_eq!(*bv.borrow(), vec![66]); // "B"
            }
            _ => panic!("Expected bytevector"),
        }
    }

    #[test]
    fn test_string_to_utf8_unicode() {
        let eval = make_evaluator();
        let s = make_string("λ");
        let result = string_to_utf8(&eval, vec![s]).unwrap();
        match result {
            Value::Bytevector(bv) => {
                assert_eq!(*bv.borrow(), vec![206, 187]); // UTF-8 encoding of λ
            }
            _ => panic!("Expected bytevector"),
        }
    }

    #[test]
    fn test_string_to_utf8_unicode_substring() {
        let eval = make_evaluator();
        // "aλb" - extract just λ (character index 1)
        let s = make_string("aλb");
        let result = string_to_utf8(&eval, vec![s, Value::Integer(1), Value::Integer(2)]).unwrap();
        match result {
            Value::Bytevector(bv) => {
                assert_eq!(*bv.borrow(), vec![206, 187]); // Just λ
            }
            _ => panic!("Expected bytevector"),
        }
    }

    #[test]
    fn test_string_to_utf8_empty() {
        let eval = make_evaluator();
        let s = make_string("");
        let result = string_to_utf8(&eval, vec![s]).unwrap();
        match result {
            Value::Bytevector(bv) => {
                assert_eq!(*bv.borrow(), vec![]);
            }
            _ => panic!("Expected bytevector"),
        }
    }

    #[test]
    fn test_string_to_utf8_start_out_of_bounds() {
        let eval = make_evaluator();
        let s = make_string("ABC");
        let result = string_to_utf8(&eval, vec![s, Value::Integer(10)]);
        assert!(result.is_err());
    }

    // Round-trip tests

    #[test]
    fn test_utf8_roundtrip_ascii() {
        let eval = make_evaluator();
        let original = "Hello, World!";
        let s = make_string(original);

        // string->utf8
        let bv = string_to_utf8(&eval, vec![s]).unwrap();

        // utf8->string
        let result = utf8_to_string(&eval, vec![bv]).unwrap();
        assert_eq!(result.to_string(), format!("\"{}\"", original));
    }

    #[test]
    fn test_utf8_roundtrip_unicode() {
        let eval = make_evaluator();
        let original = "Hello, 世界! λ ∀x∈ℕ";
        let s = make_string(original);

        // string->utf8
        let bv = string_to_utf8(&eval, vec![s]).unwrap();

        // utf8->string
        let result = utf8_to_string(&eval, vec![bv]).unwrap();
        assert_eq!(result.to_string(), format!("\"{}\"", original));
    }
}
