# Racket (BC 3m + CS-on-Chez) and Larceny collectors: lessons for Patina's GC redesign

Scope: Racket BC's precise "3m" collector (`newgc.c` and friends) and its xform
rooting; the Racket CS changes to Chez Scheme's collector; Larceny's research
collectors (generational, non-predictive, regional) and their write barrier,
remembered sets, stack cache and continuation handling. All of it is read with
Patina's three goals in mind: learn from prior art, change representations
freely, and design for a Cranelift JIT.

Sources read:
- Racket checkout `~/Project/reference/racket` (HEAD `50f1f60628`, 2026-08-29):
  `racket/src/bc/gc2/*`, `racket/src/bc/src/jit*.{c,h}`,
  `racket/src/cs/rumble/{memory,hash-code}.ss`, `racket/src/cs/main.sps`,
  `racket/collects/compiler/private/xform.rkt`.
- Chez Scheme checkout `~/Project/reference/ChezScheme` (HEAD `7d82bd86`,
  2026-09-05). Its history includes the Racket-fork GC commits, each tagged
  "Original commit: racket/ChezScheme@…".
- Larceny checkout `~/Project/reference/larceny` (HEAD `fef550c7`, 2017-09-08):
  `src/Rts/Sys/*`, `src/Rts/Shared/i386-millicode.asm`,
  `src/Rts/Standard-C/millicode.c`, `src/Asm/IAssassin/sassy-instr.sch`, `doc/`.
