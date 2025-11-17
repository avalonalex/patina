//! Lazy evaluation primitives for (scheme lazy)
//!
//! Implements force, promise?, and make-promise.
//! The delay and delay-force macros are defined in lib/scheme/lazy-extras.scm

use super::super::Evaluator;
use super::super::error::EvalError;
use super::registry::PrimitiveRegistry;
use patina_runtime::value::{PromiseState, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Register all lazy evaluation primitives
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::value::Arity;

    registry.register(PrimitiveFn::new(
        "scheme.lazy",
        "force",
        Arity::Exact(1),
        "Force evaluation of a promise. If the argument is not a promise, return it unchanged.",
        |eval, args, _| force(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.lazy",
        "promise?",
        Arity::Exact(1),
        "Returns #t if obj is a promise, #f otherwise.",
        |_eval, args, _| promise_p(args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.lazy",
        "make-promise",
        Arity::Exact(1),
        "Returns a promise which, when forced, will return obj. If obj is already a promise, it is returned.",
        |_eval, args, _| make_promise(args).map(EvalResult::Value),
    ));

    // Internal helper for delay/delay-force macros
    registry.register(PrimitiveFn::new(
        "scheme.lazy",
        "%make-delayed-promise",
        Arity::Exact(1),
        "Internal: Create a delayed promise from a thunk. Used by delay macro.",
        |_eval, args, _| make_delayed_promise(args).map(EvalResult::Value),
    ));
}

/// Force evaluation of a promise
///
/// If the argument is a promise:
/// 1. If already forced, return the cached value
/// 2. Otherwise, force the thunk and cache the result
///
/// If the argument is not a promise, return it unchanged.
fn force(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "force")?;

    let obj = &args[0];

    match obj {
        Value::Promise(promise_ref) => {
            let promise = promise_ref.borrow_mut();

            match &*promise {
                PromiseState::Forced(value) => {
                    // Already evaluated, return cached value
                    Ok(value.clone())
                }
                PromiseState::Delayed(thunk) => {
                    // Evaluate the thunk
                    let thunk = thunk.clone(); // Clone before evaluating to avoid borrow issues
                    drop(promise); // Drop the borrow before evaluating

                    // Apply the thunk (it's a zero-argument procedure)
                    let result = match &thunk {
                        Value::Procedure(_) => {
                            // Call the thunk with no arguments
                            let eval_result = evaluator.apply(thunk.clone(), vec![], false)?;
                            match eval_result {
                                super::super::EvalResult::Value(v) => v,
                                _ => {
                                    return Err(EvalError::InternalError(
                                        "Unexpected tail call from promise thunk".to_string(),
                                    ));
                                }
                            }
                        }
                        _ => {
                            // If it's not a procedure, just return it
                            // This handles make-promise case
                            thunk
                        }
                    };

                    // If the result is a promise, force it recursively (delay-force pattern)
                    let final_result = match &result {
                        Value::Promise(_) => force(evaluator, vec![result])?,
                        _ => result,
                    };

                    // Cache the result
                    *promise_ref.borrow_mut() = PromiseState::Forced(final_result.clone());

                    Ok(final_result)
                }
            }
        }
        _ => {
            // Not a promise, return unchanged
            Ok(obj.clone())
        }
    }
}

/// Check if a value is a promise
fn promise_p(args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    Ok(Value::Boolean(matches!(args[0], Value::Promise(_))))
}

/// Create a promise from a value
///
/// If the value is already a promise, return it unchanged.
/// Otherwise, wrap it in a promise that's already forced.
fn make_promise(args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let obj = &args[0];

    match obj {
        Value::Promise(_) => {
            // Already a promise, return it
            Ok(obj.clone())
        }
        _ => {
            // Wrap in a forced promise
            Ok(Value::Promise(Rc::new(RefCell::new(PromiseState::Forced(
                obj.clone(),
            )))))
        }
    }
}

/// Create a delayed promise from a thunk (internal helper for delay macro)
///
/// The thunk should be a zero-argument procedure that will be called when forced.
fn make_delayed_promise(args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let thunk = &args[0];

    // Create a delayed promise (not yet evaluated)
    Ok(Value::Promise(Rc::new(RefCell::new(
        PromiseState::Delayed(thunk.clone()),
    ))))
}
