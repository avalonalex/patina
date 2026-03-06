//! Test framework primitives (chibi test primitives)
//!
//! These primitives are registered under (chibi test primitives).
//! The full (chibi test) library is in lib/chibi/test.sld which
//! imports these primitives and adds the test macro.
use crate::registry::PrimitiveRegistry;
use patina_core::TaggedValue;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

/// Register all test framework primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    registry.register(PrimitiveFn::new_heap(
        "chibi.test.primitives",
        "test-begin",
        Arity::Exact(1),
        "Start a test suite with the given name",
        test_begin,
    ));

    registry.register(PrimitiveFn::new_heap(
        "chibi.test.primitives",
        "test-end",
        Arity::Min(0),
        "End the current test suite and display results",
        test_end,
    ));

    registry.register(PrimitiveFn::new_heap(
        "chibi.test.primitives",
        "test-increment-passed",
        Arity::Exact(0),
        "Increment the passed test count (internal use)",
        test_increment_passed,
    ));

    registry.register(PrimitiveFn::new_heap(
        "chibi.test.primitives",
        "test-increment-failed",
        Arity::Exact(0),
        "Increment the failed test count (internal use)",
        test_increment_failed,
    ));
}

fn test_begin(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap_ref = heap.borrow();

    let name = if let Some(s) = heap_ref.get_string_contents(args[0]) {
        s
    } else if let Some(s) = heap_ref.get_symbol_or_identifier_name(args[0]) {
        s.to_string()
    } else {
        return Err(EvalError::TypeError(
            "test-begin expects string or symbol".to_string(),
        ));
    };

    patina_runtime::stdlib::test_begin(&name);
    Ok(TaggedValue::UNSPECIFIED)
}

fn test_end(_heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }
    patina_runtime::stdlib::test_end();
    Ok(TaggedValue::UNSPECIFIED)
}

fn test_increment_passed(
    _heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }
    patina_runtime::stdlib::test_increment_passed();
    Ok(TaggedValue::UNSPECIFIED)
}

fn test_increment_failed(
    _heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }
    patina_runtime::stdlib::test_increment_failed();
    Ok(TaggedValue::UNSPECIFIED)
}
