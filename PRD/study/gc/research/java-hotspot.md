# HotSpot (OpenJDK) collectors, 2025–2026: what Patina should copy, and what it should leave alone

Research key: `java-hotspot`. Date: 2026-09-30. Scope: G1, Parallel, Serial, Epsilon, Shenandoah, ZGC (generational),
TLABs and inline allocation, barrier elision, oop maps, safepoints and handshakes, compressed oops, compact headers,
and Loom stack chunks (continuations under GC). Every technique below gets a verdict for Patina: a single-threaded
Scheme in Rust with a register VM and a planned Cranelift baseline JIT.

Evidence legend: **[V]** means I fetched a primary source this session (JEP, OpenJDK source, author blog, paper
abstract). **[R]** means I recalled it and did not re-verify it this session. **[I]** marks my own inference. Repo
facts carry `file:line` against `main` at `28a94f8`.

---

## 0. Patina facts this report relies on (verified in source)

- **Value encoding.** `TaggedValue(u64)` has a 3-bit low tag (`crates/patina-core/src/tagged_value.rs:57,70-84`).
  Tags 3–7 are heap references whose payload is a **u32 arena index** shifted by 3 (`:28`, `:377-412`). Flonums are
  boxed (`HeapObjectData::Real(f64)`, `heap/mod.rs:147`), so AGENTS.md's "NaN-boxed" is stale. Identity
  (`eq?`/`eqv?`) compares raw bits (`heap/mod.rs:2155`).
- **Heap.** The heap is a set of typed `Vec` arenas: `pairs: Vec<(TV,TV)>`, `vectors: Vec<Vec<TV>>`,
  `strings: Vec<Vec<char>>` and `objects: Vec<HeapObjectData>`, with per-arena free lists
  (`heap/mod.rs:304-316,367-376`). `SharedHeap = Rc<RefCell<Heap>>` (`:51`). I measured the slot sizes with a throwaway
  crate (`PRD/study/gc/probes/java-hotspot`; rustc release, this machine): an object slot (`HeapObjectData`) is **72 B**, a pair slot is 16 B, and a
  vector or string slot is a 24 B `Vec` header plus a separate malloc; `CallFrame` is 40 B. Arenas grow by `Vec`
  reallocation, so **every arena's base address can change on any allocation**. That matters for a JIT (§3.1).
- **Allocation.** `alloc_pair` = `note_alloc()` (counter plus a threshold check) → free-list `pop` or `Vec::push`
  (`heap/mod.rs:703-713`). **Collection never happens inside allocation.** Allocation only raises a pending flag
  (`:575-600`; `gc.rs:20-27`).
- **Collector.** Non-moving, stop-the-world mark-sweep with side mark bitmaps and sweep-to-free-lists
  (`gc.rs:1-18`). The `Collector` trait carries a contract that implementations must be non-moving (`gc.rs:200-212`).
  The automatic threshold is `max(min, 2×live)` (`gc.rs:973-1002`). Ephemerons and weak continuation tables are
  resolved by a fixpoint (`gc.rs:1022-1093`). The "moving ruled out" rationale is in `docs/GC_DESIGN.md:122-139` and
  repeated as a non-goal in `PRD/ARCHIVE/GC_STAGE5_PRD.md:119`.
- **Safe points.** A safe point is one `Cell<bool>` load at the top of each driver loop, gated on "outermost guard"
  (`vm_state.rs:1286-1310`; tree-walker at `cps_eval/mod.rs:116`). Nested loops, library loading and the desugarer
  **defer** collection with `GcDeferGuard` (`vm_state.rs:343,1151`, `library_loader.rs:208`,
  `desugarer/mod.rs:1841`). This is functionally HotSpot's old GCLocker (§3.6).
- **An interpreter oop map already exists.** `CodeObject::register_roots` holds per-PC bitsets of which registers may
  hold roots, computed by forward dataflow (`docs/VM_COMPILER.md` §10.4, lines 361-375). `retire_registers` overwrites
  dead slots with `UNSPECIFIED` before a collection or a continuation capture
  (`crates/patina-vm/src/runtime/vm_state/gc_roots.rs:47-68`). This is the same idea as HotSpot's interpreter
  `OopMapCache` (§3.8).
- **The VM stack is heap-independent memory.** `ExecutionState { registers: Vec<TaggedValue>, frames: Vec<CallFrame>, … }`
  (`runtime/execution_state.rs:17-23`). `CallFrame.closure` is a raw `Option<HeapIndex>` (`types/mod.rs:49`).
  Continuations clone frames and registers into side tables (`types/continuation.rs:69,79,161,173`).
- **Write sites a barrier would need to cover.** These are `set_car`/`set_cdr` (`heap/mod.rs:743,755`),
  `vector_set` (`:804`), `write_mutable_cell`, which takes `&self` through a `RefCell` (`:1273`), and
  `set_vm_closure_free_var` (`:1394`). A barrier would also need records (`Rc<RefCell<Vec<TV>>>`), parameters,
  promises, ephemerons, and environments, which are `FxHashMap`s outside the heap.
- **Identity hash is address-based.** `tagged_value_hash_identity` = `mix(heap_index)`, except for `Rc`-backed
  procedures and records (`heap/mod.rs:2541-2560`). It is exported as `hash-by-identity` through SRFI 69/125
  (`patina-primitives/src/primitives/equality.rs:66-78`, `lib/srfi/125.sld:133`). A moving collector breaks this
  unless the design handles it (§3.10).
- **GC off mode.** `GcMode::Off` (`PATINA_GC=0`) is the reference run for the differential lanes (`gc.rs:278-291`).
  Note the stale module comment at `gc.rs:25`, which still calls `Off` "the default"; the code makes `On` the default
  (`gc.rs:309-313`).

---

## 1. Where HotSpot stands (September 2026)