- Papers and pages: Clinger and Klock, "Scalable Garbage Collection with
  Guaranteed MMU", Scheme Workshop 2009
  (https://khoury.northeastern.edu/home/pnkfelix/Published/clingerklock-rgc-schemeworkshop-2009.pdf;
  I extracted the text locally); Klock's thesis page
  (https://www.khoury.northeastern.edu/home/pnkfelix/thesis); Clinger and
  Hansen, "Generational GC and the radioactive decay model", PLDI 1997 (abstract
  only); Hansen's thesis page (https://www.ccs.neu.edu/home/lth/thesis/index.html);
  Clinger, Hartheimer and Ost, "Implementation strategies for first-class
  continuations", HOSC 12(1) 1999 (abstract); the Racket blog
  (https://blog.racket-lang.org/2020/02/racket-on-chez-status.html); Racket docs
  (https://docs.racket-lang.org/inside/im_memoryalloc.html,
  https://docs.racket-lang.org/reference/garbagecollection.html); Cranelift user
  stack maps (https://fitzgen.com/2024/09/10/new-stack-maps-for-wasmtime.html).

Notation: **[V]** means verified in source or primary text, with the citation
given. **[I]** means my inference or recommendation.

---

## 0. TL;DR

1. **Collect only at safe points, never inside allocation.** Chez and Racket CS
   do this [V]: allocation only raises a "something pending" flag, and the
   collect-request handler runs at the next event check. Patina already does the
   same (`docs/GC_DESIGN.md` §6–7). Racket BC did the opposite: it collected
   inside `GC_malloc`. That forced the whole C runtime through the xform
   source-to-source rooting transformation, "park" slots for allocator
   arguments, and register-preserving retry stubs in JIT code [V]. **Keep
   Patina's model. It is what makes JIT rooting cheap.**
2. **Make the VM register file a precise runstack, and let JIT code use it.**
   BC's JIT never kept GC pointers in native frames across a GC point; Scheme
   values lived on the GC-scanned runstack [V]. Chez instead keeps values in
   native frames, described by a live mask in each return point [V]. Patina
   already has per-pc register liveness maps (`register_roots`, #423), which is
   the Chez half of the design. **[I]** A first Cranelift tier should spill to
   and reload from the register file at safe points, with no Cranelift stack
   maps. Cranelift "user stack maps" can come later for an optimizing tier.
3. **Barrier: Chez's design is the one to copy for a JIT.** It is a
   fixnum-filtered, generation-agnostic sequential store buffer (SSB) push,
   inlined in about 5 instructions, with the SSB growing down from the top of
   the thread-local allocation buffer. The buffer is drained lazily into
   per-card "youngest generation referenced" bytes [V]. Larceny's barrier
   instead compares the generation of source and target through a page table,
   does object remembering into a hash set, and is a millicode call [V]. BC's
   was mprotect/SIGSEGV page protection [V]. Both of those are worse fits for
   Rust plus Cranelift **[I]**.
4. **The old space should be mostly non-moving, with opportunistic evacuation.**
   Racket CS made Chez's copying collector mark old segments in place unless an
   earlier mark found a segment under 75% live, or its chunk under 25% used [V].
   BC marks the old generation and compacts only pages with more than a
   quarter-page of slack [V]. This combination, not "always copy" and not
   "never move", is what both Racket implementations converged on.
5. **Bounded pauses on a single thread can piggyback on minor GCs.** BC's
   incremental mode does old-generation marking in fuel-limited slices during
   each nursery collection. It uses the existing card/page barrier as an
   incremental-update barrier, and forces a non-incremental full GC if
   fragmentation exceeds 2× [V]. Larceny's regional collector shows the other
   end of the scale: provable pause and MMU bounds, at roughly 10k lines of
   C [V] and up to about 1.8× elapsed time on a near-worst-case benchmark [V].
   **[I]** Patina should do BC-style incremental marking first.
6. **Continuations should be heap objects the collector traces.** Chez's
   continuation objects are a stack segment plus a return address, walked with
   live masks; the GC trims segments to the used length [V]. Larceny makes
   frames "look almost the same in the stack and in the heap" and flushes the
   stack cache into the heap on capture and on GC [V]. Patina's weak side tables
   of `Rc<VmContinuation>` (full clones of frames and registers) are the main
   structural misfit **[I]**.
7. **Generate every traversal from one layout description.** Chez's `mkgc.ss`
   generates copy, sweep, mark, dirty-sweep, measure and self-test from one
   "Parenthe-C" description. That enabled fused GC plus memory accounting
   ("about twice as fast") and a 10–20% faster full GC on locked-object
   workloads [V]. **[I]** Patina should use a Rust derive or macro equivalent.

---

## 1. Patina baseline facts these lessons map onto

- `TaggedValue` is a u64 with a low 3-bit tag. Pair, vector, string, closure
  and object tags carry a `HeapIndex = u32` arena index, not a pointer
  (`crates/patina-core/src/tagged_value.rs:9-21,28`) [V].
- `Heap` is a set of typed `Vec` arenas: `pairs: Vec<(TV,TV)>`,
  `vectors: Vec<Vec<TV>>`, `strings: Vec<Vec<char>>`,
  `objects: Vec<HeapObjectData>`, plus free lists
  (`crates/patina-core/src/heap/mod.rs:304-376`). `SharedHeap = Rc<RefCell<Heap>>`
  (`mod.rs:51`) [V].
- Sizes measured with a throwaway crate (`PRD/study/gc/probes/racket-larceny`) on this machine (release, aarch64) [V]:
  `(TV,TV)` = 16 B; `Vec<TV>` slot = 24 B plus a separate malloc;
  `HeapObjectData` = **72 B per slot**. That covers `Real(f64)` (`mod.rs:146`)
  and `MutableCell(RefCell<TV>)` (`mod.rs:190`). `CallFrame` = 40 B;
  `VmContinuation` = 152 B plus cloned `Vec` payloads.
- The collector is stop-the-world mark-sweep with side mark bitmaps
  (`heap/gc.rs:88`) and a `GcRoots` × `Collector` trait split (`gc.rs:177,208`).
  It runs at the driver-loop safe point (`gc.rs:381`); allocation only raises a
  flag (GC_DESIGN §6). Nested loops defer through `GcDeferGuard` (§7). The
  trigger is `2 × live` [V].
- VM roots (`crates/patina-vm/src/runtime/vm_state/gc_roots.rs:70-121`) [V]:
  - the whole register file, after `retire_registers` clears slots that are dead
    according to per-pc `register_roots` bitmaps (`gc_roots.rs:47-68`,
    `types/code_object.rs:158`);
  - every loaded code object's constants (`:98`);
  - the globals `Environment` (`:101`);
  - continuation stores, traced weakly by id (`vm_state.rs:196,199`).
- Full continuations clone the entire frame vector and register vector
  (`types/continuation.rs:159-173`) [V].

---

## 2. Racket BC "3m" (`bc/gc2/newgc.c`)

### 2.1 Object model and heap layout [V]

- Pages ("apages") are 16 KB (`LOG_APAGE_SIZE 14`, 64 KB on Windows;
  `gc2_obj.h:6-10`). A two- or three-level pagemap maps an address to its
  `mpage` (`newgc.c:450-540`).
- Every small or medium object has a one-word `objhead`: 3 type bits, `mark`,
  `btc_mark` (accounting), `moved`, `dead`, a 14-bit word size, and **the rest
  of the word as hash bits** (`gc2_obj.h:11-22`). Hash bits in the header keep
  `eq?` hashing stable under moving.
- Memory kinds include tagged, atomic, array, pair, interior-array, weak box,
  ephemeron, weak array and immobile box (`gc2/README:80-205`). The runtime
  registers a size, mark and fixup procedure per type tag
  (`GC_register_traversers2`, `newgc.c:942`).
- Three size classes (`newgc.c:1150-1171`):
  - *small* objects bump-allocate in the nursery and may move;
  - *medium* objects never move and use size-segregated pages, for
    interior-pointer cases;
  - *big* objects get their own page and are promoted by relinking the page
    (`promote_marked_gen0_big_page`, `:3383`).
- A pair costs 32 B on 64-bit: an 8 B objhead, an 8 B `Scheme_Inclhash_Object`
  type/keyex header and 16 B for car/cdr (`newgc.c:1743`,
  `bc/include/scheme.h:318-359`). Chez and Larceny use 16 B headerless pairs.

### 2.2 Precise rooting from C: the shadow stack and xform [V]

- At any allocation or collection, `GC_variable_stack` points to a chain of
  frames `{next, size, &ptr1, &ptr2, …}` on the C stack. An entry of 0 means
  "(address, count) of an array" (`gc2/README:33-55`). The collector walks the
  chain and marks or fixes each slot in place (`var_stack.c:2-88`).
- Hand-written C uses `MZ_GC_DECL_REG`, `MZ_GC_VAR_IN_REG` and
  `MZ_GC_REG`/`UNREG` (Inside Racket docs). Racket's own C runtime is
  transformed automatically by **xform** (`collects/compiler/private/xform.rkt`,
  4352 lines):
  - `PREPARE_VAR_STACK` declares `void *__gc_var_stack__[size+2]` and links it
    into the chain;
  - `FUNCCALL(SETUP_…)` re-registers before each call that may collect;
  - `RET_VALUE` unlinks on return;
  - `GC_CAN_IGNORE`, `XFORM_SKIP_PROC` and `XFORM_NONGCING` are escape hatches
    (`xform.rkt:712-834`).
  - xform also splits nested call expressions so no temporary is held
    unregistered (Inside docs).
- **Copied stacks:** `GC_X_variable_stack` takes a `delta`, so the same chain
  can be walked after a C stack has been memcpy'd into the heap by continuation
  capture (`var_stack.c:2`, `bc/src/setjmpup.c:226-300`). Precise GC and
  stack-copying continuations co-exist because roots are described by position
  relative to the stack.
- **Parking:** `GC_malloc_pair(car, cdr)` saves car and cdr into `gc->park[0..1]`
  before a slow-path allocation that may collect and move them, then reloads
  them (`newgc.c:1745-1797`).
- **Immobile boxes:** `GC_malloc_immobile_box` gives non-GC memory one
  GC-updated indirection to a movable object (`README:192-205`). This is the
  "handle" mechanism for references held outside the GC'd heap.
- Hazards the docs call out: interior pointers (`SCHEME_VEC_ELS`) must not be
  held across a GC; pointers in `malloc`ed memory are invisible.

**Lesson [I].** A moving collector plus collection-inside-allocation forces
shadow-stack discipline on every host-language function. In Rust that would be
a pervasive `Root<'gc>`/handle-scope API, or a gc-arena-style generative
lifetime. Patina already avoids this: it collects only at the driver-loop safe
point and defers in nested loops. The rest of the runtime has also almost
removed Rust frames that hold values across Scheme calls (AGENTS.md: "No
primitive calls back into the program from Rust any more"). Moving objects then
needs only two things:
- an explicit handle table (immobile boxes) for the few Rust structures that
  hold `TaggedValue`s long-term: environments, `CompiledMacro`, `Library`,
  code-object constants and the source map;
- a rule that no Rust local holds a `TaggedValue` across a safe point. The VM
  already guarantees that by construction.

### 2.3 Nursery and allocation fast path [V]

- The nursery is chained pages with a bump pointer `GC_gen0_alloc_page_ptr`
  (thread-local). On overflow it moves to the next nursery page or collects
  (`allocate`, `allocate_slowpath`, `newgc.c:1594-1709`).
- Sizing (`reset_nursery`, `:2075-2093`):
  - `gen0 = 0.5 × memory_in_use + 512 KB`, capped at 32 MB;
  - initial size 4 MB;
  - 8 MB cap in incremental mode (`GEN0_MAX_SIZE / GEN0_INCREMENTAL_MAX_DIVISOR`,
    `:25-60`).
- **JIT inline allocation** (`bc/src/jitalloc.c:100-180`): load the thread-local
  pointer, then compute `((ptr-1) & (16K-1)) < 16K - size`. That tests "fits
  before the next 16 KB alignment boundary" with **no limit register and no
  limit load**, because `GC_alloc_alignment()` = `APAGE_SIZE` (`newgc.c:1847`).
  The code bumps, then stores a precomputed objhead word and type-tag word
  (`GC_initial_word`).
- On failure, the JIT calls `prepare_retry_alloc(r0, r1)`, which is xform'd C.
  It allocates filler to the boundary, possibly collecting, and returns the
  possibly-moved `r0`/`r1`; the code then retries (`jitalloc.c:31-56,298-339`).
  Live JIT registers are preserved across a moving GC by passing them as
  arguments to a rooted C function.

### 2.4 Write barrier: page protection [V]

- After a collection, old-generation pages are write-protected
  (`protect_old_pages`, defined `newgc.c:5109`, called `:5798`). The first store to a page faults. The
  handler `designate_modified_gc` (`:1070-1115`) unprotects the page, sets
  `back_pointers`, and pushes it on `modified_next`. Card granularity is the
  16 KB page; there is no barrier code in C or in JIT stores.
- A minor GC traverses every object on modified pages as roots
  (`mark_backpointers`, `:4053-4200`).
- There is a cost per first write per page per cycle. Incremental fuel charges
  100 units per page unprotect (`:3879`), so the authors treated a fault as
  expensive.

**Lesson [I].** Zero-instruction barriers look attractive for a JIT, but SIGSEGV
handling in a Rust process means unwinding-unsafe signal handlers, platform
code (BC needed `vm_osx.c` Mach exception ports, 557 lines), and coarse 16 KB
cards. Racket CS dropped this approach for Chez's software barrier. Not
recommended.

### 2.5 Old generation: mark plus selective compaction [V]

- A full GC marks gen1 in place (header mark bit; recursion depth 5, then a
  1 MB-segmented mark stack, `:80-85`).
- It then compacts only pages where `live < size - PREFIX - APAGE_SIZE/4`.
  Every other major GC uses an OS-level fragmentation heuristic instead
  (`tic_tock`, `do_heap_compact`, `:4235-4350`).
- Moved objects leave a forwarding address. A separate **fixup pass** rewrites
  every reference: roots, the variable stack, immobile boxes and the heap
  (`repair_roots`, `GC_fixup_variable_stack`, `repair_heap`, `:5729-5744`).
- Full GC trigger (`:5515-5552`):
  - `memory_in_use > 2 × max(last_full_mem_use, 20 MB)`;
  - or more than 1000 minors since the last full GC;
  - or a finalization-triggered extra full GC;
  - or fragmentation over 2× in incremental mode.

### 2.6 Incremental mode: old-gen marking piggybacked on minor GCs [V]

- `(collect-garbage 'incremental)` (or `PLT_INCREMENTAL_GC`) makes later minor
  GCs also advance an old-generation mark (`garbage_collect`; the decision is at `:5590-5612`, the slice runs at `:5680-5706`).
- Each minor GC runs `propagate_incremental_marks` with fuel
  `4096 × (memory_in_use / 100 MB)`. Each popped object costs
  `1 + copied/4 + traversed/4`, plus 100 per page unprotect (`:3849-3893`).
  When the mark stack drains, finalization runs incrementally. "Repair"
  (sweep/fixup) also proceeds incrementally at 32 units per 100 MB
  (`incremental_repair_pages`).
- **Barrier semantics:** the same page-protection remembered set serves as an
  *incremental-update* barrier. A modified page's already-marked objects are
  re-traversed (`GC_CURRENT_MODE_BACKPOINTER_REMARK`), and incrementally marked
  pages join `inc_modified_next` (`:3353-3376`, `:4093-4170`).
- While incremental, gen0 survivors go to a "gen ½" aging space instead of
  straight to gen1 (`AGE_GEN_0_TO_GEN_HALF`, `:49`). Compaction is skipped once
  incremental marking finished (`do_heap_compact` returns early, `:4250-4251`; called at `:5714`). If
  fragmentation exceeds `HIGH_FRAGMENTATION_RATIO 2`, a non-incremental full GC
  is forced (`:5545-5552`, recomputed at `:5804`).
- Docs: incremental mode "may imply longer minor-collection times and higher
  memory use" (reference manual, Garbage Collection).

**Lesson [I].** For a single-threaded interpreter this is the cheapest route to
bounded major pauses. There are no concurrent mutator threads, so marking needs
no atomics. The mutator is stopped while a slice runs, so the only invariant
the barrier must keep is "a modified card is re-scanned before marking
finishes". A card barrier Patina adds for generational GC delivers that for
free (incremental update, Steele/Dijkstra style). The final remark is bounded by
dirty cards plus roots. The fallback to a full, possibly compacting, GC when
fragmentation passes 2× is a good safety valve.

### 2.7 Weak references, ephemerons, finalization [V]

- Ephemerons waiting on an unmarked key are registered as triggers on the key's
  **page**. When an object on that page is marked, the page's waiting
  ephemerons are re-queued (`weak.c:555-597`).
- Chez does the same per segment (`trigger_ephemerons` in `seginfo`,
  `types.h:162`; `gc.c` header comment "Pending Ephemerons and Guardians").
- This replaces a global fixpoint loop over all ephemerons. **[I]** Patina's
  continuation-table and ephemeron fixpoint should become per-page or
  per-segment triggers once objects live in pages.
- BC has ordered finalization levels 1, 2 and 3, and late weak boxes cleared
  after level-2 finalization (`README:207-245`).

### 2.8 Places and memory accounting [V]

- Each place has its own `NewGC` (nursery plus old generation) and collects
  independently. A `MASTERGC` holds shared objects and needs a rendezvous
  (`places_gc.c:1-30`).
- Place messages are allocated in a private "message allocator" whose pages are
  then *adopted* by the receiver's heap (`GC_adopt_message_allocator`,
  `newgc.c:2265`).
- Custodian accounting ("BTC", blame-the-child, `mem_account.c`) is an extra
  mark pass after a full GC. It marks each owner's roots in order and charges
  each object to the first owner that reaches it. It uses a `btc_mark` header
  bit and special mark procedures for threads, custodians and ephemerons
  (`mem_account.c:10-14`).
- **[I]** A Patina heap per interpreter instance (today's `SharedHeap`) is the
  places model. Keep heaps independent so a future multi-isolate design does
  not need a global stop.

### 2.9 How BC's JIT interacted with the GC [V]

- Scheme values live on a separately allocated, GC-scanned **runstack**
  (`MZ_RUNSTACK`, a register `JIT_RUNSTACK`). Native frames hold only non-GC
  words:
  - LOCAL1 is a continuation-mark-stack offset;
  - LOCAL2 holds "some pointer, never to stack or runstack";
  - a "flostack" region holds unboxed flonums (`bc/src/jit.h:786-830`).
- Constraint stated for calls that may capture a lightweight continuation:
  "JIT_V1 does not contain a value that needs to change if the runstack moves
  (Other JIT constraints imply that it isn't a pointer to GCable memory.)"
  (`jit.h:1125-1150`).
- As a result, BC's JIT needed **no stack maps**. The cost: every live Scheme
  value is a memory slot at every potential GC point.

---

## 3. Racket CS: what Flatt changed in Chez's collector

### 3.1 Chez baseline that Racket CS inherits [V]

- Segments are 16 KB on 64-bit with 512 B cards, so 32 cards per segment
  (`s/cmacros.ss:2142-2149`). Each segment's `seginfo` has `space`,
  `generation`, `old_space`/`use_marks`/`must_mark` flags, `marked_mask`,
  `marked_count`, ephemeron/guardian triggers, and
  **`dirty_bytes[cards_per_segment]`** (`c/types.h:143-182`). A dirty byte
  holds the *youngest generation* the card may point to; 0xff means clean.
- **Compiled write barrier** (`s/cpprim.ss:665-723` `build-dirty-store`, and
  `s/cpnanopass.ss:6807-6831`):
  1. skip if the stored value is statically a fixnum, boolean or immediate;
  2. otherwise do the store, then a run-time fixnum test;
  3. then `remember`: `if (ap < eap) ; else scan-remembered-set;
     eap -= 8; *eap = &slot`.
  - The SSB is the **top end of the thread's allocation buffer**, growing down
    toward `ap`. The barrier makes no generation test.
  - `S_scan_dirty` drains the buffer, consecutive-card dedupe included. It sets
    the card's dirty byte to 0 if the slot's segment is not generation 0
    (`c/alloc.c:440-460`). `S_scan_remembered_set` runs when buffer and
    allocation meet (`alloc.c:466-491`).
  - The C-side barrier `S_dirty_set` (`alloc.c:396-414`) is the
    non-compiled-code equivalent.
- **Collection happens only at safe points.** `S_fire_collector` sets
  `collect-request-pending` and `SOMETHINGPENDING` on every thread
  (`c/schsig.c:574-594`). The `collect-request-handler` runs at the next event
  check, and allocation never collects.
- Stack frames are walked through the return address's **return-point header**:
  frame size plus a live mask (a fixnum, or a bignum for big frames)
  (`s/mkgc.ss:1037-1060`, `trace-stack`). The GC copies the thread's stack
  segment if it is in old space (`trace-tc`, `mkgc.ss:963-985`).

### 3.2 GC generated from a description: `mkgc.ss` (Flatt, 2020-03-31) [V]

Commit `37a515ca` (racket/ChezScheme@5af3877b): "Replace repetitive C code in
gc.c and vfasl.c with … a somewhat declarative description of object tracing.
From that description, we generate different kinds of tracing functions, such
as the copy function or the sweep function." It also added a generated
`compute-object-sizes` traversal, and "the GC can now perform a fused `collect`
and `compute-object-sizes` in a single traversal". Improved locked-object
detection gave "on the order of 10-20% for a full collection". `sweep_dirty_object`
was generated the same way later (`5f613b5e`). Today `mkgc.ss` is 2589 lines and
produces `gc-ocd.inc`, `gc-oce.inc` and `gc-par.inc` (`c/gc.c:24-31`).

### 3.3 Mark-in-place (non-copying) mode (`5948180b`, 2020-04-18) [V]

- Rationale from the commit: "mark and sweep objects in-place, instead of
  always copying … helpful for reducing peak memory use while performing a
  collection on a large, old heap."
- The commit also introduced **immobile allocation** for bytevectors, vectors
  and boxes: objects that never move but can still be collected. Old-style
  locking became "immobile + global list".
- Decision per segment at GC start (`c/gc.c:1008-1036`): mark in place if
  `must_mark` (pinned/immobile), or if the segment is in a generation
  ≥ `min_mark_gen` (default = max nonstatic generation = 4;
  `gcwrapper.c:59`, `cmacros.ss:2123`) and
  - the segment was not found sparse by a previous mark
    (`marked_count ≥ ¾ × segment`, `gc.c:486`), and
  - its chunk is not sparse (`nused_segs ≥ segs/4`, `gc.c:487`).

  Otherwise the segment's objects are copied (evacuated).
- Marked segments stay in place and move to the target generation. A
  reachability test is "in an `old_space` segment and (starts with
  `forward_marker` or has its mark bit set)" (`gc.c` header comment, lines
  34-90).
- Racket CS exposes `PLT_MAX_COMPACT_GC`, which sets
  `in-place-minimum-generation 254` and so always copies (`cs/main.sps:918-919`).
- **[I]** This is effectively Immix-style "mark region, evacuate sparse
  regions", decided per 16 KB segment from the previous cycle's live counts.
  It is a good template for Patina's old space once objects live in pages.

### 3.4 Incremental promotion (dyb, `a1422364`, 2020-06-22) [V]

- "the collector now promotes objects one generation higher at a time by
  default … previously … objects prematurely skipping one or more generations."
  Cost: "recording pointers from older newspace objects to younger newspace
  objects".
- The commit also added a specialized collector, `gc-011.c`, for the case
  "max copied generation 0, target 1, no locked gen-0 objects", which accounts
  for "3/4 of all collections".
- The trigger became a cheap count of "generation-bytes allocated since the
  last gc".
- **[I]** Specializing the minor collection (nursery to gen1, no pinning) is a
  cheap, large win, because most collections are that case.

### 3.5 Parallel collection (`02f36145`, `f27ab8c4`, `badd699e`, 2020-09) [V]

- "All allocation is now thread-local." Only the sweep phase is parallel.
- Segments have an owner (their creator). Only the owner copies or marks
  objects in its segments. A sweeper that finds a reference into another
  sweeper's segment sends the *referring object* to that owner for re-sweep
  ("messages instead of locks"). An object is swept at most about N times for
  N sweepers (`c/gc.c:126-198`).
- The dirty-card sweep was also moved into the parallel phase.
  Memory-accounting collections stay single-threaded.
- **[I]** Not needed for single-threaded Patina now. The relevant constraint is
  to keep per-segment ownership and thread-local allocation buffers in the data
  structures from day one, so parallel sweeping is possible later.

### 3.6 Racket CS collection policy (`cs/rumble/memory.ss`) [V]

- The nursery is `collect-trip-bytes = allocating-places × 8 MB` (`:35-39`).
- Minor generation choice uses a radix-4 counter. Generation *n* is collected
  every 4^n minors (`log-collect-generation-radix 2`, `:42-47,85-93`).
- **Major GC trigger** (`:50-56,141-160`):
  - fires when `bytes-allocated ≥ trigger` or `allocated+overhead ≥ trigger'`,
    or after 10 000 non-full GCs;
  - after each major GC the triggers become `post + 8192·√post`, applied both
    to live bytes and to bytes including allocator overhead;
  - initial triggers are 32 MB and 64 MB;
  - the rule cites Kirisame, Shenoy and Panchekha 2022 ("optimal heap limits",
    https://arxiv.org/abs/2204.10455).
  - Worked numbers [I, arithmetic]: post = 10 MB gives about 3.6× headroom,
    100 MB about 1.8×, 1 GB about 1.25×. Patina's flat `2 × live` gives the
    same multiple at every size.
- **"Incremental" on CS is not incremental marking.** It collects
  `gen = max-1` into itself without promoting to the max generation, which
  postpones major GCs (`:120-124`). Racket gave up true incremental marking
  when it moved to Chez.
- Accounting is fused into the major GC: `(collect gen 1 gen roots)` returns
  per-root sizes (`:104-118`; csug `smgmt.stex:196-206`).

### 3.7 `eq?` hashing under a moving collector [V]

- Chez's eq-hashtables key on address. Each entry is a "tlc" (transport link
  cell). When a key moves, the GC puts the tlc on `tlcs_to_rehash` and rehashes
  only those entries after collection (`c/gc.c:1734-1773`,
  `s/mkgc.ss:788-809`).
- Racket CS `eq-hash-code` uses a weak eq-hashtable of counters
  (`cs/rumble/hash-code.ss:19-30`).
- BC stores hash bits in the object header (§2.1).
- **[I]** All three are viable. Header hash bits are simplest if Patina adds
  object headers. GC_DESIGN §3.4 lists `eq?` hashing on raw bits as a blocker
  for moving; it is a solved problem.

### 3.8 Continuations in the Chez/Racket CS GC [V]

- A continuation object holds a stack segment pointer, `stack-length`, the used
  length `clength`, a return address, a `link` to the next continuation, plus
  winders and attachments (`s/mkgc.ss:217-269`).
- Sweeping it copies only `clength` of the segment (`copy_stack`,
  `c/gc.c:803`) and walks frames with `trace-stack` live masks.
- One-shot continuations are not promoted; opportunistic one-shots are
  converted to full continuations at GC (`copy-stack-length`, `mkgc.ss:728-742`).
- The per-thread stack cache is discarded at each GC (`tc-stack-cache ← nil`,
  `mkgc.ss:987`).
- Racket's 2020 status report measured generator/continuation-heavy code at
  ×335 versus an `in-range` baseline on CS, against ×2672 on BC (blog post
  above). BC copied C stacks; CS splits segments.

---

## 4. Larceny

### 4.1 Framework [V]

- "Multiple collectors in separate heaps", totally ordered, with the young heap
  special because "it must manage the stack also"
  (`doc/LarcenyNotes/noteX-newgc.html`).
- A descriptor table maps 4 KB pages to attributes, including generation
  number. The non-predictive collector *renumbers generations after a
  collection*, and the barrier reads the table, so renumbering is how semispace
  roles swap.
- There is a static area for the heap image. It acts as the oldest generation,
  is never collected, and is covered by the remembered set (`static-heap.c`;
  the barrier filter `G_FILTER_REMSET_RHS_NUM` skips stores whose target is
  static, `memmgr.c:3189,3829`).
- Three systems: generational, stop-and-copy and regional (`memmgr.c:93-110`),
  plus a Boehm build.
- Defaults (`doc/UserManual/starting.txt:55-62`): 1 MB nursery, 2 MB ephemeral
  two-space area, dynamic area with load factor 3.0, 16K remset hash entries,
  16K SSB slots.

### 4.2 Write barrier and remembered sets [V]

- Two entry points, both millicode routines called from compiled code
  (`Shared/i386-millicode.asm:281-410`; C reference at
  `Standard-C/millicode.c:798-827`):
  - `mc_full_barrier` checks `isptr(rhs)`, then falls through;
  - `mc_partial_barrier` reads `gl = genv[page(lhs)]` and `gr = genv[page(rhs)]`
    and returns if `gl <= gr`. Otherwise it appends the **lhs object** (object
    remembering, not slot remembering) to the SSB of generation `gl`, and calls
    `gc_compact_all_ssbs` on overflow.
- SSB entries drain into a fixed-size chained hash table, which removes
  duplicates. The node pool is sized so scanning costs time proportional to the
  number of remembered objects (`remset.c:1-55`, after Hosking, Moss and
  Stefanovic, OOPSLA '92).
- `noteX-newgc.html` notes the cost model: a remset add happens "no more than
  once per object per time the generation the object is in is garbage
  collected", so the add path can be "comparatively expensive".
- **Combined SATB:** when `G_CONCURRENT_MARK` is set, the full barrier also logs
  the *old* value of the slot into a SATB SSB, for Yuasa-style snapshot marking
  (`i386-millicode.asm:282-330`).
- Regional mode (`allocate_regional_system`, `memmgr.c:3880-3905`) sets
  `G_FILTER_REMSET_GEN_ORDER = FALSE`. Every region-crossing store is
  remembered, in either direction, except when the lhs is in the nursery
  (`G_FILTER_REMSET_LHS_NUM`) or the rhs is static.

### 4.3 Non-predictive collector (Clinger and Hansen, PLDI 1997) [V]

- From the abstract: for programs whose lifetimes resemble radioactive decay, a
  conventional generational collector "concentrates effort on collecting
  younger generations which contain an unusually low percentage of garbage", so
  it does worse than a non-generational collector. This motivates older-first
  collection.
- `np-sc-heap.c:1-160,255-289`: the dynamic area has *k* steps split into "old"
  (k−j) and "young" (j) semispaces. Promotions fill old first, then young. A
  COLLECT copies the *old* part. Growth phases are detected from remset size
  history, and fudge factors include "luck" and the NP remset limit.
- Hansen's thesis (2000): older-first works well for "queue-like or random
  lifetimes" and large live heaps, but is brittle on large linked structures
  and needs fallbacks (thesis page).
- **[I]** For Patina this is mostly a policy caution. Uniform-random or
  queue-like lifetimes (memo tables, long-running REPL state) defeat
  young-first generational GC, so measure survival before tuning nursery size.

### 4.4 Regional collector (Clinger and Klock 2009; Klock thesis 2011) [V]

- **Processes** (paper §2.1):
  - *collection*: Cheney copy of one region at a time, the only process that
    moves objects;
  - *summarization*: incrementally builds points-into "summary sets" for a
    batch of regions from the points-out-of remembered set;
  - *SATB marking*: Yuasa snapshot marking that **refines** the remset,
    removing entries from dead objects. It also collects cyclic cross-region
    garbage and measures live volume.
  - Summarization and marking run interleaved with the mutator, at minor-cycle
    granularity in the prototype.
- **Remembered set**: "at most one entry for each location in the heap", so its
  size is bounded by the heap. It is directed *out of* regions, because
  per-region into-sets would be quadratic (§2.2–2.3).
- **Popular regions**: if a summary set exceeds a threshold, the region is
  "waved off" and left uncollected. Lemma 3 bounds the total volume of popular
  regions.
- The marker's mark stack is threaded per region, so a collection scans only
  its region's slice (§2.6).
- **Barrier**: "logs three things: (1) the location on the left hand side …,
  (2) its previous contents, and (3) its new contents" (§2.7).
- **Stacks**: the collector assumes heap-allocated, bounded-size frames.
  Incremental stack/heap and Hieb-Dybvig-Bruggeman stack caches are "regarded
  as special parts of the nursery" (§2.8).
- **Results** on Clinger's queue benchmark (near worst case; 1M-element lists
  of two-element vectors in a circular buffer; 1 MB nursery, 8 MB regions):

  | Live set | Collector | Elapsed | GC time | Max pause | RSS |
  |---|---|---|---|---|---|
  | ~160 MB | Larceny regional | 192 s | 170 s | **0.07 s** | 386 MB |
  | ~160 MB | Larceny generational | 109 s | 88 s | 0.80 s | 555 MB |
  | ~160 MB | Larceny stop&copy | 76 s | — | 0.90 s | — |
  | ~160 MB | PLT 4.1.4 generational | — | — | 1 s | — |
  | ~800 MB | Larceny regional | — | — | 0.11 s | 1808 MB |
  | ~800 MB | Larceny generational | — | — | 4.2 s | — |
  | ~800 MB | Sun JVM 1.5 parallel | — | — | 4.2 s | — |
  | 800 MB + 50 popular objects | Larceny regional | 618 s | — | 0.35 s | 1865 MB |
  | 800 MB + 50 popular objects | Larceny generational | 162 s | — | 4.5 s | — |

  (Figures 2–4. Dashes are values I did not extract.) The thesis claims
  throughput "comparable to a tuned generational collector on a set of
  fifty-eight non-collection-intensive benchmarks".
- **Engineering cost**: the regional pieces (`summ_matrix.c`, `smircy*.c`,
  `region_group.c`, `summary.c`, `uremset*.c`, `extbmp.c`, `locset.c`,
  `seqbuf.c`) total **10 135 lines**. The whole classic generational machinery
  (`nursery`, `cheney`, `sc-heap`, `old-heap`, `remset`, `barrier`, `los`,
  `stack`) is **4 757 lines**.
- **[I]** This is a single-threaded, copying, G1-like design with proofs, and a
  useful upper bound on what bounded latency costs without a read barrier. For
  Patina the BC-style approach (§2.6) gets most of the latency win for a
  fraction of the code. The two ideas worth keeping are a single barrier that
  serves both remembered-set and snapshot needs, and remembered-set refinement
  by the old-gen marker.

### 4.5 Stack cache and continuations [V]

- `stack.c:1-80`: "The stack lives at the high end of the current ephemeral
  area … the stack pointer is the heap limit, and vice versa, saving
  registers", and "the stack can be flushed in-place". Disadvantage: "the
  stack cache must be flushed to the heap on a gc".
- Frames "look almost the same in the stack and in the heap": a header/size
  word, return address, dynamic link and saved REG0. On flush, a return
  address becomes a **byte offset into the code vector of the procedure in
  slot REG0**, so code can move.
- Continuation capture is a flush (`stk_flush`, `stack.c:125`). Return through
  an empty cache triggers `refill_stack_cache` on underflow, which restores
  frames lazily (`Standard-C/millicode.c:829-840`). This is the "incremental
  stack/heap" strategy that Clinger, Hartheimer and Ost recommend (HOSC 1999).
- The compiler must keep frames GC-scannable at every GC point.
  `ia86.t_save0` allocates the frame and "initialize[s] only the basic slots"
  (`src/Asm/IAssassin/sassy-instr.sch:639-660`), and Twobit's `store`
  instructions fill the rest before any call.

### 4.6 Large objects and instrumentation [V]

- The large-object space uses a 4-word pre-header and doubly linked per-gen
  lists. Promotion relinks the object and never copies it (`los.c:1-60`).
- `gc_mmu_log.c` (699 lines) records minimum mutator utilization at run time
  (`-mmusize`). The regional work was driven by measured MMU, not mean pause.

---

## 5. Lessons and constraints for Patina's redesign

### 5.1 Root discipline: keep "collect only at safe points" and extend it to JIT code

- **[V]** Chez and Racket CS never collect inside allocation (§3.1). BC did, and
  paid with xform, park slots and register-preserving retry stubs (§2.2–2.3).
- **[I] Constraint:** allocation, including JIT inline allocation, may refill a
  buffer or grab a new segment but must **never collect**. It may only raise
  the pending flag.

  Collections happen at these safe points:
  - the interpreter loop top (today);
  - JIT function entries and loop back-edges (a Chez-style event counter or flag
    poll);
  - calls out of JIT code into the runtime.

  If a nursery refill fails because memory is exhausted, the slow path may
  over-allocate a fresh segment, Chez-style, and let the next safe point
  collect.
- **[I] Constraint:** at every safe point, every live `TaggedValue` must be in a
  GC-visible slot: the register file, a frame slot described by a liveness map,
  or a handle. Patina's register file plus `register_roots` already satisfies
  this for the VM.
- For a baseline JIT, the cheapest correct scheme is BC's: JIT code may hold
  values in machine registers between safe points, but writes them back to the
  register-file slots before any safe point. After a possibly moving GC it
  reloads them. No Cranelift stack maps are needed (§2.9).
- An optimizing tier can move to Chez-style per-safepoint maps through
  Cranelift **user stack maps**. The CLIF producer declares GC-ref values; at
  each safepoint Cranelift spills them to stack slots and records a map. A
  moving GC rewrites those slots and the compiled code reloads them (fitzgen,
  2024).
- **[I]** Rust holders of `TaggedValue` outside the heap must become one of:
  - heap objects themselves;
  - handles into a GC-updated table (BC immobile boxes);
  - immortal-set entries (Larceny's static area, Patina's planned immortal
    set).

  This list covers environments, `CompiledMacro`, `Library`, code constants,
  source-map keys and the symbol table. It is the prerequisite for any moving
  or compacting collector, and matches the blockers in GC_DESIGN §3.4.

### 5.2 Barrier design for an interpreter plus a Cranelift JIT

- **[V]** Three designs seen:
  - BC: page protection, zero instructions, signals, 16 KB granularity.
  - Larceny: generation comparison through a page table, object remembering,
    SSB plus hash set, millicode call.
  - Chez: value filter, unconditional slot-address SSB push into the allocation
    buffer's tail, lazy card processing.
- **[I] Recommendation:** Chez's barrier, possibly with a cheap
  young-object filter. In Rust and CLIF it is: `store; if !is_fixnum(v)
  { if (eap <= ap) slow(); eap -= 8; *eap = addr }`.
  - The interpreter and JIT emit the same sequence.
  - It is generation-agnostic, so generation renumbering, aging spaces and
    regions do not change compiled code.
  - Draining into per-card "youngest generation referenced" bytes gives minor
    GCs precise old-to-young roots. The same dirty cards drive the BC-style
    incremental-update remark (§2.6). Larceny's full barrier shows the SATB
    variant: log the old value as well, if snapshot marking is preferred.
  - Patina stores that need the barrier: `set-car!`/`set-cdr!`, `vector-set!`,
    `MutableCell` writes, closure-slot writes, record field sets, global
    definitions, and continuation-frame stores if frames become heap objects.
  - Initializing stores into freshly allocated nursery objects need no barrier.
  - Chez skips the barrier when the stored value is statically immediate
    (`cpprim.ss:676-688`). The Patina compiler can do the same with its type
    information.

### 5.3 Allocation fast path

- **[V]** All three systems bump-allocate in a thread-local nursery: BC 16 KB
  alignment check, Chez `ap`/`eap`, Larceny heap pointer against the stack
  pointer.
- **[I]** Patina's `Vec`-arena-plus-free-list allocation (`heap/mod.rs:705`
  pops `free_pairs`) cannot be inlined by a JIT and has no young/old
  distinction. A JIT-ready allocator needs, in the JIT context:
  - contiguous nursery segments;
  - `ap`/`eap` in a fixed-offset context struct (or pinned registers);
  - a precomputed header word per object kind;
  - a slow-path call.

  BC's alignment trick (no limit load) and Chez's shared alloc/SSB buffer are
  two cheap ways to avoid an extra limit register.

### 5.4 Representation changes the prior art implies

- **[V]** Chez and Larceny pairs are 2 words with no header; type comes from the
  pointer tag and the segment's space. Patina's `(TV,TV)` arena slot is already
  16 B. In contrast, every `HeapObjectData` slot is 72 B, even a boxed flonum
  or a `MutableCell`, and vectors and strings keep their payload in a separate
  malloc (measured, §1).
- **[I]** For a moving or generational heap, the natural shape is:
  - pointer-tagged references (TaggedValue payload = address or segment-relative
    offset, not a per-type arena index);
  - headerless pairs, and probably headerless boxes/cells, in pair-like spaces;
  - a one-word header for everything else, holding type, size and hash bits,
    like BC's `objhead` (`gc2_obj.h:11-22`);
  - inline vector, string and closure payloads;
  - large objects (above roughly ¼ segment) in a non-moving large-object space,
    promoted by relinking (BC big pages, Larceny `los.c`);
  - per-segment metadata (space, generation, mark bitmap, dirty bytes, owner,
    ephemeron triggers), Chez `seginfo`-style.
- **[I]** Kinds whose payloads must stay Rust-owned (ports, `BigInt`, foreign
  objects) can stay as an "external" object kind. Those are atomic from the
  GC's viewpoint except for explicit trace hooks; Chez's phantom bytevectors
  and BC's atomic-tagged kind are precedents.

### 5.5 Old space: mostly non-moving, evacuate the sparse parts

- **[V]** Racket CS and BC both landed here (§2.5, §3.3). Pinning is per
  segment (`must_mark`, up to "infinity" = 3) or per object (immobile
  allocation).
- **[I]** This removes the need to rule out moving permanently. Only nursery
  survivors and sparse old segments move, so pinned objects and conservative
  interop are segment-local exceptions, not global blockers. Code objects and
  anything the JIT embeds as an absolute address go in an immobile space, or
  are referenced through a constant table the GC updates.

### 5.6 Bounded pauses for a single-threaded Scheme

- **[V]** BC incremental mode: fuel-limited old-gen marking during minor GCs,
  incremental-update via dirty cards, aging space, compaction off while
  incremental, a fragmentation fallback, and incremental finalization and
  repair (§2.6).
- **[V]** Larceny regional: provable MMU, at about 2× code and about 1.8×
  elapsed time on the near-worst case (§4.4).
- **[V]** Racket CS gave up true incremental marking: "incremental" there only
  delays promotion (§3.6).
- **[I] Recommendation:** in order,
  1. generational (nursery, then old);
  2. a specialized fast minor collection (Chez `gc-011` covered 3/4 of
     collections);
  3. BC-style incremental old-gen marking piggybacked on minors;
  4. lazy or incremental sweep;
  5. occasional evacuating full GC under a fragmentation trigger.

  Collect MMU data (Larceny `gc_mmu_log.c`), not just max pause.

### 5.7 Continuations

- **[V]** Chez: captured continuations are heap objects that own a stack
  segment, are traced with live masks, and are trimmed to the used length.
  Larceny: frames are heap-shaped, the stack is flushed on capture and on GC,
  and restore is lazy on underflow. BC: C-stack copies traced through relative
  shadow-stack chains.
- **[I]** Patina's `VmContinuation` is a full `Vec<CallFrame>` plus `Vec<TV>`
  clone in an `Rc` in a weak side table keyed by `u64` id
  (`vm_state.rs:196-199`, `gc_roots.rs:1-30`). That gives O(depth) capture and
  forces the special weak-id fixpoint. Better:
  - make the continuation a real heap object (frames plus register segment)
    traced by the normal tracer, so it is reclaimed naturally and its
    ephemeron-like handling disappears;
  - represent the stack as linked segments, so capture splits rather than
    copies (Chez) or flushes (Larceny);
  - give JIT frames the same segment-walkable layout (return address, then
    liveness map), so capture across JIT frames is a segment split and GC
    scanning uses the same maps.

  Code references inside frames should be code-relative offsets (Larceny's
  flush rule), so code objects may move or be evicted safely.

### 5.8 Tooling and engineering

- **[V]** `mkgc.ss` generates every traversal from one layout description and
  made fused accounting possible (§3.2). BC registers size, mark and fixup per
  tag. Larceny's framework kept collectors pluggable behind a heap interface.
- **[I]** In Rust, use a derive or macro over object layouts that generates
  trace, update-references, card-scan, size and verify, plus a debug "self
  test". It replaces today's hand-written `for_each_*` tracers, which GC_DESIGN
  §9.4 shows are easy to get exponentially wrong.

### 5.9 Policy numbers worth starting from

- [V] Nursery sizes: Racket CS 8 MB per allocating thread; BC
  `0.5×heap + 512 KB` capped at 32 MB; Larceny 1 MB.
- [V] Generation schedule: Chez/Racket CS radix 4.
- [V] Full-GC trigger: BC 2× after last full GC (20 MB minimum); Racket CS
  `post + 8192·√post`.
- [V] Forced full GC: BC after 1000 minors; Racket CS after 10 000.
- [V] Evacuation threshold: Chez segment under 75% live or chunk under 25%
  used; BC page slack over ¼ page.
- [V] Fragmentation fallback: BC 2×.

---

## 6. Open questions

1. **Value encoding.** Should TaggedValue payloads become raw addresses
   (Chez/Larceny/BC) or stay as compressed segment-relative 32-bit offsets
   (cheaper `eq?` and smaller fields, but an add on every dereference)? This
   decides whether JIT code can embed object addresses and how cards are
   indexed.
2. **Barrier filter.** Chez's barrier has no generation check, and filters
   cards at drain time. Is that right for Patina's store mix? Mutation-heavy
   Scheme (vectors, `set!`-boxed variables, which Patina boxes heavily as
   `MutableCell`) may fill the SSB fast; an inline "is the target in the
   nursery?" check costs a segment-table load. Measure with the interleaved A/B
   method.
3. **JIT frame representation.** Should JIT code (a) mirror values into the
   register file at every safe point (BC), or (b) keep them in native frames
   with Cranelift user stack maps (Chez)? (a) is simplest and makes
   continuation capture uniform. (b) is faster but needs segment-walkable
   native frames for `call/cc`. A hybrid where a baseline tier does (a) and an
   optimizing tier does (b) needs a deopt or OSR story.
4. **Incremental marking barrier.** If BC-style incremental marking lands,
   should it be incremental-update (re-scan dirty cards, reusing the
   generational barrier) or SATB (Larceny: also log old values)? SATB bounds
   the final remark better but doubles barrier logging.
5. **Environments.** Should environments and global bindings become heap
   objects (records or vectors indexed by compiled slot) rather than
   `FxHashMap<String, TV>` behind `Rc`? Every design studied keeps top-level
   bindings in GC-managed memory (Chez symbol value slots, BC buckets), which
   the barrier and moving GC require.
6. **Continuation re-entry.** How much of Patina's dynamic-state machinery
   (winds, prompts, handlers, `VM_RUNTIME` §5.6) must move into heap-allocated
   segments for capture to become O(1)? Is the control-flow matrix in
   `control_flow_matrix.rs` sufficient to validate a segment-split rewrite?
7. **Accounting.** Does Patina need custodian-like memory accounting, or
   per-library usage, for sandboxes and REPL limits? If so, design the
   generated tracer to support a fused accounting traversal (Racket CS) from
   the start.
8. **Symbols.** Are symbols immortal or weak? Chez sweeps unreachable
   symbols out of its oblist during collection (`c/gc.c:1275-1290`). Patina's symbol table maps names to raw indices
   (GC_DESIGN §9.2). A moving collector must update it or make it weak.
