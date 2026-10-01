# Immix family and MMTk: what Patina should take, and whether to depend on MMTk

Research report, track `immix-mmtk`. Date of research: 2026-09-30. MMTk state is as of
mmtk-core `master` at commit `0837666` (2026-09-29) and release 0.33.0 (2026-08-05).

Notation: **[V]** = verified by reading the primary source (paper text, source file, repo file:line
or API). **[I]** = my inference or design judgement. **[M]** = measured by me in a throwaway crate (`PRD/study/gc/probes/immix-mmtk`).

---

## 0. Bottom line

1. **Adopt the Immix family, not MMTk.** Build a bespoke, single-threaded, mark-region collector
   in Rust: Immix block/line heap, then sticky-mark-bit generations (Sticky Immix), then
   opportunistic evacuation with pinning (the Conservative Immix recipe). Shape Patina's
   collector seams after MMTk's `VMBinding` decomposition so an MMTk backend stays possible as
   a later experiment. [I]
2. **The decisive problems with MMTk for Patina are structural, not about quality.** MMTk is
   high quality, MIT/Apache, Rust, and has every algorithm we would want: Immix, StickyImmix,
   GenImmix, LXR (merged 2026-08-19), ConcurrentImmix, and Compressor. But:
   (a) MMTk allows one instance per process, with a fixed virtual-address heap layout. Patina
   runs many independent interpreters per process (both backends in one test binary; embedding).
   (b) GC work runs on MMTk-spawned worker threads, and every binding callback must be
   `Send`/`Sync`. Patina is `Rc<RefCell<…>>` throughout.
   (c) `scan_object`/`get_current_size` take an *untagged* `ObjectReference`, so every object
   needs a self-describing header. That turns Patina's 16-byte header-less pairs into 24-byte
   pairs (+50%), unlike Chez.
   (d) API churn: real bindings pin git revisions rather than semver releases.
   (e) macOS/aarch64 reached only "tier 2 = guaranteed to build" on 2026-09-24. [V for the facts, I for the weighting]
3. **Immix needs no heap parsing, and that is its best property for a Scheme.** Immix reclaims
   by line marks and never walks objects linearly. With an object-remembering or
   field-logging barrier (not card marking), header-less pairs work: the tag in every
   reference supplies the type. Chez does the same. [V for Immix/Chez facts, I for the
   conclusion]
4. **Barrier and fast-path costs are small and well characterised.** Object-remembering
   barrier: 1.6% ± 0.7% (Yang et al. 2012). LXR field-logging barrier: 1.6% geomean, worst case
   4.6%. Conservative roots with pinning: within 2–3% of exact. Inlining the barrier slow path
   *hurts*: overhead rose from 1.6% to 2.6% and i-cache misses rose 20%. These numbers set the
   JIT contract. [V]

---

## 1. Patina today (the facts that matter for an Immix/MMTk decision)

- `TaggedValue` is `#[repr(transparent)] struct TaggedValue(u64)`
  (`crates/patina-core/src/tagged_value.rs:57`). It uses a low-3-bit tag
  (`tagged_value.rs:9-21`, `:76-84`). The fixnum tag is `000`. Pair, vector, string, closure
  and object are heap tags, and their payload is a `HeapIndex = u32` arena index
  (`tagged_value.rs:28`). It is not NaN-boxed. [V]
- `Heap` (`crates/patina-core/src/heap/mod.rs:304-…`) consists of:
  - `pairs: Vec<(TaggedValue,TaggedValue)>` (`:307`)
  - `vectors: Vec<Vec<TaggedValue>>` (`:310`)
  - `strings: Vec<Vec<char>>` (`:313`)
  - `objects: Vec<HeapObjectData>`
  - intern tables
  - free lists
  - a `gc_pending: Rc<Cell<bool>>` flag
  - a `gc_defer_depth` re-entrancy counter.

  `SharedHeap = Rc<RefCell<Heap>>` (`mod.rs:51`). [V]
- **Measured slot sizes** [M] (`PRD/study/gc/probes/immix-mmtk`, `std::mem::size_of`):

  | Type | Size |
  |---|---|
  | `HeapObjectData` | 72 B |
  | pair slot | 16 B |
  | `Vec<TaggedValue>` vector slot header | 24 B, plus a separate malloc'd buffer |
  | `Vec<char>` string slot header | 24 B, plus 4 B per char |

  So a flonum (`HeapObjectData::Real(f64)`, `mod.rs:146`) or a `MutableCell`
  (`mod.rs:190`) costs a 72-byte slot. Immix with an 8-byte header would cost 16 bytes.
  Pairs are already dense and header-less.
- `HeapObjectData` has 28 variants (`mod.rs:143-229`). Many of them hold `Rc` payloads into
  non-heap Rust structures: `Procedure`, `Port`, `Macro`, `Continuation`, `Library`,
  `EnvironmentSpecifier`, and `VmClosure{globals: Rc<Environment>}` (`mod.rs:205-213`).
  Environments are `Rc`-linked Rust structs with hash maps (`environment.rs:487-…`). [V]
- **Collector**: non-moving, stop-the-world mark-sweep. Its parts are:
  - `Collector` trait (`heap/gc.rs:208-212`)
  - `GcRoots` (`gc.rs:177-196`), with `trace_weak_ids`/`sweep_weak` for the weak
    continuation tables
  - side `MarkBits`, one bitset per arena (`gc.rs:88-93`)
  - an ephemeron fixpoint inside `run_mark_phase` (`gc.rs:1022`)
  - sweep to free lists with tombstoning (`gc.rs:891`)
  - adaptive trigger `max(65_536, 2 × live)` (`gc.rs:969-1002`). [V]
- **Safe points**: the VM checks `gc_pending` at the top of each dispatch iteration
  (`crates/patina-vm/src/runtime/vm_state.rs:1203-1206`). It collects only at the outermost
  loop (`vm_state.rs:1288`). Allocation never collects; it only raises the flag
  (`Heap::note_alloc`, `mod.rs:581-584`; `alloc_pair`, `mod.rs:703-714`). This invariant
  matters: Rust primitives can hold unrooted `TaggedValue`s across allocations. [V]
