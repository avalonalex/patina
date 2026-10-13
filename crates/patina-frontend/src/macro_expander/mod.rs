//! Macro expander test utilities, and the crossing into the expander
//!
//! This module provides test helpers for macro expansion, and [`cross`],
//! how code that works in words calls the expander, which takes a context.
//! The actual macro system is in `patina-macros`.

mod interface;

// Re-export test helper
pub use interface::TestExpander;

use patina_core::{Cx, NoGcScope, SharedHeap, TaggedValue, Value};
use std::rc::Rc;

/// Run `f` in a context over `heap`, with `words` as values: how code that
/// works in words calls the macro expander, which takes a context (#704).
///
/// Until the desugarer takes a context itself, it is a host of the
/// expander, and its words cross as an embedder's values do (#605): each
/// through an `Owned` handle, which roots it, read back as a value inside the
/// context. No safe crate turns a word into a value (`Value::from_tagged` is
/// `unsafe`, #703). What `f` answers comes back as words, since a value
/// cannot leave its context. Nothing collects inside: the context is made
/// beneath a [`NoGcScope`], as the desugarer runs beneath its holder's.
#[track_caller]
pub(crate) fn cross<const N: usize, R>(
    heap: &SharedHeap,
    words: [TaggedValue; N],
    f: impl for<'gc> FnOnce(&mut Cx<'gc>, [Value<'gc>; N]) -> R,
) -> R {
    let handles = {
        let heap = heap.borrow();
        words.map(|word| heap.hold(word))
    };
    let instance = Rc::clone(heap.borrow().instance());
    let scope = NoGcScope::enter(heap);
    scope.mutate(&instance, |cx| {
        let values = handles.each_ref().map(|handle| cx.held(handle));
        f(cx, values)
    })
}

/// [`cross`] for a number of words known only at run time.
#[track_caller]
pub(crate) fn cross_all<R>(
    heap: &SharedHeap,
    words: &[TaggedValue],
    f: impl for<'gc> FnOnce(&mut Cx<'gc>, &[Value<'gc>]) -> R,
) -> R {
    let handles: Vec<_> = {
        let heap = heap.borrow();
        words.iter().map(|&word| heap.hold(word)).collect()
    };
    let instance = Rc::clone(heap.borrow().instance());
    let scope = NoGcScope::enter(heap);
    scope.mutate(&instance, |cx| {
        let values: Vec<Value<'_>> = handles.iter().map(|handle| cx.held(handle)).collect();
        f(cx, &values)
    })
}
