# JavaScript engines' GC and rooting: what Patina's GC redesign should take from V8, SpiderMonkey and JavaScriptCore

Sources studied (2026-09-30): V8 `main`, SpiderMonkey (`mozilla-firefox/firefox` `main`), WebKit `main`, Nova `main` and Boa `main`. I fetched individual files as raw files into a local cache (not retained) and grepped them, and read primary blog posts and docs (URLs inline). Line numbers refer to those `main` snapshots and will drift. Patina facts cite `patina:` paths at HEAD `28a94f8`.

Legend: **[V]** = verified in source, docs or the primary post cited. **[I]** = my inference or recommendation.

---

## 0. Executive summary

1. **All three engines collect only where the runtime knows every root. They differ in how they make native code tell the collector.** V8 uses precise, indirect handles (`HandleScope`/`Handle<T>`) and a clang static checker (gcmole). SpiderMonkey (SM) uses precise `Rooted<T>`/`Handle<T>` and a GCC-plugin static hazard analysis. JSC scans the native stack and registers conservatively and never moves objects. **[V]** Patina already has a fourth, cheaper answer: *collection happens only at backend-loop safe points, never inside allocation, and nested loops defer.* That answer covers exactly what SM needed three years (2010 to 2013) and a static analysis to obtain. **[I]**
2. **Exact roots are what make moving GC possible. Conservative roots force pinning.** SM moved from conservative scanning (June 2010) to exact rooting (full browser passing on 2013-06-25) *specifically* so it could add a nursery and compaction ([Clawing Our Way Back To Precision](https://blog.mozilla.org/javascript/2013/07/18/clawing-our-way-back-to-precision/)). V8 is going the other way, for ergonomics. That forced it to build object pinning and "quarantined pages" into its copying scavenger (`src/heap/scavenger.cc:827-850,1063-1125`), and as of `main` the direct-handle + CSS build is still **off by default** (`gni/v8.gni:222`, `src/flags/flag-definitions.h:438-459`). **[V]**
3. **The case for V8's switch is measured and modest.** The study migrated half of V8 (743 files, 73,544 lines). On Speedometer 2.1, direct references with CSS were 1-12% faster on 9 subtests and 2-6% slower on 5. 17,000 `HandleScope` creation sites would go away. Over 80% of V8 GCs happen in the event loop with *no* native frames on the stack (Hughes, *Retro-fitting Conservative GC*, KCL thesis 2023, ch. 3). **[V]**
4. **A baseline JIT that mirrors the interpreter frame needs no stack maps.** V8's Sparkplug keeps Ignition's frame layout ([v8.dev/blog/sparkplug](https://v8.dev/blog/sparkplug)). SM's Baseline frames hold boxed `Value`s that the GC traces by tag, clearing dead locals using bytecode liveness (`js/src/jit/BaselineFrame.cpp:27-110`). Before every interrupt check, Baseline syncs its virtual stack to the frame (`BaselineCodeGen.cpp:1593-1614`). Only optimizing tiers carry safepoint maps: TurboFan has per-safepoint tagged-slot bitmaps, and Maglev has just a tagged/untagged split point plus a register mask (`src/codegen/safepoint-table.h:27-78`, `maglev-safepoint-table.h:29-55`). **[V]**
5. **A safepoint poll is one load, one compare and one branch at loop heads and function entry.** SM checks `runtime->interruptBits` at `LoopHead` and `Finally` (`BaselineCodeGen.cpp:2530-2537,5060-5063`). V8 folds GC requests into the stack-limit check by setting `jslimit = kInterruptLimit` (`src/execution/stack-guard.cc:38`, `stack-guard.h:70,193`). Patina polls a `Cell<bool>` before *every* instruction (`patina:crates/patina-vm/src/runtime/vm_state.rs:1205,1287-1291`). **[V]**
6. **Inline allocation is about five instructions against a two-word (position, end) pair at a fixed address.** In SM, `MacroAssembler::bumpPointerAllocate` loads, adds, compares, stores and subtracts (`MacroAssembler.cpp:654-680`). JSC bump-allocates inside free *intervals* of a non-moving block (`FreeListInlines.h`, `AssemblyHelpers.cpp:916-957`). Patina's `alloc_pair` pops a free list or pushes onto a `Vec` behind `Rc<RefCell<Heap>>` (`patina:heap/mod.rs:51,703-714`). A JIT cannot inline that, and the arena base address *moves* whenever the `Vec` grows. **[V]/[I]**
7. **Three barrier designs are viable, and each engine unifies its generational and marking barriers in one check.**
   - V8 checks a host page flag, then a value page flag (`heap-write-barrier-inl.h:52-90`), and records into per-page slot bitmaps.
   - SM's post-barrier tests "value in nursery?", then "host in nursery?", then calls out of line to a store buffer with a one-element cache (`BaselineCodeGen.cpp:604-655,3330-3341`). SM also has a separate snapshot-at-the-beginning (SATB) pre-barrier that is guarded by a per-zone flag.
   - JSC loads one header byte of the *host*, compares it with a global threshold and branches (`AssemblyHelpers.h:1319-1339`). That costs about 0% outside GC and about 5% during concurrent marking ([Riptide](https://webkit.org/blog/7122/introducing-riptide-webkits-retreating-wavefront-concurrent-garbage-collector/)).

   **[V]**
8. **Pointer compression is a different thing from Patina's u32 indices.** V8's win (heap up to 43% smaller, renderer up to 20% smaller) came from storing **32-bit fields inside heap objects**. That cost a 31-bit Smi and unboxed double fields ([v8.dev/blog/pointer-compression](https://v8.dev/blog/pointer-compression)). Patina stores a u32 index in a **64-bit** slot, so it pays the decode cost (base + index × scale, with a per-type base) without the memory saving. **[V]/[I]**
9. **Conservative scanning works when references are sparse.** JSC rejects a candidate word with a tiny Bloom filter, then a block hash set, then atom alignment, then a liveness bit (`heap/HeapUtil.h:50-90`). Oilpan found that scanning *compressed halfwords* conservatively cut its compression gain from about 24% to 21% ([oilpan-pointer-compression](https://v8.dev/blog/oilpan-pointer-compression)). Patina's references are dense small integers with a 3-bit tag, which makes them about the worst case for conservative scanning. **[V]/[I]**
10. **Rust can express what C++ needs plugins for, but the ergonomic cost is real.**
    - Nova, a Rust JS engine with typed vectors and u32 indices like Patina, enforces "no unrooted handle across a GC point" with lifetimes (`GcScope`/`NoGcScope` and reborrowing). It reports about 800 bind/unbind sites in roughly 100k lines, and it **compacts** its typed vectors by rewriting every index through a shift table (`nova_vm/src/heap/heap_bits.rs:1345-1400`).
    - Servo uses a custom lint (crown's `unrooted_must_root`).
    - Boa infers roots by subtracting in-heap reference counts from total reference counts.

    **[V]**

---

## 1. Patina baseline (only what the comparisons need)

- **Value encoding.** `TaggedValue(u64)` has a 3-bit low tag. Tags `011`-`111` are pair, vector, string, closure and object references whose payload is a `HeapIndex = u32` (`patina:crates/patina-core/src/tagged_value.rs:27,55-82,376-413`). Fixnums are 61-bit with tag `000` (`:117-124`). **[V]**
- **Heap.** The heap is a set of typed `Vec` arenas (`pairs: Vec<(TV,TV)>`, `vectors: Vec<Vec<TV>>`, `strings: Vec<Vec<char>>`, `objects: Vec<HeapObjectData>`) plus per-arena free lists (`patina:heap/mod.rs:304-376`). `SharedHeap = Rc<RefCell<Heap>>` (`:51`). `HeapObjectData` slots are 72 B (measured by the companion `understand/vm-runtime.md`). **[V]**
- **Allocation never collects.** "Callers may hold partial structures in Rust locals across `alloc_*` calls" (`patina:heap/mod.rs:697-700`). `note_alloc` raises a shared pending flag (`:570-584`). **[V]**
- **Safe point.** `maybe_collect` runs before every VM instruction (`patina:vm_state.rs:1205`). Its fast path is one `Cell<bool>` load plus the hoisted `is_outermost` constant (`:1287-1291`, `heap/gc.rs:381-397`). Every dispatch loop takes a `GcDeferGuard`, so a nested loop never collects (`vm_state.rs:1142-1154`). **[V]**
- **Dead-register retirement.** Before collecting, `retire_registers` clears registers that the compiler's per-PC liveness maps exclude (`vm_state.rs:1291`; `docs/GC_DESIGN.md:363,372-381`). SM does the same with `calculateLiveFixed(pc)` in `BaselineFrame::trace` (§4.6). **[V]**
- **Root API shape.** `GcRoots::trace_roots(&self, &mut GcVisitor)`, and `GcVisitor::visit(tv: TaggedValue)` takes the value **by copy** (`heap/gc.rs:177-178,485`). That interface cannot express a moving collector, which must update root *slots*. Compare V8's `RootVisitor::VisitRootPointers(FullObjectSlot start, end)` (`scavenger.cc:1073-1085`) and SM's `TraceRoot(trc, &ptr, name)` (`JitFrames.cpp:1098-1131`). **[V]/[I]**
- **Non-moving rationale.** `docs/GC_DESIGN.md:122-139` lists seven places where raw indices escape: the symbol table, raw-bit keys in `SourceMap`, `eq?` hashing, `CallFrame.closure: Option<HeapIndex>`, code-object constants, `CompiledMacro` literals, and `Rc` environment maps. **[V]** §4 and §6 show how the JS engines dealt with each class.

---

## 2. V8

### 2.1 Heap and young generation

- **Orinoco generations.** Orinoco has a nursery and an intermediate generation (together the young generation) plus an old generation. The young generation is a semi-space scavenger, and objects that survive two scavenges are promoted ([trash-talk](https://v8.dev/blog/trash-talk), [orinoco-parallel-scavenger](https://v8.dev/blog/orinoco-parallel-scavenger)). **[V]**
- **Parallel scavenger.** It interleaves copying and pointer updating, uses work stealing over segmented worklists, and gives each GC thread its own local allocation buffer (LAB). Remembered sets are per page, "which naturally distributes the roots set among garbage collection threads". It cut main-thread young-GC time by 20-50% (about 55% on real sites). A rejected alternative, a three-phase parallel mark-evacuate, lost on mostly-dead heaps. **[V]**
- **Defaults.** Pages are 256 KiB. The default maximum semi-space is 32 MiB, or 8 MiB on low-end Android (`src/heap/heap.cc:4830-4847`). With `--minor-ms` the young-generation capacity is 72 MiB (`:4831-4833`). **[V]**
- **MinorMS** is a *non-moving* mark-sweep nursery. It promotes whole pages and recycles a page with at least 50% free space after a minor GC; pages age over four minor GCs before promotion ([wingolog 2023-12-08](https://wingolog.org/archives/2023/12/08/v8s-mark-sweep-nursery)).
  - Its motivation was CSS. Wingo lists "no need for 2x space" and "direct handles instead of indirect" as its opportunities.
  - It is still `DEFINE_EXPERIMENTAL_FEATURE(minor_ms, …)` on `main` (`flag-definitions.h:3782`).
  - A build-time `sticky_mark_bits` mode implies MinorMS and *disables compaction* (`:3806-3810`).
  - The combined barrier has a sticky-mark-bit variant (`heap-write-barrier-inl.h:28-59`).

  **[V]**

### 2.2 Marking

- **Barrier and mark bits.** Concurrent marking uses two mark bits per object (white 00, grey 10, black 11). The concurrent barrier is Dijkstra-style. If the *value* is white, it moves the value to grey with an atomic compare-and-swap (CAS) and pushes it on the worklist. The host-colour check was dropped to avoid a fence ([concurrent-marking](https://v8.dev/blog/concurrent-marking)). **[V]**
- **Races with layout changes.** Worker threads snapshot fields with atomic loads. Objects whose layout can change unsafely (code, maps, weak collections) go to a main-thread-only "bailout worklist". **[V]**
- **Worklist.** Thread-local segments are published to a global pool for stealing. **[V]**
- **Results.** Main-thread marking time per cycle fell 65% on mobile and 70% on desktop (Chrome 64). **[V]**
- **Sweeping.** Concurrent sweeping (Oilpan) cut main-thread sweeping by 25-50%, 42% on average ([high-performance-cpp-gc](https://v8.dev/blog/high-performance-cpp-gc)). Idle-time GC cut Gmail's idle heap by 45% (trash-talk). **[V]**

### 2.3 Write barrier and remembered set

- **Combined barrier fast path** (`heap-write-barrier-inl.h:52-90`):
  1. Find the host's `MemoryChunk` by masking the address (`memory-chunk.h:145,175`).
  2. If the host chunk does not have `POINTERS_FROM_HERE_ARE_INTERESTING`, return. This is the likely path: a young host needs no barrier.
  3. If the value's chunk does not have `POINTERS_TO_HERE_ARE_INTERESTING`, return.
  4. Otherwise take a slow path that handles both the generational insertion and the marking barrier (`IsMarking()`).

  The flags are bits in the chunk header (`memory-chunk.h:64-74`). One barrier serves both purposes because the page flags are flipped when marking starts. **[V]**
- **Remembered set.** Each page has a `SlotSet`: a bitmap with one bit per tagged-size slot, organised in lazily allocated buckets (`src/heap/slot-set.h:127-132`). Pointers embedded in code objects go in a separate typed-slot list (`:268-274`). Insertion deduplicates for free. **[V]**
- **Elision.** Maglev skips barriers on stores into an object it just allocated, but "once there has been another potential allocation" it must emit them again ([v8.dev/blog/maglev](https://v8.dev/blog/maglev)). **[V]**

### 2.4 Pointer compression and the sandbox, compared with Patina's indices

- **Design.** All objects live in a 4 GiB cage. A heap reference is a 32-bit offset from a base that is kept in the root register. ([pointer-compression](https://v8.dev/blog/pointer-compression)) **[V]**
  - Branchful decompression (3-4 instructions) beat branchless by 7% on x64.
  - The final "Smi-corrupting" scheme adds the base unconditionally, even to Smis, in 2 instructions, a further 2.5% win.
- **Results.** The V8 heap shrank by up to 43% and the renderer by up to 20% on desktop. The Octane gap closed from about 35% to about 3%. The remaining cost came from losing 32-bit Smis (about 1%) and double-field unboxing (about 3%). **[V]**
- **Oilpan compression** ([oilpan-pointer-compression](https://v8.dev/blog/oilpan-pointer-compression)) **[V]**
  - It shifts right by 1 inside a 4 GiB cage aligned so that one bit marks "valid heap pointer". This distinguishes null from a sentinel without a branch.
  - Decompression is sign-extend, shift left, then AND with a base whose low 32 bits are all ones (13 bytes of x64).
  - Windows memory fell 21% at P50 and 33% at P99. The first prototype cost 15% on Speedometer; the shipped version had no notable regression.
  - Scanning the stack conservatively for *halfwords* reduced the gain from an estimated 24% to 21%.
- **V8 sandbox** ([v8.dev/blog/sandbox](https://v8.dev/blog/sandbox)). It reserves 1 TB, uses 40-bit offsets, and reaches off-heap objects through an external pointer *table* of indices, at about 1% overhead. **[V]** V8 thus uses table indices exactly where Patina uses them, but only for rare off-heap objects.
- **What this means for Patina** **[I]**
  - Patina's u32 index already *is* a compressed reference, but every slot that holds one is still 8 bytes.
  - The JIT decode is `arena_base[type] + idx * sizeof(slot)`. That needs (a) the tag to pick the arena (five tags plus the object sub-tag), (b) a scale, and (c) a base that is **not stable**, because `Vec::push` reallocates.
  - V8's base never moves for the life of the isolate. That stability is what makes "base in a pinned register" work.
  - Shrinking Patina's slots to 32 bits, V8-style, would cap heap-slot fixnums at about 31 bits. V8 measured 1% for that loss, but Scheme code (bignum thresholds, `exact-integer?` loops) would see far more fixnum-to-bignum promotion. The other lever is to keep 64-bit slots and make the payload an address, or an offset within a fixed reserved region, so that decode is a single `v - tag` folded into the load displacement, as Chez and SM do. This is the more Scheme-appropriate choice.

### 2.5 Rooting: handles, gcmole, direct handles and CSS

- **Indirect handles.** A `Handle<T>` points to a slot in a handle block. Creating one loads `data->next`, compares it with `limit`, stores the value and bumps the pointer (`src/handles/handles-inl.h:276-300`). `HandleScope` saves and restores `next`/`limit` and a level counter (`:225-251`). `CloseAndEscape` re-creates one handle in the parent scope (`:254-274`). The default `Handle::New` does not canonicalize (`:44-48`); canonicalization belongs to `CanonicalHandleScope`. **[V]**
- **Cost and hazard.** Every dereference is a double indirection. Leaking a raw `Tagged<T>` past a call that can GC is undefined behaviour, which **gcmole** catches. gcmole is a clang plugin that builds a call graph of GC-causing functions, flags raw object pointers live across such calls, and also flags evaluation-order hazards with handles. It runs in CI ([tools/gcmole/README](https://chromium.googlesource.com/v8/v8/+/main/tools/gcmole/README)). Suppression is `DisallowGarbageCollection` scopes. **[V]**
- **Embedder handles** ([v8.dev/docs/embed](https://v8.dev/docs/embed)) are `Local`, `EscapableHandleScope`, `Persistent`/`Global` (optionally weak, with a callback) and `Eternal` (cheaper and never freed). **[V]**
- **Direct handles + CSS.** `DirectHandleBase` exists only when CSS is enabled (`include/v8-handle-base.h`). Turning CSS on implies `scavenger_conservative_object_pinning` (`flag-definitions.h:438-459`). **[V]**
  - The scavenger pins objects it reaches from the stack: it records them, marks their pages *quarantined*, and may promote quarantined pages wholesale (`scavenger.cc:1063-1125`).
  - Quarantined pages must be swept before the next GC, because a later conservative scan that hits a stale dead object "can result in memory corruptions" (`:835-838`).
  - Stress flags treat precise references as conservative to test pinning (`flag-definitions.h:465-480`). Chromium carries a clang rewriting tool, `tools/clang/v8_handle_migrate/`.
  - Default: `v8_enable_direct_handle = false` on `main` (`gni/v8.gni:222`). I did not check whether Chromium's own GN args override it.
- **The thesis experiment** (Hughes 2023, ch. 3; V8 11.2, Chrome 112) **[V]**
  - Handles were migrated in 743 files (73,544 lines, about half of V8). The first, wholesale migration attempt failed; the second hid the change behind a build flag with a `DirectHandle` type that shares `Handle`'s API.
  - The scanner borrowed Oilpan's register-spilling stubs, filtered by page, and found object starts by scanning the allocator free list, which is slow.
  - The young generation and compaction had to be disabled whenever native frames were on the stack.
  - Results: 1-12% faster on 9 of 16 Speedometer 2.1 tests, 2-6% slower on 5. Profiles showed the embedder API about 15% faster and partially migrated call paths about 30% slower, from conversion overhead.
  - Ergonomic argument: 17,000 `HandleScope` sites. Applicability argument: over 80% of GCs run in the event loop with no native frames (Chrome M110 UMA).

### 2.6 JIT integration

- **Ignition/Sparkplug.** Sparkplug keeps Ignition's frame layout: interpreter registers sit in their usual stack slots, and only the bytecode-offset slot changes meaning. Debuggers, OSR and stack walking therefore see "an interpreter frame", and the GC scans the register file as tagged. Sparkplug calls builtins for most work instead of inlining it. It improved Speedometer by 5-10%. **[V]** (blog)
- **Maglev.** Compilation is about 10x slower than Sparkplug and 10x faster than TurboFan. Maglev "split[s] the stack frame into a tagged and an untagged region, and only store[s] this split point". A `MaglevSafepointEntry` holds `num_tagged_slots`, `num_extra_spill_slots` and a `tagged_register_indexes` mask (`maglev-safepoint-table.h:29-55`). **[V]**
- **TurboFan.** Each `SafepointEntry` holds a tagged-slot bitmap (`safepoint-table.h:27-78`). **[V]**
- **Safepoint polls.** `StackGuard::RequestInterrupt` with `GC_REQUEST` sets `jslimit` to `kInterruptLimit` (`stack-guard.cc:38,169`; `stack-guard.h:70,193`). The stack-overflow check that JIT code already performs at function entry and loop back-edges therefore doubles as the GC poll at zero extra cost. **[V]**

### 2.7 Oilpan/cppgc (V8's C++ collector)

- **Root model.** On-heap edges are `Member<T>`, off-heap roots are `Persistent<T>`, and the native stack is scanned conservatively. Oilpan prefers to collect when there is "no interesting stack" (high-performance-cpp-gc). **[V]**
- **Non-moving young generation.** It uses an **age table**: a bytemap with one `{old, young, mixed}` entry per 4 KiB card of the caged heap, which the write barrier reads to check the generation (`include/cppgc/internal/caged-heap-local-data.h:32-52`). The cage makes "card of address" a subtract and a shift. **[V]**

---

## 3. SpiderMonkey

### 3.1 Heap shape

- **Layout constants.** Arenas are 4 KiB (`ArenaShift = 12`) and chunks 1 MiB (`ChunkShift = 20`). Cells align to 8 B, with 2 mark bits per 8-byte unit (`js/public/HeapAPI.h:40,55,59,64`). An arena header holds `firstFreeSpan` and `allocKind`. Free cells form linked *spans* of contiguous free cells (`js/src/gc/Heap.h`). **[V]**
- **Nursery test.** A cell is in the nursery iff its chunk header's `storeBuffer` pointer is non-null, which is one mask and one load (`HeapAPI.h:730`, `IsInsideNursery`). **[V]**
- **Nursery size.** Minimum 256 KiB, default maximum 64 × 1 MiB (`gc/Scheduling.h:350-353`, `HeapAPI.h:498`), and a 4 ms time goal per minor GC (`Scheduling.h:506-508`). **[V]**
- **Promotion.** Semispace nursery mode exists but is **off by default** (`Scheduling.h:564-565`), so every nursery survivor is promoted after one minor GC. Promotion is steered by pretenuring per *allocation site*: Baseline bails to the slow path when a site carries `LONG_LIVED_BIT` (`MacroAssembler.cpp:303-330`). **[V]**
- **Heap facts from the docs.** The heap is "precise, incremental, generational, partially concurrent, parallel, compacting, partitioned". Zones are independently collectable. Root marking, compaction and the start of sweeping are not incremental ([firefox-source-docs js/gc](https://firefox-source-docs.mozilla.org/js/gc.html)). **[V]**
- **GC results.** **[V]**
  - Parallel marking (2024): 20-30% less GC time and 10% lower maximum pause. In telemetry, mark rate rose 50-60% and median total GC time fell 35% ([SpiderMonkey newsletter 124-125](https://spidermonkey.dev/blog/2024/03/20/newletter-firefox-124-125.html)).
  - Compaction sorts arenas by free cells, relocates them with forwarding pointers, then updates pointers in parallel per zone. It moves **only JS objects**: strings, symbols, scripts, shapes and JIT code stay put. It runs on OOM, under memory pressure, or after about 20 s of idleness, and recovered 29.4 MiB, more than 8% of the wasted JS heap ([hacks 2015](https://hacks.mozilla.org/2015/07/compacting-garbage-collection-in-spidermonkey/)).
  - The generational GC microbenchmark went from 15 ns to 6 ns per iteration on an empty heap and from 27 ns to 6 ns on a full one ([hacks 2014](https://hacks.mozilla.org/2014/09/generational-garbage-collection-in-firefox/)).

### 3.2 History: conservative to exact, in order to move

- **2010.** SM adopted conservative scanning in June 2010 because it needed no embedder changes and cost the mutator nothing.
- **Why it was abandoned.** It blocked moving collection.
- **The fix.** `Rooted<T>` (RAII registration in LIFO order) and `Handle<T>` (a pointer to a `Rooted` slot), validated statically by Brian Hackett's GCC plugin and dynamically by using the conservative scanner to *poison* unrooted pointers.
- **2013-06-25.** An exact-rooted desktop browser with the conservative scanner disabled passed all tests. The generational GC shipped in Firefox 32 ([Clawing…](https://blog.mozilla.org/javascript/2013/07/18/clawing-our-way-back-to-precision/)).

**[V]**

### 3.3 Rooting API

- **`Rooted<T>`.** It links itself into a per-kind intrusive list on the `RootingContext`: it saves the list head as `prev` and installs itself as the new head (`js/public/RootingAPI.h:1164-1167`). Its destructor restores `prev` (`:1223-1225`).
- **Handles.** `Handle<T>` is a const pointer to a `Rooted`, and `MutableHandle<T>` a non-const one. Functions take handles, so a chain of calls does not re-root.
- **Long-lived and heap references.** `PersistentRooted` covers arbitrary lifetimes. `Heap<T>`/`TenuredHeap<T>` are *barriered* heap fields that do not keep their referent alive.
- **Cost.** Measured as "a few nanoseconds on desktop" per root. The guidance is to hoist `Rooted` out of loops and pass handles down hot stacks (`:87-99`).

**[V]**

### 3.4 Static hazard analysis and no-GC tokens

- **What it does.** A modified sixgill GCC plugin compiles Firefox into `.xdb` files. Scripts in `js/src/devtools/rootAnalysis` build a "can GC" call graph and report any unrooted GC-pointer-typed variable that is live across a call that may GC. It runs in CI as the `H` and `SM(H)` jobs, with expected-hazard counts kept under version control ([HazardAnalysis](https://firefox-source-docs.mozilla.org/js/HazardAnalysis/index.html)). **[V]**
- **Capability tokens** (`js/public/GCAPI.h:1059-1173`). **[V]**
  - `AutoRequireNoGC` is an empty base class that callees can demand as a parameter, meaning "my caller guarantees no GC".
  - `AutoAssertNoGC` asserts the same dynamically.
  - `AutoCheckCannotGC` works both ways: the analysis treats it as a GC pointer, so it reports if a GC call happens while the token is live.
  - `AutoSuppressGCAnalysis` is the escape hatch.
- **Dynamic counterpart.** `JS_GC_ZEAL` disables inline JIT allocation so that zeal modes can collect at allocation (`MacroAssembler.cpp:264-277`). **[V]**
- **Patina equivalent.** `PATINA_GC_STRESS` plus poisoning and `Free` assertions (`patina:heap/gc.rs:274-317`; `GC_DESIGN.md:783`). **[V]**

### 3.5 Barriers

The SMDOC in `js/src/gc/Barrier.h:24-240` describes three kinds. **[V]**

- **Pre-write barrier: snapshot at the beginning (SATB, Yuasa style).**
  - It marks the *old* value before an overwrite (`:150-206`), and is active only during incremental marking.
  - A store that initialises a new object skips it: `init()` is used instead of assignment.
  - JIT code: `guardedCallPreBarrier` tests the zone's `needsMarkingBarrier` flag and calls a per-type pre-barrier trampoline only when the old value is a GC thing (`MacroAssembler.h:5414-5456`).
- **Post-write barrier: generational** (`:208-230`).
  - The store buffer holds monotyped edge buffers (Value, Object, String and BigInt cell edges, slot ranges) plus a **whole-cell buffer**, which re-scans the whole object and uses per-arena `ArenaCellSet` bitmaps, and a generic buffer (`gc/StoreBuffer.h:138-174,237-450`).
  - Baseline inline sequence: if the value is not in the nursery, skip; if the host *is* in the nursery, skip; otherwise call an out-of-line stub. The stub checks a one-entry cache (`lastBufferedWholeCell`) before calling `PostWriteBarrier` (`BaselineCodeGen.cpp:604-655,3330-3341`).
- **Read barrier for weak pointers.** A weak referent read during incremental marking is marked black (`:231-240`).
- **Pointer wrapper types** pick a barrier set: `HeapPtr` (pre and post), `GCPtr` (pre and post, freed only by finalization), `PreBarriered`, `WeakHeapPtr` (read and post), `HeapSlot`, and `UnsafeBarePtr` (none).

### 3.6 JIT integration

- **Inline nursery allocation.** `bumpPointerAllocate` works against `zone->addressOfNurseryPosition()`, with `currentEnd` at a fixed offset from it (`MacroAssembler.cpp:654-680`). **[V]**
  - Sequence: load the position, add the size, compare with the end (fail if above), store the new position, subtract the size, then write a nursery cell header holding the allocation site and trace kind.
  - Whether the nursery is enabled for a kind is baked in at compile time. If that changes, the JIT code is discarded (`:663-668`).
  - Ion **elides barriers on writes to objects known to be in the nursery**. So "any allocation that can be made into the nursery must be made into the nursery", even when it takes the slow path (`:293-301`). This is a JIT/GC invariant that must be designed in from the start. **[V]**
- **Baseline frames.**
  - The GC traces `this`, the arguments, the environment chain, the return value and all value slots.
  - Locals that are dead at the current pc (`calculateLiveFixed`) are *overwritten with `undefined`*, not merely skipped (`BaselineFrame.cpp:69-104`). That is the same idea as Patina's #423 retirement.
  - Before any VM call, including the interrupt check, the code generator syncs its virtual stack into the frame: `frame.syncStack(0)` in `BaselineCodeGen.cpp:1593-1614`.
  - **[V]**
- **Ion frames.** The `SafepointReader` lists GC-pointer slots, Value slots, nunboxed (type, payload) pairs, slots/elements pointers, and spilled registers by class (`JitFrames.cpp:1072-1150`). Frame types dispatch in `TraceJitFrames` (`:1475-1520`). **[V]**
- **GC pointers embedded in code.** Ion loads nursery objects *indirectly* from a traced list in `IonScript` (`CodeGenerator.cpp:4359-4368`). The GC traces and *patches* tenured pointers in code through data-relocation tables, toggling the code writable (`jit/x86-shared/Assembler-x86-shared.cpp:41-66`). **[V]**
- **Interrupts.** `InterruptReason::{MinorGC, MajorGC, …}` bits (`vm/JSContext.h:136-143`) are what the loop-head check tests. **[V]**

### 3.7 Identity hashing under a moving GC

SM gives each cell a lazily created **unique ID** stored in a side table. The moving GC transfers it with `TransferUniqueId`, and hash tables key on it through `StableCellHasher` (`gc/StableCellHasher.h`, `Barrier.h:1346-1374`). **[V]** V8 instead stores a lazily created identity hash inside the object. Chez rehashes eq tables after GC (see the companion `chez.md`).

---

## 4. JavaScriptCore (Riptide)

- **Why it is non-moving and conservative.** Per [Riptide](https://webkit.org/blog/7122/introducing-riptide-webkits-retreating-wavefront-concurrent-garbage-collector/), without conservative root scanning, C++ code would need an API to tell the collector what it points to. Segregated storage makes it easy to ask whether a bit pattern could be an object pointer. The post refers to "the painful work of removing WebKit's previous use of copying". **[V]**
- **Candidate filter** (`heap/HeapUtil.h:50-90`), applied in order: **[V]**
  1. Precise (large) allocations are recognised by address mod 16 = 8 and looked up in a set.
  2. A `TinyBloomFilter` over block addresses.
  3. Atom alignment (16 B).
  4. The block hash set.
  5. The block's cell kind.
  6. `isLiveCell`, using the mark and newly-allocated bits.

  Conservative roots also keep *JIT stub routines and CodeBlocks* alive when a return address points into them (`MachineStackMarker.cpp:52,213`). **[V]**
- **Blocks.** `MarkedBlock` is 16 KiB with 16 B atoms. Per-block footers hold the `m_marks` and `m_newlyAllocated` bitsets (`MarkedBlock.h:77-86,317-318`). Objects up to about 8 KB go into segregated size classes; larger ones are "precise allocations". **[V]**
- **Sticky mark bits.** An eden (young-only) collection does *not* clear mark bits; a marked object counts as old. A full collection bumps a logical mark version instead of clearing bits ([understanding-gc-in-jsc](https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/)). **[V]**
- **Barrier.** `CellState` is a byte in each cell header: `PossiblyBlack = 0`, `DefinitelyWhite = 1`, `PossiblyGrey = 2`, with `blackThreshold = 0` and `tautologicalThreshold = 100` (`heap/CellState.h:30-46`). **[V]**
  - JIT code does a 1-byte load of the *host's* `cellState` and compares it with `vm.heap.addressOfBarrierThreshold()` (`AssemblyHelpers.h:1319-1339`).
  - With no GC running the threshold is 0, so only old-black hosts take the slow path, which remembers the object.
  - During concurrent marking the threshold is raised so every store takes the slow path, which issues a store-load fence and re-checks.
  - One barrier therefore covers both generational remembering and the retreating-wavefront (Steele-style re-grey) marking barrier.
  - The JIT removes barriers on newly allocated objects and on non-cell values, clusters barriers to share a fence, and removes redundant ones.
  - Cost: about 0% outside GC, about 5% during GC.
- **Allocation.** The free list is a list of **intervals**. Inline code bump-allocates within the current interval and fetches the next interval inline, with a load-pair on ARM64. The next-interval pointer is XOR-scrambled with a secret (`FreeListInlines.h`; `AssemblyHelpers.cpp:916-957`). Empty blocks get an O(1) bump arena ("bump'n'pop"). The allocation slow path is where the mutator calls `collectIfNecessaryOrDefer` (`LocalAllocator.cpp:129-142`). **[V]**
- **Concurrency.** Marking runs in parallel with work-stealing local worklists. The world stops for most of the constraint fixpoint. A space-time scheduler splits a 2 ms period as M = 1.4·H for the mutator and C = 2 − M for the collector, with a pause of at least 0.6 ms. **[V]**
  - Results: splay-latency 5× better (+5% JetStream), Octane SplayLatency 2.5×, and over-10 ms GC hiccups brought below 3 ms.
  - The "constraint fixpoint" for weak, ephemeron and DOM-specific marking resembles Patina's weak-id fixpoint (`patina:heap/gc.rs:180-196`).

---

## 5. Rust-hosted JS engines (rooting in Rust specifically)

- **Nova** ([trynova.dev blog](https://trynova.dev/blog/garbage-collection-is-contrarian); FOSDEM 2025 "Abusing reborrowing… safepoint garbage collector") **[V]**
  - Data is stored in typed (often struct-of-arrays) vectors indexed by u32, which is architecturally Patina's design.
  - `GcScope<'a,'b>` holds a `PhantomData<&'a mut GcToken>`; `NoGcScope` holds a `&'a GcToken`. `reborrow()` produces a nested scope (`nova_vm/src/engine/context.rs:47-142`). Calling anything that can GC needs the `&mut` token, so the borrow checker invalidates every handle bound to the shared token.
  - Values that must survive a GC are moved into explicit `Scoped` roots.
  - Cost: about 800 bind/unbind sites in more than 100k lines, and the author calls the result "a soup of bind/unbind".
  - The GC is mark-and-*compact*. `CompactionList` stores sorted (index, shift) runs, and every strong index is rewritten via binary search (`heap/heap_bits.rs:1345-1400,1618-1731`). Weak indices map to `None` when dead (`:1393`).
  - This is direct evidence that **typed u32-index arenas can be compacted** once roots are exact and every reference site is visited mutably.
- **Servo + SpiderMonkey** ([JS-Servos-only-GC.md](https://third-party-mirror.googlesource.com/servo/+/refs/heads/main/components/script/docs/JS-Servos-only-GC.md)) **[V]**
  - `Dom<T>` is a traced heap field and `DomRoot<T>` a stack root. `#[derive(JSTraceable)]` generates tracing. The `rooted!` macro wraps SM's `Rooted`.
  - Safety depends on a compiler lint (crown's `unrooted_must_root`) that forbids `Dom<T>` anywhere except traced locations.
- **Boa** (`core/gc/src/lib.rs:225-290`; `internals/gc_header.rs:10-36`) **[V]**
  - Each `Gc<T>` clone increments `ref_count`.
  - At GC time every heap object is traced once to compute `non_root_count`. Objects whose `ref_count` exceeds it are roots.
  - So there is no root API, but every handle copy costs a refcount operation and every GC pays a full-heap pre-pass.
  - **[I]** That is incompatible with a `Copy` `TaggedValue` and with generational collection.

---

## 6. Cross-engine comparison

| | V8 | SpiderMonkey | JSC | Patina today |
|---|---|---|---|---|
| Native-code roots | Indirect handles plus gcmole; direct+CSS experimental | `Rooted`/`Handle` plus hazard analysis | Conservative stack and registers | Safe-point discipline: no GC while Rust holds values; `GcDeferGuard` |
| JIT frame roots | Interpreter-shaped frames all tagged; Maglev split point; TurboFan bitmap | Baseline frames all boxed `Value`s plus liveness; Ion safepoints | Conservative (no maps) | N/A (interpreter: whole register file plus #423 retirement) |
| Moves objects? | Yes (scavenger, compaction); pins under CSS | Yes (nursery, compaction of JS objects only) | Never | Never (`gc.rs:200-203`) |
| Young generation | Semi-space copying; MinorMS optional | Bump nursery, promote after one survival | Sticky mark bits | None |
| Remembered set | Per-page slot bitmaps | Store buffer (edges and whole cells) | Per-object remembered state | — |
| Marking barrier | Dijkstra on value, atomic CAS | SATB on old value, zone-flag guarded | Re-grey host, threshold byte | — |
| JIT barrier check | Host page flag, then value page flag | Value-in-nursery, host-in-nursery, OOL with one-entry cache | `load8 host.cellState; cmp threshold` | — |
| JIT allocation | LAB bump | 5-instruction bump plus header | Interval bump | `RefCell` + free list / `Vec::push` |
| Safepoint poll | Stack-limit check doubles as interrupt | `interruptBits != 0` at loop heads | Allocation slow path | Flag load before every instruction |
| Reference encoding | 32-bit cage offset in 32-bit fields | 64-bit NaN-boxed `Value`, real pointers | 64-bit pointers | u32 index in 64-bit word, per-type `Vec` |

---

## 7. Lessons and constraints for the Patina redesign

**L1. Keep "allocation never collects; collect only at safe points", and promote it from convention to an enforced capability.** **[I]**
- It is Patina's equivalent of SM's three-year exact-rooting effort and of V8's 17,000 handle scopes, and it is why a moving collector is *available* to Patina at all.
- Express it in types the way SM's `AutoRequireNoGC` and Nova's `NoGcScope` do. The cheap Rust form: allocation takes `&mut Heap` and cannot reach a collector; collection needs a token only the outermost loop owns.
- Do not adopt Nova's full handle-lifetime regime. Patina's Rust code seldom holds values across a safe point (resumable primitives hand calls back to the machine, `AGENTS.md`), so bind/unbind noise would buy little.

**L2. Moving is feasible; scope it the way SM and V8 do.** **[I]**
- SM compacts only JS objects. V8's scavenger moves only young objects.
- Patina can make the **young generation moving**, or compact only pairs, vectors, closures and records. Symbols, syntax markers, code-object constants and other objects referenced by raw index from Rust structures go in a **non-moving space**, which can be pretenured.
- That set covers most of `GC_DESIGN.md:122-139`'s obstacles (1, 4, 5, 6) without rewriting every Rust structure.
- Identity hashing needs SM-style side-table unique IDs transferred on move, an in-object hash (V8), or post-GC rehash (Chez).
- `SourceMap` and syntax-provenance raw-bit keys must become weak tables keyed by stable IDs, or stay on non-moving objects.

**L3. Change the root-visiting API to visit *slots*, not values.** **[V]/[I]**
- `GcVisitor::visit(TaggedValue)` and `GcRoots::trace_roots(&self)` (`gc.rs:177-178,485`) must become slot-based, the shape of V8's `VisitRootPointers(start, end)` and SM's `TraceRoot(&ptr)`, so that a copying collector can update registers, frames, the `CallFrame.closure` index and constants in place.
- Do this first, even while the collector is still non-moving. It is the change that makes every later collector swappable.

**L4. Baseline JIT: mirror the VM frame and spill at safepoints, so stack maps are unnecessary.** **[V]/[I]**
- Sparkplug and SM Baseline show that a baseline tier which keeps the interpreter's register-file layout and syncs cached values to the frame before any GC-capable call or poll inherits root scanning, deopt and debugging for free.
- For Patina this means JIT code reads and writes `ExecutionState.registers` (`execution_state.rs:18`). It caches values in machine registers only between safepoints.
- Continuation capture stays a copy of VM-managed state (`types/continuation.rs:79`). JIT frames never sit on the native stack in a form `call/cc` must understand.
- An optimizing tier can add precise maps later, via Cranelift's user stack maps ([fitzgen 2024](https://fitzgen.com/2024/09/10/new-stack-maps-for-wasmtime.html)). The cheapest map shape is Maglev's: a tagged-slot count plus a register mask.

**L5. Move safepoint polls to loop back-edges and call/entry, and fold them into an existing check.** **[V]/[I]**
- Today's per-instruction flag load (`vm_state.rs:1205`) is fine for an interpreter but wrong for JIT code.
- Emit `cmp [ctx+pending], 0; jne slow` at back-edges and entries, as SM does. Alternatively, follow V8 and fold it into a stack-depth or fuel check the JIT needs anyway.
- Keep the decision where it is made today: the allocation slow path sets the flag.

**L6. The allocator must expose two stable words to the JIT.** **[V]/[I]**
- The inline path needs a `(cursor, limit)` pair at a fixed offset from a pinned context register (SM, JSC). Arenas therefore cannot be `Vec`s that reallocate.
- Use reserved virtual-address regions (V8's cage, SM's 1 MiB chunks, JSC's 16 KiB blocks) so bases and cursors never move under JIT code.
- Patina's current per-type `Vec` arenas break the JIT twice: no inline fast path, and an unstable base address after any allocation.
- SM's rule "if it can be nursery-allocated it must be, even on the slow path" lets JIT code elide post-barriers on fresh objects. Adopt it if a nursery is added.

**L7. Choose a barrier by what the JIT must emit; prefer one unified host-keyed check.** **[V]/[I]**
- Scheme mutation sites (`set-car!`, `set-cdr!`, `vector-set!`, cell and global writes) are few compared with allocation.
- JSC's design is the cheapest to emit: load a byte keyed by the *host*, compare it with a global threshold, branch out of line. One barrier then serves generational remembering now and incremental or concurrent marking later, by raising the threshold.
- Keyed by arena index, that byte can live in a side bytemap (`side_base + idx`), which needs no object header change.
- If headers are introduced, put the byte in the header, as JSC does.
- SM's SATB needs the *old* value, which costs an extra load per store. V8's barrier needs two page lookups.
- cppgc's 4 KiB age-table cards are the right shape if Patina becomes address-based and non-moving.

**L8. Treat conservative scanning as a representation question, not a GC-algorithm question.** **[V]/[I]**
- With dense u32 indices and 3-bit tags, any small Rust integer whose low bits are `011`-`111` and whose value is at most 8 × the arena length is a "valid" reference, and no block set can filter it.
- Oilpan's halfword cost (24% down to 21%) is the closest measurement.
- Conservative scanning is only worth considering if the payload becomes a sparse address (JSC filter chain) *and* the design accepts pinning plus V8-style page quarantine.
- Given L1 and L4, Patina does not need conservative scanning for its own Rust code or for a baseline JIT. V8's own justification (over 80% of GCs with an empty stack) already holds by construction for Patina's outermost-loop collections.

**L9. Do not shrink slots to 32 bits; do stabilise the base.** **[I]**
- V8's 43% saving came from 32-bit fields, but it would cost Patina its 61-bit heap fixnums.
- The JIT-relevant part of pointer compression, a constant base in a register, is available by reserving address space.
- Separately consider replacing (tag → arena) dispatch with a uniform header word or BiBOP-style page metadata. `HeapObjectData` is 72 B per slot today, so moving variants to size-classed blocks recovers more memory than compression would.

**L10. Make testing for a moving GC part of the gate from day one.** **[V]/[I]**
- V8 ships stress flags that treat precise references as conservative to exercise pinning (`flag-definitions.h:465-480`). SM has zeal modes, and dynamic analysis that poisons unrooted pointers.
- Patina's differential lane (`PATINA_GC_STRESS` with poison) should gain a "move everything every collection" mode that poisons from-space. That catches a stale index held in a Rust local across a safe point, the moving-GC equivalent of a missed root.

**L11. Embedded constants in JIT code must be traceable and patchable, or loaded indirectly.** **[V]/[I]**
- SM loads nursery constants from a traced per-script list and patches tenured ones through relocation tables. V8 records them as typed slots.
- For Cranelift, the simplest form is to load constants from the code object's `constants` vector, which is already a root (`GC_DESIGN.md:368`). Code then embeds no heap references and needs no W^X toggling on GC.

---

## 8. Open questions for the design team

1. **References: indices or addresses?** Should references stay u32 arena indices (Nova-style compaction, with decode `base[type] + idx·scale`), or become addresses/offsets in a reserved region? This decides L6, L8 and L9 together, and whether pair access in JIT code costs one load or three.
2. **Frame placement.** Will JIT code run on the VM-managed register file and frame stack (L4, keeping `call/cc` a snapshot), or on the native stack? The second needs Cranelift stack maps *and* a continuation story for native frames: segmented or copied stacks, see `chez.md`.
3. **Pause goal.** Is the goal throughput (generational copying, as in SM and V8) or bounded pauses (incremental or concurrent marking, as in JSC)? Patina is single-threaded; JSC and V8's concurrency needs `Send` heap structures and atomic mark bits, which `Rc<RefCell<Heap>>` rules out.
4. **Rust-held values.** Which Rust structures that hold values must remain non-moving roots (`CompiledMacro` literals, `Library` exports, `Rc` environments)? Should environments move into the GC heap, making them traceable and updatable, rather than staying `Rc` graphs (`GC_DESIGN.md:135`)?
5. **Root API cost.** Would a V8 `HandleScope`-style root vector (push onto a `Vec<TaggedValue>`, restore its length on scope exit, refer by index) be cheap enough for the rare Rust code that must hold values across a safe point? Or does making every such path resumable (`Step::Call`) remove the need entirely?
6. **Eq-hashing.** Should identity hashing under moving use SM-style unique IDs, an in-object hash word, or rehash-after-GC? R7RS-large hash tables and `SourceMap` keys both depend on the answer.
