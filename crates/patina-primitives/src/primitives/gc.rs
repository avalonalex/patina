//! GC primitives (see `docs/GC_DESIGN.md`)
//!
//! `(gc)` cannot collect in place: a primitive runs mid-evaluation, where
//! live values sit in Rust locals no root provider can see. It records a
//! request that backends honor at their next safe point (stages 2–3 of the
//! GC plan). `(gc-stats)` reports arena and collector counters.

use crate::registry::PrimitiveFn;
use crate::registry::PrimitiveRegistry;
use patina_core::TaggedValue;
use patina_runtime::Arity;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

// Both handlers are registered with Arity::Exact(0); the registry checks
// arity before dispatch, so the handlers don't re-check.

/// Register GC primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_heap(
        "patina.debug",
        "gc",
        Arity::Exact(0),
        "Request a garbage collection at the next safe point.",
        gc,
    ));

    registry.register(PrimitiveFn::new_heap(
        "patina.debug",
        "gc-stats",
        Arity::Exact(0),
        "Return an alist of heap arena sizes, free-list lengths, and GC counters.",
        gc_stats,
    ));
}

fn gc(heap: &SharedHeap, _args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    heap.borrow_mut().request_gc();
    Ok(TaggedValue::UNSPECIFIED)
}

fn gc_stats(heap: &SharedHeap, _args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    let mut h = heap.borrow_mut();
    let stats = h.stats();
    let entries = [
        ("pairs", stats.pairs),
        ("vectors", stats.vectors),
        ("strings", stats.strings),
        ("objects", stats.objects),
        ("symbols", stats.symbols),
        ("free-pairs", stats.free_pairs),
        ("free-vectors", stats.free_vectors),
        ("free-strings", stats.free_strings),
        ("free-objects", stats.free_objects),
        ("allocs-since-gc", stats.allocs_since_gc),
        ("collections", stats.gc_collections as usize),
        ("last-swept", stats.gc_last_swept),
    ];

    let alist: Vec<TaggedValue> = entries
        .iter()
        .map(|(name, count)| {
            let key = h.intern_symbol(name);
            h.alloc_pair(key, TaggedValue::fixnum(*count as i64))
        })
        .collect();
    Ok(h.list_from_iter(alist))
}
