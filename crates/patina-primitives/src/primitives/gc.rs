//! GC primitives (see `docs/GC_DESIGN.md`)
//!
//! `(gc)` collects at its call (#639). It cannot collect from Rust, where
//! the collector sees neither its frame nor its caller's, so it is a
//! resumable primitive that asks the machine for a full collection
//! ([`Step::Collect`]): the machine suspends the caller at the call's return
//! pc and collects before the caller's next instruction, in every GC mode,
//! `PATINA_GC=0` included. Where collection is deferred — a nested loop, a
//! library body loaded under a holder's guard — the collection is posted for
//! the next safe point that may collect instead, as every `(gc)` was before
//! #639, and counted in `deferred-collections`.
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
//!
//! Each limit beside its use (#663, GC_PRD §15). Each pair is a count and
//! the value at which it acts. A trigger the GC mode does not use reads
//! `#f`: `PATINA_GC=0` uses none, the default counts no allocations, and
//! `PATINA_GC_STRESS` and zeal count no bytes.
//!
//! - `allocs-since-gc` and `allocs-trigger`: allocations since the last
//!   collection, and the count that posts one (`PATINA_GC_STRESS=N`'s N,
//!   zeal's 0).
//! - `bytes-since-gc` and `bytes-trigger`: bytes charged since the last
//!   collection, and the count that posts one, `max(8 MiB, 2 × live-bytes)`
//!   as the last collection set it.
//! - `descriptors-since-gc` and `descriptors-trigger`: descriptor pressure's
//!   count, above, and its threshold, `min(128, RLIMIT_NOFILE / 4)`.
//! - `ports`: the heap's port objects' entries in its `PortTable` (#785),
//!   string ports included, and the standard ports' objects while they are
//!   reachable: a heap whose `(patina internal io)` was never built has
//!   none, and a top-level `(current-output-port p)` leaves the standard
//!   one to be collected. A dead port object's entry goes with the
//!   collection that finds it: a file port's with the drain after it,
//!   which closes the file (#785).
//! - `open-file-ports` and `descriptor-limit`: the file ports open now,
//!   whenever they opened, and the soft `RLIMIT_NOFILE` they count against,
//!   or `#f` where it is unlimited or cannot be read.
//! - `minors` and `majors`: collections by kind. Every collection is a major
//!   until a nursery comes (GC_PRD stage 7), so `minors` is 0 and `majors` is
//!   `collections`.
//!
//! Counts per owner: what one kind of holder keeps, each a count of things
//! rather than bytes, which the steady-state lane holds flat (its SS5).
//!
//! - `symbols`: the interned symbols. The intern table is a root, so a
//!   symbol, once interned, stays for the heap's life.
//! - `ports`: the port objects' entries in the `PortTable`, above.
//! - `namespaces`: the live namespaces, each of which charges its tables to
//!   `external-bytes`: the global environment, each loaded library's, each
//!   live environment specifier's, `%parameterize-swap!`'s compile
//!   namespace, and each live host namespace, the environment a host's
//!   `Interpreter::new_environment` answers (#835). A frame is not counted.
//! - `binding-records` (#834): the binding records the live namespaces hold,
//!   one for each slot, each scoped entry (the splicing `let-syntax`
//!   keywords filed at top level among them), each import alias, and each
//!   of a host namespace's read-through records (#835).
//! - `cells` (#834): the cells of their own the live namespaces' records name
//!   now. Not counted: a frame's imports, a free cell (an export with no
//!   binding behind it, a declaration's location, the one a host
//!   namespace's read-through records name), and a cell only an importer's
//!   record or an export keeps once its namespace dropped or re-pointed away
//!   from it.
//! - `code-units` (#836): the code units the VM has loaded and not released.
//!   A unit is what one compilation loaded, kept and let go whole: a form's
//!   top code object (a program's form, a library body form, a datum `eval`
//!   compiled) with the code of the lambdas nested in it. The VM lets a unit
//!   go when its form returns, unless a frame, a captured continuation or a
//!   live closure still holds its code; such a unit goes at the first
//!   collection after nothing does. The form that reads the count is
//!   running, so its own unit is counted. The tree-walker loads no code, and
//!   reads 0, never `#f`.
//!
//! `binding-records` and `cells` are running sums: raised as a namespace
//! makes a record or a cell, `cells` lowered as a record is re-pointed away
//! from its namespace's own cell, and both lowered by what a namespace holds
//! when it drops. So they are exact once the dead namespaces have dropped,
//! which a collection's sweep does for the specifiers it frees: the
//! steady-state driver reads them after `(gc) (gc)`. A record and a cell are
//! each a pair, so both are counted in `pairs` as well. `code-units` is the
//! size of the VM's table of units, written whenever the table changes. A
//! `(gc)` that collects lets go, before it returns, of every unit no live
//! closure, frame or continuation holds; but a loaded unit's constants are
//! roots, so a closure that only the constants of a unit the same
//! collection lets go reach (code `eval` compiled from a datum holding a
//! procedure) is marked by it, and keeps its own unit until the next
//! collection, one more for each such link. The steady-state driver reads
//! it after `(gc) (gc)` as well.

