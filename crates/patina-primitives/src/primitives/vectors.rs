//! Vector primitive operations (R7RS Section 6.8)
//!
//! Implements vector operations with:
//! - O(1) random access
//! - Mutable vectors via vector-set!
//! - Conversion operations with lists and strings
//!
//! Each takes the context (#706). A read borrows the vector's elements
//! through the `Cx`, and the mutators store through the context's funnel
//! (`Cx::vector_set`, and `store_range` and `fill_range` for a run).

use patina_core::{Cx, Value};
use patina_runtime::EvalError;

// ========== Argument Extraction Helpers ==========

/// Extract an integer from a value (supports fixnum, BigInteger, Rational)
fn get_index<'gc>(cx: &Cx<'gc>, tv: Value<'gc>, fn_name: &str) -> Result<usize, EvalError> {
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
    if let Some(n) = cx.get_bigint(tv) {
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
    if let Some(r) = cx.get_rational(tv) {
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

/// Extract string characters from a value
fn get_string_chars<'gc>(
    cx: &Cx<'gc>,
    tv: Value<'gc>,
    fn_name: &str,
) -> Result<Vec<char>, EvalError> {
    if tv.is_string() {
        return Ok(cx.get_string_chars(tv).to_vec());
    }
    if let Some(s) = cx.get_string_contents(tv) {
        return Ok(s.chars().collect());
    }
    Err(EvalError::TypeError(format!(
        "{} expects a string",
        fn_name
    )))
}

// ========== Vector Primitives ==========

pub(super) fn make_vector<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1 or 2".to_string(),
            actual: args.len(),
        });
    }

    let k = get_index(cx, args[0], "make-vector")?;
    let fill = if args.len() == 2 {
        args[1]
    } else {
        Value::UNSPECIFIED
    };

    Ok(cx.alloc_vector_fill(k, fill))
}

pub(super) fn vector<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    // vector accepts any number of arguments (0 or more)
    // Allocate the native heap vector directly
    Ok(cx.alloc_vector(args.to_vec()))
}

pub(super) fn vector_length<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let len = cx
        .try_vector_len(args[0])
        .ok_or_else(|| EvalError::TypeError("vector-length expects a vector".to_string()))?;
    Ok(Value::fixnum(len as i64))
}

pub(super) fn vector_ref<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    // Phase 1: validate and classify
    let (idx, vec_len) = {
        let vec_len = cx
            .try_vector_len(args[0])
            .ok_or_else(|| EvalError::TypeError("vector-ref expects a vector".to_string()))?;
        let idx = get_index(cx, args[1], "vector-ref")?;
        (idx, vec_len)
    };

    if idx >= vec_len {
        return Err(EvalError::IndexOutOfBounds(format!(
            "vector-ref index {} out of bounds for vector of length {}",
            idx, vec_len
        )));
    }

    // Phase 2: get element (all vectors are native)
    Ok(cx.vector_ref(args[0], idx))
}

pub(super) fn vector_set<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 3 {
        return Err(EvalError::WrongArity {
            expected: "3".to_string(),
            actual: args.len(),
        });
    }

    let vec_tv = args[0];
    let new_val_tv = args[2];

    // Phase 1: validate index
    let (idx, vec_len) = {
        let vec_len = cx
            .try_vector_len(vec_tv)
            .ok_or_else(|| EvalError::TypeError("vector-set! expects a vector".to_string()))?;
        let idx = get_index(cx, args[1], "vector-set!")?;
        (idx, vec_len)
    };

    if idx >= vec_len {
        return Err(EvalError::IndexOutOfBounds(format!(
            "vector-set! index {} out of bounds for vector of length {}",
            idx, vec_len
        )));
    }

    // Phase 2: set element (all vectors are native)
    cx.vector_set(vec_tv, idx, new_val_tv);

    Ok(Value::UNSPECIFIED)
}

