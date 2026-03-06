//! Multiple values primitives (R7RS Section 6.10)
//!
//! Implements:
//! - `values` - Return multiple values
//! - `call-with-values` - Call producer and consumer

use crate::apply_context::ApplyContext;
use patina_core::TaggedValue;
use patina_runtime::{EvalError, SharedHeap};

pub(super) fn values(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    match args.len() {
        1 => Ok(args[0]),
        0 => Ok(TaggedValue::UNSPECIFIED),
        _ => Ok(heap.borrow_mut().alloc_values(args)),
    }
}

/// (call-with-values producer consumer)
/// Calls producer with no arguments, then calls consumer with the values produced.
///
/// Note: The CPS evaluator handles call-with-values for TCO. This registry
/// version is a fallback for non-CPS call sites.
pub(super) fn call_with_values(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let producer = args[0];
    let consumer = args[1];

    // Call producer with no arguments to get values
    let produced_tv = ctx.apply_proc(producer, vec![])?;

    // Unpack multiple values if present, otherwise use single value
    let heap = ctx.heap();
    let consumer_args: Vec<TaggedValue> = {
        let heap_ref = heap.borrow();
        if let Some(vals) = heap_ref.get_values(produced_tv) {
            vals.to_vec()
        } else {
            vec![produced_tv]
        }
    };

    ctx.apply_proc(consumer, consumer_args)
}

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // values - Return multiple values
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "values",
        Arity::Min(0),
        "Returns all of its arguments as multiple values.",
        values,
    ));

    // call-with-values - Call producer and consumer
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "call-with-values",
        Arity::Exact(2),
        "Calls producer with no arguments, then calls consumer with the values produced.",
        call_with_values,
    ));
}
