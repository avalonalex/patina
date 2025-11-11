//! Multiple values primitives (R7RS Section 6.10)
//!
//! Implements:
//! - `values` - Return multiple values
//! - `call-with-values` - Call producer and consumer (also a special form for tail calls)

use super::super::error::EvalError;
use super::super::Evaluator;
use patina_runtime::value::Value;

pub(super) fn values(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // values accepts any number of arguments (0 or more), so no arity check needed
    match args.len() {
        0 => Ok(Value::Unspecified),
        1 => Ok(args.into_iter().next().unwrap()),
        _ => Ok(Value::Values(args)),
    }
}

/// (call-with-values producer consumer)
/// Calls producer with no arguments, then calls consumer with the values produced
///
/// Per R7RS Section 6.10: "If producer returns multiple values, consumer is called
/// with those values as arguments. If producer returns a single value, consumer is
/// called with that value as its sole argument."
///
/// This primitive can now participate in tail call optimization when `in_tail_position` is true.
/// Per R7RS Section 3.5: "the second argument passed to call-with-values must be called via a tail call"
pub(super) fn call_with_values(
    evaluator: &Evaluator,
    args: Vec<Value>,
    in_tail_position: bool,
) -> Result<super::super::EvalResult, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let producer = &args[0];
    let consumer = &args[1];

    // Call producer with no arguments to get values
    // Producer is NOT in tail position
    let produced = match evaluator.apply(producer.clone(), vec![], false)? {
        super::super::EvalResult::Value(v) => v,
        _ => {
            return Err(EvalError::InternalError(
                "Unexpected tail call from producer in call-with-values".to_string(),
            ))
        }
    };

    // Unpack multiple values if present, otherwise use single value
    let consumer_args = match produced {
        Value::Values(vals) => vals,
        other => vec![other],
    };

    // Call consumer with the produced values
    // Consumer IS in tail position when call-with-values is
    if in_tail_position {
        // Return TailCallPrimitive to trampoline
        // The trampoline will re-apply this, maintaining the current environment
        Ok(super::super::EvalResult::TailCallPrimitive {
            proc: consumer.clone(),
            args: consumer_args,
        })
    } else {
        // Not in tail position - apply directly
        evaluator.apply(consumer.clone(), consumer_args, false)
    }
}