pub(super) fn vector_to_list<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

    // Phase 1: validate bounds
    let (start, end) = {
        let vec_len = cx
            .try_vector_len(args[0])
            .ok_or_else(|| EvalError::TypeError("vector->list expects a vector".to_string()))?;

        let start = if args.len() >= 2 {
            get_index(cx, args[1], "vector->list")?
        } else {
            0
        };

        let end = if args.len() >= 3 {
            get_index(cx, args[2], "vector->list")?
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
    let items: Vec<Value<'gc>> = cx.vector_slice(args[0])[start..end].to_vec();

    // Build list
    Ok(cx.list_from_iter(items))
}

pub(super) fn list_to_vector<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let elements = cx.list_to_vec(args[0]).ok_or_else(|| {
        EvalError::TypeError("list->vector: argument must be a proper list".into())
    })?;
    Ok(cx.alloc_vector(elements))
}

pub(super) fn vector_to_string<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

    // Phase 1: validate bounds
    let (start, end) = {
        let vec_len = cx
            .try_vector_len(args[0])
            .ok_or_else(|| EvalError::TypeError("vector->string expects a vector".to_string()))?;

        let start = if args.len() >= 2 {
            get_index(cx, args[1], "vector->string")?
        } else {
            0
        };

        let end = if args.len() >= 3 {
            get_index(cx, args[2], "vector->string")?
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
        let slice = cx.vector_slice(args[0]);
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

    Ok(cx.alloc_string_chars(chars))
}

pub(super) fn string_to_vector<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

    let chars = get_string_chars(cx, args[0], "string->vector")?;

    let start = if args.len() >= 2 {
        get_index(cx, args[1], "string->vector")?
    } else {
        0
    };

    let end = if args.len() >= 3 {
        get_index(cx, args[2], "string->vector")?
    } else {
        chars.len()
    };

    if start > end || end > chars.len() {
        return Err(EvalError::IndexOutOfBounds(
            "indices out of bounds".to_string(),
        ));
    }

    let elements: Vec<Value<'gc>> = chars[start..end]
        .iter()
        .map(|c| Value::character(*c))
        .collect();
    Ok(cx.alloc_vector(elements))
}

pub(super) fn vector_copy<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 3 {
        return Err(EvalError::WrongArity {
            expected: "1 to 3".to_string(),
            actual: args.len(),
        });
    }

    // Phase 1: validate bounds
    let (start, end) = {
        let vec_len = cx
            .try_vector_len(args[0])
            .ok_or_else(|| EvalError::TypeError("vector-copy expects a vector".to_string()))?;

        let start = if args.len() >= 2 {
            get_index(cx, args[1], "vector-copy")?
        } else {
            0
        };

        let end = if args.len() >= 3 {
            get_index(cx, args[2], "vector-copy")?
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
    let elements: Vec<Value<'gc>> = cx.vector_slice(args[0])[start..end].to_vec();

    Ok(cx.alloc_vector(elements))
}

pub(super) fn vector_copy_bang<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() < 3 || args.len() > 5 {
        return Err(EvalError::WrongArity {
            expected: "3 to 5".to_string(),
            actual: args.len(),
        });
    }

    let to_tv = args[0];
    let from_tv = args[2];

    // Phase 1: validate bounds and extract source elements
    let (at, from_elements) = {
        let at = get_index(cx, args[1], "vector-copy!")?;
        let from_len = cx
            .try_vector_len(from_tv)
            .ok_or_else(|| EvalError::TypeError("vector-copy! expects a vector".to_string()))?;
        let to_len = cx
            .try_vector_len(to_tv)
            .ok_or_else(|| EvalError::TypeError("vector-copy! expects a vector".to_string()))?;

        let start = if args.len() >= 4 {
            get_index(cx, args[3], "vector-copy!")?
        } else {
            0
        };

        let end = if args.len() >= 5 {
            get_index(cx, args[4], "vector-copy!")?
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

        // Extract source elements (all vectors are native): copied first, so
        // a run copied within one vector reads none it has written
        let from_elements: Vec<Value<'gc>> = cx.vector_slice(from_tv)[start..end].to_vec();
        (at, from_elements)
    };

    // Phase 2: store the run (all vectors are native)
    cx.store_range(to_tv, at, from_elements.into_iter());
    Ok(Value::UNSPECIFIED)
}

pub(super) fn vector_append<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    // vector-append accepts any number of arguments (0 or more)

    // Validate all args are vectors first
    for &arg in args {
        if !arg.is_vector() {
            return Err(EvalError::TypeError(
                "vector-append expects a vector".to_string(),
            ));
        }
    }

    // Collect all elements
    let mut all_elements: Vec<Value<'gc>> = Vec::new();
    for &arg in args {
        all_elements.extend_from_slice(&cx.vector_slice(arg));
    }

    Ok(cx.alloc_vector(all_elements))
}

