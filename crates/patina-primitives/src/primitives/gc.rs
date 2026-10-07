//! GC primitives (see `docs/GC_DESIGN.md`)
//!
//! `(gc)` collects at its call (#639). It cannot collect from Rust, where
//! the collector sees neither its frame nor its caller's, so it is a
//! resumable primitive that asks the machine for a full collection
//! ([`Step::Collect`]): the machine suspends the caller at the call's return
//! pc and collects before the caller's next instruction, in every GC mode,
//! `PATINA_GC=0` included. Where collection is deferred — a nested loop, a
//! library body being loaded — the collection is posted for the next safe
//! point that may collect instead, as every `(gc)` was before #639, and
//! counted in `deferred-collections`.
//!
//! `(gc-stats)` reports arena and collector counters, and the byte account
//! (`patina_core::heap` `account.rs`, #606):
//!
//! - `live-bytes`: what the last collection found live — the slots of the
//!   objects it marked, their payloads (a VM continuation's snapshot among
//!   them) and the external bytes held then. 0 before the first collection.
//!   The adaptive trigger collects again after `max(8 MiB, 2 × live-bytes)`.
//! - `bytes-allocated`: every byte charged since the heap was made: each
//!   object's slot and payload, and the external bytes charged, a
//!   namespace's tables and their growth among them. External bytes are
//!   given back when their holder drops, outside `bytes-reclaimed`, so
//!   `bytes-allocated` less `bytes-reclaimed` is not what is held now: it
//!   also counts every namespace that has died.
//! - `bytes-reclaimed`: every byte the collections have freed. It grows only
//!   when a collection frees something, so a reclamation proof that sees it
//!   above 0 cannot have passed without collecting.
//! - `committed-bytes`: what the arenas hold now, live or not: every arena's
//!   capacity in slots and the payloads of the occupied slots (a tree-walker
//!   closure's with an estimate of its frame, below).
//! - `external-bytes`: what is held outside the arenas on heap objects'
//!   behalf now (GC_PRD §15), charged by its holders: today the tables of
//!   the live namespaces — the global environment, the libraries', and those
//!   `environment` and the R5RS constructors build (#615) — and the buffers
//!   of the open file ports, 8 KiB each (#607). GC_PRD's footprint is
//!   `committed-bytes` plus `external-bytes`.
//!
//! For a tree-walker closure, the payload in the first four keys includes an
//! estimate of the frame it captures (#637), not a measurement: counted once
//! per closure, so a frame several closures share is counted for each, and a
//! closure that captures a namespace rather than a frame is counted one it
//! does not hold.
//!
//! The slot counts before them (`pairs`, `free-pairs`, `allocs-since-gc`,
//! `last-swept` and the rest) stay, as diagnostics of the arenas.
//!
//! `deferred-collections` counts the collections asked for at a call where
//! collection was deferred, and posted instead (`Heap::defer_collection`,
//! #639): the windows that cannot collect, kept visible. GC_PRD's K16
//! counters, which measure those windows, are its eventual home.
//!
//! Descriptor pressure (#607, `patina_core::heap` `account.rs`):
//!
//! - `descriptors-since-gc`: the file ports opened since the last collection
//!   and not closed since. A close takes a port off only if it was opened
//!   since, and a collection starts the count again.
//! - `descriptor-collections`: the collections descriptor pressure posted,
//!   each time that count reached `min(128, RLIMIT_NOFILE / 4)` with no
//!   collection pending already. Never under `PATINA_GC=0`. An open that
//!   ran out of descriptors and collected at its call is not counted here:
//!   that collection is in `collections`, or in `deferred-collections`.

use crate::apply_context::ApplyContext;
use crate::registry::PrimitiveFn;
use crate::registry::PrimitiveRegistry;
use crate::registry::Step;
use patina_core::CollectKind;
use patina_core::TaggedValue;
use patina_runtime::Arity;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

// Both are registered with Arity::Exact(0); the registry checks arity before
// dispatch, so the handlers don't re-check.

