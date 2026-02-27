//! Vector primitive operations (R7RS Section 6.8)
//!
//! Implements vector operations with:
//! - O(1) random access
//! - Mutable vectors via vector-set!
//! - Conversion operations with lists and strings

use super::super::Evaluator;
use super::super::error::EvalError;
use patina_core::TaggedValue;

// ========== TaggedValue Extraction Helpers ==========

/// Extract an integer from a TaggedValue (supports fixnum, BigInteger, Rational)
fn get_index(tv: TaggedValue, heap: &patina_core::Heap, fn_name: &str) -> Result<usize, EvalError> {
    if tv.is_fixnum() {
        let n = tv.as_fixnum_unchecked();
        if n < 0 {
            return Err(EvalError::IndexOutOfBounds(format!(
                "{} index must be non-negative, got {}",
                fn_name, n
            )));
        }
        return Ok(n as usize);
    }
    if let Some(n) = heap.get_bigint(tv) {
        use num_traits::ToPrimitive;
        if n.sign() == num_bigint::Sign::Minus {
            return Err(EvalError::IndexOutOfBounds(format!(
                "{} index must be non-negative",
                fn_name
            )));
        }
        return n
            .to_usize()
            .ok_or_else(|| EvalError::IndexOutOfBounds(format!("{} index too large", fn_name)));
    }
    if let Some(r) = heap.get_rational(tv) {
        use num_bigint::BigInt;
        use num_traits::ToPrimitive;
        if r.denom() != &BigInt::from(1) {
            return Err(EvalError::TypeError(format!(
                "{} index must be an integer",
                fn_name
            )));
        }
        if r.numer().sign() == num_bigint::Sign::Minus {
            return Err(EvalError::IndexOutOfBounds(format!(
                "{} index must be non-negative",
                fn_name
            )));
        }
        return r
            .numer()
            .to_usize()
            .ok_or_else(|| EvalError::IndexOutOfBounds(format!("{} index too large", fn_name)));
    }

    Err(EvalError::TypeError(format!(
        "{} expects an integer index",
        fn_name
    )))
}

/// Extract string characters from a TaggedValue
fn get_string_chars(
    tv: TaggedValue,
    heap: &patina_core::Heap,
    fn_name: &str,
) -> Result<Vec<char>, EvalError> {
    if tv.is_string() {
        return Ok(heap.get_string_chars(tv).to_vec());
    }
    if let Some(s) = heap.get_string_contents(tv) {
        return Ok(s.chars().collect());
    }
    Err(EvalError::TypeError(format!(
        "{} expects a string",
        fn_name
    )))
}

/// Extract elements from multiple vectors as Vec<Vec<TaggedValue>>.
fn extract_vectors_as_tagged(
    vector_args: &[TaggedValue],
    heap: &std::rc::Rc<std::cell::RefCell<patina_core::Heap>>,
    fn_name: &str,
) -> Result<Vec<Vec<TaggedValue>>, EvalError> {
    // Extract all elements with immutable borrow
    let heap_ref = heap.borrow();
    let mut vectors = Vec::new();
    for &tv in vector_args {
        vectors.push(
            heap_ref
                .try_vector_to_vec(tv)
                .ok_or_else(|| EvalError::TypeError(format!("{} expects a vector", fn_name)))?,
        );
    }
    Ok(vectors)
}

// ========== Vector Primitives ==========

pub(super) fn make_vector(
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

    let k = get_index(args[0], &heap_ref, "make-vector")?;
    let fill = if args.len() == 2 {
        args[1]
    } else {
        TaggedValue::UNSPECIFIED
    };
    drop(heap_ref);

    Ok(heap.borrow_mut().alloc_vector_fill(k, fill))
}

pub(super) fn vector(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    // vector accepts any number of arguments (0 or more)
    // Args are already TaggedValues, allocate native heap vector directly
    Ok(evaluator.global_env.heap().borrow_mut().alloc_vector(args))
}

pub(super) fn vector_length(
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
    let len = heap_ref
        .try_vector_len(args[0])
        .ok_or_else(|| EvalError::TypeError("vector-length expects a vector".to_string()))?;
    Ok(TaggedValue::fixnum(len as i64))
}