use crate::apply_context::ApplyContext;
use crate::registry::PrimitiveFn;
use crate::registry::PrimitiveRegistry;
use crate::registry::Step;
use patina_core::CollectKind;
use patina_core::{Cx, Value};
use patina_runtime::Arity;
use patina_runtime::EvalError;

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

    registry.register(PrimitiveFn::new_cx(
        "patina.debug",
        "gc-stats",
        Arity::Exact(0),
        "Return an alist of heap arena sizes, free-list lengths, GC counters, byte totals, pause and MMU figures, K16's high-water marks, and the process's resident size and CPU time.",
        gc_stats,
    ));
}

/// Ask the machine for a full collection at this call. Nothing to keep
/// across it.
fn gc<'gc>(
    _cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    _args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
    Ok(Step::Collect {
        kind: CollectKind::Major,
        state: Value::UNSPECIFIED,
    })
}

/// The collection has run, or been posted where collection is deferred:
/// `(gc)`'s value is unspecified either way.
fn gc_done<'gc>(
    _cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    _state: Value<'gc>,
    _collected: Value<'gc>,
) -> Result<Step<'gc>, EvalError> {
    Ok(Step::Done(Value::UNSPECIFIED))
}

fn gc_stats<'gc>(cx: &mut Cx<'gc>, _args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    let stats = cx.stats();
    let n = |count: usize| Value::fixnum(count as i64);
    // A limit the GC mode does not use is installed as `usize::MAX`.
    let limit = |at: usize| {
        if at == usize::MAX {
            Value::FALSE
        } else {
            n(at)
        }
    };
    let threshold = cx.gc_threshold();
    let entries = [
        ("pairs", n(stats.pairs)),
        ("vectors", n(stats.vectors)),
        ("strings", n(stats.strings)),
        ("objects", n(stats.objects)),
        ("symbols", n(stats.symbols)),
        ("free-pairs", n(stats.free_pairs)),
        ("free-vectors", n(stats.free_vectors)),
        ("free-strings", n(stats.free_strings)),
        ("free-objects", n(stats.free_objects)),
        ("allocs-since-gc", n(stats.allocs_since_gc)),
        ("allocs-trigger", limit(threshold.allocations)),
        ("collections", n(stats.gc_collections as usize)),
        // Every collection is a major until a nursery (GC_PRD stage 7).
        ("minors", n(0)),
        ("majors", n(stats.gc_collections as usize)),
        (
            "deferred-collections",
            n(stats.gc_deferred_collections as usize),
        ),
        ("descriptors-since-gc", n(stats.descriptors_since_gc)),
        ("descriptors-trigger", limit(threshold.descriptors)),
        (
            "descriptor-collections",
            n(stats.descriptor_collections as usize),
        ),
        ("last-swept", n(stats.gc_last_swept)),
        ("live-bytes", n(stats.live_bytes)),
        ("bytes-since-gc", n(cx.bytes_since_gc())),
        ("bytes-trigger", limit(threshold.bytes)),
        ("bytes-allocated", n(stats.bytes_allocated as usize)),
        ("bytes-reclaimed", n(stats.bytes_reclaimed as usize)),
        ("committed-bytes", n(stats.committed_bytes)),
        ("external-bytes", n(stats.external_bytes)),
        ("ports", n(cx.port_entries())),
        ("open-file-ports", n(cx.open_file_ports())),
        (
            "descriptor-limit",
            patina_core::heap::descriptor_limit().map_or(Value::FALSE, n),
        ),
        ("namespaces", n(cx.namespaces())),
        ("cells", n(cx.cells())),
        ("binding-records", n(cx.binding_records())),
        ("code-units", n(cx.code_units())),
    ];

    let mut alist: Vec<Value<'gc>> = entries
        .iter()
        .map(|&(name, value)| {
            let key = cx.intern_symbol(name);
            cx.alloc_pair(key, value)
        })
        .collect();

    // What the collections cost and how long they waited (#648): pauses in
    // microseconds, the work after each (the drain's and the backend's)
    // included; the minimum mutator utilisation at 10 ms; K16's two
    // high-water marks, each with the site of the deferral window that set
    // it, or #f; and the process's resident size and CPU time, or #f where
    // they cannot be read.
    let micros = |d: std::time::Duration| Value::fixnum(d.as_micros() as i64);
    let bytes = |n: u64| Value::fixnum(n as i64);
    let wait = cx.wait_high_water();
    let deferral = cx.deferral_high_water();
    let mmu = cx.mmu(10).unwrap_or(1.0);
    let mut values = vec![
        ("last-pause-us", micros(cx.last_pause())),
        ("pause-max-us", micros(cx.pause_max())),
        ("pause-total-us", micros(cx.pause_total())),
        ("mmu-10ms", cx.alloc_real(mmu)),
        ("wait-max-bytes", bytes(wait.bytes)),
        ("wait-max-us", micros(cx.wait_time_max())),
    ];
    for (name, site) in [("wait-site", wait.site), ("deferral-site", deferral.site)] {
        let value = match site {
            Some(site) => cx.alloc_string(site.to_string()),
            None => Value::FALSE,
        };
        values.push((name, value));
    }
    values.push(("deferral-max-bytes", bytes(deferral.bytes)));
    let resident = patina_core::heap::telemetry::resident_bytes();
    let cpu = patina_core::heap::telemetry::cpu_micros();
    values.push(("resident-bytes", resident.map_or(Value::FALSE, bytes)));
    values.push(("cpu-us", cpu.map_or(Value::FALSE, bytes)));
    for (name, value) in values {
        let key = cx.intern_symbol(name);
        alist.push(cx.alloc_pair(key, value));
    }
    Ok(cx.list_from_iter(alist))
}
