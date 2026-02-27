//! Test framework primitives (chibi test primitives)
//!
//! These primitives are registered under (chibi test primitives).
//! The full (chibi test) library is in lib/chibi/test.sld which
//! imports these primitives and adds the test macro.

use super::super::Evaluator;
use super::super::error::EvalError;
use super::registry::PrimitiveRegistry;
use patina_core::TaggedValue;

/// Register all test framework primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    registry.register(PrimitiveFn::new_tagged(
        "chibi.test.primitives",
        "test-begin",
        Arity::Exact(1),
        "Start a test suite with the given name",
        |eval, args, _| test_begin(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "chibi.test.primitives",
        "test-end",
        Arity::Min(0),
        "End the current test suite and display results",
        |eval, args, _| test_end(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "chibi.test.primitives",
        "test-increment-passed",
        Arity::Exact(0),
        "Increment the passed test count (internal use)",
        |eval, args, _| test_increment_passed(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "chibi.test.primitives",
        "test-increment-failed",
        Arity::Exact(0),
        "Increment the failed test count (internal use)",
        |eval, args, _| test_increment_failed(eval, args).map(EvalResult::Tagged),
    ));
}

fn test_begin(evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
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

fn test_end(_evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
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
    _evaluator: &Evaluator,
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
    _evaluator: &Evaluator,
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
