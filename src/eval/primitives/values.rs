//! Multiple values primitives (R7RS Section 6.10)
//!
//! Implements:
//! - `values` - Return multiple values
//! - `call-with-values` - Call producer and consumer (also a special form for tail calls)

use super::super::error::EvalError;
use super::super::Evaluator;
use crate::value::Value;

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
/// NOTE: This primitive implementation exists primarily to ensure call-with-values
/// is defined in the environment (so macros can reference it). In practice, the special
/// form handler in eval/mod.rs intercepts most call-with-values expressions to provide
/// proper tail call optimization per R7RS Section 3.5. This primitive is only called
/// in rare edge cases where call-with-values is used as a first-class value.
pub(super) fn call_with_values(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let producer = &args[0];
    let consumer = &args[1];

    // Call producer with no arguments to get values
    let produced = evaluator.apply(producer.clone(), vec![])?;

    // Unpack multiple values if present, otherwise use single value
    let consumer_args = match produced {
        Value::Values(vals) => vals,
        other => vec![other],
    };

    // Call consumer with the produced values
    // Note: This is a direct apply() call, not a tail call
    evaluator.apply(consumer.clone(), consumer_args)
}
