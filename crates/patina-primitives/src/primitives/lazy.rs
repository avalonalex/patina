//! Lazy evaluation primitives for (scheme lazy)
//!
//! Implements force, promise?, and make-promise.
//! The delay and delay-force macros are defined in lib/scheme/lazy-extras.scm

use crate::apply_context::ApplyContext;
use crate::registry::PrimitiveRegistry;
use patina_core::{TaggedValue, heap::PromiseState};
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;
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
        force,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.lazy",
        "promise?",
        Arity::Exact(1),
        "Returns #t if obj is a promise, #f otherwise.",
        promise_p,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.lazy",
        "make-promise",
        Arity::Exact(1),
        "Returns a promise which, when forced, will return obj. If obj is already a promise, it is returned.",
        make_promise,
    ));

    // Internal helper for delay/delay-force macros
    registry.register(PrimitiveFn::new_heap(
        "scheme.lazy",
        "%make-delayed-promise",
        Arity::Exact(1),
        "Internal: Create a delayed promise from a thunk. Used by delay macro.",
        make_delayed_promise,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.lazy",
        "%make-forced-promise",
        Arity::Exact(1),
        "Internal: a promise already holding obj — even when obj is itself a promise, \
         unlike make-promise. What (delay e) wraps its value in (R7RS 7.3).",
        make_forced_promise,
    ));
}

/// `(%make-forced-promise obj)` — R7RS 7.3's `(make-promise #t obj)`: a done
/// promise holding `obj`, wrapping a promise rather than returning it. That
/// is what makes `(force (delay p))` yield the promise `p` itself instead of
/// forcing through it, which `delay-force` is for.
fn make_forced_promise(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let state = Rc::new(RefCell::new(PromiseState::Forced(args[0])));
    Ok(heap.borrow_mut().alloc_promise(state))
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

/// Force a promise — R7RS 7.3's reference algorithm, iteratively.
///
/// A `delay-force` thunk yields another promise. Rather than force that one
/// recursively (a Rust frame per link, so a chain of a hundred thousand —
/// the whole reason `delay-force` exists — overflowed the stack), the outer
/// promise takes over the inner's state and the inner is re-pointed at the
/// outer's box (`Heap::promise_update`), and the loop goes round again.
/// Either promise forced later sees the one memoized value; SRFI 45's leak
/// tests hold because nothing accumulates per link.
///
/// The promise's box is looked up afresh on every turn, by object. A thunk
/// may force its own promise re-entrantly, and a nested force can re-point
/// this promise at a different box while the thunk runs; a box captured
/// before the thunk would then be an orphan, and the value stored into it
/// never memoized. If the promise is already done when the thunk returns,
/// that value wins and the thunk's result is dropped (R7RS: "unless
/// (promise-done? promise)").
fn force_tagged(ctx: &dyn ApplyContext, obj: TaggedValue) -> Result<TaggedValue, EvalError> {
    let heap = ctx.heap();
    loop {
        let cell = match heap.borrow().get_promise(obj) {
            Some(p) => p,
            None => return Ok(obj), // Not a promise, return unchanged
        };
        let thunk = match *cell.borrow() {
            PromiseState::Forced(value) => return Ok(value),
            PromiseState::Delayed(thunk) => thunk,
        };
        drop(cell);

        let result = ctx.apply_proc(thunk, vec![])?;

        let cell = heap.borrow().get_promise(obj).expect("still a promise");
        if let PromiseState::Forced(value) = *cell.borrow() {
            return Ok(value);
        }
        if heap.borrow().is_promise(result) {
            heap.borrow_mut().promise_update(obj, result);
        } else {
            *cell.borrow_mut() = PromiseState::Forced(result);
            return Ok(result);
        }
    }
}

/// Check if a value is a promise
fn promise_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    Ok(TaggedValue::boolean(heap.borrow().is_promise(args[0])))
}

/// Create a promise from a value
///
/// If the value is already a promise, return it unchanged.
/// Otherwise, wrap it in a promise that's already forced.
fn make_promise(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

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
fn make_delayed_promise(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // Create a delayed promise natively on the heap
    Ok(heap
        .borrow_mut()
        .alloc_promise(Rc::new(RefCell::new(PromiseState::Delayed(args[0])))))
}
