//! Test framework primitives (chibi test primitives)
//!
//! These primitives are registered under (chibi test primitives).
//! The full (chibi test) library is in lib/chibi/test.sld which
//! imports these primitives and adds the test macro.

use super::super::Evaluator;
use super::super::error::EvalError;
use super::registry::PrimitiveRegistry;
use patina_runtime::value::Value;

/// Register all test framework primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::value::Arity;

    registry.register(PrimitiveFn::new(
        "chibi.test.primitives",
        "test-begin",
        Arity::Exact(1),
        "Start a test suite with the given name",
        |eval, args, _| test_begin(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "chibi.test.primitives",
        "test-end",
        Arity::Min(0),
        "End the current test suite and display results",
        |eval, args, _| test_end(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "chibi.test.primitives",
        "test-increment-passed",
        Arity::Exact(0),
        "Increment the passed test count (internal use)",
        |eval, args, _| test_increment_passed(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "chibi.test.primitives",
        "test-increment-failed",
        Arity::Exact(0),
        "Increment the failed test count (internal use)",
        |eval, args, _| test_increment_failed(eval, args).map(EvalResult::Value),
    ));
}

fn test_begin(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "test-begin")?;

    let name = match &args[0] {
        Value::String(s) => s.borrow().iter().collect::<String>(),
        Value::Symbol(s) => s.to_string(),
        _ => {
            return Err(EvalError::TypeError(
                "test-begin expects string or symbol".to_string(),
            ));
        }
    };

    patina_runtime::stdlib::test_begin(&name);
    Ok(Value::Unspecified)
}

fn test_end(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 0, "test-end")?;
    patina_runtime::stdlib::test_end();
    Ok(Value::Unspecified)
}

fn test_increment_passed(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 0, "test-increment-passed")?;
    patina_runtime::stdlib::test_increment_passed();
    Ok(Value::Unspecified)
}

fn test_increment_failed(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 0, "test-increment-failed")?;
    patina_runtime::stdlib::test_increment_failed();
    Ok(Value::Unspecified)
}
