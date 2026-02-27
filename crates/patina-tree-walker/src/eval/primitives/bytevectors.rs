//! Bytevector primitive operations (R7RS Section 6.9)
//!
//! Implements R7RS bytevector operations.
//! Bytevectors are vectors of exact integers in the range 0-255.

use super::super::Evaluator;
use super::super::error::EvalError;
use patina_core::TaggedValue;

// ========== TaggedValue Extraction Helpers ==========

/// Get bytevector bytes (cloned)
fn get_bv_bytes(
    tv: TaggedValue,
    heap: &std::cell::Ref<'_, patina_core::Heap>,
    fn_name: &str,
) -> Result<Vec<u8>, EvalError> {
    if let Some(bytes) = heap.get_bytevector_bytes(tv) {
        return Ok(bytes);
    }
    Err(EvalError::TypeError(format!(
        "{}: argument must be a bytevector",
        fn_name
    )))
}

/// Extract a non-negative integer from TaggedValue
fn get_index(
    tv: TaggedValue,
    _heap: &std::cell::Ref<'_, patina_core::Heap>,
    fn_name: &str,
) -> Result<usize, EvalError> {
    if tv.is_fixnum() {
        let n = tv.as_fixnum_unchecked();
        if n >= 0 {
            return Ok(n as usize);
        }
        return Err(EvalError::TypeError(format!(
            "{}: index must be non-negative, got {}",
            fn_name, n
        )));
    }
    Err(EvalError::TypeError(format!(
        "{}: index must be an exact integer",
        fn_name
    )))
}

/// Extract a byte (0-255) from TaggedValue
fn get_byte(
    tv: TaggedValue,
    _heap: &std::cell::Ref<'_, patina_core::Heap>,
    fn_name: &str,
) -> Result<u8, EvalError> {
    if tv.is_fixnum() {
        let n = tv.as_fixnum_unchecked();
        if (0..=255).contains(&n) {
            return Ok(n as u8);
        }
        return Err(EvalError::TypeError(format!(
            "{}: byte must be in range 0-255, got {}",
            fn_name, n
        )));
    }
    Err(EvalError::TypeError(format!(
        "{}: byte must be an exact integer",
        fn_name
    )))
}

// ========== Bytevector Primitives ==========

/// (bytevector? obj) - Type predicate
pub(super) fn bytevector_p(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();
    if heap_ref.is_any_bytevector(args[0]) {
        Ok(TaggedValue::TRUE)
    } else {
        Ok(TaggedValue::FALSE)
    }
}

/// (make-bytevector k [byte]) - Create bytevector of k bytes
pub(super) fn make_bytevector(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1 or 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    let k = get_index(args[0], &heap_ref, "make-bytevector")?;
    let fill = if args.len() == 2 {
        get_byte(args[1], &heap_ref, "make-bytevector")?
    } else {
        0
    };

    drop(heap_ref);
    Ok(heap.borrow_mut().alloc_bytevector(vec![fill; k]))
}

/// (bytevector byte ...) - Construct bytevector from arguments
pub(super) fn bytevector(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    let mut bytes = Vec::with_capacity(args.len());
    for (i, &arg) in args.iter().enumerate() {
        let byte = get_byte(arg, &heap_ref, &format!("bytevector argument {}", i))?;
        bytes.push(byte);
    }

    drop(heap_ref);
    Ok(heap.borrow_mut().alloc_bytevector(bytes))
}

/// (bytevector-length bytevector) - Get length
pub(super) fn bytevector_length(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();
    match heap_ref.bytevector_len(args[0]) {
        Some(len) => Ok(TaggedValue::fixnum(len as i64)),
        None => Err(EvalError::TypeError(
            "bytevector-length: argument must be a bytevector".to_string(),
        )),
    }
}

/// (bytevector-u8-ref bytevector k) - Get byte at index k
pub(super) fn bytevector_u8_ref(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    let bv_len = heap_ref.bytevector_len(args[0]).ok_or_else(|| {
        EvalError::TypeError("bytevector-u8-ref: argument must be a bytevector".to_string())
    })?;

    let index = get_index(args[1], &heap_ref, "bytevector-u8-ref")?;
    if index >= bv_len {
        return Err(EvalError::TypeError(format!(
            "bytevector-u8-ref: index {} out of bounds (length {})",
            index, bv_len
        )));
    }

    match heap_ref.bytevector_u8_ref(args[0], index) {
        Some(byte) => Ok(TaggedValue::fixnum(byte as i64)),
        None => Err(EvalError::TypeError(
            "bytevector-u8-ref: argument must be a bytevector".to_string(),
        )),
    }
}

