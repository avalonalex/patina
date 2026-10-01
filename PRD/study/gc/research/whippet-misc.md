# Prior art for Patina's GC redesign: Whippet, Go, .NET, Lua/LuaJIT, Perceus/Lean RC

Research key: `whippet-misc`. Date: 2026-09-30. Read-only; nothing in the repo was changed.

**Evidence labels.** **[src]** means I read it in source code (Patina at `main` 28a94f8, or upstream files fetched raw from GitHub on 2026-09-30). **[doc]** means a primary document or author blog, read through a summarising fetch. Numbers marked [doc] are as the author states them; I re-quote only short phrases. **[meas]** means I measured it in a throwaway crate (`PRD/study/gc/probes/whippet-misc`). **[inf]** means my inference. Upstream line numbers refer to the files as fetched today: `wingo/whippet@main`, `golang/go@master`, `dotnet/runtime@main`, `lua/lua@v5.4.7` and `@master`, `leanprover/lean4@master`, and the local `~/Project/reference/koka` at 3ac4f00 (2026-08-15).

---

## 0. Patina baseline facts this comparison leans on

- `TaggedValue` is a `u64` with a 3-bit low tag. Heap references are `u32` arena indices (`HeapIndex = u32`) under the pair/vector/string/closure/object tags (`crates/patina-core/src/tagged_value.rs:28,57,70-84`) [src].
- `Heap` is a set of typed `Vec` arenas: `pairs: Vec<(TV,TV)>`, `vectors: Vec<Vec<TV>>`, `strings: Vec<Vec<char>>` and `objects: Vec<HeapObjectData>`, each with a LIFO `Vec<HeapIndex>` free list (`heap/mod.rs:304-376`) [src]. `SharedHeap = Rc<RefCell<Heap>>` (`heap/mod.rs:51`) [src].
- Slot sizes, measured with `std::mem::size_of` in that crate on arm64 with the pinned toolchain [meas]:
  - a pair slot is 16 B, with no header;
  - a vector or string slot is a 24 B `Vec` header plus a separate malloc;
  - **every `HeapObjectData` slot is 72 B** whatever the variant, so a `MutableCell`, an `f64` flonum or an `Ephemeron` each costs 72 B;
  - strings take 4 B per char (`Vec<char>`).
- The VM cons fast path is `state.heap.borrow_mut().alloc_pair(x, y)` (`crates/patina-vm/src/runtime/vm_state.rs:2115`). `alloc_pair` does `note_alloc()`, then pops the free list or pushes onto the `Vec` (`heap/mod.rs:703-715`) [src].
  - Each closure allocation clones an `Rc<Environment>` and a `Vec` of free variables (`vm_state.rs:1549`; the `VmClosure` variant holds `globals: Rc<Environment>`) [src].
- The trigger counts **objects, not bytes**: it fires at `max(65_536, 2 × live_after_last)`, where live is `pairs + vectors + strings + objects` (`heap/gc.rs:969-1002,152-154,1112`) [src]. A 1M-element vector counts the same as one pair.
- The safepoint is one `Rc<Cell<bool>>` load per dispatched instruction, and collection only happens in the outermost loop (`vm_state.rs:218,1204,1285-1292`) [src]. This is the same shape as Whippet's `GC_COOPERATIVE_SAFEPOINT_HEAP_FLAG` (§1.8).
- Ephemerons and weak continuation ids are resolved by a round-based fixpoint that **rescans every pending ephemeron each round** (`heap/gc.rs:1050-1094`) [src]. A chain of ephemerons whose keys are the previous values costs O(n²) [inf].
- Continuations copy `Vec<CallFrame>` plus the register `Vec<TaggedValue>` (`crates/patina-vm/src/types/continuation.rs:159-175`). `CallFrame` holds `closure: Option<HeapIndex>` and `code: Rc<CodeObject>` (`types/mod.rs:41-60`) [src].
- `docs/GC_DESIGN.md` §3.4 rules out moving collection permanently, because raw indices escape: the symbol table, raw-bits hashing, `CallFrame.closure`, code constants, `CompiledMacro` and `Rc` graphs. §8 says sweep tombstoning is load-bearing, because it breaks `Rc<Environment>` cycles [src].
- `PRD/ARCHIVE/GC_STAGE5_PRD.md` measured `code_store` constants at 57% of root-tracing time [src].

---

## 1. Whippet (Andy Wingo, 2022–2025)

### 1.1 What it is and its status

- Whippet is an embed-only C library that provides one abstract allocation API and several collectors behind it [doc: README]:
  - `semi`, a semispace collector;
  - `pcc`, a parallel copying collector, which can be generational;
  - `mmc`, the mostly-marking collector. It was called "whippet" until it was renamed in 2024 [doc: wingolog 2024-09-18].
  - `bdw`, Boehm–Demers–Weiser behind the same API.
- The collector is chosen at compile time, and the embedder's object model is compiled into it through `gc-embedder-api.h`. That compile-time specialisation is how the abstraction costs nothing at run time [doc: manual.md].
- Status per the README: "As of October 2025, Whippet is feature-complete", meaning ready to replace BDW in Guile on the `wip-whippet` branch, to be merged "over the next months" [doc].
  - The same README says the collectors "need to add support for MacOS and Windows, and to test on AArch64" [doc].
  - **Patina's development machine is macOS/arm64 (16 KiB pages)** [meas: `uname -m`, `hw.pagesize`]. Linking Whippet itself is therefore not an option today [inf].
- The 2023 FOSDEM talk claimed 22 kB of binary for the collector against 184 kB for BDW [doc: wingolog 2023-02-07].

### 1.2 The nofl space: layout numbers [src: `src/nofl-space.h`, `api/mmc-attrs.h`]

| Parameter | Value | Where |
|---|---|---|
| Granule (allocation unit) | 16 B | `nofl-space.h:33`, `mmc-attrs.h:22-24` |
| Block | 64 KiB | `nofl-space.h:45` |
| Slab | 4 MiB, aligned; holds 64 blocks, 4 of them for metadata, so 60 usable | `nofl-space.h:44-49` |
| Metadata | **1 byte per granule** in a side table at the slab base, i.e. 6.25% on 64-bit | `nofl-space.h:46`, `collector-mmc.md` |
| Medium-object threshold | 256 B | `nofl-space.h:35` |
| Large-object threshold | 8192 B, served by a separate large-object space | `mmc-attrs.h:25-27` |