pub(super) fn vector_ref(
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

    // Phase 1: validate and classify with immutable borrow
    let (idx, vec_len) = {
        let heap_ref = heap.borrow();
        let vec_len = heap_ref
            .try_vector_len(args[0])
            .ok_or_else(|| EvalError::TypeError("vector-ref expects a vector".to_string()))?;
        let idx = get_index(args[1], &heap_ref, "vector-ref")?;
        (idx, vec_len)
    };

    if idx >= vec_len {
        return Err(EvalError::IndexOutOfBounds(format!(
            "vector-ref index {} out of bounds for vector of length {}",
            idx, vec_len
        )));
    }

    // Phase 2: get element (all vectors are native)
    Ok(heap.borrow().vector_ref(args[0], idx))
}

pub(super) fn vector_set(
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
    let vec_tv = args[0];
    let new_val_tv = args[2];

    // Phase 1: validate index
    let (idx, vec_len) = {
        let heap_ref = heap.borrow();
        let vec_len = heap_ref
            .try_vector_len(vec_tv)
            .ok_or_else(|| EvalError::TypeError("vector-set! expects a vector".to_string()))?;
        let idx = get_index(args[1], &heap_ref, "vector-set!")?;
        (idx, vec_len)
    };

    if idx >= vec_len {
        return Err(EvalError::IndexOutOfBounds(format!(
            "vector-set! index {} out of bounds for vector of length {}",
            idx, vec_len
        )));
    }

    // Phase 2: set element (all vectors are native)
    heap.borrow_mut().vector_set(vec_tv, idx, new_val_tv);

    Ok(TaggedValue::UNSPECIFIED)
}

pub(super) fn vector_to_list(
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

    // Phase 1: validate bounds with immutable borrow
    let (start, end) = {
        let heap_ref = heap.borrow();
        let vec_len = heap_ref
            .try_vector_len(args[0])
            .ok_or_else(|| EvalError::TypeError("vector->list expects a vector".to_string()))?;

        let start = if args.len() >= 2 {
            get_index(args[1], &heap_ref, "vector->list")?
        } else {
            0
        };

        let end = if args.len() >= 3 {
            get_index(args[2], &heap_ref, "vector->list")?
        } else {
            vec_len
        };

        if start > end || end > vec_len {
            return Err(EvalError::IndexOutOfBounds(
                "vector->list indices out of bounds".to_string(),
            ));
        }

        (start, end)
    };

    // Phase 2: extract elements (all vectors are native)
    let items: Vec<TaggedValue> = heap.borrow().vector_slice(args[0])[start..end].to_vec();

    // Build list
    Ok(heap.borrow_mut().list_from_iter(items))
}

pub(super) fn list_to_vector(
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

    // Try fast path using heap's list_to_vec for native heap lists
    let tvs_opt = { heap.borrow().list_to_vec(args[0]) };

    if let Some(tvs) = tvs_opt {
        // Fast path: native heap list - allocate native vector directly
        Ok(heap.borrow_mut().alloc_vector(tvs))
    } else {
        // Slow path: walk tagged list directly (handles boxed pairs)
        let heap_ref = heap.borrow();
        let mut elements = Vec::new();
        let mut current = args[0];
        while !current.is_null() {
            if !current.is_pair() {
                return Err(EvalError::TypeError(
                    "list->vector: argument must be a proper list".to_string(),
                ));
            }
            elements.push(heap_ref.car(current));
            current = heap_ref.cdr(current);
        }
        drop(heap_ref);
        Ok(heap.borrow_mut().alloc_vector(elements))
    }
}

