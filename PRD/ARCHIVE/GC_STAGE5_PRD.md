# GC Stage 5+: Pause Work and Collector Upgrades

> **Superseded 2026-10-01 by [`PRD/GC_PRD.md`](../GC_PRD.md)** and archived. It
> is kept for its dated measurements: do not update it. Section references are
> into `docs/GC_DESIGN.md` as it stood then. Priority 1 item 1 (#19) and
> Priority 2b (#423) were complete; the open items went to these stages of the
> new PRD:
>
> - Priority 1 item 2 → traced code liveness at stage 4e and descriptors in stage 5c's non-moving space; the immortal set is replaced by memory-contract rule M1.
> - Priority 2 → stage 2 (rooted loading) and stage 4e (nested loops collect once the weak continuation tables go).
> - Priority 3 → stage 5 (lazy sweep), stage 7 (sticky generations), decision 11 as amended by SD2 (weak symbol table).
> - Priority 4 → stage 3: the poll is folded into the stack-limit check at frame entry.
> - Non-goals → moving is no longer ruled out (stage 8, opportunistic evacuation); stop-the-world stays, and shared-memory parallelism is decision 7.

**Status:** In progress — Priority 1 item 1 (weak continuation tables) done
2026-08-05. Stage 4 complete 2026-08-03 (PRs #4–#6, #8, #10, #11):
both backends collect, adaptive collection is always on, the safe point has no
standing cost, and CI enforces byte-identical differential lanes.
**Authority:** `docs/GC_DESIGN.md` is the design document; this PRD tracks the
remaining work items it staged as "5+". Section references below are into it.

Everything here is behind the existing seams — `Collector` for algorithm
changes, `GcRoots` for root-set changes — and every item carries the same
acceptance baseline: **the differential lanes stay byte-identical, and perf
claims come from interleaved A/B runs** (main / branch / main), never a single
run against a stored baseline.

## Why stage 5 exists

Stage 4 made collection *correct and free when idle*. It did not bound
**pauses**: the collector is stop-the-world mark-and-sweep, and two root sets
scale with everything ever created rather than with live data (§9.5, measured),
so pauses grow monotonically in long sessions. Stage 5 is pause work first,
throughput work second.

## Priority 1 — Unbounded root sets (§9.5, measured)

The two known monotonic costs, in impact order:

1. **Continuation side tables.** ✅ **Done 2026-08-05.** Nothing removed
   entries from `continuation_store` / `delimited_continuation_store`; worse,
   snapshots contain other continuation refs, so the strong tables pinned
   themselves transitively — `ctak` grew to 4 GB RSS and died thrashing.
   Fixed as an ephemeron-style weak-table fixpoint: marking records ids of
   reached `VmContinuationRef` objects, `GcRoots::trace_weak_ids` traces
   payloads only for recorded ids (looped with drains to quiescence), and
   `GcRoots::sweep_weak` prunes the rest. The `is_outermost` care resolved to
   an audit: every store touch is confined to one instruction dispatch and
   nested loops defer, so an unmarked ref at a collecting safe point proves
   its payload unreachable. Measured: **ctak completes — 72.6 s at 227 MB
   peak RSS** (was CRASHED at 4 GB; Chibi itself dies on ctak at the 300 s
   cap on this machine); 20 000 dead captures leave single-digit net live
   objects. Differential lanes byte-identical (release + debug-poison, both
   backends). Design details in `docs/GC_DESIGN.md` §9.5.
2. **`code_store` constants.** Code objects are never evicted; every compiled
   top-level form adds permanent roots. Measured over one chibi run: scan grew
   8.8 µs → 107–151 µs per collection, **57% of root tracing** by the end.
   Fix: an immortal set — append constants to one flat `Vec<TaggedValue>` at
   load time and `visit_slice` it once, or a persistent immortal bitmap seeded
   into `MarkBits::for_heap`. The same immortal set should absorb the symbol
   table, which `GcVisitor::new` re-marks every collection (§9.2).

**Acceptance:** the §9.5 instrumented workloads show root-tracing time flat in
session length; `GcStats.last_pause_micros` distribution recorded before/after.

## Priority 2 — Nested-loop collection (§7 known limitation)

A long-running nested execution — `(map f huge-list)` where each `f` call is a
nested trampoline, or a large library body — cannot collect until it returns
to the outermost loop, so allocation bursts in exactly the wrong places run
unreclaimed. Fix: root the re-entrancy boundary explicitly (the suspended
`StepResult` / the boundary argument slice) and let nested loops collect; the
`GcRoots` trait already accommodates this (transient providers).

**Acceptance:** a nested-map churn workload shows collections > 0 *during* the
map and a bounded arena; the defer-guard tests keep passing.

## Priority 2b — VM register precision (triage family 32) — completed

Completed 2026-09-28 ([#423](https://github.com/avalonalex/patina/issues/423)).
Compiler maps record expression temporary lifetimes; collection and
continuation capture retire the finished slots. Local bindings and runtime
stubs remain conservative. No clearing instructions run in the normal VM
loop. See `docs/VM_COMPILER.md` §10.4 and `docs/GC_DESIGN.md` §5.2.

**Acceptance:** the inline replacement collects in its own frame; Larceny's
`ephemeron` suite passes 6 of 6 on the VM; the regression no longer hides the
replacement in a helper that returns. `reference-barrier` was re-audited and
has a collection test. Full/delimited continuation and release/debug GC
checks cover the same register-vector tracing contract.

Interleaved main/change/main measurements against `0f1cd12`, five measured
rounds after warmup on the development machine: `fib(35)` +0.9%, a 20-million
iteration tail sum +1.1%, and 10-million-pair churn −0.1% (wall time,
including startup). These small workloads do not measure GC pause tails.

## Priority 3 — Collector upgrades (behind `Collector`)

In likely order of value:

- **Lazy sweep** — move sweep cost off the pause and onto allocation.
- **Non-moving generational** — sticky mark bits plus write barriers on
  `set-car!` / `set-cdr!` / `vector-set!` / `MutableCell` stores. The barrier
  is the risky part: it taxes the mutator's hottest stores, so it must be
  measured with the interleaved methodology before it can land.
- **Weak symbol table (§9.2)** — subsumed by the Priority 1 immortal set for
  the marking cost; actual reclamation of dead symbols is its own item and
  needs `eq?`/intern-table care.

## Priority 4 — Residual trigger cost (§6.1, optional)

The safe point's remaining ~1% vs a no-check control is the flag load and
branch itself, removable only by specializing the dispatch loop on
"outermost + collection possible". Only worth doing if a measured workload
shows it; the interleave must beat the 3–7% run spread to count.

## Non-goals

- Moving/compacting collection — ruled out permanently (§3.4): raw arena
  indices escape `TaggedValue` (symbol table, `eq?` semantics, VM constants).
- Concurrency — single-threaded runtime; stop-the-world stays, the work above
  makes the stops small.