The metadata byte packs the whole per-object GC state (`nofl-space.h:250-268`):

- **Bits 0–2: mark state.** The values are `YOUNG=1`, `MARK_0..2=2..4`, `BUSY=5` and `FORWARDED=6`. Three rotating mark values make "dead / survivor / marked" distinguishable without clearing marks; they rotate after each major collection.
- **Bits 3–4: trace kind.** The kinds are precise (0), none (8, pointer-free) and conservative (16). Value 16 doubles as `PINNED` for objects traced precisely.
- **Bit 5: `END`**, marking the end of an object. It lets the sweeper find object extents and holes by reading only the metadata, never object memory.
- **Bits 6–7: `LOGGED_0` and `LOGGED_1`**, the field-logging bits for the two words of a granule.

The key insight comes from the `collector-mmc.md` "Differences from Immix" section [doc]:

- Immix needs both a line-mark table (128 B lines in 32 KiB blocks) and per-object mark bits.
- If you instead keep **one mark byte per granule**, "you effectively have a line mark table where the granule size is the line size".
- Allocation then becomes "bump-pointer allocate into holes in the mark byte table" without wasting partial lines. The 2023 talk gives the same figure [doc].
- The mark table also answers the conservative-root question "does this address start an object?" (`nofl-space.h:204-208`, "we use the metadata byte for this purpose, setting the 'young' mark") [src].

### 1.3 Allocation and sweeping

- **Inline bump allocation** into the current hole of a block (`api/gc-allocate.h:58-85`) [src]:
  1. Round the size up to 16.
  2. Load `hp` and `limit` from *fixed offsets off the mutator pointer*: offset 0 and 8 (`mmc-attrs.h:29-34`).
  3. Compare, and return `NULL` (slow path) on overflow.
  4. Store the new `hp`.
  5. Write the begin pattern (young | trace kind) and the end bit into the metadata table, found as `addr & ~(4 MiB−1)` plus the granule index (`gc-allocate.h:31-56`, `mmc-attrs.h:41-65`).

  That is about 8–10 instructions, with no call, and memory is zeroed [inf from source].
- **Lazy sweeping.** When a mutator takes a block it scans the block's metadata bytes for holes and clears the stale marks of dead objects [doc: `collector-mmc.md`; src: `nofl-space.h:772-830`]. This makes sweep "naturally cache-friendly and parallel". Sweeping only ever touches metadata.
- **Hole-too-small fragmentation.** In 2025-08 Wingo added size-segregated free lists that capture holes discarded during sweeping. His own verdict was "I don't know if it's worth it!"; it also fought evacuation by refilling sparse blocks [doc: wingolog 2025-08-07].

### 1.4 Mostly-marking with opportunistic evacuation

- **Decision logic** is in `determine_collection_kind` (`src/mmc.c:656-715`, defaults at `mmc.c:1319-1326`) [src]:
  - compact when fragmentation exceeds 10%, and keep compacting until it falls below 5%;
  - otherwise run minor collections while the yield stays at or above 30%, and drop to a major collection when yield falls below the threshold (clamped at no less than 5%);
  - compact immediately when allocation cannot be satisfied.
- **Evacuation reserve.** Normally 2% of blocks, but 0 when conservative tracing is configured (`nofl-space.h:2135-2136`). During compaction up to 50% of free space is taken as evacuation targets (`nofl-space.h:1291-1293`) [src].
- **Evacuation is strictly optional per object.** Conservative roots are traced first and marked in place, which pins them. Everything reached only through precise heap edges may be moved [doc: `collector-mmc.md`].
  - Per-object pinning is a CAS on the metadata byte (`nofl-space.h:233-241`).
  - Pinning "does not keep the object alive" [src].
- **Three cycles to compact.** The 2023 talk notes that full compaction typically takes three GC cycles [doc].
- **Movable objects need a forwarding protocol** from the embedder (`gc-embedder-api.h:73-86`; `manual.md` "Forwarding objects") [src/doc]:
  - a non-atomic version, plus an atomic BUSY/FORWARDED state machine for parallel evacuation;
  - the embedder usually stores the forwarding pointer in the tag word, using a low-bits pattern that no live tag can have.
- **A live bug.** Wingo's 2025-07 notebook reports an unfixed race in parallel evacuation: a marker briefly swaps the first word to "busy" while another thread has already published the object [doc: wingolog 2025-07-08].
  - Lesson [inf]: in-place marking and evacuation sharing one header word is subtle under parallelism. A single-threaded Patina avoids it, but a future parallel tracer would not.

### 1.5 Conservative versus precise roots

- Whippet supports precise roots (`gc_trace_mutator_roots` and `gc_trace_heap_roots`) and conservative ones (`gc_trace_*_pinned_roots` with `trace_ambiguous(start, end, possibly_interior)`) (`gc-embedder-api.h:34-71`) [src].
  - Embedders filter candidates with `gc_is_valid_conservative_ref_displacement`, which says which low-bit tag patterns may be pointers [doc: `manual.md`].
- Wingo's argument for conservative stacks [doc: wingolog 2024-09-07, `collector-mmc.md`]:
  - no stack maps to emit or look up per frame;
  - C/C++ runtime code can hold references in locals freely;
  - slots can be reused across types;
  - his measured gain was only "maybe a percent".
  - The performance question remains open in his own words. He cites that JavaScriptCore uses conservative stack scanning and that V8 was investigating it.
- **Guile's partitioning lesson** [doc: wingolog 2025-04-25]:
  - Ambiguous edges are confined to the *root phase*: C stacks, static data and possibly Scheme stacks.
  - Every edge in the main trace must be precise; only then can the rest of the heap move.
  - Captured stack slices (continuations) are ambiguous. Guile's options are to disable relocation while undelimited continuations are live, or to eagerly pin the referents of a freshly captured slice.
  - `hashq` exposing addresses also forces pinning [doc: wingolog 2025-07-08].