pub(super) fn vector_to_string(
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

    // Phase 1: validate bounds
    let (start, end) = {
        let heap_ref = heap.borrow();
        let vec_len = heap_ref
            .try_vector_len(args[0])
            .ok_or_else(|| EvalError::TypeError("vector->string expects a vector".to_string()))?;

        let start = if args.len() >= 2 {
            get_index(args[1], &heap_ref, "vector->string")?
        } else {
            0
        };

        let end = if args.len() >= 3 {
            get_index(args[2], &heap_ref, "vector->string")?
        } else {
            vec_len
        };

        if start > end || end > vec_len {
            return Err(EvalError::IndexOutOfBounds(
                "indices out of bounds".to_string(),
            ));
        }

        (start, end)
    };

    // Phase 2: extract characters from vector elements (all vectors are native)
    let chars = {
        let heap_ref = heap.borrow();
        let slice = heap_ref.vector_slice(args[0]);
        let mut chars = Vec::new();
        for tv in &slice[start..end] {
            if tv.is_char() {
                chars.push(tv.as_char_unchecked());
            } else {
                return Err(EvalError::TypeError(
                    "vector->string requires vector of characters".to_string(),
                ));
            }
        }
        chars
    };

    Ok(heap.borrow_mut().alloc_string_chars(chars))
}

pub(super) fn string_to_vector(
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

    let chars = get_string_chars(args[0], &heap_ref, "string->vector")?;

    let start = if args.len() >= 2 {
        get_index(args[1], &heap_ref, "string->vector")?
    } else {
        0
    };

    let end = if args.len() >= 3 {
        get_index(args[2], &heap_ref, "string->vector")?
    } else {
        chars.len()
    };
    drop(heap_ref);

    if start > end || end > chars.len() {
        return Err(EvalError::IndexOutOfBounds(
            "indices out of bounds".to_string(),
        ));
    }

    let elements: Vec<TaggedValue> = chars[start..end]
        .iter()
        .map(|c| TaggedValue::character(*c))
        .collect();
    Ok(heap.borrow_mut().alloc_vector(elements))
}

pub(super) fn vector_copy(
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

    // Phase 1: validate bounds
    let (start, end) = {
        let heap_ref = heap.borrow();
        let vec_len = heap_ref
            .try_vector_len(args[0])
            .ok_or_else(|| EvalError::TypeError("vector-copy expects a vector".to_string()))?;

        let start = if args.len() >= 2 {
            get_index(args[1], &heap_ref, "vector-copy")?
        } else {
            0
        };

        let end = if args.len() >= 3 {
            get_index(args[2], &heap_ref, "vector-copy")?
        } else {
            vec_len
        };

        if start > end || end > vec_len {
            return Err(EvalError::IndexOutOfBounds(
                "indices out of bounds".to_string(),
            ));
        }

        (start, end)
    };

    // Phase 2: extract elements (all vectors are native)
    let elements: Vec<TaggedValue> = heap.borrow().vector_slice(args[0])[start..end].to_vec();

    Ok(heap.borrow_mut().alloc_vector(elements))
}

pub(super) fn vector_copy_bang(
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
    let to_tv = args[0];
    let from_tv = args[2];

    // Phase 1: validate bounds and extract source elements
    let (at, from_elements) = {
        let heap_ref = heap.borrow();
        let at = get_index(args[1], &heap_ref, "vector-copy!")?;
        let from_len = heap_ref
            .try_vector_len(from_tv)
            .ok_or_else(|| EvalError::TypeError("vector-copy! expects a vector".to_string()))?;
        let to_len = heap_ref
            .try_vector_len(to_tv)
            .ok_or_else(|| EvalError::TypeError("vector-copy! expects a vector".to_string()))?;

        let start = if args.len() >= 4 {
            get_index(args[3], &heap_ref, "vector-copy!")?
        } else {
            0
        };

        let end = if args.len() >= 5 {
            get_index(args[4], &heap_ref, "vector-copy!")?
        } else {
            from_len
        };

        if start > end || end > from_len {
            return Err(EvalError::IndexOutOfBounds(
                "from indices out of bounds".to_string(),
            ));
        }

        let copy_len = end - start;

        if at > to_len {
            return Err(EvalError::IndexOutOfBounds(
                "at index out of bounds".to_string(),
            ));
        }

        if to_len - at < copy_len {
            return Err(EvalError::IndexOutOfBounds(
                "not enough space in destination vector".to_string(),
            ));
        }

        // Extract source elements (all vectors are native)
        let from_elements: Vec<TaggedValue> = heap_ref.vector_slice(from_tv)[start..end].to_vec();
        (at, from_elements)
    };

    copy_elements_to_vector(heap, to_tv, at, &from_elements)
}

