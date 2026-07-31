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

fn gc(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut().request_gc();
    Ok(TaggedValue::UNSPECIFIED)
}

fn gc_stats(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }

    let mut h = heap.borrow_mut();
    let stats = h.stats();
    let entries: [(&str, usize); 10] = [
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
    ];

    let mut list = TaggedValue::NULL;
    for (name, count) in entries.iter().rev() {
        let key = h.intern_symbol(name);
        let entry = h.alloc_pair(key, TaggedValue::fixnum(*count as i64));
        list = h.alloc_pair(entry, list);
    }
    Ok(list)
}