- **Relevance to Patina [inf].**
  - The VM already keeps every Scheme value in a precise register file (`ExecutionState::registers`, `execution_state.rs:18`), and stage-5 work made register liveness precise (PRD Priority 2b).
  - A JIT that **spills live GC values back into that register window at every safepoint** (calls, allocation slow paths, back-edges) gets precise, map-free root scanning for free; the register window becomes a shadow stack.
  - Continuation capture stays a `memcpy` of the window. Conservative scanning would only be needed if JIT code kept references in machine registers or on the native stack across safepoints.

### 1.6 Generational collection: sticky mark bits and a field-logging barrier

- **Sticky mark bits** [doc: wingolog 2022-10-22]:
  - minor collections do not flip mark sense, so last cycle's survivors stay "marked" and are implicitly old;
  - "Marking an object is tenuring, in-place";
  - minor traces start from roots plus the remembered set, and only major collections flip;
  - Wingo's own assessment: "better than nothing, not quite as good as semi-space nursery".
- **Block-level promotion.** A block whose hole granules fall below `promotion_threshold × granules` is moved to the `promoted` list and is not allocated into until the next full GC (`nofl-space.h:623-648`) [src].
- **The barrier evolved from card marking (one byte per 256 B) to field logging** [doc: wingolog 2024-10-03]:
  - Cards misfire with sticky bits: a store into a new object marks a card that also covers old objects, which are then rescanned.
  - The field-logging fast path (`api/gc-barrier.h:61-91`, constants `mmc-attrs.h:67-105`) [src]:
    1. Check that the holder is old: load its metadata byte and test `(byte & 7) != YOUNG`.
    2. Load the log byte for the field (2 fields per byte, at bit 64 for field 0).
    3. Take the slow path only if the bit is clear. The slow path CASes the bit and pushes the edge onto a sequential store buffer.
  - The emitted x86 is two `testb` instructions. Measured against card marking it gave **1.05×–1.5× speedups** on Whiffle benchmarks [doc].
  - Large objects (>8 KiB) always take the slow barrier (`mmc-attrs.h:82-89`).
  - `collector-mmc.md` still describes the old card barrier; the source is authoritative [src vs doc, a doc/source drift].
- **Ephemerons versus generations.** Whippet maintains "an ephemeron E is never older than its K or V, by construction". It never promotes E without its K and V, so no remembered-set entries are needed for ephemeron edges [doc: wingolog 2025-01-09].
- **Results were disappointing** [doc: wingolog 2025-02-09, "Baffled by generational garbage collection"]:
  - On splay and nboyer (8-core Ryzen 7840U), "at all heap sizes, and for these two very different configurations (mmc and pcc), generational collection takes more time than whole-heap collection".
  - Example: nboyer at a 2.5× heap ran 20 majors whole-heap, against 8 majors plus 3,471 minors generational.
  - Median pauses fell but maximum pauses did not.
  - His hypotheses include barrier cost, nursery sizing (2 MB per active thread), weak generational behaviour in the benchmarks, and promoting after one cycle.
  - Wastrel (2026-04) uses stack-conservative-parallel-generational-mmc: 281 minor + 3 major collections, 0.256 ms median pause, 7.168 ms max. Wingo says the numbers are "consistent" whether or not the generational variant is used [doc: wingolog 2026-04-09].
  - Lesson [inf]: Patina should not assume generational is a win. Build the barrier hooks so they can be turned on, then measure.

### 1.7 Parallel tracing

- Each worker has a local queue of 1024 entries and publishes 3/4 of it to a shared worklist when full; idle workers steal [doc: `collector-mmc.md`].
- The worklist is malloc'd outside the heap budget.
- Not relevant to single-threaded Patina at first. The one lesson is that the mark loop should be a worklist, not recursion [inf].

### 1.8 The embedder and JIT interface (the most transferable part)

`api/gc-attrs.h` is a set of `static inline` attribute functions that a JIT can read **at JIT-compile time** to emit fast paths matching the C ones [src]:

- **Allocation:**
  - `gc_inline_allocator_kind`: bump pointer, free list or none;
  - `gc_allocator_small_granule_size` and `gc_allocator_large_threshold`;
  - `gc_allocator_allocation_pointer_offset` and `..._limit_offset`, both relative to the mutator struct;
  - `gc_allocator_alloc_table_alignment` and the begin/end patterns.
- **Generation check:** `gc_old_generation_check_kind`, either an ALLOC_TABLE tag mask, a SMALL_OBJECT_NURSERY address range or SLOW. The nursery bounds are fixed and "the embedder might want to … inline them into the output of JIT-generated code" (`gc-barrier.h:42-51`).
- **Write barrier:** `gc_write_barrier_kind` (none, field or slow), the field-table alignment and offset, fields per byte, and the first bit pattern.
- **Safepoints:** `gc_safepoint_mechanism` (cooperative or signal) and `gc_cooperative_safepoint_kind` (none, mutator flag or heap flag). The fast path is one relaxed load of `*gc_safepoint_flag_loc(mut)` (`gc-safepoint.h:15-36`). mmc uses a heap-wide flag (`mmc-attrs.h:107-113`).
- **Capabilities:** `gc_can_pin_objects` and `gc_can_move_objects`.
- **Allocation kinds** (`gc-allocation-kind.h`): TAGGED, TAGGED_POINTERLESS, UNTAGGED_CONSERVATIVE and UNTAGGED_POINTERLESS. Pointerless kinds are never traced.
  - That matches Patina's bytevectors, strings, flonums and bignums [inf].

`gc_allocate_fast` and `gc_allocate_slow` are split so that, in the manual's words, "you can punt some root-saving overhead to the slow path" [doc: `manual.md`]. That is precisely the JIT's problem: spill live values to the register window only on the slow path [inf].

### 1.9 Ephemerons and finalizers

- **The ephemeron state machine** (`src/gc-ephemeron.c` header comment) [src]:
  - states are Traced, Claimed, Pending and Resolved, with an epoch counter;
  - pending ephemerons sit in a buckets-of-chains hash table keyed by K;
  - during the first trace resolution is lazy. After that `check_pending_ephemerons=1`, and **every newly marked object looks itself up in the pending table** (`mmc.c:164-165,191-192,830-840`), so resolving an ephemeron costs O(1) per key instead of a rescan.
  - Ephemerons can be chained into weak structures, and the GC splices dead links out.