| Release | GC change | Source |
|---|---|---|
| JDK 21 | Generational ZGC (JEP 439) | https://openjdk.org/jeps/439 [V] |
| JDK 22 | G1 region pinning replaces GCLocker for JNI critical regions (JEP 423) | https://openjdk.org/jeps/423 [V] |
| JDK 23 | ZGC generational by default (JEP 474). Parallel's full GC replaced by G1's parallel full GC (JDK-8329203) | https://openjdk.org/jeps/474, https://tschatzl.github.io/2024/07/22/jdk23-g1-serial-parallel-gc-changes.html [V] |
| JDK 24 | Non-generational ZGC removed (JEP 490). G1 late barrier expansion (JEP 475). Compact headers experimental (JEP 450). Generational Shenandoah experimental (JEP 404) | JEPs 490/475/450/404 [V] |
| JDK 25 | Compact headers become a product feature (JEP 519). Generational Shenandoah becomes a product feature (JEP 521). G1 merges old-region remsets: 2 GB → 0.75 GB peak on a 64 GB heap | https://tschatzl.github.io/2025/08/12/jdk25-g1-serial-parallel-gc-changes.html [V] |
| JDK 26 | G1 double card table, write barrier ~50 → 12 x64 instructions (JEP 522). AOT object caching with any GC, using a GC-neutral, **index-based** object format (JEP 516). Eager reclaim for all humongous objects | https://openjdk.org/jeps/522, https://openjdk.org/jeps/516, https://tschatzl.github.io/2026/02/26/jdk26-g1-serial-parallel-gc-changes.html [V] |
| JDK 27 (GA 2026-09-15) | **G1 is the default everywhere, including where Serial used to be chosen** (JEP 523). **Compact headers on by default** (JEP 534) | https://openjdk.org/jeps/523, https://openjdk.org/jeps/534 [V] |
| JDK 28 (targeted) | Shenandoah generational by default (JEP 535) | https://openjdk.org/jeps/535 [V] |

**The trend [I, from the above].** All three collector families have converged on *generational + region-based +
the simplest possible barrier*. ZGC and Shenandoah each kept a second mode for years and then retired it, citing
maintenance cost (JEP 474/490/535: maintaining two modes "slows the development of new features"). The JDK 27 default
change is the clearest signal. G1 replaced Serial at small heaps only once its write barrier had been cut down to
roughly a card-table barrier. JEP 523 says G1's maximum throughput is now "close to that of Serial" because of
JEP 522. **The barrier is the throughput tax of a generational design, and HotSpot spent a decade making it smaller.**

---

## 2. The collectors in one paragraph each

**Serial** [R]. Single-threaded. The young generation is a copying collector (eden plus two survivor spaces, Cheney
scan). The old generation is a sliding mark-compact. The write barrier is an unconditional card mark. This is the
closest structural analogue to what Patina can use: one mutator thread and stop-the-world pauses.

