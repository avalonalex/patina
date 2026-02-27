//! Multiple values primitives (R7RS Section 6.10)
//!
//! Implements:
//! - `values` - Return multiple values
//! - `call-with-values` - Call producer and consumer (also a special form for tail calls)

use super::super::Evaluator;
use super::super::error::EvalError;
use patina_core::TaggedValue;

pub(super) fn values(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    match args.len() {
        1 => Ok(args[0]),
        0 => Ok(TaggedValue::UNSPECIFIED),
        _ => Ok(evaluator.global_env.heap().borrow_mut().alloc_values(args)),
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
    args: Vec<TaggedValue>,
    in_tail_position: bool,
) -> Result<super::super::EvalResult, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let producer = args[0];
    let consumer = args[1];

    // Call producer with no arguments to get values
    // Producer is NOT in tail position
    let produced_tv = match evaluator.apply(producer, vec![], false)? {
        super::super::EvalResult::Tagged(tv) => tv,
        super::super::EvalResult::TailCallPrimitive { .. } => {
            return Err(EvalError::InternalError(
                "Unexpected tail call from producer in call-with-values".to_string(),
            ));
        }
    };

    // Unpack multiple values if present, otherwise use single value
    let heap = evaluator.global_env.heap();
    let consumer_args: Vec<TaggedValue> = {
        let heap_ref = heap.borrow();
        if let Some(vals) = heap_ref.get_values(produced_tv) {
            vals.to_vec()
        } else {
            vec![produced_tv]
        }
    };

    // Call consumer with the produced values
    // Consumer IS in tail position when call-with-values is
    if in_tail_position {
        // Return TailCallPrimitive to trampoline
        Ok(super::super::EvalResult::TailCallPrimitive {
            proc: consumer,
            args: consumer_args,
        })
    } else {
        // Not in tail position - apply directly
        evaluator.apply(consumer, consumer_args, false)
    }
}

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // values - Return multiple values
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "values",
        Arity::Min(0),
        "Returns all of its arguments as multiple values.",
        |eval, args, _tail| values(eval, args).map(EvalResult::Tagged),
    ));

    // call-with-values - Call producer and consumer (supports TCO)
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "call-with-values",
        Arity::Exact(2),
        "Calls producer with no arguments, then calls consumer with the values produced.",
        call_with_values,
    ));
}