- **Finalizers** [doc: `manual.md` §Finalizers; wingolog 2024-07-22]:
  - they have priorities; level 0 blocks levels 1 and above;
  - they support resurrection, and fire into a queue that the embedder drains (`gc_pop_finalizable`), so the GC never runs program code;
  - this is the guardian model Chez uses as well.
- Lesson [inf]: Patina should replace the O(n²)-worst-case fixpoint with key-indexed pending resolution. Its weak continuation table is the same pattern as Whippet's weak-id handling.

### 1.10 Heap sizing

- **Three policies** are available: fixed, growable and adaptive [doc: README, wingolog 2023-01-27].
  - Growable never shrinks. It keeps `heap ≥ live × multiplier`, and Guile uses 1.75× [doc: wingolog 2025-05-22].
  - Adaptive is MemBalancer (Kirisame, Shenoy and Panchekha, OOPSLA 2022) [src: `src/adaptive-heap-sizer.h:16-79`]. The size is `live × (1 + c·sqrt(live × alloc_rate / collection_rate))`, clamped between minimum and maximum multipliers, with a guaranteed minimum free space and exponential smoothing of live bytes, pause time and allocation rate. A background heartbeat updates it.
  - The paper reports a 16.0% memory reduction at constant GC time on web pages, or up to 30.0% less GC time at constant memory, measured in V8 [doc: arXiv 2204.10455].
  - Wingo's caveat: per-GC resizing is like "giving control of your stereo's volume knob to a hyperactive squirrel" [doc: wingolog 2024-09-18].
- **The fragmentation livelock** [doc: wingolog 2025-05-22]:
  - A non-moving heap that meets its multiplier target can still fail to fit a 32 B object when every hole is 16 B. The policy then refuses to grow, and allocation loops forever.
  - "if you can't deal with fragmentation, then it is impossible to just rely on a heap multiplier to size your heap."
  - Fixes are reserving empty blocks after GC regardless of the multiplier, plus Immix-style overflow blocks. The real fix is evacuation.

### 1.11 Guile integration: status and lessons

- **Scale of the change.** About 18,000 lines added (Whippet) and about 3,000 removed. The last direct BDW API use was removed in May 2025 [doc: wingolog 2025-05-15].
- **Weak tables** were rebuilt on ephemerons, giving lock-free reads and writes, though they cannot yet resize in response to GC [doc].
- **Movability** needed a pile of refactors so that one central `scm_trace_object` could exist, plus pinning for ambiguous continuation slices and for `hashq` addresses. The result was "a net improvement over the non-moving configuration and a marginal improvement over BDW", with more variance [doc: wingolog 2025-07-08].
- **Lesson [inf]: the work is mostly in the embedder, not the collector.** Guile's cost was making every object precisely traceable through a single function and finding every place an address escapes. Patina's escape list in `GC_DESIGN.md` §3.4 is the same inventory, with indices standing in for addresses.

### 1.12 Verdict for Patina

**Adopt the design, not the code.**

- **Adopt:**
  - the nofl/Immix-style mark-region space: 16 B granules, a side metadata byte per granule with an end bit, 64 KiB blocks in aligned slabs, and lazy sweeping over metadata only;
  - pointerless allocation kinds;
  - mostly-marking with *optional* evacuation, so non-moving stays a valid configuration;
  - MemBalancer-style sizing, but driven by bytes;
  - the attrs-style JIT contract: allocation pointer and limit at fixed offsets from a VM-context pointer, a one-load safepoint flag, and a barrier described by constants.
- **Defer:**
  - sticky-mark-bit generational collection with a field-logging barrier, kept as phase 2 behind a measured gate. Wingo's own data shows generational can lose.
- **Reject:**
  - linking Whippet itself (C, compile-time specialised, no macOS or AArch64 support yet);
  - conservative stack scanning. Patina's register-file VM already gives precise roots cheaply.

---

## 2. Go's collector (to Go 1.26)

### 2.1 Facts

- **Algorithm.** Non-moving, concurrent, tri-color mark-sweep. Heap objects never move [doc: go.dev/doc/gc-guide]. Goroutine stacks *are* copied on growth; heap objects are not [inf, well known].
- **Allocator.** 68 size classes (`NumSizeClasses = 68`) from 8 B to 32 KiB on 8 KiB pages (`PageShift = 13`, `MaxSmallSize = 32768`). Each span holds objects of a single size class, and the table records per-class tail waste, from 29.24% at 24 B down to single digits (`internal/runtime/gc/sizeclasses.go:3-90`) [src].
- **The hybrid write barrier** (`runtime/mbarrier.go` header) [src]:
  ```
  writePointer(slot, ptr):
      shade(*slot)                 // Yuasa deletion
      if current stack is grey:
          shade(ptr)               // Dijkstra insertion
      *slot = ptr
  ```
  - It is unconditional on the holder's color, because a color check would need a store→load fence.
  - It is enabled only while marking: `writeBarrier.enabled = gcphase == _GCmark || gcphase == _GCmarktermination` (`mgc.go:259`) [src]. It is emitted for heap pointer stores and for globals, but not for stores to the current frame.
  - It removed STW stack rescans. Go 1.8 pauses became "usually under 100 microseconds and often as low as 10 microseconds" [doc: go1.8 notes].
- **Pacing** [doc: gc-guide; src: `mgcpacer.go:39`]:
  - target heap = live + (live + GC roots) × GOGC/100; roots counted since 1.18;
  - 25% of the CPU goes to background marking (`gcBackgroundUtilization = 0.25`), with mutator *assists* when allocation outruns marking;
  - `GOMEMLIMIT` is a soft limit; a GC CPU limiter caps GC at about 50% over a 2×GOMAXPROCS CPU-second window.
- **Why Go is not generational** [doc: go.dev/blog/ismmkeynote, Hudson ISMM 2018]:
  - The Request-Oriented Collector's barrier caused "30, 40, 50% and more slowdowns" on compilers.
  - A non-moving generational prototype's barrier was "fast but it simply wasn't fast enough".
  - Escape analysis keeps young objects on the stack, and Go chose to "avoid always-on barriers in favor of increasing memory".