**Parallel** [V/R]. Serial's algorithm run with N GC threads. The barrier is the same unconditional byte store, one
byte per 512-byte card, written as `movb $0, (cardbase + addr>>9)`. The card table base is hard-coded into generated
code, and "zero is the dirty value" lets the store use a zero register (https://shipilev.net/jvm/anatomy-quarks/13-intergenerational-barriers/).
Measured against Epsilon on a HashMap-iteration loop, the barriers cost **~20%**: 3 extra stores, ~22 cycles and
12 instructions per operation (same source). That is a reference-store-heavy worst case. Its full GC used to
overwrite headers and had a step quadratic in live objects. JDK 23 replaced it with G1's full GC, which also removed
a 1.5%-of-heap bitmap (JDK 23 post above). **Lilliput also needed objects to be able to *grow* when moved (§3.10),
which the old algorithm could not do.**

**G1** [V]. Equal-sized regions, about 2048 of them, ergonomic maximum 32 MB, configurable 1–512 MB. Young-only
collections alternate with "space-reclamation" mixed collections. Concurrent marking is SATB. Remembered sets are
per region and card-based (512 B cards), built lazily for old regions. The collection set is chosen by efficiency
under a pause goal (`MaxGCPauseMillis=200`). Humongous objects (≥ half a region) get contiguous regions and eager
reclaim. Evacuation failure is handled by self-forwarding and region pinning
(https://docs.oracle.com/en/java/javase/25/gctuning/garbage-first-g1-garbage-collector1.html).
Overheads: Zhao & Blackburn measured G1's barriers at **~12%** of total performance, remset space typically **<1%**,
and showed that concurrent marking and generational collection cut 95th-percentile pauses by 64% and 93%
("Deconstructing the Garbage-First Collector", VEE 2020, https://doi.org/10.1145/3381052.3381320). The marking bitmap
costs 1/64 of the heap (~1.5%) (https://tschatzl.github.io/2022/08/04/concurrent-marking.html).

**Epsilon** (JEP 318) [V]. Bump allocation with no collection and an empty barrier set. Its stated purpose is
differential performance testing, to "filter out GC-induced performance artifacts" including barrier cost
(https://openjdk.org/jeps/318).

**Shenandoah** [V]. Concurrent mark plus concurrent evacuation plus concurrent update-refs. Its stated profile is
pauses of 0–10 ms and "throughput losses … within 0..15%" (https://wiki.openjdk.org/display/shenandoah/Main). It has
been generational since JDK 24/25, reusing card marking "borrowed from the Parallel and CMS" collectors
(https://openjdk.org/jeps/404).

**ZGC** [V]. Colored pointers, load barriers, concurrent relocation, sub-millisecond pauses. The original goal was
"no more than 15% throughput reduction compared to G1". On SPECjbb2015 the published numbers were max-jOPS ZGC 100%
vs G1 91.2%, critical-jOPS 76.1% vs 54.7%, and average pause 1.091 ms vs 156.806 ms
(https://openjdk.org/jeps/333). Generational ZGC adds store barriers and remembered sets. On Cassandra it needs **a
quarter of the heap and gets 4× the throughput** of non-generational ZGC (https://openjdk.org/jeps/439).

**Independent cost accounting** [V]. Cai, Blackburn, Bond and Maas (ISPASS 2022, https://arxiv.org/abs/2112.07880)
measured the OpenJDK 17 collectors against a distilled lower bound. All of them cost **7–82% more wall-clock time and
6–92% more CPU cycles**. "Newer low-pause GCs are significantly more expensive than older GCs, and … sometimes deliver
worse application latency than stop-the-world GCs." LXR (PLDI 2022, https://arxiv.org/abs/2210.17175) argues the same
from the other side: brief, regular stop-the-world pauses with a cheap combined barrier beat G1 by 4% on throughput
and Shenandoah by 43%.

---

## 3. Technique by technique

### 3.1 TLABs and inline bump-pointer allocation (all HotSpot GCs, C1/C2 and the interpreter)

**Mechanism** [V]. Each thread owns a buffer `[top, end)`. The inline fast path is: load `top`, add the size,
compare with `end`, store `top`, then write the header (mark/klass) and zero the fields. On x86 that is about
16 bytes of code. The slow path calls the VM to get a new TLAB or allocate outside one
(https://shipilev.net/jvm/anatomy-quarks/4-tlab-allocation/). The template interpreter has the same inline path in
`TemplateTable::_new` [R].

**Numbers** [V]. Same source: 50 M allocations take 231 ms with TLABs and 2,785 ms without, roughly 10× single-threaded
and 20× with two threads. Most of the no-TLAB cost is the shared CAS plus the VM transition.

**Problem it solves.** It takes allocation off the shared-heap critical path and makes it inlinable in JIT code.

**Verdict for Patina: ADOPT the bump pointer, skip the "thread-local" part** [I]. The multithreading half does not
apply. The other half, a `(top, limit)` pair at a fixed address that both the interpreter and JIT code can bump,
is the single largest allocation win available. Patina cannot do this today:

1. Arena base addresses move whenever a `Vec` grows (§0), so JIT code cannot hold a base pointer across any
   allocation.
2. Objects are 72-byte enum slots built in Rust, which JIT code cannot initialise.
3. Every allocation goes through `RefCell::borrow_mut` and a free-list pop.

**A JIT-friendly heap needs stable, chunked memory, which means regions or blocks.** It does not need "moving" to get
this, but a bump-allocated *nursery* is most valuable when it is evacuated (§3.2).

Keep one Patina-specific property: **allocation never collects**. HotSpot's allocation slow path *is* a safepoint,
which is why its C++ runtime needs `Handle`s and a debug checker (§3.9). If Patina's slow path only fetches a new
nursery chunk and raises the pending flag, the soft limit can be overshot until the next safe point. Rust primitives
then keep their current freedom to hold `TaggedValue`s across allocations, and JIT code needs stack maps only at safe
points, not at every allocation site.

### 3.2 Generational collection: copying nursery and promotion (Serial, Parallel, G1, gen-ZGC, gen-Shenandoah)

**Problem it solves.** Most objects die young, so a nursery collection costs time proportional to *survivors*, not
to the heap. Old-generation work is amortised.

**HotSpot evidence** [V]. Every concurrent collector added generations and then removed its non-generational mode
(table in §1). Generational ZGC: 1/4 of the heap and 4× the throughput on Cassandra (JEP 439). Zhao & Blackburn:
generational collection alone cut G1's p95 pauses by 93%.

**Two refinements worth copying** [V]:

- **Age dense regions in place.** Generational ZGC evaluates "the density of young-generation regions" and leaves
  dense ones in place, either aging them as survivors or promoting them without copying (JEP 439). JEP 423 does the
  same for pinned young regions: they are treated "as having failed evacuation, thus promoting them". **[I]** For
  Patina this means a nursery block that is mostly live, or that is pinned by a Rust borrow, can be relabelled
  "old" instead of copied. That handles both the pinning problem and the copy-cost problem.
- **Do not force large objects into the old generation.** Generational ZGC allocates large objects in young
  regions because it does not relocate those regions. G1 instead puts humongous objects in dedicated contiguous
  regions and reclaims dead ones eagerly at any pause (JDK 26: every humongous type).

**Verdict: ADOPT** [I]. Use a bump-allocated, evacuated nursery. Old objects go to a non-moving or mostly-non-moving
space (mark-sweep or mark-region with opportunistic defragmentation, which other research keys cover). Large objects
go to a non-moving large-object space. Concurrency is irrelevant here: the payoff is pure single-thread throughput,
because survivors are cheap to copy and dead objects cost nothing.

### 3.3 Write barriers and the remembered set: card marking, G1's journey, JEP 522

**Mechanisms and costs** [V]:

- **Parallel/Serial**: an unconditional card mark, 3–4 instructions (JEP 522 implementation RFR,
  https://mail.openjdk.org/pipermail/graal-dev/2025-March/010656.html).
- **G1 before JDK 26**: same-region filter → null filter → young-card filter → **StoreLoad fence** → already-dirty
  filter → mark the card → enqueue it in a thread-local dirty-card queue, flushed to a global set when full. That is
  40–50 instructions, and the RFR says it "prevents some compiler optimizations like loop unrolling or inlining".
  Cited cost: 10–20% throughput.
- **G1 since JDK 26 (JEP 522)**: the post-barrier is the XOR/shift cross-region filter, a null filter, an optional
  conditional-card-mark check, and the card store. The card-table base is loaded from thread-local data
  (`g1BarrierSetAssembler_x86.cpp`,
  https://raw.githubusercontent.com/openjdk/jdk/master/src/hotspot/cpu/x86/gc/g1/g1BarrierSetAssembler_x86.cpp).
  The fence and the queue are gone because mutator and refiner threads now use *different* card tables, swapped
  atomically when enough cards accumulate. Result: **~50 → 12 x64 instructions**, **+5–15% throughput** on
  reference-store-heavy applications, **+≤5%** elsewhere. Cost: a second card table at 0.2% of the heap.
- **G1 elision (JEP 475)**: "~60% of executed write post-barriers can be simplified or removed" through nullness
  analysis. Stores into a newly allocated object need no barrier "as long as there is no safepoint between the
  allocation and the writes".
- **Compensation for elided barriers**: if a slow-path allocation lands in the old generation, the runtime dirties
  every card of the new object in `on_slowpath_allocation_exit` (`cardTableBarrierSet.cpp`,
  https://raw.githubusercontent.com/openjdk/jdk/master/src/hotspot/share/gc/shared/cardTableBarrierSet.cpp).
- **Cross-VM evidence**: Yang et al., "Barriers reconsidered, friendlier still!" (ISMM 2012), report average
  overheads of **0.9% for write barriers** and **5.4% for read barriers**, more exposed on DaCapo than on
  SPECjvm98 [V via abstract/search].

**Verdict: ADOPT a post-write generational barrier with Scheme-specific filters. Reject the double card table**
(it only removes mutator↔refiner-thread synchronisation, which Patina does not have) **and reject concurrent
refinement.** [I] The ordered filter chain for Patina:

1. **Is the stored value an immediate?** That is a fixnum, char, boolean, `()` or unspecified, or a tag test
   `v & 7 < 3` (`tagged_value.rs:76-79`). This is the Scheme counterpart of G1's null filter and should catch far
   more than 60% of stores, because loop counters, characters and booleans are immediates.
2. **Is the target object young?** This is G1's cross-region XOR filter turned into an "is it in the nursery"
   test: a range compare if the nursery is one contiguous reservation, otherwise a block-header or side-table
   lookup.
3. **Record the store**, either with an unconditional card byte (Parallel style, which needs contiguous heap
   addresses) or by appending to a sequential store buffer.

**Elide all initialising stores** into objects allocated since the last safe point. A Scheme baseline JIT emits
`cons`/`vector`/closure construction constantly, so this matters more than it does in Java. If allocation can land
somewhere other than the nursery (for example the large-object space), use G1's compensation trick: the slow path
"pre-dirties" the new object.

### 3.4 SATB concurrent/incremental marking (G1, Shenandoah, the old generation in gen-ZGC)

**Mechanism** [V]. While marking is active, a pre-write barrier enqueues the *overwritten* value into a per-thread
SATB buffer (`if marking_active && old != null: enqueue(old)`). Objects allocated during marking are implicitly live
above TAMS (top-at-mark-start). The fast path when inactive is `cmpb [thread+satb_active], 0; je done`
(G1 assembler above; Shenandoah tests a `gc_state` byte the same way,
https://raw.githubusercontent.com/openjdk/jdk/master/src/hotspot/cpu/x86/gc/shenandoah/shenandoahBarrierSetAssembler_x86.cpp).

**Problem it solves.** It bounds old-generation marking pauses by letting marking overlap the mutator.

**Verdict: DEFER, but reserve the barrier ABI now** [I]. With one thread, "concurrent" collapses to *incremental*:
mark the old generation in slices at safe points. That only pays off if old-generation marking pauses become a
measured problem. Patina's heaps are small, and Stage-5 pause work so far is about root sets, not heap size
(`GC_STAGE5_PRD.md:35-68`). Still, make the JIT barrier ABI **a single `gc_state` byte in the VM context**, tested
on the fast path, as Shenandoah does. A later incremental marker can then add a SATB arm without changing JIT code
shape.

### 3.5 Concurrent relocation: Brooks pointers → Shenandoah LRB, ZGC colored pointers and load barriers

**Mechanisms** [V]:

- **Shenandoah's history.** The original Brooks design added a forwarding word to every object and needed barriers
  on *every* read and write, including primitive fields. JDK 13 replaced these with **load-reference barriers**,
  which enforce a strong to-space invariant where a reference is *loaded* ("definition sites" instead of "use
  sites"). Once the forwarding pointer moved into the old copy's mark word, the extra word went away
  (https://developers.redhat.com/blog/2019/06/27/shenandoah-gc-in-jdk-13-part-1-load-reference-barriers; JDK-8225831).
  The LRB fast path is `testb [thread+gc_state], HAS_FORWARDED; jz stable`, followed by a collection-set bitmap
  lookup.
- **ZGC.** Each in-heap reference is a 64-bit "zpointer" with 16 low metadata bits: 4 remapped bits, 2+2 marked
  young/old bits, 2 finalizable bits, 2 remembered bits and 4 reserved (`zAddress.hpp`,
  https://raw.githubusercontent.com/openjdk/jdk/master/src/hotspot/share/gc/z/zAddress.hpp). On load, the barrier
  tests against a thread-local bad mask and uncolors with a shift whose immediate is patched per GC phase. On store,
  it compares the *old* field value against a patched "store-good" color, then colors the new value with `shl; or`
  (`zBarrierSetAssembler_x86.cpp`).
  - **Colorless stack.** "Object references stored in the JVM stack … are implemented as colorless pointers"
    (JEP 439). Barriers sit only on heap access.
  - **Store barrier buffer.** A 32-entry, 256-byte per-thread buffer of (field address, previous value) defers
    slow-path work, giving a "medium path" (`zStoreBarrierBuffer.hpp`).
  - **Remembered set.** Two bitmaps per old region, one bit per *field*, swapped at each young collection, so "no
    extra memory barriers" are needed.
  - **No multi-mapping.** Generational ZGC dropped it; the old scheme made RSS look ~3× too high.
- **Barrier elision in C2 for ZGC.** Accesses dominated by an allocation or an earlier access to the same field,
  with no safepoint in between, have their barriers elided, extended to variable-index array stores in 2022. Its
  author reports "no significant overall throughput changes" in DaCapo and SPECjvm2008
  (https://mail.openjdk.org/pipermail/zgc-dev/2022-November/001177.html).

**Problem it solves.** These designs move objects *while* mutator threads run, which is how they keep pauses under
1 ms on terabyte heaps.

**Verdict: REJECT the mechanism. It only pays off with concurrent GC threads on spare cores** [I]. A single-threaded
interpreter would pay load-barrier tax on every `car`/`cdr`/`vector-ref` (Yang et al.: read barriers average 5.4%
even in a well-tuned VM) to overlap work with nothing. The *design lessons* do transfer:

1. **Keep the in-register and stack representation barrier-free** (ZGC's colorless roots). With STW evacuation,
   JIT code never needs load barriers at all.
2. **Put whatever barrier you have at the least-hot point** (Shenandoah's definition-site argument). For a
   generational STW collector that is the store, not the load.
3. **Batch slow paths in a buffer** (ZGC's store barrier buffer). A sequential store buffer of slot addresses is a
   reasonable remembered-set representation for a single-threaded VM: precise, flushed at minor GC, with no card
   scanning.
4. **Per-field bitmaps instead of cards** (generational ZGC). Precise remembered sets avoid scanning a whole
   512-byte card. That matters less with Scheme's small objects, but it is an option if cards prove imprecise.
5. **Prefer a VM-context byte over patched immediates** [I]. ZGC patches barrier immediates in compiled code when
   the GC phase changes. Code patching in a Cranelift JIT is awkward (W^X, relocation bookkeeping), so a load from
   the pinned VM context is the Cranelift-friendly equivalent.

### 3.6 Region pinning (JEP 423) and the GCLocker it replaced

**Mechanism** [V]. Before JDK 22, G1 *disabled GC* while any thread was inside a JNI critical region. Users reported
it "blocking their entire application for minutes", along with OOMs and premature VM shutdowns. JEP 423 keeps a
per-region pin count instead. Pinned young regions are promoted as evacuation failures, and pinned old regions are
not evacuated (https://openjdk.org/jeps/423). JDK 25 also fixed the Serial/Parallel GCLocker starvation case
(JDK-8192647, JDK 25 post).

**Verdict: ADOPT the idea, because Patina already has the GCLocker problem** [I]. `GcDeferGuard` disables collection
throughout nested dispatch loops, library loading and desugaring (§0). Stage-5 Priority 2 already names the symptom:
"allocation bursts in exactly the wrong places run unreclaimed" (`GC_STAGE5_PRD.md:70-79`). Under a moving nursery
the guard becomes *more* constraining, because the nursery must grow without bound while a guard is held. HotSpot's
answer has two parts:

1. **Root the boundary explicitly.** Stage 5 already plans this.
2. **Pin only what a Rust borrow actually needs**, for example a string or bytevector buffer handed to I/O, at
   block granularity, and promote pinned nursery blocks in place.

Desugaring and macro expansion hold `TaggedValue`s on the Rust stack by design (`desugarer/mod.rs:1841`). Keeping
those phases non-collecting is acceptable if the nursery has a soft limit.

### 3.7 Humongous and large objects

**Mechanism** [V]. G1 gives an object that is ≥ ½ region its own contiguous regions, never copies it, and reclaims it
eagerly at any pause if nothing references it (JDK 26 extended this from primitive arrays to all humongous types).
Generational ZGC keeps large objects in young regions because they are not relocated.

**Verdict: ADOPT** [I]. Use a non-moving large-object space for vectors, strings and bytevectors above some kilobyte
threshold. Strings are `Vec<char>` at 4 B/char today, so a 64 KB string is a 256 KB allocation. Copying these in a
nursery is wasteful, and Rust code borrowing their buffers wants them to stay still anyway.

### 3.8 Root discovery: interpreter oop maps, compiled oop maps, derived pointers

**Mechanisms** [V]:

- **Interpreter frames.** `OopMapCache` lazily computes, per (method, bci), a 2-bit-per-slot mask that separates oop,
  value and dead slots, using bytecode abstract interpretation (`GenerateOopMap`), and caches it
  (`interpreter/oopMapCache.hpp`).
- **Compiled frames.** C1 and C2 emit `ImmutableOopMap`s keyed by PC offset, *only at safepoints* (calls, polls and
  allocation slow paths). Entries are `oop`, `narrowoop`, `callee_saved` or `derived_oop` (`compiler/oopMap.hpp`).
  Derived (interior) pointers go into a `DerivedPointerTable` during GC and are rebased afterwards ("updated based
  on their base pointers new value and an offset"), which is C2-only complexity.
- **Precise liveness.** JIT oop maps omit dead locals, so an object can be collected mid-method. Java needed
  `Reference.reachabilityFence` as a result (https://shipilev.net/jvm/anatomy-quarks/8-local-var-reachability/).
- **Cranelift today** [V]. Cranelift's 2024 "user stack maps" have the frontend spill live GC refs to stack slots
  before each safepoint, reload them afterwards, and annotate the safepoint. The reload is what makes a *moving*
  GC safe. The register-allocator-based maps were removed because of miscompiles, including field-address
  computations deduplicated across a safepoint (use-after-move) (https://fitzgen.com/2024/09/10/new-stack-maps-for-wasmtime.html).

**Verdict: ADOPT HotSpot's split. Avoid derived pointers entirely** [I]:

1. **Baseline JIT: keep Scheme frames in the VM register stack** (Sparkplug-style). JIT code reads and writes
   `registers[base + r]` in memory and keeps nothing GC-relevant in machine registers across a safe point (calls,
   polls, allocation slow paths). The existing per-PC `register_roots` bitsets then serve JIT frames unchanged,
   because the JIT PC maps back to a bytecode PC. No Cranelift stack maps are needed for this tier. This is the
   exact counterpart of HotSpot using the same `OopMapCache` for every interpreted frame.
2. **A later optimizing tier** that keeps references in native frames should use Cranelift user stack maps, with
   the rule that **no untagged or interior pointer stays live across a safe point**. Recompute `base - tag + offset`
   after every safe point. That avoids both HotSpot's `DerivedPointerTable` and the Cranelift miscompile class.
3. Patina's register-retirement design is roughly "interpreter oop map + ZAP_DEAD_LOCALS" (`oopMapCache.hpp`'s
   optional dead-slot bit). It is already precise enough for expression temporaries; local bindings remain
   conservative (`VM_COMPILER.md` §10.4).

### 3.9 Safepoints, polls, handshakes, and runtime-code rooting

**Mechanisms and numbers** [V]:

- **Poll placement.** Polls go at method returns and loop back-edges. Counted loops are strip-mined into an outer
  loop that carries the poll every `LoopStripMiningIter` = 1000 iterations (JDK-8192977 and the zgc-dev 2017 thread).
- **Polling mechanism.** JDK 10+ polls a *thread-local* word (JEP 312). The global poll cost 0.383 ns/op, the
  thread-local one 0.590 ns/op: about half a cycle for one extra L1-hitting load
  (https://shipilev.net/jvm/anatomy-quarks/22-safepoint-polls/). JEP 312's success bound was ≤1% overhead.
- **Return polls and stack watermarks.** Return polls compare the stack pointer against a per-thread watermark, so
  the same branch also traps "returns to unprocessed frames" (JEP 376, https://openjdk.org/jeps/376).
- **Runtime C++ code.** Raw oops on the C++ stack must be wrapped in `Handle`s across anything that can safepoint.
  Debug builds enforce this with `CheckUnhandledOops`: every stack `oop` registers itself and is overwritten with
  `0xfffffffffffffff1` at a safepoint, so misuse crashes nearby (`runtime/unhandledOops.hpp`).

**Verdict** [I]:

- **ADOPT polls at function entry and loop back-edges.** In Scheme most loops are tail calls, so a poll at entry
  plus one at `TailCall` covers them. The poll should be one `cmp byte [vmctx+pending],0; jne`, which matches today's
  `Cell<bool>` (`vm_state.rs:1291`). Fold other interrupt reasons (Ctrl-C, profiler tick, JIT tier-up) into the
  same word, as HotSpot does with its armed poll word.
- **REJECT thread-local handshakes** (multi-thread only).
- **KEEP Patina's "no GC inside allocation, no Rust temporaries live at a safe point" discipline** (GC_DESIGN
  decision 6). It is what lets Patina avoid HotSpot's `Handle` system entirely, and it is also the precondition that
  makes a *moving* collector tractable: the only roots are the ones `GcRoots` enumerates.
- **ADOPT an `UnhandledOops`-style debug lane for moving.** A "move everything every GC" stress mode, plus poisoning
  of from-space, would turn any missed root into an immediate crash. That generalises today's `GC_POISON` lane.

### 3.10 Object layout: compressed oops, compact headers (Lilliput), identity hash under motion

**Mechanisms and numbers** [V]:

- **Compressed oops.** A 32-bit offset ×8 plus a heap base gives a ~32 GB limit. In zero-based mode decoding folds
  into the addressing mode (`mov 0xc(%r12,%r11,8)`). Crossing 32 GB inflated one dataset by ~40%
  (https://shipilev.net/jvm/anatomy-quarks/23-compressed-references/).
- **Compact headers** (JEP 450/519/534). The 96–128-bit mark+klass header shrinks to one 64-bit word: 22-bit
  compressed class pointer, 31-bit identity hash, 4 Valhalla bits, 4 age bits, a self-forwarded bit and 2 lock
  bits. The forwarding pointer during a sliding GC is encoded in the low 42 bits (≤ 8 TB), and a self-forwarded
  *bit* keeps the class pointer intact. Results: **SPECjbb2015 −22% heap and −8% CPU, −15% GC count** (G1 and
  Parallel), and a JSON parser 10% faster. On by default in JDK 27 (https://openjdk.org/jeps/534).
- **Identity hash under motion** [V]. Lilliput's plan is to derive the identity hash from the object's address while
  it has not moved, set a bit when it is hashed, and **append a hash word when the GC moves a hashed object**
  (https://wiki.openjdk.org/display/lilliput/Main; lilliput-dev 2025). Fewer than 1% of objects are ever hashed or
  locked. This is why the collector must let objects *grow* during a copy, and why Parallel's full GC had to be
  replaced (§2).

**Verdict** [I]:

- **Compressed oops: largely moot.** `TaggedValue` is already 64 bits and Scheme fields hold `TaggedValue`s, so
  32-bit references would only help if field slots became 32-bit, which is not proposed. The *lesson* is that
  decoding must fold into the addressing mode. Patina's current decoding (per-tag arena base load +
  `index*sizeof(slot)` + enum discriminant match) does not fold. A real address in the 61-bit payload
  (8-byte-aligned, tag in the low bits, field access as `[v + off - tag]`) does. This is the representation change
  that makes JIT field access a single instruction.
- **Headers: ADOPT one 64-bit header word on every non-pair object** [I]. It would hold a type code, a length or
  size, and the GC bits: forwarded, hashed, hashed-and-moved, age (if aging), and pinned. Pairs stay headerless at
  16 B with the type in the pointer tag. For headerless pairs, forwarding needs a sentinel in `car`. Patina already
  reserves never-constructed special immediates (`GC_POISON`, `tagged_value.rs:103`), so one more sentinel value is
  cheap. Moving the `Rc`-holding payloads (`Procedure`, `Port`, `CompiledMacro`, `Library` …) out of the 72-byte
  enum slot is a prerequisite, whether into handles in a side table or into native heap layouts.
- **Identity hash: ADOPT Lilliput's hashed/moved bits for headered objects. Pairs need a separate scheme.** Today
  `hash-by-identity` is `mix(heap_index)` (`heap/mod.rs:2553`), so under a moving GC every SRFI 69/125 table keyed
  by identity would silently break. Options for pairs and other headerless objects: (a) a GC-maintained side table
  from hashed object to hash, updated on move (a weak table, like the existing continuation tables); or (b)
  eq-hashtables that the GC rehashes. Other research keys (Chez) cover (b).

### 3.11 Continuations under a GC: Loom stack chunks

**Mechanism** [V]. A virtual thread's frames are *frozen* into `StackChunk` heap objects and *thawed* back lazily
through a **return barrier**, a few frames at a time (`runtime/continuationFreezeThaw.cpp`). The fast path copies raw
frames without parsing them, but only when every frame is compiled **and the chunk does not `requires_barriers()`**.
For G1 that means the chunk is young; for ZGC it means it is in an allocating region. Once a chunk requires
barriers, frames are only ever copied *out* of it, and freezing allocates a new chunk. Old chunks are put into a
"GC mode" with an oop bitmap and relativised derived pointers so the GC sees stable oop locations
(loom-dev, "Semantics of CollectedHeap::requires_barriers()", https://mail.openjdk.org/pipermail/loom-dev/2021-February/002097.html).
Native frames pin the continuation.

**Verdict: ADOPT the shape** [I]. Patina's VM already captures by copying `frames` and `registers`
(`types/continuation.rs:69-79`), but into side tables keyed by ID that need a weak fixpoint (`gc.rs:1022-1093`).
HotSpot's lessons:

1. **Make captured segments ordinary heap objects**, traced by the same per-PC register maps and allocated young.
   A young, freshly written chunk needs no barriers, and Scheme's multi-shot continuations are *immutable* after
   capture (re-entry copies out), so old chunks never need a write barrier.
2. **Lazy thaw through a return/underflow barrier** bounds the re-entry cost for deep captures. This converges with
   Chez's segmented stacks, which other keys cover.
3. **Never put native (Rust or JIT-native) frames inside a capturable segment.** Loom pins in that case. Patina's
   "Rust primitives never call back into the program" rule (AGENTS.md) is the same invariant, and the JIT must
   keep it: JIT frames that a continuation can capture must live in the VM stack (§3.8), not on the native stack.

### 3.12 Concurrent thread-stack processing and stack watermarks (JEP 376)

**Mechanism** [V]. After a safepoint, each thread's stack is logically invalid above a watermark. Returning into an
unprocessed frame trips the return poll, which fixes one frame and moves the watermark. The goal is <1 ms in ZGC
safepoints, with no per-thread root processing in the pause.

**Verdict: REJECT the concurrency. ADOPT a single-thread variant for minor GCs** [I, not taken from HotSpot]. A deep
non-tail recursion (`map` over 10⁶ elements) leaves a huge register stack that every minor GC rescans. Frames below
the lowest depth the mutator has *returned to* since the last GC cannot have been written since, because suspended
frames are immutable until resumed. After a minor GC they reference only old objects, so the next minor GC can skip
them. Tracking costs one `min()` on `Return`, and continuation reinstatement resets the watermark. This is the
generational analogue of JEP 376's watermark (GHC's dirty stack chunks are similar). It should be measured before it
is built.

### 3.13 Late barrier expansion (JEP 475) as an engineering lesson for the Cranelift JIT

**Facts** [V]. Expanding G1 barriers early in C2's IR cost **10–20% of C2's compile overhead**. A single barrier was
">100 operations in C2's IR … around 50 x64 instructions", and early expansion made barrier-ordering bugs hard to rule
out. JEP 475 keeps accesses tagged with barrier information and expands them at emission using **the interpreter's
assembly-level barrier code**. "A naive implementation … without any optimizations, already achieves quality close
to that of C2-optimized code."

**Verdict: ADOPT** [I]. Emit barriers late, from one definition shared with the interpreter. In CLIF, that means one
emitter function producing the fast path inline plus a cold-block call to a single Rust slow path, the same function
the interpreter calls. Do not let Cranelift's mid-end see a barrier split by a safepoint. The fitzgen miscompile
(§3.8) is exactly that hazard. Keep barriers small enough (≤ ~8 instructions on the fast path) that inlining and
unrolling decisions are not distorted, which was G1's pre-JEP-522 problem.

### 3.14 Heuristics and sizing

**Facts** [V]. G1 sizes the heap from a GC-CPU-time goal (JDK 26 lowered the default from 8% to 4%; automatic heap
sizing is JDK-8359211) and has a pause-time goal. It also has a GC overhead limit (98% CPU and 2% free) and adaptive
IHOP for when to start marking.

**Verdict: ADOPT the shape of the policy** [I]. Patina's trigger counts allocations (`max(min, 2×live)`). It should
also use the measured GC time fraction, which is what G1 converged on, so small and large programs both get bounded
overhead. Keep the trigger decision in `note_alloc`, which is already O(1) and borrow-free (`gc.rs:20-27`).

---

## 4. Applicability matrix

| Technique | Solves | Published cost | Needs multicore? | Patina verdict |
|---|---|---|---|---|
| Bump-pointer allocation (TLAB fast path) | Allocation cost, JIT inlining | ~10× vs. no TLAB (incl. CAS) | No (the TL part, yes) | **Adopt** (stable chunked memory) |
| Copying nursery + promotion | Cheap reclamation of short-lived data | Barrier tax (below) | No | **Adopt** |
| In-place aging of dense or pinned young blocks | Copy cost, pinning | — | No | **Adopt** |
| Card-marking post-barrier | Old→young roots | 3–4 instr; ~20% worst-case loop; 0.9% avg (Yang) | No | **Adopt** with immediate + young filters |
| G1 cross-region + null filters | Barrier frequency | ~60% of post-barriers removable | No | **Adopt** as immediate/young filters |
| Init-store elision + slow-path compensation | Barrier frequency on new objects | — | No | **Adopt** |
| Double card table (JEP 522) | Mutator↔refiner sync | 0.2% of heap | **Yes** | Reject |
| Per-region remsets / mixed GCs | Incremental old evacuation under a pause goal | <1% space; complex | Mostly | Reject for now |
| SATB pre-barrier | Concurrent/incremental marking | 1 load + branch when idle | Pays off with concurrency | Defer; reserve `gc_state` byte |
| Load barriers (LRB, colored pointers) | Concurrent relocation | ~5.4% avg read barrier; ZGC ≤15% vs G1 goal | **Yes** | Reject |
| Store barrier buffer (ZGC) | Batching barrier slow paths | 32 × 16 B/thread | No | Consider as SSB remset |
| Region pinning (JEP 423) | GC disabled during raw borrows | — | No | **Adopt**; replaces `GcDeferGuard` uses |
| Humongous / large-object space | Copying big objects | — | No | **Adopt** |
| Oop maps per PC; interpreter OopMapCache | Precise roots | — | No | **Have it** (`register_roots`); reuse for baseline JIT |
| Derived-pointer tables | Interior pointers across safepoints | C2-only complexity | No | Reject; ban interior pointers across safe points |
| Thread-local poll word | Cheap polls, per-thread ops | ~0.2 ns/poll | Partly | Adopt one VM-context flag word |
| Handshakes, concurrent stack scanning | Per-thread pauses | <1 ms target | **Yes** | Reject; consider minor-GC stack watermark |
| Compressed oops | Footprint | Decode folds into addressing | No | Moot; the folding lesson applies |
| Compact 64-bit header + hashed/moved bits | Footprint; hash under motion | −22% heap, −8% CPU (SPECjbb) | No | **Adopt** header; pairs headerless |
| Loom stack chunks (young ⇒ barrier-free) | Continuations under GC | — | No | **Adopt** shape |
| Late barrier expansion | JIT compile cost, correctness | C2 +10–20% compile overhead before | No | **Adopt** (one barrier definition) |
| Epsilon / LBO methodology | Measuring GC cost honestly | — | No | **Adopt**; `GcMode::Off` is Patina's Epsilon |

---

## 5. Constraints for the redesign, through the HotSpot lens [I]

1. **Generational, stop-the-world, one collector.** HotSpot's 2023–2026 consolidation (§1) argues against keeping a
   menu of collectors behind `Collector`. Build one good generational design. Keep the trait seam for testing only.
2. **The write barrier budget is the main design constraint.** G1 had to fall to ~12 instructions before it could
   replace Serial. Patina's fast path should be ≤ ~6 instructions: tag test, nursery test, record. Measure it the
   way Stage 5 already demands (interleaved A/B), with `GcMode::Off` as the Epsilon baseline.
3. **Moving is feasible because of rules Patina already has.** No GC inside allocation, no live Rust temporaries at
   safe points, and no Rust frames inside continuations. Together these give Patina what HotSpot needed `Handle`s,
   `CheckUnhandledOops` and Loom pinning to get. The obstacles that remain are the ones `GC_DESIGN.md` §3.4 lists:
   raw indices or bits escaping into `symbol_table`, `SourceMap`/syntax provenance keyed by `raw_bits` (32 call
   sites across 10 files), `CallFrame.closure`, code-object constants, `CompiledMacro` literals, and environment
   maps. These are *root-enumeration and re-keying* problems, not reasons to stay non-moving. Every one must become
   a traced, updatable root or move into the heap.
4. **Stable addresses are needed regardless.** Whether or not anything moves, the JIT needs heap memory that does not
   relocate when a `Vec` grows, plus a fixed VM-context block holding `alloc_top`, `alloc_limit`, `gc_pending`,
   `gc_state`, the nursery bounds and the remset buffer pointer.
5. **JIT frames live in the VM stack.** That gives roots through the existing `register_roots`, capture by memcpy,
   and no native frames inside a capturable continuation. Cranelift user stack maps are only for a later tier, under
   the rule of no interior pointers across a safe point.
6. **Pin by region, never by disabling GC** (the JEP 423 lesson), and give the nursery a soft limit so
   desugaring and library loading can keep deferring safely.
7. **A header word is needed on non-pair objects** for type, size and GC bits (forwarded, hashed, moved, age,
   pinned). The identity hash must survive motion: Lilliput-style expansion for headered objects, and a side table
   for pairs.
8. **Barrier and poll ABI as one byte or one word** loaded from the VM context rather than patched immediates. That
   leaves room for incremental SATB marking later without touching JIT code shape.

## 6. Open questions

- Will the Cranelift tier keep Scheme frames in `ExecutionState::registers` (Sparkplug-style), or on the native
  stack? This single decision determines whether JIT stack maps, native-frame walking for `call/cc`, and Loom-like
  freeze/thaw are needed at all.
- Is multithreading (SRFI 18, futures, parallel GC threads) ever in scope? If so, TLABs, SATB buffers and
  handshake-style polls become relevant, and the barrier ABI should not assume a single mutator.
- How many of the 32 `raw_bits()` uses key long-lived maps (needs re-keying or a GC-updated weak table) rather than
  transient comparisons (harmless under STW moving)?
- What share of Patina's stores are immediates vs. heap pointers, and how many store into young objects? This
  decides whether an unconditional card mark (simplest, needs contiguous addresses) or a filtered SSB is cheaper.
  It needs an instrumented run over the chibi suite and benchmarks.
- What does a typical Patina heap look like (MBs vs GBs)? G1-style per-region remsets and pause-goal-driven mixed
  collections look like overkill below ~1 GB, but that is unmeasured.
- Should environments (`FxHashMap<String, TaggedValue>` outside the heap) become heap objects? Under a moving GC they
  are otherwise a large, irregular root set that must be updated in place on every collection.
- Is a minor-GC stack watermark (§3.12) worth building? That depends on measuring deep-recursion workloads under
  frequent minor GCs.

## Sources (primary)

- JEPs: 312, 318, 333, 376, 404, 423, 439, 450, 474, 475, 490, 516, 519, 521, 522, 523, 534, 535 — https://openjdk.org/jeps/<n>
- OpenJDK source (master): `gc/z/zAddress.hpp`, `cpu/x86/gc/z/zBarrierSetAssembler_x86.cpp`, `gc/z/zStoreBarrierBuffer.hpp`,
  `cpu/x86/gc/g1/g1BarrierSetAssembler_x86.cpp`, `cpu/x86/gc/shenandoah/shenandoahBarrierSetAssembler_x86.cpp`,
  `gc/shared/cardTableBarrierSet.cpp`, `compiler/oopMap.hpp`, `interpreter/oopMapCache.hpp`, `runtime/unhandledOops.hpp`,
  `runtime/continuationFreezeThaw.cpp`, `oops/instanceStackChunkKlass.hpp` (via raw.githubusercontent.com/openjdk/jdk/master/src/hotspot/…)
- Thomas Schatzl: JDK 23/24/25/26 G1/Parallel/Serial posts and "Concurrent Marking in G1" (tschatzl.github.io)
- JEP 522 implementation RFR (graal-dev, 2025-03): https://mail.openjdk.org/pipermail/graal-dev/2025-March/010656.html
- Aleksey Shipilev, JVM Anatomy Quarks #4, #8, #13, #22, #23: https://shipilev.net/jvm/anatomy-quarks/
- Roman Kennke, Shenandoah LRB: https://developers.redhat.com/blog/2019/06/27/shenandoah-gc-in-jdk-13-part-1-load-reference-barriers
- Shenandoah wiki: https://wiki.openjdk.org/display/shenandoah/Main; Lilliput wiki: https://wiki.openjdk.org/display/lilliput/Main
- Oracle G1 tuning guide (JDK 25): https://docs.oracle.com/en/java/javase/25/gctuning/garbage-first-g1-garbage-collector1.html
- ZGC barrier elision RFR: https://mail.openjdk.org/pipermail/zgc-dev/2022-November/001177.html
- Loom `requires_barriers` thread: https://mail.openjdk.org/pipermail/loom-dev/2021-February/002097.html
- Papers: Zhao & Blackburn VEE 2020 (G1 deconstructed); Yang et al. ISMM 2012 (barriers); Cai et al. ISPASS 2022
  (https://arxiv.org/abs/2112.07880); Zhao, Blackburn & McKinley PLDI 2022 LXR (https://arxiv.org/abs/2210.17175)
- Cranelift user stack maps: https://fitzgen.com/2024/09/10/new-stack-maps-for-wasmtime.html
