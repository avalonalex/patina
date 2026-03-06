//! Bytevector primitive operations (R7RS Section 6.9)
//!
//! Implements R7RS bytevector operations.
//! Bytevectors are vectors of exact integers in the range 0-255.
use patina_core::TaggedValue;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

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
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    if heap_ref.is_any_bytevector(args[0]) {
        Ok(TaggedValue::TRUE)
    } else {
        Ok(TaggedValue::FALSE)
    }
}

/// (make-bytevector k [byte]) - Create bytevector of k bytes
pub(super) fn make_bytevector(
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1 or 2".to_string(),
            actual: args.len(),
        });
    }

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
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
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
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

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
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

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
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 3 {
        return Err(EvalError::WrongArity {
            expected: "3".to_string(),
            actual: args.len(),
        });
    }

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
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

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
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 3 || args.len() > 5 {
        return Err(EvalError::WrongArity {
            expected: "3 to 5".to_string(),
            actual: args.len(),
        });
    }

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
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
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
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

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
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

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

pub(super) fn register(registry: &mut crate::registry::PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // bytevector? - Type predicate
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "bytevector?",
        Arity::Exact(1),
        "Returns #t if obj is a bytevector.",
        bytevector_p,
    ));

    // make-bytevector - Create bytevector
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "make-bytevector",
        Arity::Range(1, 2),
        "Returns a newly allocated bytevector of k bytes, optionally filled with byte.",
        make_bytevector,
    ));

    // bytevector - Construct from arguments
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "bytevector",
        Arity::Min(0),
        "Returns a newly allocated bytevector whose elements are the given arguments.",
        bytevector,
    ));

    // bytevector-length - Get length
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "bytevector-length",
        Arity::Exact(1),
        "Returns the length of bytevector in bytes.",
        bytevector_length,
    ));

    // bytevector-u8-ref - Get byte
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "bytevector-u8-ref",
        Arity::Exact(2),
        "Returns the kth byte of bytevector.",
        bytevector_u8_ref,
    ));

    // bytevector-u8-set! - Set byte
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "bytevector-u8-set!",
        Arity::Exact(3),
        "Stores byte as the kth byte of bytevector.",
        bytevector_u8_set,
    ));

    // bytevector-copy - Copy bytevector
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "bytevector-copy",
        Arity::Range(1, 3),
        "Returns a newly allocated bytevector containing the bytes in bytevector between start and end.",
        bytevector_copy,
    ));

    // bytevector-copy! - Copy bytes
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "bytevector-copy!",
        Arity::Range(3, 5),
        "Copies the bytes of from between start and end to to, starting at at.",
        bytevector_copy_mut,
    ));

    // bytevector-append - Concatenate bytevectors
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "bytevector-append",
        Arity::Min(0),
        "Returns a newly allocated bytevector whose bytes are the concatenation of the bytes in the given bytevectors.",
        bytevector_append,
    ));

    // utf8->string - Decode UTF-8 bytevector to string
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "utf8->string",
        Arity::Range(1, 3),
        "Decodes the bytes of a bytevector between start and end as UTF-8 and returns the corresponding string.",
        utf8_to_string,
    ));

    // string->utf8 - Encode string as UTF-8 bytevector
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "string->utf8",
        Arity::Range(1, 3),
        "Encodes the characters of a string between start and end as UTF-8 and returns the corresponding bytevector.",
        string_to_utf8,
    ));
}