- **Green Tea** (`runtime/mgcmark_greenteagc.go` header; go.dev/blog/greenteagc; Go 1.26 notes; issue #73581):
  - **Mechanism** [src]: marking works on 8 KiB spans of objects of 16 to 512 B (`gcUsesSpanInlineMarkBits`: `heapBitsInSpan(size) && size >= 16`, and `MinSizeForMallocHeader = 8×64 = 512`).
    - Each span keeps inline `marks[63]` and `scans[63]` bitmaps plus an ownership byte and a class byte, 128 B per 8 KiB span or 1.56% [src + inf].
    - Discovering a pointer sets a mark bit and enqueues the *span*, not the object, on a FIFO queue. FIFO lets marks accumulate before the span is scanned.
    - Dequeue computes `marks & ~scans` and scans those objects in address order. A fast path handles spans holding one mark.
  - **Motivation** [doc]: about 35% of marking time was spent stalled on heap memory under the object-graph flood.
  - **Results** [doc]:
    - 10–40% less GC CPU, modally about 10% (blog);
    - the Go 1.26 release notes promise "between a 10–40% reduction";
    - about 10% more on Ice Lake / Zen 4 and later using AVX-512 kernels (`VGF2P8AFFINEQB` expands per-object bits to per-word bits);
    - the proposal measured about 50% fewer cache misses on GC-heavy benchmarks, and near parity on tree-rotation-heavy bleve-index.
  - **Status:** experiment in Go 1.25, default in Go 1.26, with opt-out `nogreenteagc` expected to be removed in 1.27 [doc].

### 2.2 Verdict for Patina

- **Adopt the allocator and marking ideas, not the concurrency.**
  - Size-class-segregated pages where the page determines the object size are what make Green Tea possible. A Patina page holding only pairs knows its stride, so marks can be bitmaps indexed by `(addr − page) / 16` [inf].
  - Green Tea's batching ("scan spans, not objects") fits Patina's dominant small objects: pairs at 16 B, cells and closures at 16–64 B [inf]. It is a cheap later optimisation of a mark loop over size-segregated blocks, with no barrier or pacer needed.
- **Reject Go's concurrent design for now.** The pacer, assists and hybrid barrier exist because Go is multi-threaded and latency-bound. Patina is single-threaded.
  - If incremental marking is ever wanted, a snapshot-at-the-beginning (Yuasa) barrier gated on "marking in progress" is the simplest form. Go shows it costs nothing while GC is idle, because the check is one global flag [inf].
- **Take Hudson's lesson seriously.** Always-on generational barriers may cost more than buying memory with a larger GOGC-equivalent multiplier. That argues again for a measured gate on generational mode [inf].

---

## 3. .NET (CoreCLR) GC

### 3.1 Facts

- **Generations and allocation** [doc: BOTR `garbage-collection.md`]:
  - generations 0, 1 and 2, plus the LOH for objects of at least 85,000 B (logically part of gen2) and the POH (.NET 5) for pinned objects;
  - allocation is a bump pointer in per-thread *allocation contexts*, refilled in quanta of about 8 KiB;
  - a "plan phase simulates a compaction" and the GC compacts only "if compaction is productive", otherwise it sweeps. This per-GC compact-or-sweep choice is a production example of mostly-non-moving [doc].
- **Regions** (default for 64-bit since .NET 7):
  - many uniform regions, 4 MB basic and 32 MB for large or pinned objects (8×), instead of 256 MB–4 GB segments [doc: Maoni Stephens via itnext; region size verified in asm below];
  - generations are no longer contiguous, which enables DPAD (Dynamic Promotion And Demotion), in which a region is assigned whatever generation is most useful [doc: devblogs "Put a DPAD on that GC"].
- **The write barrier is a runtime-patched assembly stub, called from JIT code** (`src/coreclr/vm/amd64/JitHelpers_FastWriteBarriers.asm:198-266`, `JIT_WriteBarrier_Byte_Region64`) [src]:
  1. store, then `shr dst, 22` to get the 4 MB region index;
  2. load the region→generation byte; return if the destination is gen0;
  3. range-check the source against the ephemeral bounds;
  4. compare the generations of source and destination regions;
  5. on old→young, `shr dst, 11` gives a **2 KB card byte**, set to 0xFF if not already set;
  6. optionally set a card-bundle byte (1 per 1024 cards).

  The `Bit_Region64` variant uses a bit per 256 B with `lock or` (lines 268-342). Constants are `PATCH_LABEL`s that the runtime rewrites when the heap layout changes. There are also write-watch variants for background GC.
- **Thread suspension** [doc: BOTR `threading.md`]:
  - cooperative and preemptive modes;
  - the JIT emits either *fully interruptible* code (GC info at every instruction, no polls) or *partially interruptible* code (GC info only at calls and explicit GC polls);
  - threads stopped in non-safe code are caught by **hijacking the return address** of the top frame;
  - GC info is precise stack maps.
- **Pinning** uses pinned locals reported in GC info, pinned GC handles and the POH. The advice is "pin early, pin in batches" [doc: Maoni mem-doc].
- **DATAS** (Dynamic Adaptation To Application Sizes) [doc: learn.microsoft.com DATAS]:
  - opt-in in .NET 8 and the default for Server GC in .NET 9;
  - the gen0 budget is bounded by long-lived data size; the heap count grows from 1 toward the core count; full compacting GCs control fragmentation;
  - TechEmpower on 48 cores showed "over 80%" working-set reduction for a 2–3% RPS loss.

### 3.2 Verdict for Patina

- **Adopt:**
  - *regions as the unit of generation*: region index = `addr >> k`, plus a byte table mapping region to generation. This makes "is the holder old?" one shift and one load, works with sticky-mark promotion, and allows demotion. It is the same idea as Whippet's promoted-block list [inf];
  - *the plan-then-decide compact-or-sweep step*, which matches Whippet's fragmentation thresholds;
  - *patched or out-of-line barrier stubs* as a fallback strategy for the JIT. Cranelift can call a stub with a cheap calling convention instead of inlining the whole barrier [inf];
  - *DATAS's principle*: heap proportional to live data, with throughput as a secondary goal. That suits an embeddable interpreter better than throughput-first sizing [inf].
- **Reject:**
  - fully interruptible code and return-address hijacking. That machinery is for preemptive multi-threaded suspension. Single-threaded Patina with cooperative polls only needs stack maps, or register-window spills, at call sites, allocation slow paths and loop back-edges [inf];
  - card tables as the remembered set for a sticky-mark-bit heap. Wingo's measurement (§1.6) shows field logging beats cards there.

---

## 4. Lua 5.4/5.5 and LuaJIT

### 4.1 Lua 5.4 and 5.5 [src: `lua/lua` `lgc.h`, `lstate.c`]

- **Incremental mode** (`lgc.h@v5.4.7`):
  - tri-color marking with *two whites* (`WHITE0BIT`/`WHITE1BIT`), so sweep can tell this cycle's new objects from last cycle's dead ones without a flip pass;
  - the invariant is "a black object can never point to a white one";
  - **forward barrier** `luaC_barrier`, which marks the white child, is used for most objects;
  - **backward barrier** `luaC_barrierback`, which re-grays the black parent. Pall's design doc explains that Lua uses it for tables because containers "usually receive several stores in succession" (see also §4.2);
  - parameters are pause 200% (wait for memory to double), step multiplier 100 and step size 2¹³ = 8 KB;
  - all objects sit on intrusive linked lists, with a `marked` byte in the header.
- **Generational mode** (added in 5.4) uses seven ages in 3 bits: `G_NEW`, `G_SURVIVAL`, `G_OLD0` (made old by a forward barrier), `G_OLD1`, `G_OLD`, `G_TOUCHED1` and `G_TOUCHED2`.
  - Objects are promoted after surviving two minors.
  - "Touched" old objects (backward barrier) are re-traversed for two cycles.
  - Defaults are `LUAI_GENMINORMUL 20` (a minor collection after 20% growth) and `GENMAJORMUL 100`.
  - The default mode is still incremental (`lstate.c:387`, `g->gckind = KGC_INC`).
- **Lua 5.5** (5.5.1 released 2026-08-03 per lua.org/versions.html [doc]):
  - "major garbage collections done incrementally". There is a new `KGC_GENMAJOR` mode, so generational mode's majors no longer stop the world (`master lgc.c:1102-1113`);
  - it switches minor→major when old bytes exceed 70% (`LUAI_MINORMAJOR`), and major→minor when a major frees at least 50% of new bytes (`LUAI_MAJORMINOR`) [src];
  - pause rises to 250% and the multiplier to 200 [src].

### 4.2 LuaJIT

- **Current GC.** "essentially the same as the Lua 5.1 GC", with tri-color incremental marking over linked lists. LuaJIT 2.0 refines the table barrier to check only "black table", ignoring the stored value's color [doc: Pall's design doc, §Rationale and §Tri-Color].
- **Mike Pall's new-GC design** for LuaJIT 3.0. The original wiki is gone (wiki.luajit.org now serves the project page); I read a verbatim copy in the LuaVela docs (`ujit.readthedocs.io/.../tut-new-gc.html`) [doc]. The document says "No code is available, yet"; to my knowledge it was never implemented [inf].
  - **Arenas:** 64 KB–1 MB, aligned to their size, so masking an address finds the metadata. Each arena is either all traversable or all non-traversable; non-traversable arenas are never scanned and their objects need no type tag.
  - **Cells and bitmaps:** 16-byte cells. Block and mark bitmaps, 1 bit each per cell, use 1/64 of the arena (1.5%) and are kept separate for cache behaviour.
  - **Differential block encoding** (block bit, mark bit):

    | Block | Mark | Meaning |
    |---|---|---|
    | 0 | 0 | extent |
    | 0 | 1 | free |
    | 1 | 0 | white |
    | 1 | 1 | black |

    Allocating sets one bit, marking sets one bit, and a whole block changes state by flipping its first cell.
  - **Bitmap sweep:**
    - major: `block' = block & mark`, `mark' = block ^ mark`;
    - minor: `block' = block & mark`, `mark' = block | mark` (black stays black, so this is sticky-mark generational);
    - the sweep is word-parallel or SIMD over 1/64 of memory and "doesn't bring neither live nor dead object data back into the cache".
  - **Quad-color marking:** gray is split into light and dark gray. The *gray bit lives inline in the object* while black and white live in the side bitmap.
    - New objects are light gray, so the barrier "only checks for a cleared gray bit", in 2–3 instructions, and almost never fires for fresh objects.
    - A triggered barrier pushes onto a sequential store buffer, later distributed to per-arena gray stacks.
    - A priority queue of arenas, largest gray stack first, keeps tracing arena-local.
  - **Allocator:** a bump allocator that switches to a bounded best-fit segregated allocator under fragmentation pressure, then back.
  - **Generational mode:** "automatically triggered by workloads with a high death rate for young allocations", and abandoned when that stops paying.