/// Helper: copy elements into a destination vector (all vectors are native)
fn copy_elements_to_vector(
    heap: &std::rc::Rc<std::cell::RefCell<patina_core::Heap>>,
    to_tv: TaggedValue,
    at: usize,
    elements: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    let mut heap_mut = heap.borrow_mut();
    for (i, &elem) in elements.iter().enumerate() {
        heap_mut.vector_set(to_tv, at + i, elem);
    }
    Ok(TaggedValue::UNSPECIFIED)
}

pub(super) fn vector_append(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    // vector-append accepts any number of arguments (0 or more)
    let heap = evaluator.global_env.heap();

    // Validate all args are vectors first
    for &arg in &args {
        if !arg.is_vector() {
            return Err(EvalError::TypeError(
                "vector-append expects a vector".to_string(),
            ));
        }
    }

    // Collect all elements
    let mut all_elements: Vec<TaggedValue> = Vec::new();
    {
        let heap_ref = heap.borrow();
        for &arg in &args {
            all_elements.extend_from_slice(heap_ref.vector_slice(arg));
        }
    }

    Ok(heap.borrow_mut().alloc_vector(all_elements))
}

pub(super) fn vector_fill(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 || args.len() > 4 {
        return Err(EvalError::WrongArity {
            expected: "2 to 4".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let vec_tv = args[0];
    let fill_tv = args[1];

    // Phase 1: validate bounds
    let (start, end) = {
        let heap_ref = heap.borrow();
        let vec_len = heap_ref
            .try_vector_len(vec_tv)
            .ok_or_else(|| EvalError::TypeError("vector-fill! expects a vector".to_string()))?;

        let start = if args.len() >= 3 {
            get_index(args[2], &heap_ref, "vector-fill!")?
        } else {
            0
        };

        let end = if args.len() >= 4 {
            get_index(args[3], &heap_ref, "vector-fill!")?
        } else {
            vec_len
        };

        if start > end || end > vec_len {
            return Err(EvalError::IndexOutOfBounds(
                "indices out of bounds".to_string(),
            ));
        }

        (start, end)
    };

    // Phase 2: fill (all vectors are native)
    let mut heap_mut = heap.borrow_mut();
    for i in start..end {
        heap_mut.vector_set(vec_tv, i, fill_tv);
    }

    Ok(TaggedValue::UNSPECIFIED)
}

pub(super) fn vector_map(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let proc = args[0];

    // Extract vector elements as TaggedValues
    let vectors: Vec<Vec<TaggedValue>> = extract_vectors_as_tagged(&args[1..], heap, "vector-map")?;

    if vectors.is_empty() {
        return Ok(heap.borrow_mut().alloc_vector(Vec::new()));
    }

    let min_len = vectors.iter().map(|v| v.len()).min().unwrap_or(0);
    let mut result = Vec::new();

    for i in 0..min_len {
        let proc_args: Vec<TaggedValue> = vectors.iter().map(|v| v[i]).collect();
        let val = match evaluator.apply(proc, proc_args, false)? {
            super::super::EvalResult::Tagged(tv) => tv,
            super::super::EvalResult::TailCallPrimitive { .. } => {
                return Err(EvalError::InternalError(
                    "Unexpected tail call in vector-map".to_string(),
                ));
            }
        };
        result.push(val);
    }

    Ok(heap.borrow_mut().alloc_vector(result))
}

pub(super) fn vector_for_each(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let proc = args[0];

    // Extract vector elements as TaggedValues
    let vectors: Vec<Vec<TaggedValue>> =
        extract_vectors_as_tagged(&args[1..], heap, "vector-for-each")?;

    if vectors.is_empty() {
        return Ok(TaggedValue::UNSPECIFIED);
    }

    let min_len = vectors.iter().map(|v| v.len()).min().unwrap_or(0);

    for i in 0..min_len {
        let proc_args: Vec<TaggedValue> = vectors.iter().map(|v| v[i]).collect();
        match evaluator.apply(proc, proc_args, false)? {
            super::super::EvalResult::Tagged(_) => {}
            _ => {
                return Err(EvalError::InternalError(
                    "Unexpected tail call in vector-for-each".to_string(),
                ));
            }
        }
    }

    Ok(TaggedValue::UNSPECIFIED)
}

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Make-vector
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "make-vector",
        Arity::Range(1, 2),
        "Returns a newly allocated vector of k elements.",
        |eval, args, _tail| make_vector(eval, args).map(EvalResult::Tagged),
    ));

    // Vector constructor
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "vector",
        Arity::Min(0),
        "Returns a newly allocated vector whose elements contain the given arguments.",
        |eval, args, _tail| vector(eval, args).map(EvalResult::Tagged),
    ));

    // Vector-length
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "vector-length",
        Arity::Exact(1),
        "Returns the number of elements in vector.",
        |eval, args, _tail| vector_length(eval, args).map(EvalResult::Tagged),
    ));

    // Vector-ref
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "vector-ref",
        Arity::Exact(2),
        "Returns the contents of element k of vector.",
        |eval, args, _tail| vector_ref(eval, args).map(EvalResult::Tagged),
    ));

    // Vector-set!
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "vector-set!",
        Arity::Exact(3),
        "Stores obj in element k of vector.",
        |eval, args, _tail| vector_set(eval, args).map(EvalResult::Tagged),
    ));

    // Vector->list
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "vector->list",
        Arity::Range(1, 3),
        "Returns a newly allocated list of the objects contained in the elements of vector.",
        |eval, args, _tail| vector_to_list(eval, args).map(EvalResult::Tagged),
    ));

    // List->vector
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "list->vector",
        Arity::Exact(1),
        "Returns a newly allocated vector of the objects contained in the list.",
        |eval, args, _tail| list_to_vector(eval, args).map(EvalResult::Tagged),
    ));

    // Vector->string
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "vector->string",
        Arity::Range(1, 3),
        "Returns a newly allocated string of the characters contained in the elements of vector.",
        |eval, args, _tail| vector_to_string(eval, args).map(EvalResult::Tagged),
    ));

    // String->vector
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "string->vector",
        Arity::Range(1, 3),
        "Returns a newly allocated vector of the characters contained in the string.",
        |eval, args, _tail| string_to_vector(eval, args).map(EvalResult::Tagged),
    ));

    // Vector-copy
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "vector-copy",
        Arity::Range(1, 3),
        "Returns a newly allocated copy of the elements of the given vector.",
        |eval, args, _tail| vector_copy(eval, args).map(EvalResult::Tagged),
    ));

    // Vector-copy!
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "vector-copy!",
        Arity::Range(3, 5),
        "Copies the elements of vector from at to into vector to, starting at at.",
        |eval, args, _tail| vector_copy_bang(eval, args).map(EvalResult::Tagged),
    ));

    // Vector-append
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "vector-append",
        Arity::Min(0),
        "Returns a newly allocated vector whose elements are the concatenation of the elements of the given vectors.",
        |eval, args, _tail| vector_append(eval, args).map(EvalResult::Tagged),
    ));

    // Vector-fill!
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "vector-fill!",
        Arity::Range(2, 4),
        "Stores fill in the elements of vector.",
        |eval, args, _tail| vector_fill(eval, args).map(EvalResult::Tagged),
    ));

    // Vector-map
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "vector-map",
        Arity::Min(2),
        "Returns a newly allocated vector of the results of applying proc element-wise to the elements of the vectors.",
        |eval, args, _tail| vector_map(eval, args).map(EvalResult::Tagged),
    ));

    // Vector-for-each
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "vector-for-each",
        Arity::Min(2),
        "Applies proc element-wise to the elements of the vectors for side effects.",
        |eval, args, _tail| vector_for_each(eval, args).map(EvalResult::Tagged),
    ));
}
