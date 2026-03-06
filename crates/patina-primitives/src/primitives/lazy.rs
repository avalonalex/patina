//! Lazy evaluation primitives for (scheme lazy)
//!
//! Implements force, promise?, and make-promise.
//! The delay and delay-force macros are defined in lib/scheme/lazy-extras.scm

use crate::apply_context::ApplyContext;
use crate::registry::PrimitiveRegistry;
use patina_core::{TaggedValue, heap::PromiseState};
use patina_runtime::EvalError;
use std::cell::RefCell;
use std::rc::Rc;

/// Register all lazy evaluation primitives
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    registry.register(PrimitiveFn::new_higher_order(
        "scheme.lazy",
        "force",
        Arity::Exact(1),
        "Force evaluation of a promise. If the argument is not a promise, return it unchanged.",
        |ctx, args| force(ctx, args),
    ));

    registry.register(PrimitiveFn::new_higher_order(
        "scheme.lazy",
        "promise?",
        Arity::Exact(1),
        "Returns #t if obj is a promise, #f otherwise.",
        |ctx, args| promise_p(ctx, args),
    ));

    registry.register(PrimitiveFn::new_higher_order(
        "scheme.lazy",
        "make-promise",
        Arity::Exact(1),
        "Returns a promise which, when forced, will return obj. If obj is already a promise, it is returned.",
        |ctx, args| make_promise(ctx, args),
    ));

    // Internal helper for delay/delay-force macros
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.lazy",
        "%make-delayed-promise",
        Arity::Exact(1),
        "Internal: Create a delayed promise from a thunk. Used by delay macro.",
        |ctx, args| make_delayed_promise(ctx, args),
    ));
}

/// Force evaluation of a promise
///
/// If the argument is a promise:
/// 1. If already forced, return the cached value
/// 2. Otherwise, force the thunk and cache the result
///
/// If the argument is not a promise, return it unchanged.
fn force(ctx: &dyn ApplyContext, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    force_tagged(ctx, args[0])
}

/// Internal recursive forcing that works entirely with TaggedValue
fn force_tagged(ctx: &dyn ApplyContext, obj: TaggedValue) -> Result<TaggedValue, EvalError> {
    let heap = ctx.heap();

    // Check if it's a promise
    let promise_ref = match heap.borrow().get_promise(obj) {
        Some(p) => p,
        None => return Ok(obj), // Not a promise, return unchanged
    };

    let promise = promise_ref.borrow_mut();

    match *promise {
        PromiseState::Forced(value_tagged) => {
            // Already evaluated, return cached TaggedValue directly
            Ok(value_tagged)
        }
        PromiseState::Delayed(thunk_tagged) => {
            // Drop borrow before evaluating
            drop(promise);

            // Check if thunk is a procedure using heap
            let is_proc = heap.borrow().is_procedure(thunk_tagged);

            let result_tagged = if is_proc {
                // Call the thunk with no arguments — thunk is already TaggedValue
                ctx.apply_proc(thunk_tagged, vec![])?
            } else {
                // If it's not a procedure, just return it
                thunk_tagged
            };

            // If the result is a promise, force it recursively (delay-force pattern)
            let final_tagged = if heap.borrow().is_promise(result_tagged) {
                force_tagged(ctx, result_tagged)?
            } else {
                result_tagged
            };

            // Cache the result as TaggedValue
            *promise_ref.borrow_mut() = PromiseState::Forced(final_tagged);

            Ok(final_tagged)
        }
    }
}

/// Check if a value is a promise
fn promise_p(ctx: &dyn ApplyContext, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();
    Ok(TaggedValue::boolean(heap.borrow().is_promise(args[0])))
}

/// Create a promise from a value
///
/// If the value is already a promise, return it unchanged.
/// Otherwise, wrap it in a promise that's already forced.
fn make_promise(ctx: &dyn ApplyContext, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();

    // Check if already a promise using heap method
    if heap.borrow().is_promise(args[0]) {
        // Already a promise, return it unchanged
        return Ok(args[0]);
    }

    // Wrap in a forced promise natively on the heap
    Ok(heap
        .borrow_mut()
        .alloc_promise(Rc::new(RefCell::new(PromiseState::Forced(args[0])))))
}

/// Create a delayed promise from a thunk (internal helper for delay macro)
///
/// The thunk should be a zero-argument procedure that will be called when forced.
fn make_delayed_promise(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();

    // Create a delayed promise natively on the heap
    Ok(heap
        .borrow_mut()
        .alloc_promise(Rc::new(RefCell::new(PromiseState::Delayed(args[0])))))
}