### 4.3 Verdict for Patina

- **Adopt from Pall's design:**
  - segregate traversable from pointer-free memory at the block or arena level (strings, bytevectors, flonums, bignum limbs);
  - metadata-only sweeping;
  - word-parallel bitmap sweep formulas, which apply directly to a granule-bitmap variant of the nofl table [inf];
  - a "1 inline bit plus side bitmap" barrier, if an incremental mode is ever wanted.
- **Adopt from Lua:**
  - automatic switching between modes based on measured yield. Lua 5.5's 70%/50% rules and Whippet's yield and fragmentation thresholds are the same kind of hysteresis;
  - the forward-versus-backward barrier choice by object kind. Vectors and records take many successive stores; pairs and cells take single stores [inf].
- **Reject:**
  - linked-list object lists, the pre-redesign structure Pall calls "a dead end";
  - LuaJIT's GC itself, which is the Lua 5.1 design.

---

## 5. Perceus (Koka) and Lean 4 reference counting

### 5.1 Facts

- **Koka's header** (`kklib/include/kklib.h:136-141`) [src] is 8 bytes:
  - `scan_fsize: u8`, the number of fields to scan (0xFF means the count is in the first field);
  - `_field_idx: u8`;
  - `tag: u16`;
  - `refcount: _Atomic(i32)`.
