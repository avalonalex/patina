//! `(patina debug)`'s primitives that are not the collector's: macro
//! debugging's switch, and the library predicate.
//!
//! Both take only the context and `patina_runtime::macro_debug`'s switch, so
//! they are registered here for both backends (#742): the tree-walker
//! registered them alone, and the VM, which has only these shared ones, had
//! neither.

use crate::registry::{PrimitiveFn, PrimitiveRegistry};
use patina_core::{Arity, Cx, Value};
use patina_runtime::EvalError;

/// Register `(patina debug)`'s `macro-debug-mode` and `library?`.
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_cx(
        "patina.debug",
        "macro-debug-mode",
        Arity::Exact(1),
        "Control macro expansion and hygiene debugging ('on, 'off, 'status)",
        macro_debug_mode,
    ));

    registry.register(PrimitiveFn::new_cx(
        "patina.debug",
        "library?",
        Arity::Exact(1),
        "Returns #t if obj is a library.",
        library_p,
    ));
}

fn library_p<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::boolean(cx.is_library(args[0])))
}

fn macro_debug_mode<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let mode = cx.get_symbol_name(args[0]).map(|s| s.to_string());

    match mode.as_deref() {
        Some("on") => {
            patina_runtime::macro_debug::enable();
            Ok(cx.intern_symbol("macro-debug-enabled"))
        }
        Some("off") => {
            patina_runtime::macro_debug::disable();
            Ok(cx.intern_symbol("macro-debug-disabled"))
        }
        Some("status") => {
            let status = if patina_runtime::macro_debug::is_enabled() {
                "enabled"
            } else {
                "disabled"
            };
            Ok(cx.intern_symbol(status))
        }
        _ => Err(EvalError::InvalidSyntax(
            "macro-debug-mode expects 'on, 'off, or 'status".to_string(),
        )),
    }
}
