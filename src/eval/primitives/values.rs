//! Multiple values primitives (R7RS Section 6.10)
//!
//! Implements:
//! - `values` - Return multiple values
//! - `call-with-values` - Receive multiple values
//!
//! INSTRUCTIONS: Move from primitives.rs:
//! - primitive_values()
//! - primitive_call_with_values()

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

pub(super) fn call_with_values(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "call-with-values")?;

    let producer = &args[0];
    let consumer = &args[1];

    let produced = evaluator.apply(producer.clone(), vec![])?;

    let consumer_args = match produced {
        Value::Values(vals) => vals,
        other => vec![other],
    };

    evaluator.apply(consumer.clone(), consumer_args)
}