- **Koka's refcount encoding** (`kklib.h:100-122`) [src]:
  - **rc 0 means unique**, which makes the "free on drop" test a signed compare against zero;
  - negative values mean thread-shared (atomic operations), sticky on overflow, or static (`INT32_MIN`);
  - `kk_block_drop` is `if rc <= 0 then slow-path else rc-1` (`kklib.h:775-784`).
- **Perceus** (Reinking, Xie, de Moura and Leijen, PLDI 2021):
  - precise RC instructions make cycle-free programs *garbage free*;
  - reuse analysis turns a unique match-then-construct into in-place update, the "functional but in-place" style;
  - Koka's 2020 figure shows times relative to Koka = 1.00 (`doc/bench-amd3600-nov-2020.png`) [src: read the image]:

    | Benchmark | OCaml | Haskell | Swift | Java | C++ | Koka without Perceus opts |
    |---|---|---|---|---|---|---|
    | rbtree | 1.26 | 2.40 | 7.85 | 1.67 | 0.92 | 2.45 |
    | deriv | 1.43 | 2.08 | 2.23 | 0.86 | 1.33 | — |
    | nqueens | 1.45 | 10.27 | 3.96 | 1.33 | 0.79 | — |

  - Perceus "cannot collect cycles, and works best in a language with limited use of (concurrent) mutable references" [doc: MSR publication page and search abstract]. Koka relies on "inductive data types cannot form cycles" (`doc/spec/why.kk.md:224`) [src].
- **Lean 4's header** (`src/include/lean/lean.h:142-178`) [src] is `int m_rc; m_cs_sz:16; m_other:8; m_tag:8`:
  - rc > 0 means single-threaded, rc < 0 multi-threaded and **rc == 0 persistent or compact-region objects that are never counted**;
  - a band of deeply negative values is "sticky" (frozen, never freed) for memory safety on overflow;
  - during deallocation the rc and size fields are reused as an inline deletion worklist.
  - The Lean reference says: "Because Lean cannot create cyclic data, no technique is needed to detect it" [doc: lean-lang.org reference, Run-Time Code / Reference Counting].
  - "Counting Immutable Beans" (Ullrich and de Moura) adds borrowed-parameter inference and reset/reuse [doc: arXiv 1908.05647].

### 5.2 Verdict for Patina: reject as the primary strategy; keep two ideas

**Why it does not transfer [inf]:**

1. **Scheme makes cycles routinely.**
   - Every recursive procedure built with `define` or `letrec` closes over a boxed cell or environment that refers back to it (`MutableCell` boxing of internal defines; `GC_DESIGN.md` §8 documents closure↔environment cycles).
   - `set-cdr!` and `vector-set!` build arbitrary graphs.
   - RC would need a backup tracing or cycle collector anyway, which means two memory managers.
2. **First-class continuations.** Copying a register window of N slots would cost N increments under RC, and N decrements when it dies. Tracing pays nothing at capture.
3. **JIT cost.** Every register move or overwrite of a heap value needs an inc/dec unless the compiler proves borrowing. That is a Perceus-grade static analysis that Patina's dynamic, `set!`-heavy IR cannot do soundly in general.
4. **Reuse needs uniqueness plus immutability.** Scheme objects are mutable and `eq?`-observable, so in-place reuse applies only to compiler-internal temporaries such as multiple-values buffers or rest-argument lists.
5. **Patina's own history.** Its current `Rc` payloads inside heap slots (`Rc<Environment>`, `Rc<RefCell<Vec<TV>>>` for records and parameters, `Rc<CpsContinuation>`) already caused trouble:
   - exponential re-tracing until dedup was added (§9.4: 6.8 s for one collection at depth 26);
   - leak avoidance that relies on sweep tombstoning (§8);
   - all of which the redesign should remove by moving those payloads into the traced heap.

**Ideas worth keeping:**

- (a) **An "rc == 0 means persistent" class, as an immortal or pre-tenured space.** Code-object constants, interned symbols, library exports and macro templates are allocated there, never swept and scanned only through a remembered set. This directly addresses PRD Priority 1 item 2 (57% of root tracing).
- (b) **An 8-byte header layout** with tag, scan-field count and a spare byte. It gives the GC a size and a pointer-field count without a Rust `enum` match. Lean's reuse of header bits as a deletion-list link is the same trick as Whippet's forwarding pattern in the tag word.

---

## 6. Synthesis: what this family of systems says about Patina's redesign

### 6.1 Comparison

| System | Moving? | Old-generation mechanism | Barrier fast path | Allocation fast path | Roots | Sizing |
|---|---|---|---|---|---|---|
| Whippet mmc | Optional per block (evacuate if fragmentation >10%) | Sticky marks plus block promotion | 2 tests: holder metadata byte, then field log bit | Bump into holes; hp and limit at mutator+0/+8 | Precise, or conservative and pinned | Fixed, growable 1.75×, or MemBalancer |
| Go 1.26 | No | None (deliberately) | Hybrid shade, only while marking | Per-P size-class span cache | Precise stack maps | GOGC plus soft GOMEMLIMIT |
| .NET 9 | Yes (compact if productive) | Regions with gen table, cards | Out-of-line patched stub: region gen compare, 2 KB card | Bump in 8 KB allocation context | Precise GC info, hijack, polls | Budgets; DATAS |
| Lua 5.5 | No | Ages in 3 header bits | Forward or backward on header color | malloc | Precise (VM stack) | pause% and stepmul |
| LuaJIT 3 design | No | Sticky "minor sweep" | Inline gray bit, 2–3 instructions | Bump, then segregated fit | Precise | Arena-granular limits |
| Koka / Lean | n/a (RC) | n/a | inc/dec on every copy | malloc (mimalloc) | n/a | n/a |

### 6.2 Constraints and lessons for the redesign [inf, each grounded above]

1. **Use a mark-region space with side metadata as the core.**
   - Whippet nofl, LuaJIT 3 and Go spans agree on segregated metadata, sweeping that only touches metadata, and size or kind segregation.
   - With 16 B granules, a Patina pair is exactly one granule and can stay **headerless**: its type comes from the pointer tag, its extent from the end bit.
   - That requires pairs to be traced from the *edge* (the tagged value), not from object contents; Whippet's `gc_trace_object(ref)` assumes self-describing objects. Either give each block a type (pair blocks, as in Chez spaces or Go spans), or give pairs a header.