- **VM roots** are precise and live off the native stack:
  - `ExecutionState { registers: Vec<TaggedValue>, frames: Vec<CallFrame>, … }`
    (`crates/patina-vm/src/runtime/execution_state.rs:17-23`)
  - `CallFrame.closure: Option<HeapIndex>` (`crates/patina-vm/src/types/mod.rs:50`)
  - full continuations snapshot `frames` and `registers` into side tables
    (`types/continuation.rs:159-178`). [V]
- Prior docs declared moving GC "ruled out permanently" (`PRD/ARCHIVE/GC_STAGE5_PRD.md:119`;
  `docs/GC_DESIGN.md:122-139`). The reason was raw indices escaping into the symbol table,
  raw-bits keyed maps, `eq?` hashing, code constants and `CompiledMacro` literals. Concurrency
  is a non-goal (`GC_STAGE5_PRD.md:121`). The same PRD already names "non-moving
  generational — sticky mark bits plus write barriers" and "lazy sweep" as the next upgrades
  (`GC_STAGE5_PRD.md`, Priority 3). [V]
- The heap is explicitly per-interpreter-instance, and "both backends exist in a single test
  binary" (`heap/mod.rs:339`). Patina is MIT-licensed (`Cargo.toml:21`). It pins Rust 1.97.1
  (`rust-toolchain.toml`). `patina-core` depends on only `num-*`, `rustc-hash` and `smallvec`
  (`crates/patina-core/Cargo.toml:9-20`). [V]

---

## 2. Immix (Blackburn & McKinley, PLDI 2008)

Source: <https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf>. I extracted the text
with PDFKit and read it; quotes are from that text. [V throughout this section]

**Mechanism**

- **Mark-region heap**:
  - 32 KB blocks, divided into 128-byte lines.
  - Wholly free blocks are reclaimed at block grain, and free runs of lines ("holes") in
    partially used blocks are reclaimed at line grain.
  - Allocation is bump-pointer into the current hole. Recyclable blocks are tried in address
    order before free blocks.
- **No object may span blocks.** The large-object threshold is 8 KB (MMTk's LOS default at the
  time), and a 32 KB block holds at least four Immix objects. That bounds worst-case
  block-level fragmentation to about 25%.
- **Line marks are bytes, not bits.** This avoids synchronisation among parallel markers and
  costs 1/128 metadata. Total metadata is "260 bytes per 32KB block, which amounts to 0.8%".
- **Conservative line marking.** Most objects are smaller than a line (under 128 B), so the
  collector marks only the line holding the object's start. The allocator implicitly treats
  the first line of each hole as possibly occupied by a straddling small object. A single
  header bit marks "medium" objects (larger than one line), and those get exact line marking.
  Exact marking of every object's line range was found to be "quite expensive" compared with
  setting one mark bit.
- **Demand-driven overflow allocation.** When a medium object does not fit in the current hole
  but free lines remain, it goes to a second bump allocator that uses only empty blocks. This
  avoids skipping and wasting holes. It handles 4% of allocation on average.
- **Opportunistic evacuation (defragmentation).** At GC start, statistics from the previous
  GC choose candidate (source) blocks and targets. In one marking pass, live objects found on
  candidates are copied, leaving forwarding pointers. Copying stops when the headroom runs
  out, and the remaining objects are simply marked in place. **Pinned objects are never
  moved.** The default headroom is 2.5% of the heap; sensitivity is low between 1 and 3%.
  The header needs 4 bits: a mark bit, a "spans lines" bit, and two forwarding bits
  (summarised in RC Immix §3).

**Numbers**

- Total time is better than the best canonical collector by "7 to 25% on average" (abstract).
  The analysis says "typically by around 5-10%". The worst case was a 4.9% slowdown (hsqldb at
  1.1× heap); the best was 74%.
- Minimum heap: Immix needs 86% of mark-sweep's minimum, against 83% for mark-compact. It
  "requires on average only 3% more memory than mark-compact … 15% less than mark-sweep".
  Its mutator locality matches semispace.
- Dark matter: block-only reclamation inflates the amount marked by 93%. Line marking without
  defragmentation inflates it by 23%. Conservative line marking wastes under 6% on most
  benchmarks (javac and fop: 20–25%).
- At 2× heap, 79% of allocation goes into free or mostly-free blocks (under 25% marked).
- Line size matters more than block size. Line size tracks the language's object-size
  demographics, not the cache line.

**Lessons for Patina** [I]

- Scheme objects are mostly 16–32 B: pairs, closures with few free variables, boxes,
  flonums, small records. That is smaller than Java's average, so 128 B lines (or MMTk's
  current 256 B) hold 4–8 objects. Conservative line marking fits well.
- Patina should measure the object-size histogram first (closures, records, vectors) and
  choose the line size from it. This is the paper's explicit advice.
- Immix's sweep is a scan of line-mark bytes per block. It never reads object memory. **This
  makes Immix compatible with header-less objects**, provided the tracer always knows an
  object's type from the referencing tag, as in Patina. The only consumers of heap
  parsability are card scanning (avoid it) and conservative-root filtering (use an object-start
  bitmap).

---

## 3. Generational Immix: Sticky Immix and GenImmix

