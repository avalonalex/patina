//! GC primitives (see `docs/GC_DESIGN.md`)
//!
//! `(gc)` cannot collect in place: a primitive runs mid-evaluation, where
//! live values sit in Rust locals no root provider can see. It records a
//! request that backends honor at their next safe point (stages 2–3 of the
//! GC plan). `(gc-stats)` reports arena and collector counters, and the
//! byte account (`patina_core::heap` `account.rs`, #606):
//!
//! - `live-bytes`: what the last collection found live — the slots of the
//!   objects it marked, their payloads (a VM continuation's snapshot among
//!   them) and the external bytes held then. 0 before the first collection.
//!   The adaptive trigger collects again after `max(8 MiB, 2 × live-bytes)`.
//! - `bytes-allocated`: every byte charged since the heap was made, each
//!   object's slot and payload.
//! - `bytes-reclaimed`: every byte the collections have freed. It grows only
//!   when a collection frees something, so a reclamation proof that sees it
//!   above 0 cannot have passed without collecting.
//! - `committed-bytes`: what the heap holds now, live or not: every arena's
//!   capacity in slots, the payloads of the occupied slots, and the external
//!   bytes.
//!
//! The slot counts before them (`pairs`, `free-pairs`, `allocs-since-gc`,
//! `last-swept` and the rest) stay, as diagnostics of the arenas.

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
        "Return an alist of heap arena sizes, free-list lengths, GC counters and byte totals.",
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
        ("live-bytes", stats.live_bytes),
        ("bytes-allocated", stats.bytes_allocated as usize),
        ("bytes-reclaimed", stats.bytes_reclaimed as usize),
        ("committed-bytes", stats.committed_bytes),
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