/// Register GC primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_resumable(
        "patina.debug",
        "gc",
        Arity::Exact(0),
        "Collect garbage at this call, before the caller's next instruction; where collection \
         is deferred, at the next safe point that may collect.",
        gc,
        gc_done,
    ));

    registry.register(PrimitiveFn::new_heap(
        "patina.debug",
        "gc-stats",
        Arity::Exact(0),
        "Return an alist of heap arena sizes, free-list lengths, GC counters, byte totals, pause and MMU figures, K16's high-water marks, and the process's resident size and CPU time.",
        gc_stats,
    ));
}

/// Ask the machine for a full collection at this call. Nothing to keep
/// across it.
fn gc(_ctx: &dyn ApplyContext, _args: &[TaggedValue]) -> Result<Step, EvalError> {
    Ok(Step::Collect {
        kind: CollectKind::Major,
        state: TaggedValue::UNSPECIFIED,
    })
}

/// The collection has run, or been posted where collection is deferred:
/// `(gc)`'s value is unspecified either way.
fn gc_done(
    _ctx: &dyn ApplyContext,
    _state: TaggedValue,
    _collected: TaggedValue,
) -> Result<Step, EvalError> {
    Ok(Step::Done(TaggedValue::UNSPECIFIED))
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
        (
            "deferred-collections",
            stats.gc_deferred_collections as usize,
        ),
        ("descriptors-since-gc", stats.descriptors_since_gc),
        (
            "descriptor-collections",
            stats.descriptor_collections as usize,
        ),
        ("last-swept", stats.gc_last_swept),
        ("live-bytes", stats.live_bytes),
        ("bytes-allocated", stats.bytes_allocated as usize),
        ("bytes-reclaimed", stats.bytes_reclaimed as usize),
        ("committed-bytes", stats.committed_bytes),
        ("external-bytes", stats.external_bytes),
    ];

    let mut alist: Vec<TaggedValue> = entries
        .iter()
        .map(|(name, count)| {
            let key = h.intern_symbol(name);
            h.alloc_pair(key, TaggedValue::fixnum(*count as i64))
        })
        .collect();

    // What the collections cost and how long they waited (#648): pauses in
    // microseconds, the backend's work after each included; the minimum
    // mutator utilisation at 10 ms; K16's two high-water marks, each with
    // the site of the deferral window that set it, or #f; and the process's
    // resident size and CPU time, or #f where they cannot be read.
    let micros = |d: std::time::Duration| TaggedValue::fixnum(d.as_micros() as i64);
    let bytes = |n: u64| TaggedValue::fixnum(n as i64);
    let wait = h.wait_high_water();
    let deferral = h.deferral_high_water();
    let mmu = h.mmu(10).unwrap_or(1.0);
    let mut values = vec![
        ("last-pause-us", micros(h.last_pause())),
        ("pause-max-us", micros(h.pause_max())),
        ("pause-total-us", micros(h.pause_total())),
        ("mmu-10ms", h.alloc_real(mmu)),
        ("wait-max-bytes", bytes(wait.bytes)),
        ("wait-max-us", micros(h.wait_time_max())),
    ];
    for (name, site) in [("wait-site", wait.site), ("deferral-site", deferral.site)] {
        let value = match site {
            Some(site) => h.alloc_string(site.to_string()),
            None => TaggedValue::FALSE,
        };
        values.push((name, value));
    }
    values.push(("deferral-max-bytes", bytes(deferral.bytes)));
    let resident = patina_core::heap::telemetry::resident_bytes();
    let cpu = patina_core::heap::telemetry::cpu_micros();
    values.push(("resident-bytes", resident.map_or(TaggedValue::FALSE, bytes)));
    values.push(("cpu-us", cpu.map_or(TaggedValue::FALSE, bytes)));
    for (name, value) in values {
        let key = h.intern_symbol(name);
        alist.push(h.alloc_pair(key, value));
    }
    Ok(h.list_from_iter(alist))
}
