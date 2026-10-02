# Chez Scheme's storage manager: what Patina's GC redesign should take from it

Source studied: `~/Project/reference/ChezScheme` at commit `7d82bd8667c1` (2026-09-05). This is Chez Scheme 10.x, which includes the Racket CS collector work that was merged back upstream. All paths below are relative to that checkout unless prefixed `patina:`.

Legend: **[V]** means verified in source at the cited line. **[I]** means my inference or a design recommendation. Instruction counts are derived by hand from the compiler's lowering code. I did not disassemble anything.

---

## 0. Executive summary

1. **Objects are typed real pointers, and segment metadata sits beside the heap (BiBOP).** A `ptr` is a machine address with a 3-bit low tag. `TYPE(x,t) = x - 8 + t` (`c/types.h`, `TYPE` macro), so the tag folds into every load displacement and no untagging instruction is needed (for example, the car of a pair is at `ptr+7`). All metadata (space, generation, card bytes, mark bitmap) lives in a per-16 KiB-segment `seginfo` that is found from the address (`c/segment.h:28-70`). Pairs have no header. **[V]**
2. **Gen-0 is a single bump region holding every object type** (`space_new`). Objects are sorted into typed "spaces" (pure / impure / data / symbol / code / continuation / weak / ephemeron, …) only when the collector copies them out. Compiled code allocates with about 4 instructions against `ap`/`eap` held in the thread context (`s/cpnanopass.ss:6748-6782`). **[V]**
3. **Allocation never collects.** Running out of room only *queues* a collect request (`c/alloc.c:208-221`, `c/schsig.c:574-593`). The collection runs later, at the next interrupt "event" check. That check is a 2-instruction trap-counter decrement at the entry of procedures that make calls and at loop heads (`s/cpnanopass.ss:3997-4012`, `s/library.ss:1194-1240`). When the GC runs, every live Scheme value is in a stack frame described by return-point live masks, or in the thread context. The collector never scans machine registers or the C stack, and C code can hold raw pointers across allocation. **[V]** This is the most important lesson for a Cranelift JIT.
4. **Young objects are copied generationally and old ones are marked in place (the hybrid).** Generations run 0..4 by default, plus a static generation 7. Objects promote one generation per collection, and a collection is triggered every 8 MiB of gen-0 allocation. Generation *g* is collected every 4^g collections, and the oldest generation is collected only once the heap has doubled (`s/7.ss:870-893`). Segments of the oldest generation are marked in place unless a previous mark found them sparse (under 3/4 live) (`c/gc.c:1010-1036`). **[V]**
5. **The remembered set is a card table whose bytes hold generation numbers.** Each card is 512 B, there are 32 per segment, and each dirty byte records the *youngest generation referenced* from that card (0xff = clean). Segments with dirty cards sit on lists indexed by (from_g, to_g) (`c/types.h:212-250`). Compiled code does not mark cards directly. It pushes the slot address onto a **sequential store buffer that grows downward from the top of the nursery allocation area** (`eap -= 8`), filtering out fixnums at run time and statically known immediates at compile time (`s/cpprim.ss:665-725`, `s/cpnanopass.ss:6807-6834`). The buffer is drained into cards when the area fills or a GC starts (`c/alloc.c:433-464`). **[V]**
6. **Continuations are segmented heap stacks** (Hieb/Dybvig/Bruggeman). `call/cc` is O(1): it seals the current stack segment into a continuation object. Reinstating one copies at most about 128 B plus one frame (`underflow-limit`, `s/cmacros.ss:2196`; `c/schsig.c:58-150`). The GC traces stacks frame by frame using the live masks stored just before each return address (`s/mkgc.ss:1037-1095`). **[V]**
7. **For Patina:** Chez answers every obstacle that `patina:docs/GC_DESIGN.md:122-139` gives for ruling out moving GC: symbol-table rebuild, eq-table rehash, frame maps, code relocation tables, and "everything in the heap or in a ≤100-entry protect list". The redesign can copy the *discipline*: safepoints only at calls and back-edges, allocation that never collects, frames as stack maps, and one declarative layout spec that generates every traversal. It should *not* copy the complexity (19 spaces × 8 generations, 3-level segment radix, the card-scanning start-finding for records, ownership-based parallelism). **[I]**

---

## 1. Object representation

**Primary tags (64-bit)** (`s/cmacros.ss:822-829`) **[V]**

| tag | type | notes |
|---|---|---|
| `000` | fixnum | 61-bit (`fixnum-bits` 61, `s/cmacros.ss:919-922`) |
| `001` | pair | no header: car and cdr only, 16 B |
| `010` | flonum | boxed, 16 B (8 B payload + alignment) |
| `011` | symbol | fixed size, own space |
| `100` | unused on 64-bit | |
| `101` | closure | first word is the code entry pointer, then free variables. Continuations are also closures |
| `110` | immediate | `#f #t () eof void bwp unbound forward-marker …` (`s/cmacros.ss:840-851`) |
| `111` | typed object | first word is a type word (vector, string, record, code, box, port, …) |

- **Alignment is 16 bytes** (`byte-alignment = max(typemod, 2*ptr-bytes)`, `s/cmacros.ss:478-481`). The stated reason is that every object must be able to hold a **forward marker plus a forwarding address** (2 words). Odd-length vectors and closures are padded with `FIX(0)` (`s/mkgc.ss:404-405`).
- **Header words look like fixnums or immediates where it matters.** A vector's type word is its length as a fixnum (`type-vector = type-fixnum`, `s/cmacros.ss:875`). The comment in `s/mkgc.ss:387-388` says: "Assumes vector lengths look like fixnums; if not, vectors will need their own space." Because of this, vectors, pairs, boxes and mutable closures can share `space_impure`, and both the Cheney scan and the dirty-card scan treat that space as an **untyped array of words, two at a time**, with no need to parse object boundaries (`c/gc.c:1964-1975`, `2290-2320`). **[V]** This is the single trick that makes Chez's card scanning cheap.
- **Some type is carried by the segment instead of the header.** `weak-pair?` and `ephemeron-pair?` check `seginfo->space` (`c/prim5.c:~218-220`), and the compiler inlines the segment-table lookup for them (`s/cpprim.ss:3022-3110`). Weak pairs stay 2 words. **[V]**
- Records carry two masks in their record-type descriptor: a pointer mask `pm` (which fields are pointers, so unboxed fields are possible) and a mutable-pointer mask `mpm`, which the dirty sweep uses (`s/mkgc.ss:811-860`). **[V]**
- Code objects have an 8-word header: type, length, reloc, name, arity-mask, closure-length, info, pinfo* (`s/cmacros.ss:1552-1561`). The machine code follows, and a separate relocation table lists the embedded object pointers.

