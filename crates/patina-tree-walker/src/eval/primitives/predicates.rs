//! Type predicate primitives (R7RS Section 6.x)
//!
//! Implements type checking predicates for all Scheme types.
//! Uses Heap methods directly on TaggedValue for efficient type checking
//! without conversion to Value.

use super::super::Evaluator;
use super::super::error::EvalError;
use patina_core::TaggedValue;

pub(super) fn rational_p(
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
    Ok(TaggedValue::boolean(
        heap.borrow().is_rational_r7rs(args[0]),
    ))
}

pub(super) fn real_p(
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
    Ok(TaggedValue::boolean(heap.borrow().is_real_r7rs(args[0])))
}

pub(super) fn complex_p(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // R7RS: All numbers are complex numbers
    let heap = evaluator.global_env.heap();
    Ok(TaggedValue::boolean(heap.borrow().is_number(args[0])))
}

pub(super) fn symbol_p(
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
    Ok(TaggedValue::boolean(heap.borrow().is_symbol(args[0])))
}

pub(super) fn list_p(
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
    Ok(TaggedValue::boolean(heap.borrow().is_list(args[0])))
}

pub(super) fn inexact_p(
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
    Ok(TaggedValue::boolean(
        heap.borrow().is_inexact_number(args[0]),
    ))
}

pub(super) fn boolean_equal(
    _evaluator: &Evaluator,
    args: Vec<TaggedValue>,
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
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
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
    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();
    // R7RS: Continuations captured by call/cc satisfy procedure?
    let is_proc = heap_ref.is_procedure(args[0]) || heap_ref.is_continuation(args[0]);
    Ok(TaggedValue::boolean(is_proc))
}

// ===== TaggedValue predicates with fast paths =====

/// (null? obj) - Fast path using TaggedValue
pub(super) fn null_p(
    _evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(args[0].is_null()))
}

/// (pair? obj) - Check for pair (native or boxed)
pub(super) fn pair_p(
    _evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(args[0].is_pair()))
}

/// (boolean? obj) - Fast path using TaggedValue
pub(super) fn boolean_p(
    _evaluator: &Evaluator,
    args: Vec<TaggedValue>,
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
pub(super) fn char_p(
    _evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(args[0].is_char()))
}

/// (string? obj) - Check for string (native heap string)
pub(super) fn string_p(
    _evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(args[0].is_string()))
}

/// (vector? obj) - Check for vector (native heap vector)
pub(super) fn vector_p(
    _evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(args[0].is_vector()))
}

/// (integer? obj) - Fast path for fixnums
pub(super) fn integer_p(
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
    Ok(TaggedValue::boolean(heap.borrow().is_integer_r7rs(args[0])))
}

/// (exact-integer? obj) - Fast path for fixnums
pub(super) fn exact_integer_p(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // exact-integer? is true for exact numbers that are also integers
    // This is fixnum, BigInt, or Rational with denominator 1
    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();
    let is_exact_int = heap_ref.is_exact_number(args[0]) && heap_ref.is_integer_r7rs(args[0]);
    Ok(TaggedValue::boolean(is_exact_int))
}

/// (exact? obj) - Fast path for fixnums
pub(super) fn exact_p(
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
    Ok(TaggedValue::boolean(heap.borrow().is_exact_number(args[0])))
}

/// (number? obj) - Fast path for fixnums
pub(super) fn number_p(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
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
    let heap = evaluator.global_env.heap();
    Ok(TaggedValue::boolean(heap.borrow().is_number(args[0])))
}

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Numeric type predicates (with TaggedValue fast paths)
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "number?",
        Arity::Exact(1),
        "Returns #t if obj is a number.",
        |eval, args, _tail| number_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "complex?",
        Arity::Exact(1),
        "Returns #t if obj is a complex number.",
        |eval, args, _tail| complex_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "real?",
        Arity::Exact(1),
        "Returns #t if obj is a real number.",
        |eval, args, _tail| real_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "rational?",
        Arity::Exact(1),
        "Returns #t if obj is a rational number.",
        |eval, args, _tail| rational_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "integer?",
        Arity::Exact(1),
        "Returns #t if obj is an integer.",
        |eval, args, _tail| integer_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "exact-integer?",
        Arity::Exact(1),
        "Returns #t if obj is an exact integer.",
        |eval, args, _tail| exact_integer_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "exact?",
        Arity::Exact(1),
        "Returns #t if obj is an exact number.",
        |eval, args, _tail| exact_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "inexact?",
        Arity::Exact(1),
        "Returns #t if obj is an inexact number.",
        |eval, args, _tail| inexact_p(eval, args).map(EvalResult::Tagged),
    ));

    // Data type predicates (with TaggedValue fast paths)
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "boolean?",
        Arity::Exact(1),
        "Returns #t if obj is a boolean.",
        |eval, args, _tail| boolean_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "string?",
        Arity::Exact(1),
        "Returns #t if obj is a string.",
        |eval, args, _tail| string_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "symbol?",
        Arity::Exact(1),
        "Returns #t if obj is a symbol.",
        |eval, args, _tail| symbol_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "char?",
        Arity::Exact(1),
        "Returns #t if obj is a character.",
        |eval, args, _tail| char_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "vector?",
        Arity::Exact(1),
        "Returns #t if obj is a vector.",
        |eval, args, _tail| vector_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "procedure?",
        Arity::Exact(1),
        "Returns #t if obj is a procedure.",
        |eval, args, _tail| procedure_p(eval, args).map(EvalResult::Tagged),
    ));

    // List predicates (with TaggedValue fast paths)
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "null?",
        Arity::Exact(1),
        "Returns #t if obj is the empty list.",
        |eval, args, _tail| null_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "pair?",
        Arity::Exact(1),
        "Returns #t if obj is a pair.",
        |eval, args, _tail| pair_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "list?",
        Arity::Exact(1),
        "Returns #t if obj is a list.",
        |eval, args, _tail| list_p(eval, args).map(EvalResult::Tagged),
    ));

    // Boolean equality
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "boolean=?",
        Arity::Min(2),
        "Returns #t if all arguments are booleans and are all equal.",
        |eval, args, _tail| boolean_equal(eval, args).map(EvalResult::Tagged),
    ));
}