2. **Replace `Vec<HeapObjectData>` (72 B per slot, plus `Rc`/`Vec` side allocations) with variable-size objects.** Each gets an 8-byte header (Koka/Lean-style tag, size, scan count), and vectors, strings and closures store their fields inline. This alone is roughly a 2–4× density gain for cells, flonums and small closures [meas: 72 B against 16–32 B].
3. **Byte-based accounting.** Replace the object-count trigger (`gc.rs:1001-1002`) with bytes allocated and bytes live. Then layer a MemBalancer-style rule on top: a sqrt term, clamps and a *minimum free reserve*. Wingo's livelock shows a multiplier alone is unsafe in a non-moving heap.
4. **Make evacuation possible, keep it optional.** Fragmentation is the failure mode of every non-moving system here (Whippet's livelock, LuaJIT's fit allocator, .NET's compact-if-productive). That means overturning `GC_DESIGN.md` §3.4:
   - every edge must be visible to the tracer and updatable: the symbol table, `CallFrame.closure`, code constants, `CompiledMacro` literals and `Rc` graphs, which should move into the heap or a scanned root table;
   - `eq?`-hash tables and anything keyed by raw bits (provenance, source map) need either address-based rehash after moving, or a stable hash stored in the header with objects pinned on first hash. Guile chose pinning for `hashq`.
5. **The JIT contract**, modelled on `gc-attrs.h`:
   - a stable `#[repr(C)]` mutator or VM-context struct exposing `alloc_ptr` and `alloc_limit` at fixed offsets, a safepoint flag, the slab alignment and the metadata layout, so Cranelift can emit bump allocation, a one-load poll, and the barrier fast path (old-check plus log-bit) inline, with out-of-line slow stubs in the .NET style;
   - this rules out `Rc<RefCell<Heap>>` on the fast path. The heap must be reachable from a raw context pointer that JIT code holds in a register.
6. **Precise roots through the register window.** JIT frames keep GC references in the VM register file across every safepoint, spilling only on slow paths, per Whippet's "punt root-saving to the slow path". That keeps root scanning precise and map-free and keeps call/cc as a copy of the window. Conservative scanning is the fallback only if Cranelift-allocated machine registers must hold references across calls; Cranelift's user stack maps are another team's topic.
7. **Generational collection as a measured phase 2.** Use sticky mark bits, block or region promotion, and a field-logging barrier on `set-car!`, `set-cdr!`, `vector-set!`, cell writes, record setters and closure free-variable writes. Gate it on interleaved A/B runs, because Wingo (2025) and Hudson (2018) both found generational losing on some workloads.
   - Keep ephemerons never older than their key and value, as Whippet does, to avoid remembered-set edges for them.
8. **Weak and ephemeron processing.** Replace the rescan fixpoint with a pending table keyed by key, consulted when an object is marked. Finalization, if added for ports, should use queue-based guardians with no program code running inside GC.
9. **Observability from day one.** Whippet added tracepoints (LTTng/Perfetto, `doc/tracepoints.md`) and an event-listener API (`api/gc-event-listener.h`). Patina's `GcStats` should grow per-phase timings and byte histograms before tuning starts.

---

## 7. Sources

**Whippet:**
- https://github.com/wingo/whippet — README.md, doc/manual.md, doc/collector-mmc.md, doc/collectors.md, doc/guile.md, api/gc-api.h, gc-embedder-api.h, gc-attrs.h, gc-allocate.h, gc-barrier.h, gc-safepoint.h, gc-allocation-kind.h, mmc-attrs.h, src/nofl-space.h, src/mmc.c, src/gc-ephemeron.c, src/adaptive-heap-sizer.h, src/growable-heap-sizer.h
- wingolog.org/archives:
  - 2022/10/22 sticky-mark-bit; 2023/01/27 heap sizing; 2023/02/07 towards a new local maximum; 2023/10/16 safepoints
  - 2024/07/10 block-structured copying; 2024/09/07 conservative vs precise; 2024/09/18 feature-complete; 2024/10/03 field-logging barrier
  - 2025/01/09 ephemerons vs generations; 2025/02/09 baffled by generational; 2025/04/25 ambiguous edges; 2025/05/15 Guile waypoint; 2025/05/22 heap growth; 2025/07/08 on the move; 2025/08/07 freelists
  - 2026/04/09 Wastrel generational
- MemBalancer: https://arxiv.org/abs/2204.10455

**Go:**
- go.dev/doc/gc-guide; go.dev/blog/greenteagc; go.dev/doc/go1.26; go.dev/doc/go1.8; go.dev/blog/ismmkeynote; github.com/golang/go/issues/73581
- golang/go source: src/runtime/mbarrier.go, mgcmark_greenteagc.go, mgcpacer.go, mbitmap.go, src/internal/runtime/gc/sizeclasses.go

**.NET:**
- dotnet/runtime: docs/design/coreclr/botr/garbage-collection.md and threading.md; src/coreclr/vm/amd64/JitHelpers_FastWriteBarriers.asm
- learn.microsoft.com/dotnet/standard/garbage-collection/datas; devblogs.microsoft.com/dotnet/put-a-dpad-on-that-gc; github.com/Maoni0/mem-doc; itnext.io (Maoni Stephens) on segments vs regions

**Lua and LuaJIT:**
- github.com/lua/lua at v5.4.7 and master: lgc.h, lgc.c, lstate.c
- lua.org/versions.html
- ujit.readthedocs.io/en/latest/public/tut-new-gc.html (copy of Pall's wiki page)

**RC:**
- ~/Project/reference/koka: kklib/include/kklib.h, doc/spec/why.kk.md, doc/bench-amd3600-nov-2020.png
- microsoft.com/en-us/research/publication/perceus-garbage-free-reference-counting-with-reuse/
- leanprover/lean4 src/include/lean/lean.h; lean-lang.org/doc/reference (Run-Time Code / Reference Counting); arXiv 1908.05647