- **Sticky Immix**. It applies the sticky-mark-bits algorithm of Demers et al. (POPL 1990)
  to Immix. Old objects keep their mark. A nursery GC traces only from roots plus remembered
  old→young edges (from a write barrier), marking only new objects. New objects are allocated
  into the same Immix holes; there is no separate nursery space. It may opportunistically
  copy young survivors to compact them.
  - Described in the Immix paper (abstract: "an in-place generational variant based on the
    sticky mark bits algorithm … to support pinning").
  - Conservative Immix §3.2: "performs generational collection without a copying nursery …
    A write barrier records old to young references". [V]
- **GenImmix**. A copying (semispace) nursery with an Immix mature space. It matched or beat
  Jikes RVM's best production generational collector and improved jbb2000 by 5% or more at
  all heap sizes. It requires all nursery referents to be movable. [V]
- **In MMTk today** [V]:
  - `PlanSelector::StickyImmix` is "an Immix collector that uses a sticky mark bit to allow
    generational behaviors without a copying nursery". `GenImmix` uses "a copying nursery, and
    Immix as its mature space" (`src/util/options.rs`).
  - StickyImmix uses an object-remembering barrier with *unlog bits*.
  - Feature `sticky_immix_non_moving_nursery` disables nursery copying while keeping other
    copying (`Cargo.toml` features).
  - StickyImmix forces full-heap GCs on user requests (`src/plan/sticky/immix/global.rs:383`).
- **Production choice** [V]:
  - Julia's MMTk build defaults to `MMTK_PLAN=StickyImmix`, with `MMTK_MOVING=0` ("not
    supported yet") (<https://docs.julialang.org/en/v1.14-dev/devdocs/gc-mmtk/>).
  - Ruby's in-tree binding enables `sticky_immix_non_moving_nursery`, `vo_bit` and
    `object_pinning` (`ruby/ruby gc/mmtk/Cargo.toml`, mmtk-core rev `9a2c733`).
  - Wingo's Whippet `mmc` gets generations "via the sticky mark-bit algorithm"
    (`doc/collector-mmc.md`).
  - So **all three recent "retrofit a modern GC into an older runtime" efforts chose sticky
    mark bits first**.
- **Lesson** [I]: Sticky Immix is the natural second step after plain Immix for Patina. It is
  the PRD's own Priority 3 "non-moving generational" item. It needs no copy reserve, no
  precise-everywhere guarantee, and no nursery space. It can later turn on young-object
  evacuation once roots allow.

---

## 4. RC Immix (Shahriyar, Blackburn, Yang, McKinley, OOPSLA 2013)

Source: <https://www.steveblackburn.org/pubs/papers/rcix-oopsla-2013.pdf>. [V]

- **Thesis**: the remaining gap between reference counting and tracing came from **heap
  organisation** (a free list), not from RC itself. With the same barrier style:
  - RC's mutator was 9.3% slower than Sticky Immix
  - with 9.2% more retired instructions
  - and 33% more L1D misses (Table 2).
- **Contiguous allocation**:
  - Free-list allocation added about 7% retired instructions, mostly from cell-by-cell
    zeroing rather than bulk zeroing.
  - Bump-pointer allocation is cheaper chiefly because it zeroes in bulk.
  - This is directly relevant to Patina. Today Patina allocates via free lists of arena slots
    (`alloc_pair` pops `free_pairs`, `mod.rs:705`), and vectors and strings go through
    `malloc`.
- **Design**:
  - Per-line *live object counts* replace line marks.
  - New objects are "born dead". Coalescing deferred RC ignores mutations of new objects.
  - Proactive copying of surviving young objects, plus reactive copying during backup cycle
    tracing.
  - Header: 8 GC bits (2 logging, 1 mark, 1 new, 4 RC). A 4-bit count suffices for more than
    99.8% of objects; with 3 bits, 0.65% overflow.
- **Results**: 12% faster than prior RC on average. 3% faster than GenImmix at 2× minimum heap.
- **Lesson** [I]: RC is a poor fit for a Scheme interpreter's first redesign.
  - Closures and environments, `letrec`, and continuation graphs create many cycles, so
    backup tracing would carry much of the load.
  - RC's benefit (prompt reclamation, short pauses) matters less with a single thread and
    small heaps.
  - The transferable lesson is RC Immix's heap-organisation result: **bump allocation into
    lines beats free lists for mutator locality.**

---

## 5. Conservative Immix (Shahriyar, Blackburn, McKinley, OOPSLA 2014, "Fast Conservative Garbage Collection")

Source: <https://www.steveblackburn.org/pubs/papers/consrc-oopsla-2014.pdf>. [V]

- **Setting**: stacks and registers are ambiguous; the heap is precise. Under Java,
  ambiguous roots falsely retain **under 0.01%** of objects and pin **0.03%**.
- **The cost comes from collector design, not ambiguity.** Prior designs (BDW mark-sweep, and
  mostly-copying MCC with page-grain pinning) cost 12% and 45% relative to generational
  copying. Making mark-sweep's roots conservative costs only about 1%.
- **Conservative Immix, Sticky Immix and RC Immix come within 2–3% of their exact
  counterparts.** Conservative RC Immix is slightly faster than a well-tuned exact
  generational collector. It is 13% faster than BDW on average, and up to 41%.
- **Mechanism**:
  - An **object map**, a bitmap of object starts, set at allocation and rebuilt during
    tracing, filters ambiguous words.
  - Header-format tricks reduced it to 1 bit per 8 B (1.5%), and its mutator overhead fell
    from 2.3% to 1.3%.
  - Referents of ambiguous roots are **pinned at line granularity** via a per-object pin
    bit. Everything else is still opportunistically evacuated.
- **Whippet's `mmc` uses the same recipe** (`doc/collector-mmc.md`) [V]. Conservative roots
  are traced first and are "marked instead of evacuated, implicitly pinned"; objects not
  directly referenced by roots can still be evacuated.
- **Lesson** [I]:
  - This is the bridge that overturns "moving is ruled out permanently".
  - Patina does not have to make every Rust primitive's locals precise to get compaction.
    Any root it cannot enumerate precisely (Rust locals during a primitive, JIT frames
    without a stack map) can be scanned conservatively and pinned.
  - Precise roots (VM register file, `VmState` tables, globals) are updated.
  - Pinning also handles `eq?`-hash identity ("pin on first identity hash") and FFI.

---

## 6. LXR (Zhao, Blackburn, McKinley, PLDI 2022)

Source: <https://www.steveblackburn.org/pubs/papers/lxr-pldi-2022.pdf>. [V]

- **Design**: brief stop-the-world RC pauses over an Immix heap, plus occasional concurrent
  SATB tracing for cycles, plus judicious evacuation via RC-maintained remembered sets. It
  uses no read barrier.
  - The single **field-logging write barrier** (Fig. 3: `if (isUnlogged(field))
    logField(field)`) feeds coalescing RC (decbuf/modbuf), the SATB snapshot, and remset
    maintenance.
  - It uses **one unlogged bit per field in side metadata**. New objects are zeroed, hence
    "logged", so mutations of young objects are never logged (the implicitly-dead
    optimisation).
- **Barrier cost**: "geometric mean overhead of this write barrier compared to no write
  barrier on Immix is 1.6%, with a worst case for h2o at 4.6%".
  - Loads outnumber stores by about 15× (64.3/µs vs 4.3/µs). LVB read barriers (C4,
    Shenandoah, ZGC) are therefore "on the order of five times more expensive than an object
    remembering barrier".
- **Results**: on Lucene in a tight heap, 7.8× the throughput and 10× better 99.99% tail
  latency than Shenandoah. On 17 workloads at a moderate heap, 4% better throughput than G1
  and 43% better than Shenandoah.
- **Now in MMTk core** [V]:
  - "Simplified LXR GC (#1508)" by Wenyu Zhao merged 2026-08-19, with follow-ups through
    2026-09-23: pinning roots for LXR (#1572), object log bit for probable writes (#1580),
    tuning knobs (#1584).
  - `PlanSelector::LXR` exists. Features include `lxr_stw` (no concurrent marking or lazy
    decrements), `lxr_no_evac` and `lxr_object_log` (`Cargo.toml` `[features]`).
  - It is not in any release yet: 0.33.0 predates it.
- **Lesson** [I]: LXR's *barrier design* is the lasting takeaway: a single conditional
  unlog-bit check in side metadata, logged once per epoch, serving every collector mode. The
  full RC machinery suits large, latency-critical, multi-core heaps, which is the wrong
  target for Patina now. A Patina barrier should be *the same instruction sequence* whether
  the collector is sticky (object-remembering) or a future LXR-like mode (field-remembering),
  so JIT code does not change.

---

## 7. Barrier cost studies

- **Yang, Blackburn, Frampton, Hosking, "Barriers Reconsidered, Friendlier Still!"
  (ISMM 2012)**, <https://www.steveblackburn.org/pubs/papers/barrier-ismm-2012.pdf>, on an
  i7 [V]:

  | Barrier | Overhead | Notes |
  |---|---|---|
  | Card marking | 0.9% ± 0.8% | 2.2% on P4, 1.8% on Atom: the most architecture-sensitive |
  | Object-remembering | 1.6% ± 0.7% | |
  | Boundary | 1.7% ± 0.8% | |

  Forcing the object barrier's **slow path inline raised overhead to 2.6% and i-cache misses
  by 20%**, against 5.7% for out-of-line. Modern DaCapo exposes barrier cost more than
  SPECjvm98.
- **The trade-off is precision.** Card marking makes the collector scan whole cards, which
  needs heap parsability. Object barriers log each object once. LXR's field barrier is the
  most precise.
- **Concrete JIT-inlinable sequence from mmtk-openjdk** [V]
  (`openjdk/barriers/mmtkUnlogBitBarrier.hpp`): the unlog bit lives in global side metadata
  at a base address, `meta = BASE + (addr >> 6)`, `bit = (addr >> 3) & 7`, giving 1 bit per
  8 B word. The fast path is shift, add, byte load, shift, test, branch to a cold stub. C1
  and C2 each have their own emitter (`mmtkBarrierSetC1.cpp`, `mmtkBarrierSetC2.cpp`,
  `mmtkBarrierSetAssembler_x86.cpp`).
- **Lesson for Patina's JIT** [I]:
  1. Use a conditional unlog-bit barrier in side metadata, not cards (cards need heap
     parsing, which header-less pairs prevent).
  2. Emit the slow path as an out-of-line cold block in Cranelift.
  3. Elide the barrier when the stored value is statically an immediate (fixnum, char,
     boolean), and when the target is known to be freshly allocated.
  4. In a bytecode interpreter the barrier hides behind dispatch cost. In JIT code it is
     the 1–2% the literature reports.

  The barrier sites in Patina are few:
  - `set-car!`/`set-cdr!`, `vector-set!`
  - the VM's `WriteCell`/`WriteLocalCell` on `MutableCell`
  - record field set
  - global or environment slot writes, if environments move into the heap
  - promise and parameter updates
  - hash-table internals.

  Register-file writes are root writes and need no barrier.

---

## 8. MMTk core: architecture and binding contract

Repo: <https://github.com/mmtk/mmtk-core>. All facts in this section are from source on
master or from `CHANGELOG.md`. [V]

**Identity and maturity**

- Crate `mmtk` 0.33.0 (2026-08-05). License `MIT OR Apache-2.0`, compatible with Patina's
  MIT. MSRV 1.84, edition 2021.
- Source size: about 2.4 MB of Rust in 291 files under `src/`, measured with the GitHub tree
  API. That includes about 154 KB for `plan/lxr` and about 127 KB of mock tests.
- Recent cadence: 0.29 (2024-11), 0.30 (2024-12), 0.31 (2025-04), 0.32 (2026-02), 0.33
  (2026-08), with an active master.
- Platform tiers (README): x86_64 Linux and i686 Linux are tier 1, "guaranteed to work".
  aarch64-apple-darwin and aarch64 Linux are **tier 2, "guaranteed to build"**; the darwin
  entry was added 2026-09-24 in "Tier 2 support for aarch64 macOS (#1599)".

**Plans** (`PlanSelector`, `src/util/options.rs`): NoGC, SemiSpace, GenCopy, GenImmix,
MarkSweep, PageProtect, Immix, Lisp2, OVC (offset-vector compaction), StickyImmix, LXR,
ConcurrentImmix. Concurrent Immix (SATB) arrived in 0.32 and gained pinning-root support in
0.33. Parallel and serial Compressor arrived in 0.32.

**Immix constants in MMTk**

- Block = 32 KB (`policy/immix/block.rs`, `LOG_BYTES = 15`; 8 KB with `immix_smaller_block`).
- **Line = 256 B** (`line.rs`, `LOG_BYTES = 8`), twice the paper's size.
- `MAX_IMMIX_OBJECT_SIZE = Block::BYTES >> 1`, i.e. 16 KB; larger objects go to the LOS
  (`policy/immix/mod.rs`).
- `MARK_LINE_AT_SCAN_TIME = true`, and `mark_lines_for_object` marks the exact line range
  using `get_current_size`. This is not the paper's conservative line marking.
- Block metadata (mark state and defrag state) is in side tables accessed atomically
  (`block.rs`).

**`VMBinding`** (`src/vm/mod.rs:47-50`): `Self: Sized + 'static + Send + Sync + Default`.
Associated types are `ObjectModel`, `Scanning`, `Collection`, `ActivePlan`, `ReferenceGlue`,
`Slot` and `MemorySlice`.

- **`ObjectModel`** (`src/vm/object_model.rs`)
  - The binding places each metadata item in the header or on the side:
    - `GLOBAL_LOG_BIT_SPEC` (unlog bit)
    - `GLOBAL_FIELD_UNLOG_BIT_SPEC` (LXR)
    - `LOCAL_FORWARDING_POINTER_SPEC`, a word, "usually in object header"
    - `LOCAL_FORWARDING_BITS_SPEC`, 2 bits
    - `LOCAL_MARK_BIT_SPEC`
    - `LOCAL_PINNING_BIT_SPEC`
    - `LOCAL_LOS_MARK_NURSERY_SPEC`.
  - Required methods include `copy`, `get_current_size(object)`, `get_size_when_copied`,
    `get_align_when_copied`, `ref_to_object_start` and `ref_to_header`.
  - `ObjectReference(NonZeroUsize)` (`src/util/address.rs`) must be non-null, word-aligned
    and inside the object.
- **`Slot`** (`src/vm/slot.rs`) explicitly supports **tagged** slots. `Slot::load` decodes a
  tagged word to an `ObjectReference` (or `None` for immediates), and `store` re-encodes it.
  So a slot can carry Patina's tags, but **the `ObjectReference` handed to `scan_object` and
  `get_current_size` cannot**.
- **`Scanning`** (`src/vm/scanning.rs`) has:
  - `scan_object(tls: VMWorkerThread, object, slot_visitor)`, or
    `scan_object_and_trace_edges`
  - `scan_roots_in_mutator_thread(tls: VMWorkerThread, mutator, factory)`
  - `scan_vm_specific_roots`
  - `process_weak_refs(worker, tracer_context) -> bool`, which, "if returns true … will be
    called again". That is the hook a Patina ephemeron / weak-continuation fixpoint would use.
  - `RootsWorkFactory: Clone + Send + 'static`, with `create_process_roots_work`,
    `create_process_pinning_roots_work` and `create_process_tpinning_roots_work`
    (transitively pinning).
- **`Collection`** (`src/vm/collection.rs`):
  - `stop_all_mutators(tls: VMWorkerThread, visitor)`, `resume_mutators`
  - `block_for_gc(tls: VMMutatorThread)`
  - `spawn_gc_thread(tls, GCThreadContext::Worker(..))`
  - optional `create_gc_trigger()` for the `Delegated` trigger, plus `vm_live_bytes`, etc.
  - `initialize_collection(&'static self, …)` calls `scheduler.spawn_gc_threads`
    (`src/mmtk.rs:260-268`).
  - The thread count defaults to `num_cpus`. The `single_worker` feature forces one worker,
    still a separate thread.
- **Instances**:
  - "MMTk currently assumes that there is only one `MMTK` instance in your runtime process.
    Multiple `MMTK` instances are currently not supported" (porting guide `howto/nogc.md`).
  - `src/mmtk.rs:114-115` says "multi-instances is not fully supported yet". Global
    `VM_MAP`/`MMAPPER`/`SFT_MAP` statics are at `mmtk.rs:44-58`.
  - Issue #100 "Support MMTk instances" has been open since 2020-06.
  - The 64-bit layout reserves a fixed virtual range starting at `0x0000_0200_0000_0000`
    (`src/util/heap/layout/vm_layout.rs:147-149`). Issue #1347 (dynamic heap ranges) is open.
- **Mutator and allocation fast path**:
  - `bind_mutator` returns a boxed `Mutator`. It contains allocator arrays (6 bump, 2 LOS,
    2 Immix, 2 free-list, …; `src/util/alloc/allocators.rs:20-25`) and a barrier object.
  - The inlinable state is `#[repr(C)] BumpPointer { cursor, limit }`
    (`src/util/alloc/bumpallocator.rs:34-41`).
  - The perf guide (`docs/userguide/src/portingguide/perf_tuning/alloc.md`) recommends one
    of three options:
    - embed the whole `Mutator` in thread-local storage ("a few hundreds of bytes")
    - embed only `BumpPointer`
    - pre-compute the allocator offset

    JITs should "generate the code sequence for the allocation fast-path, rather than simply
    emitting a call". OpenJDK C1/C2, JikesRVM and Julia do this.
  - `post_alloc` (setting VO bits and similar) has its own fast path.
- **`AllocationOptions`** (`src/util/alloc/allocator.rs:33-62`) offers
  `allow_overcommit`, `at_safepoint` ("If `false`, the allocation will immediately return a
  null address if the allocation cannot be satisfied without a GC") and `allow_oom_call`.
  This is how a binding keeps an invariant like Patina's "allocation never collects".
- **Barriers** (`src/plan/barriers.rs`):
  - `NoBarrier`, `ObjectBarrier` (fast path `if object_is_unlogged(src) { slow }`),
    `FieldBarrier` (per-field unlog bits) and `SATBBarrier`.
  - Pre/post `object_reference_write`, `memory_region_copy`, and `object_probable_write`
    for bulk or unknown writes.
- **Conservative roots and pinning**:
  - The `vo_bit` feature (valid-object bit, 1 bit per 8 B, `MIN_OBJECT_SIZE = word`) gives
    `is_mmtk_object(addr)` and `find_object_from_internal_pointer`. Enabling it also forces
    `eager_sweeping` (`Cargo.toml: vo_bit = ["eager_sweeping"]`).
  - `object_pinning` gives `pin_object`/`unpin_object`/`is_pinned`.
  - Pinning roots arrived for Immix and StickyImmix in 0.20/0.25 and for ConcurrentImmix and
    LXR in 2026.
- **Triggers**: `FixedHeapSize` (default 50% of physical memory), `DynamicHeapSize(min, max)`
  backed by a simplified MemBalancer (`src/util/heap/gc_trigger.rs:472-476`), or `Delegated`.
- **Dependencies** (`Cargo.toml [dependencies]`): about 30 direct crates. Among them are
  `crossbeam`, `regex`, `sysinfo`, `strum`/`strum_macros`, `enum-map`, `itertools`,
  `downcast-rs`, `spin`, `atomic`, `portable-atomic`, `probe`, `num_cpus`, `lazy_static`,
  `libc` and the proc-macro crate `mmtk-macros`. jemalloc and mimalloc are optional. Binary
  size and build time for Patina were **not measured**: that would need a crates.io download,
  which I did not do.
- **Weak references and ephemerons**: done by the binding through `process_weak_refs` (with
  re-invocation for fixpoints) and `forward_weak_refs`. The user guide has chapters on
  finalizers and weak references and on **address-based hashing** (states
  `Unhashed → Hashed → HashedAndMoved`, extending the object by a hash word only when it is
  hashed and then moved).

---

## 9. The real bindings: what they show

| Binding | Where / status | Plans in use | Roots | Takeaway |
|---|---|---|---|---|
| **mmtk-openjdk** | <https://github.com/mmtk/mmtk-openjdk>. Pins a git rev of mmtk-core and of an OpenJDK fork | nogc, semispace, gencopy, marksweep, immix, genimmix, stickyimmix, markcompact (`mmtk/Cargo.toml` features) | Precise (stack maps from C1/C2) | The research reference binding. Full JIT-inlined allocation and barrier fast paths in C1, C2 and x86 assembly (`openjdk/mmtkBarrierSet*.cpp`) [V]. Shows the API supports JIT inlining when the binding owns the code generator. |
| **Ruby** (in-tree since 3.4) | `ruby/ruby` `gc/mmtk/` (Rust, edition 2024). Pins mmtk-core **git rev `9a2c733`** (2026-09-28), `lto = true` | Default non-moving-nursery StickyImmix; Immix; MarkSweep (mmtk-ruby README) | Roots handed as **pinning roots** (`create_process_pinning_roots_work`, batches of 4096). Object scanning by **upcall into C** (`call_gc_mark_children`). Header prefix word holds size, overwritten by the forwarding pointer (`object_model.rs`) [V] | Retrofit of a conservative C runtime. RubyKaigi 2026 abstract: MMTk was "significantly slower" at 3.4. It is now "on par with the default garbage collector", with Moving Immix, after about a year of work by a dedicated team [V]. |
| **Julia** | Binding now lives in the Julia repo (`src/gc-mmtk/`). The external `mmtk-julia` README says "maintenance mode and will be archived soon" and supports "only … (non-moving) Immix and StickyImmix … only x86_64 Linux" [V] | StickyImmix default, Immix; `MMTK_MOVING=1` "not supported yet" (Julia devdocs) [V] | Conservative stack scanning, pinning and transitive pinning in research builds. JuliaLang/julia#56819 reports 60–99% of objects movable on v1.9.2 [V] | The hard part was hidden references in runtime C code and JIT-generated code with embedded heap pointers. **This is exactly Patina's situation**, with `Rc` side structures, code-object constants and `CompiledMacro` literals. |
| **mmtk-v8** | Last commits 2024-09 and 2025-01 (toolchain bumps only) [V] | — | — | Effectively dormant. A JS engine retrofit stalled. |
| *(contrast)* **Whippet** (Guile) | <https://github.com/wingo/whippet>. Embed-only C library, MIT. "Feature-complete … ready to replace Guile's use of" BDW (Oct 2025) [V] | `semi`, `pcc`, `bdw`, `mmc` (Immix-derived "mostly-marking" with "nofl" space) | Conservative or precise, selectable at compile time | **The closest analogue to Patina: a Scheme that evaluated MMTk and wrote its own Immix-like collector.** Wingo: MMTk is what convinced him to work on GC, but Guile wanted C with no new toolchain dependency. Whippet's API exposes allocation, barrier and safepoint fast paths "parameterized by collector-specific attributes" so JITs can inline them (`doc/manual.md`). Its `mmc` uses one mark *byte* per 16 B granule, so the mark table is the line table and doubles as an object-start oracle for conservative roots. It has lazy sweeping, a card barrier (1 byte per 256 B, slabs 2 MB-aligned), and per-object pinning [V]. |

---

## 10. MMTk or bespoke, for Patina specifically

| Dimension | MMTk as a dependency | Bespoke Immix-family in Patina |
|---|---|---|
| **Algorithms** | Every relevant one, tested on OpenJDK, Ruby and Julia [V]. Huge head start for StickyImmix, defrag and LXR. | Must implement them. Immix itself is small: the ISMM'16 Rust Immix prototype was 1,449 LOC, 4% unsafe (Lin et al., <https://www.steveblackburn.org/pubs/papers/rust-ismm-2016.pdf>) [V]. Sticky and defrag add perhaps 2–4k LOC [I]. |
| **Threads** | GC runs on worker threads it spawns. Callbacks run on `VMWorkerThread`s. `VMBinding: Send+Sync`, `RootsWorkFactory: Send` [V]. Patina's `Rc`/`RefCell` world (environments, code objects, `VmState`) would need `unsafe impl Send` wrappers or a full re-architecture. Each GC costs at least two thread handoffs, even with `single_worker` [I]. | Runs on the mutator thread. No atomics or work packets. Can borrow `&mut` everything at a safe point (today's `GcController::safe_point` model). |
| **Instances** | One per process, fixed VA range, statics [V]. Conflicts with "both backends in one test binary" (`heap/mod.rs:339`), with parallel `cargo test` threads (each its own interpreter), and with embedding two interpreters. Would turn independent interpreters into one shared heap with global stop-the-world [I]. | Per-interpreter heap, as today. Block allocation via `mmap` per heap. |
| **Object model** | `scan_object` and `get_current_size` take an untagged `ObjectReference`, so every object needs a header encoding its type and size [V for the API, I for the consequence]. Pairs go from 16 to 24 B. A 16 B-alignment trick (bit 3 of the reference tells "pair" from "headered") is conceivable but unproven [I]. | Type comes from the reference tag, so pairs stay header-less at 16 B (Chez `cmacros.ss:1431-1433`: `pair` = car and cdr, no header [V]). Other objects carry one header word. GC bits all live in side metadata. |
| **Safe-point invariant** | Achievable: `AllocationOptions{at_safepoint:false, allow_overcommit:true}` plus a binding yieldpoint that calls into MMTk to block [V for the API]. GC is still triggered by MMTk's trigger, and the mutator must park in `block_for_gc` while a worker collects [I]. | Keep today's model: allocation raises a flag, and collection runs only at dispatch-loop safe points or JIT polls. |
| **JIT fast paths** | Possible and proven (OpenJDK). `BumpPointer` is `repr(C)`. Unlog-bit side-metadata base and shift constants are available [V]. Constants and layout can change across mmtk-core revisions, so the JIT emitter is coupled to a pinned revision [I]. | Same sequences, owned by Patina. One `#[repr(C)] GcContext { cursor, limit, unlog_base, poll_flag }` reachable from the JIT's VM-context pointer. |
| **Conservative roots and pinning** | First-class: VO bit, pinning, pinning roots, transitive pinning [V]. | Must build: object-start bitmap (1 bit per 16 B granule) plus pin bits, about 300–600 LOC [I]. |
| **Weak, ephemeron, continuation tables** | `process_weak_refs` with re-invocation fixpoint maps directly onto Patina's `trace_weak_ids` and ephemeron loop [V/I]. | Already exists (`gc.rs:1022`). Carry it over. |
| **Determinism and debug lanes** | Parallel work packets make trace order nondeterministic. MMTk has `sanity` and `extreme_assertions` features. Patina's poison-assertion differential lane would need re-plumbing [I]. | Deterministic single-threaded trace. Patina's debug poison and differential lanes extend naturally. |
| **Dependency and build** | About 30 direct crates, 2.4 MB of source, proc macros. Real bindings pin **git revs** (Ruby `9a2c733`, OpenJDK `e25ad8b`) [V]. LXR is master-only [V]. Binary size and compile time unmeasured. | No new dependencies. |
| **Platform** | macOS aarch64 is tier 2 ("guaranteed to build") since 2026-09-24 [V]. That is the owner's development platform. | Whatever Patina supports. `mmap`/`VirtualAlloc` only. |
| **Upside if Patina later goes multi-threaded (SRFI 18)** | Parallel GC and parallel mutators for free. | Would need a redesign of the allocator handoff and stop-the-world, though Immix's TLAB-of-blocks design is thread-ready [I]. |

**Verdict** [I]: bespoke. The representation rewrite (tagged pointers into a block heap,
environments and closures as GC objects, removing `Rc` payloads from `HeapObjectData`) is the
dominant cost *either way*, because MMTk also cannot manage `Vec`-arena indices or
`Rc`-graph-reachable values. After that rewrite, MMTk would save the algorithm code but
impose a global singleton, worker threads, headers on pairs, and git-rev coupling. For a
single-threaded, multi-instance, embeddable Scheme those are worse trades than writing about
5k lines of focused Immix.

Two conditions would reverse this:
- MMTk gains multi-instance support (#100).
- Patina adopts native threads.

Keep the seams compatible so that reversal stays cheap.

---

## 11. Recommended shape (Immix-family, MMTk-shaped seams) [I]

**Stage 1: representation (needed for any of this)**

- `TaggedValue` stays a 64-bit word with the 3-bit low tag. Heap tags carry an
  8/16-byte-aligned **address** instead of a `u32` index.
- Pairs are header-less, 16 B. Everything else has a single header word: type sub-tag, size or
  length, and a few VM bits.
- Flonums become 16 B (header plus `f64`). Vectors and strings are inline (header plus
  elements; strings UTF-32 or a compact encoding). Objects of 8 KB or more go to a large
  object space.
- `Rc` payloads leave the heap: `Environment`, `CompiledMacro`, `Library`, code objects'
  constants and `CallFrame.closure` either become GC-managed objects or register as explicit
  root providers.
- `eq?` stays raw-bits equality. `eq?`-hashing either pins on first hash (cheap under Immix,
  which rarely moves) or uses MMTk-style address-based hashing.

**Stage 2: non-moving Immix**

- 32 KB blocks. Pick the line size by measuring Patina's object-size histogram: 128 B per
  the paper, 256 B per MMTk.
- Mark bits and line-mark bytes in side tables, using an epoch-based mark state so no
  clearing pass is needed (MMTk lines use `RESET_MARK_STATE`/`MAX_MARK_STATE` cycling).
- Bump allocation into holes, overflow allocator for medium objects, and lazy "sweep"
  (scan line marks as blocks are acquired).
- STW marking on the mutator thread, keeping the `GcRoots` trait.

**Stage 3: Sticky Immix**

- Unlog bit in side metadata, 1 bit per 16 B granule (0.78%).
- Object-remembering post-barrier on the barrier sites listed in §7. Young objects are born
  "logged", so stores into them never take the slow path.
- Nursery GC from roots plus modbuf. Full GC by trigger or `(gc)`.

**Stage 4: opportunistic evacuation**

- Object-start bitmap plus pin bit.
- Conservative scan of anything not precisely enumerable (Rust primitive frames, early JIT
  frames), pinning those referents. Precise update of VM registers, `VmState` tables and
  globals.
- Defrag candidates from the previous GC's line-occupancy histogram, with about 2.5%
  headroom.
- Optionally evacuate young survivors, which is GenImmix-like behaviour inside Sticky Immix.

**Stage 5 (optional, measured)**: field-granularity logging (the LXR barrier shape) if
remembered-set precision becomes a problem, and incremental marking if pause tails matter.
Stop-the-world is the default.

**Seams**

- `ObjectModel`-like: size and type from tag or header, copy, forwarding in side bits plus a
  word overwrite.
- `Scanning`-like: per-type `scan`, and `GcRoots` as today.
- `Collection`-like: safepoint, trigger, out-of-memory.
- `Barrier` and `Alloc` "attributes" exported as constants and offsets for the JIT, à la
  Whippet.

This keeps an MMTk backend or LXR experiment possible without rewriting mutator code.

---

## 12. Constraints the JIT design must honour [mix; marked inline]

1. **Allocation fast path**: `cursor + size <= limit` on a `#[repr(C)]` pair at a fixed offset
   from the JIT's context pointer, with an out-of-line slow path. This is the same shape as
   MMTk's `BumpPointer` [V]. Holes make `limit` the end of the current hole, not the block.
   The slow path finds the next hole or block, or raises the GC flag [I]. MMTk's
   recommendation, generating the sequence rather than calling, is the lesson [V].
2. **Write barrier**: inline the unlog-bit test, keep the slow path cold (the 1.6% → 2.6%
   inline-slow-path result [V]), and skip it for immediate values and fresh objects [I].
3. **Safepoints**:
   - Today the interpreter polls one `Cell<bool>` per dispatch [V].
   - JIT code should poll at loop back-edges and function entry, and treat allocation slow
     paths as safepoints [I].
   - Cranelift's user stack maps treat **all non-tail calls as safepoints**. The CLIF
     producer declares which values "need stack map". Cranelift spills them to stack slots
     and **replaces later uses with reloads from the slot**. A moving collector can therefore
     update the slots (`cranelift/codegen/src/ir/user_stack_maps.rs`;
     `cranelift/frontend/src/frontend/safepoints.rs:599-795`). Values must be 16 B or smaller
     [V].
4. **Root discipline options for JIT frames**:
   - (a) Keep Scheme values in the VM register file (already precise and off the native
     stack, `execution_state.rs:18`). Only cache in SSA between non-safepoint instructions.
   - (b) Use Cranelift user stack maps for values live across calls (precise, so moving
     works).
   - (c) Scan JIT frames conservatively and pin, per Conservative Immix: 2–3% cost and about
     0.03% pinned in Java [V].

   (a) or (c) is the cheapest first JIT. (b) is the end state [I].
5. **Continuations**:
   - VM continuations snapshot `frames` and `registers` [V].
   - If the JIT captures continuations by copying native stack segments, those copies are
     heap objects. Under Immix, segments of 8 KB or more go to the non-moving LOS, and their
     contents must be scanned either with stack maps or conservatively [I].
   - Re-entering a continuation whose referents were evacuated requires the copied frames to
     be precisely scanned (updated) or their referents pinned [I].
6. **No card marking.** Header-less pairs make card scanning (heap parsing) impossible
   without an extra object-start structure. Object or field logging avoids it [I]. Whippet
   uses cards only because its mark-byte table records object extents [V].

---

## 13. Verified facts vs inference: summary

- **Verified**:
  - all paper numbers in §2–§7, from the papers' extracted text
  - every MMTk API, constant, feature, plan, platform tier, dependency, commit and date in §8
  - binding facts in §9: Ruby `Cargo.toml` and `scanning.rs`; Julia devdocs, README and
    issue; V8 commit dates; the OpenJDK barrier header
  - Patina facts in §1, with file:line
  - `size_of` numbers, measured in `PRD/study/gc/probes/immix-mmtk`.
- **Inferred**:
  - the MMTk-vs-bespoke weighting
  - the consequences of the singleton and worker-thread model for Patina's tests and embedding
  - the pair-header cost under MMTk (follows from the API, but nobody has published it for
    a Scheme binding)
  - the LOC estimates for the bespoke stages
  - all of §11 and the JIT root-discipline recommendations.
- **Not measured**:
  - MMTk's compile-time and binary-size impact on Patina
  - Patina's object-size histogram, which should choose the line size
  - barrier and allocation costs in Patina's own interpreter loop.

---

## 14. Open questions

1. **Object-size histogram.** What is the distribution of allocation sizes and lifetimes
   across chibi, r7rs-benchmarks and the compat corpus? It decides the line size (128 vs
   256 B), whether medium objects matter, and how effective Sticky Immix's nursery will be.
2. **Environments as heap objects?** If globals and top-level environments become
   GC-managed, every `define`/`set!` of a global needs a barrier. If they stay as explicit
   roots, they are rescanned each GC. The Stage 5 PRD measured root-scan growth (§9.5).
   Which costs less for Patina's hot paths?
3. **Identity hashing under evacuation.** "Pin on first `eq?` hash" or address-based hashing?
   How many objects are eq-hashed in typical programs, given SRFI 69/125 tables and
   `SourceMap` raw-bits keys?
4. **Conservative scanning of Rust frames.** Is it acceptable, given `-C force-frame-pointers`,
   stack bounds and register spilling via `setjmp`-like tricks? Or should Patina keep
   "collect only at the dispatch safe point", which keeps Rust frames GC-free and needs no
   conservative scan until the JIT exists?
5. **MMTk A/B.** Is it worth a throwaway spike that binds MMTk's StickyImmix behind the new
   seams, to obtain a measured baseline? This needs approval to download crates. It would
   also need a test mode that runs one interpreter per process, because of the singleton.
6. **Threads.** Does the roadmap include SRFI 18 or native threads? If so, the MMTk calculus
   shifts, and the bespoke allocator should use per-thread block caches from day one.
7. **Pause targets.** Is stop-the-world Sticky Immix enough, or is a latency target
   (incremental or concurrent marking, LXR-style) a requirement for REPL or embedding users?