pub(super) fn vector_fill<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() < 2 || args.len() > 4 {
        return Err(EvalError::WrongArity {
            expected: "2 to 4".to_string(),
            actual: args.len(),
        });
    }

    let vec_tv = args[0];
    let fill_tv = args[1];

    // Phase 1: validate bounds
    let (start, end) = {
        let vec_len = cx
            .try_vector_len(vec_tv)
            .ok_or_else(|| EvalError::TypeError("vector-fill! expects a vector".to_string()))?;

        let start = if args.len() >= 3 {
            get_index(cx, args[2], "vector-fill!")?
        } else {
            0
        };

        let end = if args.len() >= 4 {
            get_index(cx, args[3], "vector-fill!")?
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
    cx.fill_range(vec_tv, start, end - start, fill_tv);

    Ok(Value::UNSPECIFIED)
}

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Make-vector
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "make-vector",
        Arity::Range(1, 2),
        "Returns a newly allocated vector of k elements.",
        make_vector,
    ));

    // Vector constructor
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "vector",
        Arity::Min(0),
        "Returns a newly allocated vector whose elements contain the given arguments.",
        vector,
    ));

    // Vector-length
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "vector-length",
        Arity::Exact(1),
        "Returns the number of elements in vector.",
        vector_length,
    ));

    // Vector-ref
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "vector-ref",
        Arity::Exact(2),
        "Returns the contents of element k of vector.",
        vector_ref,
    ));

    // Vector-set!
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "vector-set!",
        Arity::Exact(3),
        "Stores obj in element k of vector.",
        vector_set,
    ));

    // Vector->list
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "vector->list",
        Arity::Range(1, 3),
        "Returns a newly allocated list of the objects contained in the elements of vector.",
        vector_to_list,
    ));

    // List->vector
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "list->vector",
        Arity::Exact(1),
        "Returns a newly allocated vector of the objects contained in the list.",
        list_to_vector,
    ));

    // Vector->string
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "vector->string",
        Arity::Range(1, 3),
        "Returns a newly allocated string of the characters contained in the elements of vector.",
        vector_to_string,
    ));

    // String->vector
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "string->vector",
        Arity::Range(1, 3),
        "Returns a newly allocated vector of the characters contained in the string.",
        string_to_vector,
    ));

    // Vector-copy
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "vector-copy",
        Arity::Range(1, 3),
        "Returns a newly allocated copy of the elements of the given vector.",
        vector_copy,
    ));

    // Vector-copy!
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "vector-copy!",
        Arity::Range(3, 5),
        "Copies the elements of vector from at to into vector to, starting at at.",
        vector_copy_bang,
    ));

    // Vector-append
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "vector-append",
        Arity::Min(0),
        "Returns a newly allocated vector whose elements are the concatenation of the elements of the given vectors.",
        vector_append,
    ));

    // Vector-fill!
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "vector-fill!",
        Arity::Range(2, 4),
        "Stores fill in the elements of vector.",
        vector_fill,
    ));
}