**Patina today:** the same 3-bit low-tag idea (`patina:crates/patina-core/src/tagged_value.rs:70-84`). Fixnum is `000`, and pair, vector, string, closure and object have their own tags. The payload, however, is a `u32` arena index (`tagged_value.rs:28, 376-412`) into typed `Vec` arenas (`patina:crates/patina-core/src/heap/mod.rs:304-316`, free lists at `367-376`). Every dereference is a shift, a bounds-checked `Vec` index and often an enum match. Flonums are boxed (`HeapObjectData::Real`, `heap/mod.rs:146`), as in Chez. (Note that `patina:AGENTS.md` describes the encoding as "NaN-boxed". The code uses low tags.) **[V]**

**Transfer [I]:**
- Make `TaggedValue` either a raw address or a 32-bit heap offset with the low tag kept, and fold the tag into field displacements as Chez does. Cranelift then emits one `load [v + (k - tag)]` per field access.
- Use 16-byte alignment, so forwarding fits in place.
- Pick headers so that pointer-only spaces can be scanned word by word.

---

## 2. Segments, chunks, spaces, generations (BiBOP)

- **Segment = 16 KiB** on 64-bit (`segment-offset-bits 14`, `s/cmacros.ss:2142`). **Card = 512 B**, so 32 cards per segment (`card-offset-bits 9`, `s/cmacros.ss:2143-2150`). On 32-bit the values are 8 KiB and 256 B. **[V]**
- **`seginfo` metadata** (`c/types.h:143-186`) includes: space and generation bytes (at fixed offsets 0 and 1 so the compiler can inline-load them), `old_space`, `use_marks`, `must_mark:2` (a lock count where 3 means infinite), `min_dirty_byte`, the segment `number`, the owning-thread `creator`, sweep chain fields, dirty-list links, ephemeron and guardian trigger lists, `marked_mask` (allocated on demand), `marked_count`, and `dirty_bytes[32]`. I measured a replica struct (a C file re-declaring Chez's `seginfo`; not retained, as adapted third-party code) at **168 B per 16 KiB segment, 1.03 %**. A mark bitmap is `segment_bitmap_bytes = 16384 >> 6 = 256 B` (1.56 %) and is allocated only for segments being marked (`c/types.h:127`; `init_mask`, `c/gc.c:504-511`). **[V]**
- **Address → seginfo lookup** uses a 3-level radix table with 17/17/16 bits (`s/cmacros.ss:2139-2141`; `SegInfo`, `c/segment.h:28-38`). That is three dependent loads for every pointer the collector touches (`relocate_*` macros, `c/gc.c:563-683`). **[V]** Patina can avoid this by reserving one contiguous range and indexing a flat array. **[I]**
- **Chunks.** Memory is requested from the OS in chunks of at least `minimum-segment-request = 128` segments (2 MiB, plus one segment for alignment) (`s/cmacros.ss:2154`; `allocate_segments`, `c/segment.c:393-448`). Code gets separate chunk lists so W^X can be managed (`S_code_chunks`). Empty chunks are returned to the OS after collections that reach `release-minimum-generation`. About `heap-reserve-ratio` (default 1.0) empty segments are kept for each occupied non-static segment (`S_free_chunks`, `c/segment.c:460-484`). **[V]**
- **Spaces** (`s/cmacros.ss:755-781`): new, impure, symbol, port, pure, continuation, code, pure-typed-object, impure-record, impure-typed-object, closure, immobile-impure, count-pure, count-impure, weakpair, ephemeron, reference-array (all swept), data and immobile-data (unswept), and empty. That makes **19 real spaces**. "Pure" means the fields are never mutated after the object reaches that space (immutable vectors, non-mutable closures), so the dirty-card machinery skips them. "Data" holds pointer-free objects (strings, bytevectors, flonums, bignums, stack segments, mark masks) and is never swept. **[V]**
- **Generations** run 0..`collect-maximum-generation` (default 4, at most 6), plus `static-generation = 7` (`s/cmacros.ss:1586, 2123`). Objects in the static generation are never collected, and their code relocation tables are dropped (`s/mkgc.ss:1155-1160`). `Scompact_heap` collects everything into static (`c/gcwrapper.c:481-490`). This is how the boot heap becomes free for later GCs. **[V]**
- **Invariant:** an object spans segments only if it starts at a segment boundary (`c/alloc.c:321-329`, comment on `S_reset_allocation_pointer`). The record card-scanner relies on this to find object starts. **[V]**
- **Large objects.** A request bigger than what remains of the nursery, when more than `alloc-waste-maximum = 2 KiB` (segment/8) still remains, gets its own fresh multi-segment run instead of wasting the nursery tail (`c/alloc.c:513-558`, `s/cmacros.ss:2161`). Runs longer than **128 segments (2 MiB)** are marked `must_mark = ∞` and are never copied (`c/segment.c:381-384`). Because `space_new` ignores `must_mark` (`c/gc.c:1034`), such an object is copied **once** out of gen 0 and stays put after that. **[V]+[I]**

---

## 3. Allocation

- **Thread context fields** `ap`, `eap`, `real_eap` (`s/cmacros.ss:1597-1605`). The nursery allocation area is one segment (`S_reset_allocation_pointer`, `c/alloc.c:321-355`): `ap` grows up from the base, and `eap` starts at `base + 16 KiB`. **[V]**
- **Inline allocation** (`build-alloc`, `s/cpnanopass.ss:6748-6782`) **[V]**:
  ```
  xp  = ap + (tag - 8)          ; tagged result pointer, tag folded in
  ap  = ap + size               ; immediate size < segment: no carry check (last segment never used)
  if (eap <u ap) goto get-room  ; "pariah" (cold) block → asm stub → S_get_more_room
  ```
  On x86-64, `%tc` (r14), `%sfp` (r13) and `%ap` (rdi) are reserved registers, while `eap` and `trap` live in the thread context (`s/x86_64.ss:18-25`). On arm64, `%trap` is also a register (`s/arm64.ss:21-26`). So the fast path is about 4 instructions (lea, add, cmp-with-memory, jcc) plus the initialising stores. Variable sizes add a carry check. **[V]**
- **The C runtime uses the same pointers** (`newspace_find_room`, `c/types.h:101-114`). Allocation into a specific space or generation (`find_room`/`find_gc_room`, `c/types.h:84-95`) uses per-thread `next_loc[g][s]`/`bytes_left[g][s]` bump pointers (`thread_gc`, `struct thread_gc` in `c/types.h`). The collector itself allocates its to-space, sweep stacks, mark masks and new-dirty-card records this way, so the GC needs no `malloc`. **[V]**
- **The refill slow path never collects.** `S_find_more_gc_room` closes off the segment, takes new segments, and calls `maybe_queue_fire_collector`. That function only sets a flag once gen-0 bytes since the last GC reach `collect_trip_bytes` (`c/alloc.c:208-305`). `S_fire_collector` sets `$collect-request-pending` and `something-pending` for each thread (`c/schsig.c:574-593`). **[V]**

---

## 4. When a collection happens (safepoints)

- **Trap check.** At the start of every procedure clause whose body makes a call, and at every loop head, the compiler inserts `trap -= 1; if zero → call library entry event` (`np-insert-trap-check`, `s/cpnanopass.ss:3997-4012`). The placement policy is in `s/cpnanopass.ss:3005-3010, 3123-3143`. Leaf procedures without loops get no check. That is **2 instructions**: on x86-64 a read-modify-write `sub [tc+trap],1; je`, and on arm64 `subs; b.eq`. **[V]**
- **`event`** (`s/library.ss:1194-1240`) resets the timer to `default-timer-ticks = 1000` (`s/cmacros.ss:2119`). It then checks `something-pending`, followed by timer, signal, keyboard, and finally the collector. If `$collect-request-pending` is set, it calls `$collect-rendezvous` → `collect-request-handler` → `collect` → the foreign procedure `do_gc` (`s/7.ss:1253-1305, 804-830`). The longest delay between "trip" and GC is therefore about 1000 trap checks of further mutator work. Long-running C primitives consume fuel with `USE_TRAP_FUEL` (`c/types.h`). **[V]**
- **Consequences [V]+[I]:**
  - At GC time the mutator is inside a non-tail foreign call. Every Scheme frame is a well-formed frame with a return address that carries a live mask. `CP` (the current code) is locked for the duration (`c/gcwrapper.c:1187-1195`). The thread's scratch registers `U V W X Y` are simply zeroed (`s/mkgc.ss:1002-1011`).
  - **No conservative scanning, no register maps, and no stack maps at allocation sites.** C code can hold raw `ptr`s across allocation, because allocation cannot move anything. Only C code that *calls back into Scheme* must lock its objects (`Slock_object`, `c/gcwrapper.c:297-320`). The C-global roots are a fixed array `S_G.protected` of at most **100** entries (`max_protected`, `c/types.h:121`; `S_protect`, `c/alloc.c:97-102`).
- **Stopping all threads** is cooperative. Each thread reaches its own event check, and `$collect-rendezvous` waits until `$active-threads = 1` (`s/7.ss:1253-1297`). There are no signals or page-protection polling. **[V]**

**Patina today:** this structure has already been converged on. `Heap::note_alloc` raises a pending flag, and backends collect at `GcController::safe_point`, "a single flag load" (`patina:crates/patina-core/src/heap/gc.rs:20-27`). **[V]** The redesign should keep that and extend it to the JIT. JIT allocation slow paths call a refill stub that never collects, so allocation sites are not safepoints and need no stack maps. Safepoints are only (a) calls that can reach a backend safepoint and (b) the event check at function entry and loop back-edges. This agrees with the Patina rule that Rust primitives never call back into the program (`patina:AGENTS.md`, "A procedure that calls back into the program…", #471/#476-#478): under Chez's discipline, **Rust primitive frames never need rooting even with a moving collector.** **[I]**

---

## 5. Collection policy

- **Trip size.** `collect-trip-bytes` defaults to 2^(20+3) = **8 MiB** on 64-bit (`s/cmacros.ss:2120-2121`). **[V]**
- **Default schedule** (`collect0`, `s/7.ss:870-893`; CSUG `csug/smgmt.stex:66-118`). A counter `gc-trip` increases by 1 per collection. The maximum collected generation is the largest *g* with `gc-trip mod r^g = 0`, where `r = collect-generation-radix = 4` (`s/7.ss:609-615`). So gen 1 is collected every 4th GC, gen 2 every 16th, gen 3 every 64th and gen 4 every 256th. The **maximum** generation is collected only if `bytes-allocated ≥ k × (bytes after the previous max-gen GC)`, with `k = collect-maximum-generation-threshold-factor = 2` (`s/7.ss:617-623`). Otherwise the counter is rewound. **[V]**
- **Promotion one generation at a time** (since 9.5.4; `release_notes.stex:1163-1186`). A collection of gens 0..MAX_CG computes each segment's target as `g == MAX_TG ? g : g < MIN_TG ? MIN_TG : g+1` (`c/gc.c:311-331`). Because a promoted object can then point into a *younger* target generation, the collector itself records "new dirty cards" (`S_record_new_dirty_card`, `c/alloc.c:357-375`) and applies them at the end (`c/gc.c:1725-1731`). **[V]**
- **The policy is Scheme code.** Users can replace `collect-request-handler` (`s/7.ss:1299-1305`) or call `(collect g min-tg max-tg)`. **[V]**
- **Specialised collector builds.** The same `gc.c` is compiled several times with different preprocessor constants:
  - `gc-011.c`: MAX_CG=0, MIN_TG=MAX_TG=1, `NO_NEWSPACE_MARKS`, for the common minor GC;
  - `gc-ocd.c`: the general sequential build;
  - `gc-oce.c`: with object counting and back-references;
  - `gc-par.c`: the parallel build.

  `S_gc` picks one per collection (`c/gcwrapper.c:1320-1340`). **[V]** The minor collection is specialised so that target-generation computations constant-fold. **[I]**

---

## 6. The collection algorithm (`GCENTRY`, `c/gc.c:895-1800`)

Phases, in order **[V]**:
1. Drain each thread's store buffer into card bytes (`S_scan_dirty(EAP, REAL_EAP)`, `c/gc.c:930`). Close off the thread-local allocation regions and lay down a `forward_marker` end sentinel so to-space scanning knows where to stop.
2. Flag "old space": every segment in generations 0..MAX_CG gets `old_space = 1`, and its `generation` is set to the target *now*, so dirty bookkeeping during GC uses post-GC generations. Decide `use_marks` per segment (§7) (`c/gc.c:1005-1050`).
3. Mark locked objects that live in `space_new`. Then process roots: each thread's context and stack (`sweep_thread`), `S_threads`, symbols with values, and the protected C pointers (`c/gc.c:1200-1310`).
4. Sweep dirty cards of older generations (`setup_sweep_dirty`, `sweep_dirty_segments`, `c/gc.c:1321-1325, 2217-2565`).
5. **Sweep loop** (`sweep_generation_pass`, `c/gc.c:1954-2085`). This is a Cheney scan of each target generation's to-space, *per space*: for each space it runs from `sweep_loc` to `next_loc`, then follows chained closed-off segments via `sweep_next` (`sweep_space` macros, `c/gc.c:1829-1858`). Marked (non-copied) objects are pushed on an explicit `sweep_stack` (`push_sweep`, `c/gc.c:336-341`). Pending ephemerons are checked only when a pass made **no** other progress. The loop repeats until a fixpoint.
6. Guardians, both unordered and ordered (§9).
7. Weak pairs: each car either forwards or becomes `#!bwp` (`resweep_weak_pairs`, `forward_or_bwp`, `c/gc.c:1569-1571, 1869-1950`). Ephemerons whose keys were never reached are set to bwp/bwp (`c/gc.c:1573-1574`; `finish_pending_ephemerons`).
8. Rebuild the oblist and prune unreachable symbols (§10). Rebuild the rtd-counts lists and the child-process lists.
9. Copied-out segments go back to `space_empty`. Segments with a `marked_mask` are **promoted in place** to their target generation, with all cards reset (`c/gc.c:1665-1715`). Then chunks are freed and new dirty cards applied.
10. Rehash eq-table cells whose keys moved (§9). Promote opportunistic one-shot continuations (§12) and resize the oblist.

**Copying.** `copy()` writes `forward_marker` into word 0 and the new address into word 1 (`s/cmacros.ss:1747-1752`). Flonums cannot hold a marker, so they use a side bitmap `forwarded_flonums` to keep `eq?` working (`c/gc.c:530-552`). Whether an object has been reached is decided by `!old_space || FORWARDEDP || marked`; there is no header mark bit (`c/gc.c:84-92`). **[V]**

**Pure vs impure relocation** (`c/gc.c:563-683`). `relocate_pure` is for fields that cannot point to younger objects or need no generation tracking. `relocate_impure` additionally computes the target generation of what it points to and, if that is younger than the containing object's generation, calls `S_record_new_dirty_card`. **[V]**

**One declarative spec generates all traversals.** `s/mkgc.ss` describes each type's space, size, mark flags, and traced/copied/pure/impure fields in a small DSL ("Parenthe-C"). From it the build generates copy, sweep, sweep-in-old, mark, self-test, size, measure and check code, plus the vfasl writer and the heap checker (`s/mkgc.ss:1-118`; outputs `gc-ocd.inc`, `gc-oce.inc`, `gc-par.inc`). **[V]** IMPLEMENTATION.md notes that adding an object type means updating mkgc, gc.c, the compiler, fasl, vfasl, strip and the inspector (`IMPLEMENTATION.md:374-381`). For Patina, a single Rust layout table (via macro or `build.rs`) that produces the trace, copy and size code, the JIT field offsets and the heap verifier would remove a whole class of "forgot to trace field X" bugs. **[I]**

---

## 7. Mark-in-place: the hybrid from the Racket CS work

The overview comment is at `c/gc.c:35-110`; the 10.0.0 release notes are at `release_notes.stex:515-540`. **[V]**
- **Which segments are marked instead of copied.** In the old-space loop (`c/gc.c:1010-1040`), a segment is marked in place if:
  - `must_mark` is set (locked or immobile), or
  - `g ≥ in-place-minimum-generation` (default = max nonstatic generation = 4, `c/gcwrapper.c:55-59`) **and** `g ≥ MIN_TG` **and** (the segment was never marked before **or** its last marking found ≥ 3/4 of it live, i.e. `marked_count ≥ 12 KiB`, `c/gc.c:486`) **and** its chunk is ≥ 1/4 used (`c/gc.c:487`).

  So the oldest generation is normally marked in place. Sparse segments are evacuated (copied) to compact them. `space_new` is never marked wholesale.
- **Mark state.** A `marked_mask` bitmap per segment, with one bit per word: the object-start bit, plus extra bits over impure object bodies so a later *dirty* sweep can tell live words from garbage (`c/gc.c:94-107`). `marked_count` holds the live bytes. Objects spanning segments share one `fully_marked_mask` per generation for the interior segments (`init_fully_marked_mask`, `c/gc.c:523-529`; `s/mkgc.ss:2236-2251`).
- **Promotion without copying.** A segment that ends with any marks is kept and moved to the target generation's `occupied_segments`. The unmarked holes are *not* put on a free list: the segment is only reused when it later goes empty or gets evacuated. **[V]+[I]** This is not a mark-sweep allocator. It trades fragmentation for lower peak memory on large old heaps.
- **Locking and immobility.** `Slock_object` increments `must_mark` (saturating at 3 = ∞) and adds the object to `locked_objects[g]`. Locked objects are never moved and never reclaimed (`c/gcwrapper.c:297-345`). **Immobile** objects (`box-immobile`, `make-immobile-vector`, `make-immobile-bytevector`) are allocated in `space_immobile_*`, get `must_mark++`, never move, and *can* be reclaimed (`c/prim5.c:223-251`). Because `must_mark` is per segment, an immobile object pins only its own segment. **[V]**

**Transfer [I]:** for a single-threaded Rust runtime this hybrid is a strong default: copy the young generations, mark the old in place, and evacuate segments found sparse. Pinning is a per-segment knob, which gives Patina an escape hatch for anything that cannot move yet (for example, values held only through the tree-walker's `Rc` continuations). Those can be allocated in an immobile space or pinned through a `must_mark`-style counter, while everything else moves.

---

## 8. Write barrier and remembered set

**What compiled code emits** (`build-dirty-store`, `s/cpprim.ss:665-725`; the `%remember` lowering, `s/cpnanopass.ss:6807-6834`) **[V]**:
- **Static elision:**
  - no barrier if the stored expression is `$fixmediate`-wrapped;
  - no barrier if it is a quoted immediate;
  - no barrier if it is a call whose `*result-type*` is `fixnum` or `boolean`, looking through `if`/`seq` with fuel 5;
  - for a quoted non-immediate, the barrier is emitted *without* the run-time fixnum test.
- **Fast path (x86-64, `eap` in the thread context):**
  ```
  a = lea [base + index + offset]
  mov [a], v
  test v, 7 ; jz done             ; runtime fixnum filter (only fixnums! other immediates are logged)
  td = [tc+eap]
  cmp ap, td ; jae slow           ; SSB shares the nursery's limit check
  td -= 8 ; [tc+eap] = td         ; write-through so the buffer bounds are known at faults
  [td] = a                        ; log the *slot address*
  ```
  That is about 10 instructions and 3 stores for a pointer store, and about 4 for a fixnum store. There is **no generation check in compiled code**: stores into gen-0 objects are logged as well and dropped when the buffer is drained (`S_scan_dirty` skips `from_g == 0` and deduplicates consecutive identical cards, `c/alloc.c:433-464`). On weakly ordered architectures a store-store fence is added (`add-store-fence`).
- **Overflow** (the `scan-remembered-set` asm stub → `S_scan_remembered_set`, `c/alloc.c:466-498`) drains the buffer into cards and may start a new nursery segment. Buffer entries take up nursery bytes, so mutation-heavy code reaches the collect trip sooner. **[V]+[I]**
- **The C runtime** (`S_dirty_set`, `c/alloc.c:396-418`) marks cards directly. If the target is not a fixnum and the containing segment is not gen 0, it sets `dirty_bytes[card] = 0` and links the segment onto `DirtySegments(from_g, 0)` under the allocation mutex. **[V]**

**Card table semantics [V]:**
- One byte per 512 B card, holding the **youngest generation referenced** from the card. 0 means "may point anywhere", 0xff means clean.
- `seginfo.min_dirty_byte` is the minimum over a segment's cards.
- Dirty segments sit on doubly linked lists `DirtySegments[from_g][to_g]`, stored as a triangular array (`DIRTY_SEGMENT_INDEX`, `c/types.h:212-250`).
- A collection of gens 0..MAX_CG walks only lists with `to_g ≤ MAX_CG` and `from_g > MAX_CG` (`c/gc.c:2217-2240`).
- After scanning a card, the GC writes back the exact youngest generation it found (`c/gc.c:2545-2550`). A card that points only into gen 3 therefore costs nothing during the next fifteen gen-0..2 collections. This is a precise multi-generation remembered set in one byte per card.
- Words are checked 8 bytes at a time with `*dp == -1` to skip clean runs of cards quickly (`c/gc.c:2276-2282`).

**Card scanning by space [V]:**
- Impure spaces are scanned **two words at a time** with no knowledge of object boundaries. This works because headers look like fixnums or immediates (§1). If the segment was marked in place, only marked words are scanned (`c/gc.c:2290-2320`).
- Symbols and ports have fixed sizes, so the start of an object is found arithmetically.
- **Records** need a backwards search through mark bits or a forward walk from the start of the segment group. This is commented "abandon hope all ye who enter here" (`c/gc.c:2363-2475`).
- Weak-pair cards trace only the cdr, and ephemeron cards are re-checked (`check_dirty_ephemeron`).

**Transfer [I]:**
- **Barrier.** For Cranelift, Chez's buffer is clever *only because* `ap`/`eap` are already hot. A plain card-marking barrier is simpler, has no overflow path, and needs only the card-table base in the thread context: `if v & TAG_HEAP_MASK { card[(slot_addr - heap_base) >> 9] = 0 }`, about 4-5 instructions and 1 extra store.
- **Card bytes.** Keep Chez's "card byte = youngest generation" encoding and the per-(from,to) dirty-segment lists, which make multi-generation scanning precise.
- **Scanning without parsing.** Design object layouts so that every word of a pointer-bearing space can be read as a value: headers encoded as fixnum-like `TaggedValue`s, padding filled with immediates. Then card scanning never needs to find object starts.
- **Static elision.** Do the same compile-time elision. Patina's compiler already knows fixnum, boolean and char results for many primitives.

---

## 9. Weak pairs, ephemerons, guardians, eq-hashtables

- **Weak pairs** are allocated directly in `space_weakpair` (`S_weak_cons` → `S_cons_in(..., space_weakpair, 0, ...)`, `c/alloc.c:1122-1125`). The car is skipped during sweeping and fixed up or bwp'd afterwards. The cdr is strong. **[V]**
- **Ephemerons** (`space_ephemeron`, 4 words: car, cdr, prev-ref, next) (`c/alloc.c:619-630`). When reached, an ephemeron goes on a pending list. If its key has not been reached yet, the ephemeron is moved onto **the key's segment's `trigger_ephemerons` list** (`check_ephemeron`, `c/gc.c:2717-2763`). When *any* object on that segment is later copied or marked, `check_triggers` puts the segment's ephemerons back on the pending list (`c/gc.c:747-766`). Pending ephemerons are processed only when sweeping made no progress (`c/gc.c:2066-2072`). The source states the worst case: "quadratic in the number of objects that fit into a segment" (`c/gc.c:747-752`). **[V]**
- **Guardians** (finalization). Each entry has obj, rep, tconc, ordered?, pending. Entries are split into hold, final and pend-final, and use the same segment-trigger trick for tconcs (`c/gc.c:1330-1565`). **Ordered guardians** are a Racket-CS addition: the GC sweeps from the representative *without* marking it, and only declares the entry final if the object is still unreached afterwards (`c/gc.c:1395-1420`). **[V]**
- **eq-hashtables under a moving collector.** Buckets hold `tlc` cells. When the GC copies or marks a tlc whose keyval pair is in old space, it queues the tlc on `tlcs_to_rehash` (`s/mkgc.ss:788-809`). After the GC, only those cells are moved to their new bucket, or removed if a weak key went to bwp (`c/gc.c:1734-1773`). Address hashing is `eq_hash` (`c/segment.h:76-88`). Rehash cost is proportional to *moved* entries, and in practice that is the young ones. **[V]**

**Transfer [I]:**
- **Ephemerons.** Patina currently resolves weak tables and ephemerons with a fixpoint over side tables (`patina:docs/GC_DESIGN.md` §9.5; `patina:crates/patina-core/src/heap/gc.rs`). Chez's segment-trigger approach removes repeated full rechecks, so adopt it if Patina has segments.
- **Hashing under a moving GC.** For `eq?`/`eqv?` hashing, either copy the tlc mechanism (rehash only moved entries) or store a lazily assigned identity hash in a header word (the JVM approach). Chez avoids header bits by paying for rehashing.

---

## 10. Symbols and the oblist

- Symbols live in `space_symbol` (6 words: value, pvalue, plist, name, splist, hash). The hash is stored in the symbol, so the oblist does not depend on addresses. **[V]**
- The oblist buckets are tracked per generation in `buckets_of_generation[g]`. **[V]**
- During a collection, a symbol in old space is treated as a root only if it has a top-level value, a property list or a system property list ("coordinate with alloc.c", `c/gc.c:1272-1304`). Afterwards the buckets are rebuilt in the target generation and unforwarded or unmarked symbols are pruned (`c/gc.c:1580-1625`). **Interned symbols with no binding are therefore collectable**, which is the "weak symbol table" that Patina's Stage 5 PRD lists as future work. **[V]+[I]**
- Top-level variables *are* symbol value slots. Compiled code reaches a global through the symbol, which is embedded via a relocation, and assigns through the dirty-store barrier (`s/cpprim.ss:3393`). **[V]** In Patina, the analogue is a heap-allocated binding cell ("location") that code objects reference from their constant tables. That matches Patina's rule that "an import installs a binding, not a value" (`patina:AGENTS.md`) and would move globals out of `Rc` `Environment` hash maps (`patina:crates/patina-core/src/environment.rs:51, 487`) and into the heap. **[I]**

---

## 11. Parallel collection

- Parallel collection is used only when more than one Scheme thread is active, or threads are waiting in the collect rendezvous (`S_gc`, `c/gcwrapper.c:1325-1330`). Each waiting Scheme thread donates a sweeper OS thread, up to `maximum-parallel-collect-threads = 16` (`s/cmacros.ss:1587`; `c/thread.c:540-580`; `setup_sweepers`, `c/gc.c:2985-3035`). **[V]**
- **Work is partitioned by segment ownership.** Each segment's `creator` thread is the only one allowed to copy or mark objects in it. A sweeper that meets a pointer into another owner's segment sends the *containing object* to that owner to re-sweep (`push_remote_sweep`). An object may be swept up to N times for N sweepers. Only the main sweep phase is parallel; roots, weak references and guardians run sequentially (`c/gc.c:121-185`). A Racket commit message confirms the design: "records an owner for each allocated segment … solely responsible for copying or marking". **[V]**
- **Transfer [I]:** this gives **no speed-up for a single-threaded mutator**, which is Patina's case. If Patina wants a parallel GC for one mutator, it needs work-stealing (HotSpot, MMTk, Whippet), not Chez's ownership model. Two parts are still worth copying: the discipline of never needing recursive copying (every copy/mark step is bounded), and the per-thread to-space bump pointers.

---

## 12. Stacks and continuations

- **Layout** (`IMPLEMENTATION.md:381-460`). The Scheme stack is separate from the C stack, is allocated **in the GC heap**, and grows upward. `SFP[0]` is the return address and `SFP[1..]` are the frame's variables. Calls are jumps, and the *caller* moves SFP. The return address points just after an **rp-header** embedded in the code:
  - a compact 2-word form: toplink plus `mask+size+mode`, with 5 bits of frame size (≤ 31 words) and **57 bits of live mask** on 64-bit;
  - a full 4-word form: mv-return-address, livemask (a fixnum or a **bignum** for large frames), toplink and frame-size

  (`s/cmacros.ss:1758-1797`; `ENTRYFRAMESIZE`/`ENTRYLIVEMASK`, `c/types.h:330-337`). **[V]**
- **How the GC walks a stack** (`trace-stack`, `s/mkgc.ss:1037-1095`). Starting from the top frame's return address, it loops: subtract the frame size, read the live mask, trace the live slots, then follow `SFP[0]` to the next return address. `trace-return-code` uses the header's toplink (the distance back to the code object) to find the **code object behind each return address**, relocates the code object, and rewrites the return address by the same delta (`s/mkgc.ss:1097-1110`). Return addresses are therefore interior pointers that the GC handles exactly. **[V]**
- **Sizes.** `default-stack-size` = 4 segments − 2 words, about 64 KiB. `stack-slop` = size/64, about 1 KiB, which covers bounded-stack primitives without overflow checks. `one-shot-headroom` = 3 × slop. `underflow-limit` = **16 words = 128 B** (`s/cmacros.ss:2176-2196`). **[V]**
- **call/cc** (`reify-cc-help`, `s/cpnanopass.ss:5147-5222`) allocates an 8-word (64 B) continuation (code=`nuate`, stack, stack-length, stack-clength, link, return-address, winders, attachments; `s/cmacros.ss:1568-1577`). It points the continuation at `[stack-base, sfp)`, makes the remaining space above `sfp` the new current stack segment, and replaces the return address with `dounderflow`. That is **O(1), with no copying**. Capturing again in the same frame reuses the existing link (`build-maybe-reify`). **[V]**
- **Invocation and underflow** (`S_split_and_resize`, `split`, `S_overflow`, `c/schsig.c:58-300`). A continuation segment larger than `underflow-limit` is **split at frame boundaries** so that only about 128 B plus one frame is copied back into the current stack. The rest stays shared, and later underflows copy one bounded piece at a time. Stack overflow also splits: frames below the split point are sealed into a continuation, and frames above are copied to a new stack. **[V]**
- **One-shots** (`call/1cc`). The stack-length field holds `opportunistic-1-shot-flag`; a shot one-shot is marked with `scaled-shot-1-shot-flag`, and the GC skips tracing its stack (`s/mkgc.ss:236`). Opportunistic one-shots are **promoted to multi-shot by every GC** (`conts_to_promote`, `c/gc.c:1776-1780`; `s/mkgc.ss:728-742`), because the GC can move the segment and the "fuse back into the current stack" optimisation is no longer valid. **[V]**
- **What the GC does to stacks [V]:**
  - Stack segments are untyped data objects. `copy_stack` copies only the live portion (`clength`) and trims headroom left by huge `apply` frames (`c/gc.c:803-856`).
  - Thread stacks in old space are copied, and `SFP`/`ESP` are rebased (`s/mkgc.ss:970-983`).
  - The per-thread `stack-cache` is dropped and `cached-frame` is cleared (`s/mkgc.ss:987-998`).

**Patina today:** the VM keeps one `registers: Vec<TaggedValue>` window stack and a `frames: Vec<CallFrame>` (`patina:crates/patina-vm/src/runtime/execution_state.rs:17-23`). `CallFrame` holds `closure: Option<HeapIndex>` and an `Rc` code object (`patina:crates/patina-vm/src/types/mod.rs:41-57`). Continuations **snapshot-copy** the registers and frames (`patina:crates/patina-vm/src/types/continuation.rs:65-80`) and live in weak side tables outside the heap. **[V]**

**Transfer [I]:**
- **Continuations.** Chez's representation fits Patina's semantics (full continuations, `dynamic-wind` winders stored in the continuation, delimited control on top):
  - make the VM register file a sequence of **heap-allocated stack segments**;
  - give each call site a live mask, which Patina's linear-scan allocator (pass 4) already knows;
  - let continuations be heap objects that point at sealed segments.

  Capture then becomes O(1). Reinstatement copies a bounded amount. The GC traces continuations precisely, which retires both the full-copy snapshots and the weak continuation side tables.
- **JIT frames.** Chez **never keeps Scheme values on the native stack**. A Cranelift JIT can do the same: JIT functions use the VM's heap stack segment for every value live across a call or safepoint, and hold GC references in SSA registers only between safepoints. Root discovery for JIT frames is then the same frame walk as for interpreter frames, and **continuation capture across JIT frames needs no native-stack walking**.

  If GC references must instead live in native frames, Cranelift's *user stack maps* require the frontend to spill before each safepoint and **reload afterwards, because the object may have moved** (fitzgen, "New Stack Maps for Wasmtime", 2024: <https://fitzgen.com/2024/09/10/new-stack-maps-for-wasmtime.html>; API: <https://docs.rs/cranelift-codegen/latest/src/cranelift_codegen/ir/user_stack_maps.rs.html>).

  Keeping the Scheme stack off the native stack also needs a way for JIT code to "return" to arbitrary frames. Cranelift has no block addresses, but it does have the `tail` calling convention and `return_call` (<https://docs.wasmtime.dev/api/cranelift_codegen/isa/enum.CallConv.html>). Return points can then be separate functions, or JIT code can return to a dispatcher. That design choice is the main open question (§15).

---

## 13. Code objects and relocation

- **Where code lives.** Code objects are allocated in `space_code`, at least 16 segments at a time (`c/alloc.c:268-269`), from separate code chunks (`S_code_chunks`). `S_record_code_mod` and an icache flush follow every code write (`c/alloc.c:1092-1107`). W^X is handled with `S_thread_start/end_code_write`, which brackets the whole GC (`c/gc.c:910, 1720`). **[V]**
- **How code moves.** The GC moves code objects. `trace-code` walks the relocation table (entries packed into one word, or an extended 3-word form). For each entry it reads the embedded object with `S_get_code_obj`, relocates the object, and writes it back with `S_set_code_obj`, which patches the instruction stream. It also copies the reloc table and records the code modification (`s/mkgc.ss:1112-1185`). **[V]**
- **Static code.** When code reaches the static generation, its reloc table is **dropped** unless `retain_static_relocation` is set (`s/mkgc.ss:1155-1160`). Static code never moves again. **[V]**
- **Closures** point at the code entry (code + `code-data-disp`), which is another interior pointer the GC handles exactly (`CLOSCODE`/`CLOSENTRY`). **[V]**

**Transfer [I]:** Cranelift emits position-dependent machine code with relocations (`MachReloc`), and patching it on every GC would require mprotect calls and icache flushes. Treat JIT machine code like Chez's *static* code: never move it, keep it outside the moving heap, and **embed no movable heap pointers**. Heap constants should be loaded from a per-function constants vector that is a traced heap object (reached through the closure or the thread context). The GC then updates the vector, not the instruction stream. The return-point metadata (frame size and live mask per return address) can live in a side table keyed by native return address, like Chez's rp-header but out of line.

---

## 14. Quantitative cheat-sheet (64-bit)

| quantity | value | source |
|---|---|---|
| segment / card | 16 KiB / 512 B (32 cards) | `s/cmacros.ss:2142-2150` |
| OS request granule | 128 segments (2 MiB) + 1 | `s/cmacros.ss:2154`, `c/segment.c:399-402` |
| seginfo | 168 B per segment (1.03 %) | measured replica of `c/types.h:143-186` |
| mark bitmap | 256 B per segment (1.56 %), allocated on demand | `c/types.h:127` |
| object alignment | 16 B (forward marker + address) | `s/cmacros.ss:478-481` |
| pair / flonum / symbol / continuation | 16 / 16 / 48 / 64 B | layouts in `s/cmacros.ss` |
| gen-0 trip | 8 MiB | `s/cmacros.ss:2120-2121` |
| generations | 0..4 (+ static 7); radix 4; max-gen threshold ×2 | `s/cmacros.ss:1586,2123`, `s/7.ss:609-623` |
| in-place threshold | segment ≥ 3/4 live; chunk ≥ 1/4 used | `c/gc.c:486-487` |
| immobile-by-size | > 128 segments (2 MiB) | `c/segment.c:381-384` |
| large-object split | > 2 KiB nursery tail left → separate run | `s/cmacros.ss:2161`, `c/alloc.c:513-558` |
| alloc fast path | about 4 instructions | `s/cpnanopass.ss:6748-6782` |
| barrier fast path | about 10 instructions / 3 stores (pointer); about 4 (fixnum) | `s/cpnanopass.ss:6807-6834` |
| safepoint poll | 2 instructions, timer 1000 ticks | `s/cpnanopass.ss:3997-4012`, `s/cmacros.ss:2119` |
| stack segment | about 64 KiB default; underflow copy ≤ 128 B + 1 frame | `s/cmacros.ss:2176-2196` |
| live mask | 57 bits inline (compact), else fixnum or bignum | `s/cmacros.ss:1772-1797` |
| C roots | ≤ 100 protected slots | `c/types.h:121` |
| parallel sweepers | ≤ 16, one per waiting Scheme thread | `s/cmacros.ss:1587` |

---

## 15. Mapping to Patina: what to adopt, adapt, avoid [I]

**Adopt:**
1. **Allocation never collects; safepoints only at polls and calls.** Keep Patina's pending-flag design (`patina:gc.rs:20-27`) and extend it: the JIT polls at function entry and loop back-edges (a decrement of a thread-context counter, or a flag load), and the allocation slow path refills without collecting. The results: no stack maps at allocation sites, no rooting in Rust primitives, and C and Rust code may hold raw values across allocation.
2. **One thread-context struct in a pinned register.** Cranelift supports a single pinned register (`enable_pinned_reg`, with `get_pinned_reg`/`set_pinned_reg`; <https://docs.rs/cranelift-codegen/latest/cranelift_codegen/settings/struct.Flags.html>). Put the thread context there, as Chez does with `%tc`. Keep `ap`/`limit`, the poll counter, and the card-table and heap base in it. Within a JIT function, `ap` can be an SSA variable that is written back before calls, as Chez writes `eap` through to the thread context.
3. **A single gen-0 bump region holding all types.** Promote into segregated segments, with 16-32 KiB segments and side metadata in a flat array indexed by `(addr - heap_base) >> 14`.
4. **The hybrid: young generations copied, old ones marked in place with sparse evacuation,** plus per-segment pinning counters for immobile objects and for whatever cannot move yet.
5. **Card bytes holding the youngest referenced generation,** with dirty-segment lists by (from, to), and static barrier elision for values known to be immediates.
6. **Pointer-only spaces that can be scanned without parsing.** Headers should look like immediates.
7. **One declarative layout spec** that generates trace, copy, size and verify code and the JIT field offsets. Specialise the minor collector with constants (`gc-011`).
8. **Ephemeron and guardian triggers by segment;** process pending ephemerons only at a no-progress fixpoint.
9. **Collectable bare symbols,** and a **static generation** for the boot image (Patina's `lib/scheme` stdlib and code objects).
10. **Segmented heap stacks with return-point live masks** for VM and JIT frames, with O(1) `call/cc`.

**Adapt:**
- **Barrier.** Prefer a direct card mark over Chez's store buffer for Cranelift, because it has no overflow path and does not interact with allocation limits. Benchmark both.
- **Eq-hashing under moving.** Choose between the tlc-style rehash of moved entries and a header identity hash.
- **External memory.** Account for memory outside the heap (any remaining Rust `Vec` payloads) the way Chez's phantom bytevectors do (`c/alloc.c:1127-1161`), so it counts toward GC triggering.

**Avoid:**
- the 3-level segment radix (use a reserved contiguous range);
- 19 spaces × 8 generations of per-thread bump pointers;
- record card-scanning that has to rediscover object starts;
- ownership-partitioned parallel GC (no gain with one mutator);
- moving machine code and patching relocations on each GC.

**Precondition [I]:** Chez works because *everything* the mutator can reach is either in the heap or in the thread context, the protected array, or the locked list. Patina's `Rc` graphs that hold `TaggedValue`s outside the heap, listed in `patina:docs/GC_DESIGN.md:122-136`, must move into the heap or go through a small, explicitly traced root registry before a moving collector can work. The specific items are: `Environment` maps, `CompiledMacro` literals, `CodeObject.constants` (`patina:crates/patina-vm/src/types/code_object.rs:127-140`), `CallFrame.closure`, the `SourceMap` raw-bits keys and the tree-walker's `CpsContinuation`. Chez's answers to each of the seven obstacles listed there:
1. Symbol table → oblist rebuild (§10).
2. Raw-bits side tables → tlc-style rehash.
3. `eq?` hashing → same as 2.
4. Frame closure index → frames as heap stack segments with live masks.
5. Code constants → a traced constants object, or reloc tables.
6. Macro literals → heap objects.
7. Environments → heap binding cells.

---

## 16. Open questions

1. **Stack discipline for the JIT.** Should JIT code use a separate heap-allocated Scheme stack, with return points implemented as `tail`-convention functions or a dispatcher? Chez's model gives free continuation capture and uniform frame walking. The alternative is native frames with Cranelift user stack maps, which is cheaper per call but makes `call/cc` and the GC frame walk harder. This needs a prototype measurement.
2. **Address representation.** Raw 64-bit addresses (Chez) or 32-bit compressed offsets from a reserved base (HotSpot compressed oops, V8)? Compressed offsets keep an 8-byte `TaggedValue` with room for other tags, but add a base add on each dereference unless the base is in a register.
3. **Card marking vs. store buffer under Cranelift.** Which barrier does better on Patina's mutation-heavy benchmarks (`set-car!`, `vector-set!`, MutableCell writes)?
4. **The tree-walker backend.** Can its `CpsContinuation` and `Rc` structures move into the heap, or should the tree-walker run in a "pin everything it holds" mode (a per-segment `must_mark`) while the VM gets full moving?
5. **Finalization.** Does Patina need guardians or finalizers at all? R7RS-small does not. Weak and ephemeron hash tables do need the ephemeron machinery.
6. **Flonum boxing.** Chez boxes flonums (16 B each) and relies on unboxing in the compiler. Should Patina's JIT plan float unboxing, which matters for allocation rate and therefore for the right nursery size?
7. **Policy constants.** Chez's 8 MiB trip, radix 4 and 5 generations were tuned for a native compiler. Patina's interpreter allocation rate and cache sizes may want different values. Measure with `run_gc_differential.sh`-style lanes.

---

### Sources

- Chez Scheme source (local): `c/gc.c`, `c/gcwrapper.c`, `c/alloc.c`, `c/segment.c`, `c/segment.h`, `c/types.h`, `c/gc-011.c`, `c/gc-par.c`, `c/gc-ocd.c`, `c/schsig.c`, `c/thread.c`, `c/prim5.c`, `s/cmacros.ss`, `s/cpnanopass.ss`, `s/cpprim.ss`, `s/mkgc.ss`, `s/7.ss`, `s/library.ss`, `s/x86_64.ss`, `s/arm64.ss`, `IMPLEMENTATION.md`, `csug/smgmt.stex`, `release_notes/release_notes.stex`.
- Papers cited in `IMPLEMENTATION.md`:
  - Dybvig, Eby, Bruggeman, *Don't Stop the BiBOP*, IU TR 400 (1994). The URL listed there did not resolve from this machine.
  - Hieb, Dybvig, Bruggeman, *Representing Control in the Presence of First-Class Continuations*, PLDI 1990.
  - Flatt, Dybvig, *Compiler and Runtime Support for Continuation Marks*, PLDI 2020.
- Racket CS GC commits summarising the ownership-based parallel GC and mark-in-place: <https://gitea.suzanne.soy/suzanne.soy/racket/commits/commit/41623f3027e78aa0aa543f34b3d2499c140345ec?page=22>
- Cranelift:
  - user stack maps: <https://docs.rs/cranelift-codegen/latest/src/cranelift_codegen/ir/user_stack_maps.rs.html> and <https://fitzgen.com/2024/09/10/new-stack-maps-for-wasmtime.html>
  - pinned register: <https://docs.rs/cranelift-codegen/latest/cranelift_codegen/settings/struct.Flags.html>
  - tail calling convention: <https://docs.wasmtime.dev/api/cranelift_codegen/isa/enum.CallConv.html>
- Patina (read-only):
  - `crates/patina-core/src/tagged_value.rs`
  - `crates/patina-core/src/heap/mod.rs`
  - `crates/patina-core/src/heap/gc.rs`
  - `crates/patina-core/src/environment.rs`
  - `crates/patina-vm/src/runtime/execution_state.rs`
  - `crates/patina-vm/src/types/{mod.rs,continuation.rs,code_object.rs}`
  - `docs/GC_DESIGN.md`
  - `AGENTS.md`