/// (bytevector-u8-set! bytevector k byte) - Set byte at index k
pub(super) fn bytevector_u8_set(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 3 {
        return Err(EvalError::WrongArity {
            expected: "3".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    let bv_len = heap_ref.bytevector_len(args[0]).ok_or_else(|| {
        EvalError::TypeError("bytevector-u8-set!: argument must be a bytevector".to_string())
    })?;

    let index = get_index(args[1], &heap_ref, "bytevector-u8-set!")?;
    if index >= bv_len {
        return Err(EvalError::TypeError(format!(
            "bytevector-u8-set!: index {} out of bounds (length {})",
            index, bv_len
        )));
    }

    let byte = get_byte(args[2], &heap_ref, "bytevector-u8-set!")?;
    drop(heap_ref);

    let mut heap_mut = heap.borrow_mut();
    if !heap_mut.bytevector_u8_set(args[0], index, byte) {
        return Err(EvalError::TypeError(
            "bytevector-u8-set!: argument must be a bytevector".to_string(),
        ));
    }
    Ok(TaggedValue::UNSPECIFIED)
}

/// (bytevector-copy bytevector [start [end]]) - Copy bytevector
pub(super) fn bytevector_copy(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();
    let bytes = get_bv_bytes(args[0], &heap_ref, "bytevector-copy")?;

    let start = if args.len() >= 2 {
        let s = get_index(args[1], &heap_ref, "bytevector-copy")?;
        if s > bytes.len() {
            return Err(EvalError::TypeError(format!(
                "bytevector-copy: start index out of bounds: {}",
                s
            )));
        }
        s
    } else {
        0
    };

    let end = if args.len() >= 3 {
        let e = get_index(args[2], &heap_ref, "bytevector-copy")?;
        if e < start || e > bytes.len() {
            return Err(EvalError::TypeError(format!(
                "bytevector-copy: end index out of bounds or less than start: {}",
                e
            )));
        }
        e
    } else {
        bytes.len()
    };

    let copied = bytes[start..end].to_vec();
    drop(heap_ref);
    Ok(heap.borrow_mut().alloc_bytevector(copied))
}

/// (bytevector-copy! to at from [start [end]]) - Copy bytes from one bytevector to another
pub(super) fn bytevector_copy_mut(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 3 || args.len() > 5 {
        return Err(EvalError::WrongArity {
            expected: "3 to 5".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    let at = get_index(args[1], &heap_ref, "bytevector-copy!")?;

    // Get source bytes
    let from_bytes = get_bv_bytes(args[2], &heap_ref, "bytevector-copy!")?;

    let start = if args.len() >= 4 {
        let s = get_index(args[3], &heap_ref, "bytevector-copy!")?;
        if s > from_bytes.len() {
            return Err(EvalError::TypeError(format!(
                "bytevector-copy!: start index out of bounds: {}",
                s
            )));
        }
        s
    } else {
        0
    };

    let end = if args.len() >= 5 {
        let e = get_index(args[4], &heap_ref, "bytevector-copy!")?;
        if e < start || e > from_bytes.len() {
            return Err(EvalError::TypeError(format!(
                "bytevector-copy!: end index out of bounds or less than start: {}",
                e
            )));
        }
        e
    } else {
        from_bytes.len()
    };

    let src_slice = &from_bytes[start..end];
    let copy_len = src_slice.len();

    // Validate destination has enough space
    let to_len = heap_ref.bytevector_len(args[0]).ok_or_else(|| {
        EvalError::TypeError("bytevector-copy!: first argument must be a bytevector".to_string())
    })?;

    if at + copy_len > to_len {
        return Err(EvalError::TypeError(format!(
            "bytevector-copy!: not enough space in destination (need {}, have {})",
            at + copy_len,
            to_len
        )));
    }

    // Copy into a temp buffer (src_slice already cloned via get_bv_bytes)
    let temp = src_slice.to_vec();
    drop(heap_ref);

    let mut heap_mut = heap.borrow_mut();
    if !heap_mut.bytevector_copy_into(args[0], at, &temp) {
        return Err(EvalError::TypeError(
            "bytevector-copy!: first argument must be a bytevector".to_string(),
        ));
    }

    Ok(TaggedValue::UNSPECIFIED)
}

/// (bytevector-append bytevector ...) - Concatenate bytevectors
pub(super) fn bytevector_append(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    let mut result_bytes = Vec::new();
    for (i, &arg) in args.iter().enumerate() {
        let bytes = get_bv_bytes(arg, &heap_ref, &format!("bytevector-append argument {}", i))?;
        result_bytes.extend_from_slice(&bytes);
    }

    drop(heap_ref);
    Ok(heap.borrow_mut().alloc_bytevector(result_bytes))
}

/// (utf8->string bytevector [start [end]]) - Decode UTF-8 bytevector to string
pub(super) fn utf8_to_string(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();
    let bytes = get_bv_bytes(args[0], &heap_ref, "utf8->string")?;

    let start = if args.len() >= 2 {
        let s = get_index(args[1], &heap_ref, "utf8->string")?;
        if s > bytes.len() {
            return Err(EvalError::TypeError(format!(
                "utf8->string: start index out of bounds: {}",
                s
            )));
        }
        s
    } else {
        0
    };

    let end = if args.len() >= 3 {
        let e = get_index(args[2], &heap_ref, "utf8->string")?;
        if e < start || e > bytes.len() {
            return Err(EvalError::TypeError(format!(
                "utf8->string: end index out of bounds or less than start: {}",
                e
            )));
        }
        e
    } else {
        bytes.len()
    };

    let slice = &bytes[start..end];
    match std::str::from_utf8(slice) {
        Ok(s) => {
            drop(heap_ref);
            Ok(heap.borrow_mut().alloc_string(s.to_string()))
        }
        Err(e) => Err(EvalError::TypeError(format!(
            "utf8->string: invalid UTF-8 sequence at byte {}",
            e.valid_up_to()
        ))),
    }
}

/// (string->utf8 string [start [end]]) - Encode string as UTF-8 bytevector
pub(super) fn string_to_utf8(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    let s = heap_ref.get_string_contents(args[0]).ok_or_else(|| {
        EvalError::TypeError("string->utf8: argument must be a string".to_string())
    })?;

    let chars: Vec<char> = s.chars().collect();
    let char_count = chars.len();

    let start = if args.len() >= 2 {
        let idx = get_index(args[1], &heap_ref, "string->utf8")?;
        if idx > char_count {
            return Err(EvalError::TypeError(format!(
                "string->utf8: start index out of bounds: {}",
                idx
            )));
        }
        idx
    } else {
        0
    };

    let end = if args.len() >= 3 {
        let idx = get_index(args[2], &heap_ref, "string->utf8")?;
        if idx < start || idx > char_count {
            return Err(EvalError::TypeError(format!(
                "string->utf8: end index out of bounds or less than start: {}",
                idx
            )));
        }
        idx
    } else {
        char_count
    };

    // Extract the substring by character indices and encode to UTF-8
    let substring: String = chars.iter().skip(start).take(end - start).collect();
    let bytes = substring.into_bytes();

    drop(heap_ref);
    Ok(heap.borrow_mut().alloc_bytevector(bytes))
}

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // bytevector? - Type predicate
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "bytevector?",
        Arity::Exact(1),
        "Returns #t if obj is a bytevector.",
        |eval, args, _tail| bytevector_p(eval, args).map(EvalResult::Tagged),
    ));

    // make-bytevector - Create bytevector
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "make-bytevector",
        Arity::Range(1, 2),
        "Returns a newly allocated bytevector of k bytes, optionally filled with byte.",
        |eval, args, _tail| make_bytevector(eval, args).map(EvalResult::Tagged),
    ));

    // bytevector - Construct from arguments
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "bytevector",
        Arity::Min(0),
        "Returns a newly allocated bytevector whose elements are the given arguments.",
        |eval, args, _tail| bytevector(eval, args).map(EvalResult::Tagged),
    ));

    // bytevector-length - Get length
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "bytevector-length",
        Arity::Exact(1),
        "Returns the length of bytevector in bytes.",
        |eval, args, _tail| bytevector_length(eval, args).map(EvalResult::Tagged),
    ));

    // bytevector-u8-ref - Get byte
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "bytevector-u8-ref",
        Arity::Exact(2),
        "Returns the kth byte of bytevector.",
        |eval, args, _tail| bytevector_u8_ref(eval, args).map(EvalResult::Tagged),
    ));

    // bytevector-u8-set! - Set byte
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "bytevector-u8-set!",
        Arity::Exact(3),
        "Stores byte as the kth byte of bytevector.",
        |eval, args, _tail| bytevector_u8_set(eval, args).map(EvalResult::Tagged),
    ));

    // bytevector-copy - Copy bytevector
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "bytevector-copy",
        Arity::Range(1, 3),
        "Returns a newly allocated bytevector containing the bytes in bytevector between start and end.",
        |eval, args, _tail| bytevector_copy(eval, args).map(EvalResult::Tagged),
    ));

    // bytevector-copy! - Copy bytes
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "bytevector-copy!",
        Arity::Range(3, 5),
        "Copies the bytes of from between start and end to to, starting at at.",
        |eval, args, _tail| bytevector_copy_mut(eval, args).map(EvalResult::Tagged),
    ));

    // bytevector-append - Concatenate bytevectors
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "bytevector-append",
        Arity::Min(0),
        "Returns a newly allocated bytevector whose bytes are the concatenation of the bytes in the given bytevectors.",
        |eval, args, _tail| bytevector_append(eval, args).map(EvalResult::Tagged),
    ));

    // utf8->string - Decode UTF-8 bytevector to string
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "utf8->string",
        Arity::Range(1, 3),
        "Decodes the bytes of a bytevector between start and end as UTF-8 and returns the corresponding string.",
        |eval, args, _tail| utf8_to_string(eval, args).map(EvalResult::Tagged),
    ));

    // string->utf8 - Encode string as UTF-8 bytevector
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "string->utf8",
        Arity::Range(1, 3),
        "Encodes the characters of a string between start and end as UTF-8 and returns the corresponding bytevector.",
        |eval, args, _tail| string_to_utf8(eval, args).map(EvalResult::Tagged),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Evaluator;

    fn make_evaluator() -> Evaluator {
        Evaluator::new()
    }

    /// Allocate a native bytevector on the heap
    fn alloc_bv(eval: &Evaluator, bytes: Vec<u8>) -> TaggedValue {
        let heap = eval.global_env.heap();
        heap.borrow_mut().alloc_bytevector(bytes)
    }

    /// Allocate a native string on the heap
    fn alloc_str(eval: &Evaluator, s: &str) -> TaggedValue {
        let heap = eval.global_env.heap();
        heap.borrow_mut().alloc_string(s.to_string())
    }

    /// Get bytevector bytes from a TaggedValue result
    fn get_result_bytes(eval: &Evaluator, tv: TaggedValue) -> Vec<u8> {
        let heap = eval.global_env.heap();
        let heap_ref = heap.borrow();
        heap_ref
            .get_bytevector_bytes(tv)
            .expect("Expected a bytevector")
    }

    /// Get string from a TaggedValue result
    fn get_result_string(eval: &Evaluator, tv: TaggedValue) -> String {
        let heap = eval.global_env.heap();
        let heap_ref = heap.borrow();
        heap_ref.get_string_contents(tv).expect("Expected a string")
    }

    // utf8->string tests

    #[test]
    fn test_utf8_to_string_basic() {
        let eval = make_evaluator();
        let bv = alloc_bv(&eval, vec![65, 66, 67]); // "ABC"
        let result = utf8_to_string(&eval, vec![bv]).unwrap();
        assert_eq!(get_result_string(&eval, result), "ABC");
    }

    #[test]
    fn test_utf8_to_string_with_start() {
        let eval = make_evaluator();
        let bv = alloc_bv(&eval, vec![0, 65, 66, 67]);
        let result = utf8_to_string(&eval, vec![bv, TaggedValue::fixnum(1)]).unwrap();
        assert_eq!(get_result_string(&eval, result), "ABC");
    }

    #[test]
    fn test_utf8_to_string_with_start_and_end() {
        let eval = make_evaluator();
        let bv = alloc_bv(&eval, vec![0, 65, 66, 67, 0]);
        let result = utf8_to_string(
            &eval,
            vec![bv, TaggedValue::fixnum(1), TaggedValue::fixnum(4)],
        )
        .unwrap();
        assert_eq!(get_result_string(&eval, result), "ABC");
    }

    #[test]
    fn test_utf8_to_string_unicode() {
        let eval = make_evaluator();
        // Greek lambda: U+03BB = 206 187 in UTF-8
        let bv = alloc_bv(&eval, vec![206, 187]);
        let result = utf8_to_string(&eval, vec![bv]).unwrap();
        assert_eq!(get_result_string(&eval, result), "λ");
    }

    #[test]
    fn test_utf8_to_string_unicode_with_range() {
        let eval = make_evaluator();
        let bv = alloc_bv(&eval, vec![0, 206, 187, 0]);
        let result = utf8_to_string(
            &eval,
            vec![bv, TaggedValue::fixnum(1), TaggedValue::fixnum(3)],
        )
        .unwrap();
        assert_eq!(get_result_string(&eval, result), "λ");
    }

    #[test]
    fn test_utf8_to_string_empty() {
        let eval = make_evaluator();
        let bv = alloc_bv(&eval, vec![]);
        let result = utf8_to_string(&eval, vec![bv]).unwrap();
        assert_eq!(get_result_string(&eval, result), "");
    }

    #[test]
    fn test_utf8_to_string_invalid_utf8() {
        let eval = make_evaluator();
        let bv = alloc_bv(&eval, vec![0xFF, 0xFE]);
        let result = utf8_to_string(&eval, vec![bv]);
        assert!(result.is_err());
    }

    #[test]
    fn test_utf8_to_string_start_out_of_bounds() {
        let eval = make_evaluator();
        let bv = alloc_bv(&eval, vec![65, 66, 67]);
        let result = utf8_to_string(&eval, vec![bv, TaggedValue::fixnum(10)]);
        assert!(result.is_err());
    }

    // string->utf8 tests

    #[test]
    fn test_string_to_utf8_basic() {
        let eval = make_evaluator();
        let s = alloc_str(&eval, "ABC");
        let result = string_to_utf8(&eval, vec![s]).unwrap();
        assert_eq!(get_result_bytes(&eval, result), vec![65, 66, 67]);
    }

    #[test]
    fn test_string_to_utf8_with_start() {
        let eval = make_evaluator();
        let s = alloc_str(&eval, "ABC");
        let result = string_to_utf8(&eval, vec![s, TaggedValue::fixnum(1)]).unwrap();
        assert_eq!(get_result_bytes(&eval, result), vec![66, 67]); // "BC"
    }

    #[test]
    fn test_string_to_utf8_with_start_and_end() {
        let eval = make_evaluator();
        let s = alloc_str(&eval, "ABC");
        let result = string_to_utf8(
            &eval,
            vec![s, TaggedValue::fixnum(1), TaggedValue::fixnum(2)],
        )
        .unwrap();
        assert_eq!(get_result_bytes(&eval, result), vec![66]); // "B"
    }

    #[test]
    fn test_string_to_utf8_unicode() {
        let eval = make_evaluator();
        let s = alloc_str(&eval, "λ");
        let result = string_to_utf8(&eval, vec![s]).unwrap();
        assert_eq!(get_result_bytes(&eval, result), vec![206, 187]);
    }

    #[test]
    fn test_string_to_utf8_unicode_substring() {
        let eval = make_evaluator();
        let s = alloc_str(&eval, "aλb");
        let result = string_to_utf8(
            &eval,
            vec![s, TaggedValue::fixnum(1), TaggedValue::fixnum(2)],
        )
        .unwrap();
        assert_eq!(get_result_bytes(&eval, result), vec![206, 187]); // Just λ
    }

    #[test]
    fn test_string_to_utf8_empty() {
        let eval = make_evaluator();
        let s = alloc_str(&eval, "");
        let result = string_to_utf8(&eval, vec![s]).unwrap();
        assert_eq!(get_result_bytes(&eval, result), vec![]);
    }

    #[test]
    fn test_string_to_utf8_start_out_of_bounds() {
        let eval = make_evaluator();
        let s = alloc_str(&eval, "ABC");
        let result = string_to_utf8(&eval, vec![s, TaggedValue::fixnum(10)]);
        assert!(result.is_err());
    }

    // Round-trip tests

    #[test]
    fn test_utf8_roundtrip_ascii() {
        let eval = make_evaluator();
        let original = "Hello, World!";
        let s = alloc_str(&eval, original);

        // string->utf8
        let bv = string_to_utf8(&eval, vec![s]).unwrap();

        // utf8->string
        let result = utf8_to_string(&eval, vec![bv]).unwrap();
        assert_eq!(get_result_string(&eval, result), original);
    }

    #[test]
    fn test_utf8_roundtrip_unicode() {
        let eval = make_evaluator();
        let original = "Hello, 世界! λ ∀x∈ℕ";
        let s = alloc_str(&eval, original);

        // string->utf8
        let bv = string_to_utf8(&eval, vec![s]).unwrap();

        // utf8->string
        let result = utf8_to_string(&eval, vec![bv]).unwrap();
        assert_eq!(get_result_string(&eval, result), original);
    }
}
