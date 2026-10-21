//! Multiple values primitives (R7RS Section 6.10)
//!
//! Implements:
//! - `values` - Return multiple values
//! - `call-with-values` - Call producer and consumer

use crate::apply_context::ApplyContext;
use patina_core::{Cx, Value};
use patina_runtime::EvalError;

pub(super) fn values<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    Ok(cx.values_from(args.to_vec()))
}

/// (call-with-values producer consumer)
/// Calls producer with no arguments, then calls consumer with the values produced.
///
/// Note: The CPS evaluator handles call-with-values for TCO. This registry
/// version is a fallback for non-CPS call sites.
pub(super) fn call_with_values<'gc>(
    cx: &mut Cx<'gc>,
    ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let producer = args[0];
    let consumer = args[1];

    // Call producer with no arguments to get values. Both machines intercept
    // `call-with-values` (the VM's `VM_INTERCEPTED_PRIMITIVES`, the
    // tree-walker's `apply_call_with_values`), so this registry fallback runs
    // only where neither does: the VM's unit tests (`install_primitives`), and
    // the tree-walker's detached `ApplyContext for Evaluator`.
    let produced = ctx.apply_in(cx, producer, &[])?;

    // Unpack multiple values if present, otherwise use single value
    let consumer_args: Vec<Value<'gc>> = match cx.get_values(produced) {
        Some(vals) => vals.to_vec(),
        None => vec![produced],
    };

    ctx.apply_in(cx, consumer, &consumer_args)
}

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // values - Return multiple values
    registry.register(PrimitiveFn::new_cx(
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
