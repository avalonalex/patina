//! Tree-walker-specific debug primitives
//!
//! Registers debug-related primitives that can work without access to the
//! Evaluator's internal DebugConfig (which is tree-walker-specific state).
//!
//! Primitives that require DebugConfig (debug-enable, debug-disable, etc.) are
//! not registered here; they will be added once debug config is made accessible
//! through ApplyContext or a thread-local.

use patina_core::TaggedValue;
use patina_primitives::{PrimitiveFn, PrimitiveRegistry};
use patina_runtime::{Arity, EvalError, SharedHeap};

/// Register tree-walker debug primitives into the shared registry.
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_heap(
        "patina.debug",
        "macro-debug-mode",
        Arity::Exact(1),
        "Control macro expansion and hygiene debugging ('on, 'off, 'status)",
        macro_debug_mode,
    ));

    registry.register(PrimitiveFn::new_heap(
        "patina.debug",
        "library?",
        Arity::Exact(1),
        "Returns #t if obj is a library.",
        library_p,
    ));
}

fn library_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(heap.borrow().is_library(args[0])))
}

fn macro_debug_mode(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let mode = heap
        .borrow()
        .get_symbol_name(args[0])
        .map(|s| s.to_string());

    match mode.as_deref() {
        Some("on") => {
            patina_runtime::macro_debug::enable();
            Ok(heap.borrow_mut().intern_symbol("macro-debug-enabled"))
        }
        Some("off") => {
            patina_runtime::macro_debug::disable();
            Ok(heap.borrow_mut().intern_symbol("macro-debug-disabled"))
        }
        Some("status") => {
            let status = if patina_runtime::macro_debug::is_enabled() {
                "enabled"
            } else {
                "disabled"
            };
            Ok(heap.borrow_mut().intern_symbol(status))
        }
        _ => Err(EvalError::InvalidSyntax(
            "macro-debug-mode expects 'on, 'off, or 'status".to_string(),
        )),
    }
}
