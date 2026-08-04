# GC Stage 5+: Pause Work and Collector Upgrades

**Status:** Not started. Stage 4 complete 2026-08-03 (PRs #4–#6, #8, #10, #11):
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

1. **Continuation side tables.** Nothing removes entries from
   `continuation_store` / `delimited_continuation_store`. Measured: with 20 000
   dead `call/cc` captures, **81% of a 1.79 ms pause** is root tracing, and
   every register/frame snapshot those dead continuations pin stays
   unreclaimable. Fix: treat the stores as weak — drop an entry when its
   `VmContinuationRef` heap object did not survive marking. Sweep-side change;
   needs care with the `is_outermost` protocol.
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
