//! Type predicate primitives (R7RS Section 6.x)
//!
//! Implements type checking predicates for all Scheme types.
//! Uses Heap methods directly on TaggedValue for efficient type checking
//! without conversion to Value.
use patina_core::TaggedValue;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

pub(super) fn rational_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    Ok(TaggedValue::boolean(
        heap.borrow().is_rational_r7rs(args[0]),
    ))
}

pub(super) fn real_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    Ok(TaggedValue::boolean(heap.borrow().is_real_r7rs(args[0])))
}

pub(super) fn complex_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // R7RS: All numbers are complex numbers
    Ok(TaggedValue::boolean(heap.borrow().is_number(args[0])))
}

pub(super) fn symbol_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    Ok(TaggedValue::boolean(heap.borrow().is_symbol(args[0])))
}

pub(super) fn list_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    Ok(TaggedValue::boolean(heap.borrow().is_list(args[0])))
}

pub(super) fn inexact_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    Ok(TaggedValue::boolean(
        heap.borrow().is_inexact_number(args[0]),
    ))
}

pub(super) fn boolean_equal(
    _heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    // Check that first argument is a boolean
    if !args[0].is_boolean() {
        return Err(EvalError::TypeError(
            "boolean=? expects all arguments to be booleans".to_string(),
        ));
    }
    let first_val = args[0].as_bool_unchecked();

    // Check that all remaining values are booleans and equal to the first
    for arg in &args[1..] {
        if !arg.is_boolean() {
            return Err(EvalError::TypeError(
                "boolean=? expects all arguments to be booleans".to_string(),
            ));
        }
        if arg.as_bool_unchecked() != first_val {
            return Ok(TaggedValue::FALSE);
        }
    }

    Ok(TaggedValue::TRUE)
}

pub(super) fn procedure_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // Fast path: closures have their own tag
    if args[0].is_closure() {
        return Ok(TaggedValue::TRUE);
    }

    // Check heap for procedures and continuations
    let heap_ref = heap.borrow();
    // R7RS: Continuations captured by call/cc satisfy procedure?
    let is_proc = heap_ref.is_procedure(args[0]) || heap_ref.is_continuation(args[0]);
    Ok(TaggedValue::boolean(is_proc))
}

// ===== TaggedValue predicates with fast paths =====

/// (null? obj) - Fast path using TaggedValue
pub(super) fn null_p(_heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(args[0].is_null()))
}

/// (pair? obj) - Check for pair (native or boxed)
pub(super) fn pair_p(_heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(args[0].is_pair()))
}

/// (not obj) - Returns #t if obj is #f, #f otherwise
pub(super) fn not_p(_heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(!args[0].is_truthy()))
}

/// (boolean? obj) - Fast path using TaggedValue
pub(super) fn boolean_p(
    _heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(args[0].is_boolean()))
}

/// (char? obj) - Fast path using TaggedValue
pub(super) fn char_p(_heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(args[0].is_char()))
}

/// (string? obj) - Check for string (native heap string)
pub(super) fn string_p(_heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(args[0].is_string()))
}

/// (vector? obj) - Check for vector (native heap vector)
pub(super) fn vector_p(_heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(args[0].is_vector()))
}

/// (integer? obj) - Fast path for fixnums
pub(super) fn integer_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    Ok(TaggedValue::boolean(heap.borrow().is_integer_r7rs(args[0])))
}

/// (exact-integer? obj) - Fast path for fixnums
pub(super) fn exact_integer_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // exact-integer? is true for exact numbers that are also integers
    // This is fixnum, BigInt, or Rational with denominator 1
    let heap_ref = heap.borrow();
    let is_exact_int = heap_ref.is_exact_number(args[0]) && heap_ref.is_integer_r7rs(args[0]);
    Ok(TaggedValue::boolean(is_exact_int))
}

/// (exact? obj) - Fast path for fixnums
pub(super) fn exact_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    Ok(TaggedValue::boolean(heap.borrow().is_exact_number(args[0])))
}

/// (number? obj) - Fast path for fixnums
pub(super) fn number_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // Fast path: fixnums are numbers
    if args[0].is_fixnum() {
        return Ok(TaggedValue::TRUE);
    }

    // Check heap for other numeric types
    Ok(TaggedValue::boolean(heap.borrow().is_number(args[0])))
}

pub(super) fn register(registry: &mut crate::registry::PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Numeric type predicates (with TaggedValue fast paths)
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "number?",
        Arity::Exact(1),
        "Returns #t if obj is a number.",
        number_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "complex?",
        Arity::Exact(1),
        "Returns #t if obj is a complex number.",
        complex_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "real?",
        Arity::Exact(1),
        "Returns #t if obj is a real number.",
        real_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "rational?",
        Arity::Exact(1),
        "Returns #t if obj is a rational number.",
        rational_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "integer?",
        Arity::Exact(1),
        "Returns #t if obj is an integer.",
        integer_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "exact-integer?",
        Arity::Exact(1),
        "Returns #t if obj is an exact integer.",
        exact_integer_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "exact?",
        Arity::Exact(1),
        "Returns #t if obj is an exact number.",
        exact_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "inexact?",
        Arity::Exact(1),
        "Returns #t if obj is an inexact number.",
        inexact_p,
    ));

    // Boolean negation (§6.1)
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "not",
        Arity::Exact(1),
        "Returns #t if obj is #f, and #f otherwise.",
        not_p,
    ));

    // Data type predicates (with TaggedValue fast paths)
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "boolean?",
        Arity::Exact(1),
        "Returns #t if obj is a boolean.",
        boolean_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "string?",
        Arity::Exact(1),
        "Returns #t if obj is a string.",
        string_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "symbol?",
        Arity::Exact(1),
        "Returns #t if obj is a symbol.",
        symbol_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "char?",
        Arity::Exact(1),
        "Returns #t if obj is a character.",
        char_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "vector?",
        Arity::Exact(1),
        "Returns #t if obj is a vector.",
        vector_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "procedure?",
        Arity::Exact(1),
        "Returns #t if obj is a procedure.",
        procedure_p,
    ));

    // List predicates (with TaggedValue fast paths)
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "null?",
        Arity::Exact(1),
        "Returns #t if obj is the empty list.",
        null_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "pair?",
        Arity::Exact(1),
        "Returns #t if obj is a pair.",
        pair_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "list?",
        Arity::Exact(1),
        "Returns #t if obj is a list.",
        list_p,
    ));

    // Boolean equality
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "boolean=?",
        Arity::Min(2),
        "Returns #t if all arguments are booleans and are all equal.",
        boolean_equal,
    ));
}
