# OCaml 5 and Gambit memory management: lessons for Patina's GC and Cranelift JIT

Research note for the Patina GC redesign. Read-only study; nothing in the repo was changed.

**Sources.** OCaml checkout `~/Project/reference/ocaml` at `70165ff` (5.6.0+dev, 2026-09-04). Gambit checkout `~/Project/reference/gambit` at `ab6f255` (2026-08-27). Patina `main` at `28a94f8`. Paths below are relative to those roots. **[V]** marks a fact checked in source or a primary document. **[I]** marks my inference or recommendation.

---

## 0. Executive summary

1. **One limit word does all the polling.** OCaml compares `young_ptr` against a single `young_limit` word. That one check covers four things: the minor heap filling, the half-way trigger for a major slice, memprof sampling, and every asynchronous request (signals, requests from other domains). To interrupt, a thread sets `young_limit = UINTNAT_MAX` [V]. Gambit does the same with `stack_trip`: an interrupt makes the next stack/poll check fail [V]. For Patina [I]: a JIT allocation fast path and its poll should be "bump, compare against one limit word in VM state". The interpreter's current per-instruction `gc_pending` load (vm_state.rs:1205, 1287-1291) can become that same check, done only at allocations and loop back-edges.
2. **Stack maps are keyed by return address.** OCaml looks them up in a hash table (`frame_descriptors.c:383-396`). Gambit stores the descriptor in the return-point label object, so lookup is O(1) (`gambit.h.in:9237-9310`). Both work because GC happens only at calls and polls. In OCaml, **every call destroys every register** (`asmcomp/amd64/proc.ml:307-309`), so at calls the descriptors name only stack slots; registers are named only at GC-entry points, which save them into a `gc_regs` bucket. Cranelift's current "user stack maps" use the same model: the frontend spills live GC refs to stack slots, safepoints are non-tail calls, and the runtime looks maps up by PC [V, Cranelift docs]. Patina already computes per-pc register-liveness bitmaps for the interpreter (`CodeObject::register_roots`, code_object.rs:151-158; pass5_codegen.rs:223). These are the interpreter's equivalent of frame descriptors.
3. **OCaml's write barrier serves both GCs at once.** `caml_modify` (memory.c:306-355) is a SATB/Yuasa deletion barrier for the incremental major GC *and* a precise slot remembered set for the minor GC. The compiler emits it as an out-of-line call that is **not allowed to allocate** (`Cextcall(..., false)`, cmm_helpers.ml:771). It is therefore never a safepoint and needs no stack map. Initialising stores into fresh young objects take no barrier.
4. **Continuations need no stack write barrier when a captured stack is immutable or is reached only through a freshly allocated object.** In OCaml 5, each `perform` allocates a new one-word `Cont_tag` object in the minor heap (amd64.S:1127-1131). Promoting that object scans the fiber stack (minor_gc.c:270-288). Resuming it darkens the stack if marking is in progress (fiber.c:688-716). Gambit's call/cc is O(1): it pushes a "break frame" and builds the continuation object *on the stack* (gambit.h.in:6577-6593). Frames below the break become immutable and are restored lazily, one per return, by an underflow "break handler" (_kernel.scm:1077-1270). The GC moves them into the heap as ordinary frame objects (mem.c:3667-3814). Patina instead copies all frames and the whole register prefix on every capture (`VmContinuation`, types/continuation.rs:159-196).
5. **Gambit's three allocation strategies match Patina's escape problem.** Gambit has *permanent* objects (never moved, never scanned), *still* objects (non-moving, mark-sweep, with a refcount for references held by C) and *movable* objects (copying) (mem.c:58-186). A moving young generation is usable even when some raw references escape into Rust, provided those references point only at still or permanent objects, or go through counted handles.
6. **Recommended direction [I]:**
   - Bump-allocated, copying nursery.
   - Non-moving, size-classed old generation with lazy sweep. Start it stop-the-world; make it incremental later using OCaml's pacing model.
   - Precise roots: the register file plus per-pc maps; Cranelift user stack maps later.
   - A combined generational + SATB barrier.
   - Segmented VM stacks with Gambit-style O(1) capture.
   - Optional explicit compaction later; OCaml 5.2 shows it can be added to a pooled heap.

---

## 1. Patina facts this note depends on [V]

- **`TaggedValue` encoding.** A `u64` with a 3-bit low tag: fixnum 0, special 1, char 2, then pair, vector, string, closure and object (tagged_value.rs:70-84). Heap references are `index << 3 | tag` (tagged_value.rs:377-412). So a reference is a 29-61-bit arena index, not an address. (AGENTS.md says "NaN-boxed"; the source says otherwise.)
- **`Heap` layout** (heap/mod.rs:304-427):
  - Typed arenas: `pairs: Vec<(TV,TV)>`, `vectors: Vec<Vec<TV>>`, `strings: Vec<Vec<char>>`, `objects: Vec<HeapObjectData>`.
  - Four free lists, a `symbol_table` of raw `HeapIndex`, and `gc_defer_depth`.
  - `allocs_since_gc` counts **objects, not bytes** (`note_alloc`, heap/mod.rs:581-584).
- **Measured sizes** (`PRD/study/gc/probes/ocaml-gambit`, `size_of`, release):
  - `TaggedValue` 8, pair slot 16, `CallFrame` 40.
  - `Vec<TV>` vector slot 24, plus a separate malloc for the elements.
  - `HeapObjectData` **72**. A flonum (`Real(f64)`) therefore costs a 72-byte slot; OCaml uses 16 bytes (header + double).
  - A VM closure is a 72-byte slot, plus a separate `Vec` for its free variables, plus an `Rc<Environment>` clone (heap/mod.rs:205-213, 1299-1310).
- **Collector.** Non-moving STW mark-sweep with side mark bitmaps (gc.rs:1-27). It is explicitly a "non-moving contract" (gc.rs:13-15; GC_DESIGN.md §3.4).
- **Safe points.** One at the top of each driver-loop iteration. A nested loop never collects (`GcDeferGuard::is_outermost`, vm_state.rs:1146-1154; GC_DESIGN.md §7). The VM safe point first calls `retire_registers` (vm_state.rs:1291), which clears registers that are not live per the per-pc `register_roots` bitmap (gc_roots.rs:47-68). These maps already exist and normal dispatch does no work for them (code_object.rs:151-158).
- **Continuations.** A `VmContinuation` stores a cloned `Vec<CallFrame>`, wind/prompt/handler stacks, and `registers: Vec<TaggedValue>` covering all live frames (types/continuation.rs:159-196). Capture is O(stack depth). Continuation payloads live in weak side tables keyed by id (gc_roots.rs:1-30).
- **Barrier sites a generational design would need.** Mutation APIs include `set_car` (heap/mod.rs:743), `set_cdr` (755), `vector_set` (804), `write_mutable_cell` (1273, through a `RefCell` inside the object) and `set_vm_closure_free_var` (1394). Global stores go through `Environment` `FxHashMap`s outside the heap.

---

## 2. OCaml 5 runtime

### 2.1 Object header and heap colours [V]

- **Header layout:** `[reserved R | wosize | colour:2 | tag:8]` (caml/mlvalues.h:144-164).
- **Colours are relabelled, not reset.** The major GC uses three statuses, MARKED, UNMARKED and GARBAGE, held in a global `caml_global_heap_state`. Initially they are 0, 1, 2 (shared_heap.c:42-46). A fourth colour, `NOT_MARKABLE` (3), marks free blocks and out-of-heap static data (caml/shared_heap.h:68-71).
- **Cycle start is O(1).** The domains stop and the bit patterns are relabelled: Marked becomes Unmarked, Unmarked becomes Garbage, Garbage becomes Marked. No pass over the heap clears mark bits. (Quoted in "Retrofitting Parallelism onto OCaml", ICFP 2020: https://arxiv.org/abs/2004.11663.)
- **Allocation colour.** New major allocations take `caml_allocation_status()`: MARKED while marking is in progress, otherwise UNMARKED (caml/shared_heap.h:93-98). This is the "allocate black" rule.
- **Patina relevance [I].** Patina keeps marks in side bitmaps and clears them each cycle. If headers are introduced, colour rotation removes the "clear marks" pass; sweep then frees GARBAGE and leaves everything else alone. With side bitmaps, a "flip which bitmap means marked" variant gets the same effect.

### 2.2 Minor heap: inline bump allocation and promotion [V]

**Per-domain state.** `young_ptr`, `young_limit`, `young_start`, `young_end` and `young_trigger` are fields of `Caml_state` (caml/domain_state.tbl:17-37). Allocation is **downward**: `young_ptr -= size; if (young_ptr < young_limit) slow`.

**Native code** reserves `r15` as the allocation pointer and `r14` as the domain-state pointer (asmcomp/amd64/proc.ml:47-48). An allocation compiles to (emit.mlp:607-635):

```
sub  $n, %r15
cmp  young_limit(%r14), %r15
jb   Lgc            ; out-of-line, at function end: call caml_call_gc; jmp Lret
Lret: lea 8(%r15), %res
```

That is three instructions plus a predicted branch. The slow-path stubs are emitted once per function at its end (emit.mlp:261-264, 937).

**Comballoc.** Allocations in one basic block are merged into a single bump. The frame descriptor records each allocation's length, so the GC entry can recover the sizes (signals_nat.c:62-84; caml/frame_descriptors.h:43-46).

**Limits and defaults.**
- Objects of at most `Max_young_wosize` = 256 words go in the minor heap; larger ones go straight to the major heap (config.h:197).
- Default minor heap: `Minor_heap_def` = 262144 words, i.e. **2 MiB** on 64-bit (config.h:206).

**Promotion copies survivors straight into the major heap.** There are no survivor spaces or ageing (`oldify_one`, minor_gc.c:236-396; `alloc_shared` targets the shared pools).
- A forwarded block's header becomes `Promoted_hd` (0), and field 0 holds the new address (caml/minor_gc.h:79; minor_gc.c:254-258).
- Promotion is depth-first through a todo list threaded through the old copies' field 1 (minor_gc.c:316-326). There is no Cheney scan pointer, because the destination is a free-list heap rather than a contiguous to-space.
- Minor roots: the remembered set `major_ref` (minor_gc.c:557-628), finaliser and memprof roots, the local roots and current stack (`caml_do_local_roots`, minor_gc.c:676-678), and global young roots.
- Minor GC is **stop-the-world, parallel** across domains. The ICFP 2020 paper measured this variant ("ParMinor") against a concurrent one: ParMinor was 3.5% slower than stock OCaml 4, ConcMinor 4.9% slower. The maximum pause on `menhir.ocamly` was 1125 ms for stock and 689 ms for ParMinor (arXiv 2004.11663, §6).

**Patina relevance [I].**
- A nursery needs one property Patina lacks today: allocation sites that bump a pointer into memory addressable by the mutator.
- Arenas indexed by `u32` could be bumped per type (`pairs.len()` is already a bump index). But a heterogeneous nursery, and JIT-inlined allocation, want either raw addresses or 32-bit offsets into one reserved region (Wasmtime's compressed 32-bit GC refs are precedent: Cranelift's old stack maps "forced 64-bit extensions", which motivated the redesign; https://fitzgen.com/2024/09/10/new-stack-maps-for-wasmtime.html).
- OCaml's "promote directly, no ageing" shows that a simple two-generation design is competitive when the nursery is small (a few MiB) and most objects die young. That is the Scheme profile too.

### 2.3 Write barrier and remembered set [V]

The barrier, as implemented in memory.c:306-327:

```c
write_barrier(obj, field, old, new):
  if (!Is_young(obj)) {
    if (Is_block(old)) {
      if (Is_young(old)) return;                  // slot already in remembered set
      if (caml_marking_started()) caml_darken(old); // SATB / Yuasa deletion barrier
    }
    if (Is_block_and_young(new)) Ref_table_add(&major_ref, &Field(obj, field));
  }
```

- **`Is_young` is an address-range test** against one reserved region holding every domain's minor heap: `caml_minor_heaps_start < v < caml_minor_heaps_end` (caml/address_class.h:50-58). There is no header load and no page table. This needs the nursery inside one contiguous virtual reservation [I].
- **The remembered set stores slot addresses.** `major_ref` is a growable array of `value*` with no deduplication (caml/minor_gc.h:58-75, 111-118). At minor GC, each recorded slot is re-read and its current value is oldified (minor_gc.c:620-628). Stale entries are harmless.
- **`caml_initialize`** is for the first store into a field of an object just allocated in the major heap (`caml_alloc_shr`). It records a remembered-set entry only when the slot is old and the value is young (memory.c:427-439). Stores into fresh *young* objects use no barrier at all.
- **The compiler emits `caml_modify` as a non-allocating external call** (`Cextcall("caml_modify", typ_void, [], false)`, cmm_helpers.ml:771-776; the bool means "may allocate", cmm.mli:144-148). A barrier can never trigger a GC. It therefore needs no frame descriptor and is not a safepoint; only C-caller-saved registers are clobbered (proc.ml:310-313).
- **Cost model** (ICFP 2020 paper): "reads are fast and updates are comparatively slower". OCaml has no read barrier.

**Patina relevance [I].** This is the barrier to copy. In JIT code, inline only the filter `if obj_is_old && new_is_young_or_marking` and make the slow path a non-safepoint call. Patina's barrier sites are few and already centralised in `Heap` methods (§1), so the interpreter needs perhaps five call sites. The hard part is the *off-heap* `TaggedValue` holders: `Environment` maps, `CodeObject.constants`, `CompiledMacro`, records in `Rc` structs (GC_DESIGN.md §3.4). Under a moving nursery each one is either:
1. made a heap object, so it gets the barrier;
2. treated as a root scanned at every minor GC (cost proportional to its size); or
3. registered OCaml-style as a **generational global root**. OCaml keeps separate young and old skiplists so minor GC scans only the young ones, and updates go through `caml_modify_generational_global_root` (caml/memory.h:565-593; globroots.c:48-50).

### 2.4 Safepoints, `young_limit` polling and interrupts [V]

**One word, many reasons to stop.**
- The minor heap filling.
- The **half-way** trigger: after each minor GC `young_trigger` is set to `young_start + (end-start)/2` (minor_gc.c:689-694). Crossing it schedules a major slice (`caml_poll_gc_work`, domain.c:2093-2137). The major GC is thus paced by minor-heap consumption.
- Memprof sampling (`memprof_young_trigger`).
- Asynchronous actions: signals, STW requests from other domains, finalisers.

Any thread can interrupt a domain with `atomic_store(young_limit, UINTNAT_MAX)` (domain.c:387-396). Only the owner resets the limit, through `caml_reset_young_limit`. That function re-checks pending flags after an atomic exchange so that no request is lost (domain.c:2022-2058).

**One slow-path entry.** `caml_call_gc` (amd64.S:626-643) saves all registers into a `gc_regs` bucket and calls `caml_garbage_collection` (signals_nat.c:42-87). That function finds the frame descriptor for the return address:
- If it records zero allocations, the site was a **poll**, and only pending actions are processed.
- Otherwise it sums the combined allocation sizes and calls `caml_alloc_small_dispatch`, which may run the minor GC, a major slice and signal handlers, and then performs the allocation.

**Poll insertion is a compiler analysis** (asmcomp/polling.ml).
- **Loops:** a backward dataflow pass marks a loop head "Safe" if every path from it reaches `Ialloc`, `Ipoll`, a return or a tail call. Otherwise an `Ipoll` goes on the back edge (polling.ml:36-89, 180ff).
- **Tail calls:** an infinite sequence of calls must contain infinitely many *potentially recursive tail calls* (PRTCs), meaning tail calls to unknown functions or to functions defined later. The prologue of a function that might reach a PRTC without allocating gets a poll (polling.ml:90-160).
- `Ipoll` emits `cmp young_limit(%r14), %r15; jbe Lgc` (emit.mlp:637-656).

**The bytecode interpreter polls at fewer points:** allocation (`Alloc_small` with `Caml_check_gc_interrupt`), a dedicated `CHECK_SIGNALS` instruction, and `POPTRAP` (interp.c:946, 1028-1035). Before entering the GC it spills `accu`, `env` and `pc` onto the scanned OCaml stack (`Setup_for_gc`, interp.c:76-80). So at a GC point the whole interpreter state is in scannable memory.

**Patina relevance [I].**
- **Interpreter.** Today every dispatch iteration loads `gc_pending` (vm_state.rs:1205). With a nursery limit word, the check can sit only at allocating instructions, call and return, and backward jumps, and it doubles as the allocation-exhausted test.
- **Scheme loops are tail calls.** OCaml's PRTC rule is the right model: a JIT'd procedure that can reach an unknown tail call without allocating polls at its entry. A self-tail-call loop polls on its back edge.
- **Interrupts.** Keyboard interrupts, timers for green threads, and requests from a future background compiler thread to stop the world all become "set `young_limit` to max". This is cheap and needs no extra instruction on the fast path.

### 2.5 Frame descriptors: OCaml's stack maps [V]

**What gets recorded.** The compiler emits a descriptor for **every return address that can be a GC point**: each call site, plus each `caml_call_gc` site (caml/frame_descriptors.h:27-54). The layout (frame_descriptors.h:61-75) is:

```
uintnat retaddr; uint16 frame_data (size | has_alloc | has_debug); uint16 num_live;
uint16 live_ofs[num_live]   // even = byte offset from sp; odd = (reg_index<<1)|1 into gc_regs
[uint8 num_allocs; uint8 alloc_len[...]]   // when the site is an allocation
[uint32 debuginfo[...]]
```

`frame_data == 0xFFFF` marks a "return to C" boundary that ends an OCaml stack chunk (frame_descriptors.h:59-81).

**Why stack slots suffice at calls.** OCaml calls destroy every physical register (`destroyed_at_oper (Icall_* ) = all_phys_regs`, proc.ml:307-309). So at a call, live values are always in stack slots, and only `caml_call_gc`/poll sites name registers, through the saved `gc_regs` bucket.

**Lookup.** An open-addressing hash table of descriptors keyed by `retaddr >> 3`, sized to twice the descriptor count (frame_descriptors.c:28-63, 120-140, 383-396).

**Code loaded at run time.** Frametables are registered and unregistered dynamically:
- `caml_register_frametables` and `caml_unregister_frametables` (frame_descriptors.c:302-362);
- `caml_copy_and_register_frametable` for natdynlink;
- unregistering defers frees to a "zombie" list processed inside a STW section, because a table may still be in use during a scan (frame_descriptors.c:53-59, 177-192).

**Stack walk** (`scan_stack_frames`, fiber.c:262-318): look up the descriptor for the return address and visit each live slot. On a `return_to_C` descriptor, switch to the saved `gc_regs` of the previous chunk and skip the C-to-OCaml header. The walk follows fiber parents (fiber.c:321-336).

**Generational stack scanning.** On arm64, POWER and RISC-V the runtime sets a bit in the saved return address of each frame it has already scanned (`CODE_POINTER_MARK_BIT`). Minor GCs stop at the first marked frame, so a deep stack is not rescanned on every minor GC (caml/stack.h:117-126; fiber.c:283-294). The bit is masked off before lookup. amd64 does not define it.

**JIT relevance [I].** This is the template for Cranelift frames:
- Cranelift's **user stack maps** have the same contract. The CLIF producer declares GC values (`FunctionBuilder::declare_value_needs_stack_map`), they are spilled to stack slots before each safepoint and reloaded after, and "all non-tail call instructions are considered safepoints". Compiled maps give SP-relative offsets ([user_stack_maps.rs](https://docs.wasmtime.dev/api/src/cranelift_codegen/ir/user_stack_maps.rs.html); [FunctionBuilder docs](https://docs.rs/cranelift-frontend/*/cranelift_frontend/struct.FunctionBuilder.html)).
- Wasmtime stores maps as sorted PC offsets, found by binary search, with a bitmap of 4-byte SP offsets ([wasmtime_environ::stack_map](https://docs.wasmtime.dev/api/src/wasmtime_environ/stack_map.rs.html)).
- The reload-after-safepoint discipline is what makes a moving collector sound with this model, as fitzgen's post states.
- **Missing piece:** GC entry from an *allocation slow path* is a call too, so in Cranelift it is just another safepoint call with a map. There is no need for OCaml's register-saving `caml_call_gc`.
- **Code lifetime:** Patina will load and drop JIT code (it already drops bytecode by refcount, #338). Unregistration must be deferred, as OCaml's zombie list does, until no frame can reference the code.

### 2.6 Major heap: size-class pools, lazy sweep, incremental mark, pacing [V]

**Layout.**
- 32 size classes up to 128 words, in **pools of 4096 words (32 KiB)** with a 4-word header (caml/sizeclasses.h:9-29).
- Objects above 128 words are malloc'd as `large_alloc` with a linked header (shared_heap.c:48-80).
- A pool's free list is threaded through free blocks. A free block has the `NOT_MARKABLE` colour and `No_scan_tag`, and runs of adjacent free blocks are merged (shared_heap.c:58-64, 555-620).

**Sweeping is lazy and incremental.** Allocation pulls from `avail_pools`; when that runs dry it sweeps an `unswept_*` pool (shared_heap.c:438-470, 536-620). Sweep work done during allocation is credited to the GC's work counter (`sweep_work_done_between_slices`, domain_state.tbl:155-159). Garbage blocks with custom finalisers run them during sweep (shared_heap.c:571-575).

**Marking is incremental.** It runs in budgeted slices with a mark stack and a prefetch buffer (`do_some_marking`, major_gc.c:1566ff). SATB holds because the write barrier darkens the old value of a field while marking (§2.3), new objects are allocated MARKED, and roots, including the current stack, are scanned at cycle start in a STW phase (`caml_mark_roots_stw`, major_gc.c:1828).

**Pacing** (major_gc.c:871-1190):
- Two monotone counters, `alloc_counter` and `work_counter`, in "work units". Each slice converts words allocated since the last slice into work:

  `alloc_work = allocated_words × total_cycle_work × 3(100+o) / (2 × heap_words × o)`

  where `o = space_overhead` (default **120**, config.h:216) and `total_cycle_work = heap_words (sweep) + heap_words × 100/(100+o) (mark)`.
- The slice target is `alloc_counter`. If work falls behind by more than two cycles' worth, the gap is artificially closed (major_gc.c:1121-1139).
- Off-heap memory feeds in through `caml_alloc_dependent_memory`/`caml_adjust_gc_speed` (memory.c:365-414).
- The slice runs at the minor-heap half-way trigger (§2.4).

**Recent tuning.**
- **5.4.** `Gc.ramp_up` lets programs mark ramp-up phases so that their allocation is not charged as collection work. It was "the main known slowdown of OCaml 5 relative to OCaml 4 for Coq/Rocq" (Changes:1710-1715).
- **5.5.** A sweep-only phase at cycle start (#13580, Changes:891-893) reduces latent-garbage delay.

**Patina relevance [I].**
- Patina's typed arenas are already size-segregated in effect (pairs 16 B, objects 72 B).
- A size-class pool heap with headers is a better old generation for a JIT: fixed-size slots per class, a free list per pool, lazy sweep amortised into allocation, non-moving.
- The pacing formula is worth copying even for a STW old-generation collector: trigger on bytes promoted with a `space_overhead`-style knob, not on object counts (Patina counts objects today).

### 2.7 Ephemerons and weak data [V]

- Each domain keeps `todo` and `live` lists of ephemerons. The data of an ephemeron is darkened only once all its keys are marked. Rounds repeat until no domain makes progress (`ephe_mark`, major_gc.c:516-633; round control at 396-465).
- Marking the data immediately runs `mark()` so that dependency-ordered chains finish in one round (major_gc.c:592-604).
- A major-heap ephemeron whose key or data is young is recorded in a separate `ephe_ref` table for the minor GC (caml/minor_gc.h:49-56).
- Patina's fixpoint (gc.rs:1013-1095) is the same idea. A generational version needs the `ephe_ref` analogue.

### 2.8 Compaction returns in 5.2 [V]

- `caml_compact_heap` (shared_heap.c:1149-1540) is explicit only, on `Gc.compact` ("Explicit only for now", Changes:3000-3006). It is parallel, and runs right after a full major GC (#12859).
- **Algorithm.** Per size class: count live blocks in partially filled pools, keep just enough pools to hold them, and evacuate the rest into the kept pools. Then rewrite every pointer by walking all roots (stacks included, `caml_scan_stack(&compact_update_value...)`, shared_heap.c:1083) and the whole heap, finalisers and ephemerons. Finally release the evacuated pools.
- **Lesson [I].** A non-moving pooled heap can gain defragmentation later, as long as *every* reference is reachable and updatable by the collector. Patina's raw-index escapes (symbol table, raw-bits maps; GC_DESIGN.md §3.4) are exactly what this would require fixing.

### 2.9 Fibers (OCaml 5 effects) as a model for continuation stacks [V]

- **Stack layout.** A fiber stack is a separately allocated `stack_info` block. Fields: `sp`, `exception_ptr`, `handler`, a size-class bucket, and a `stack_handler` at the top holding the value/exception/effect handlers and the parent stack (caml/fiber.h:30-87).
- **Size and growth.** The initial size is `caml_fiber_wsz = 2 × Stack_threshold / 8` = **64 words** (gc_ctrl.c:345; config.h:176-177). The PLDI 2021 paper says 16 words, an earlier value. Overflow is detected against `Stack_threshold`, and the stack is copied to a new one twice the size (`caml_try_realloc_stack`, fiber.c:537-580). Copying works because OCaml stacks hold no interior pointers except the exception-handler chain and C-stack links, both rewritten on move (`caml_rewrite_exception_stack`; caml/fiber.h:99-113). Stacks are recycled through per-domain size-class caches.
- **Continuation objects.** A continuation is a **one-word `Cont_tag` object pointing at the suspended stack**. `caml_perform` receives a *freshly allocated* continuation in `%rbx` (amd64.S:1127-1131).
- **Minor GC.** When a young `Cont_tag` is promoted, the whole stack it owns is scanned with `oldify_one` (minor_gc.c:270-288). The stack's young references are thereby handled without any stack write barrier. A suspended stack cannot change, and a stack in use is the current stack, which is a root.
- **Major GC.** `caml_darken_cont` CASes the header to `NOT_MARKABLE` as a lock, scans the stack, then publishes MARKED (major_gc.c:1753-1782). `caml_continuation_use_noexc` (fiber.c:688-716) darkens a major-heap continuation **before resuming it** while marking is on. This is the only "barrier" stacks need under SATB: "since fibers are program stacks, before switching control to a fiber, all the objects on the fiber stack must be marked" (ICFP 2020).
- **One-shot.** Resuming twice raises an exception (fiber.c:718-724). The paper justifies this on efficiency grounds (PLDI 2021 §3.1). Sweep has no `Cont_tag` case (no stack-freeing hook in `pool_sweep`, shared_heap.c:536ff, checked by grep), so a dropped, never-resumed continuation's stack is not freed by the GC [V by absence; I on the consequence].
- **Measured costs** (PLDI 2021, https://arxiv.org/abs/2104.00250):
  - Code that does not use effects is "less than 1% slower than stock" (geometric mean, §6.1).
  - The perform/resume segments measured 23, 5, 11 and 7 ns (§6.3).
  - Generators were 2.76× slower than a hand-CPS version (§6.3.1).

**Patina relevance [I].** R7RS `call/cc` is multi-shot, so OCaml's one-shot fibers cannot be adopted directly. Two of their GC ideas do transfer:
1. Represent a captured continuation as **an object allocated young at capture time that owns an immutable stack segment**. The segment's young references are then handled when that object is promoted, with no barrier on the segment.
2. **Darken a captured segment on reinstatement** if the old generation's marking is incremental.

Patina's `call-with-continuation-prompt`/`abort`, generators and green threads, if added, would be one-shot and could use fibers directly.

### 2.10 Rooting C code [V]

- `CAMLparam`/`CAMLlocal` link `caml__roots_block`s, each holding up to 5 `value*` table pointers, into `Caml_state->local_roots`. `CAMLreturn` restores the head (caml/memory.h:241-246, 290-413). The GC walks the list in `caml_do_local_roots` (roots.c:51-57) and updates the slots after moving objects.
- `CAMLnoalloc` is a debug assertion region for code that must not allocate (used in fiber.c:692).
- **Patina relevance [I].** This is the shadow-stack pattern a moving Patina would need in Rust primitives that hold `TaggedValue`s across an allocating call. The alternative is the current deferral rule, which must change. With a nursery, "cannot collect here" must mean "allocate this object in the old generation and use `caml_initialize`-style recording" (memory.c:427-439, 542-574), not "keep filling the nursery". Otherwise a deferred region could overflow the nursery. OCaml's `alloc_shr` + `caml_initialize` pair is exactly that escape hatch.

---

## 3. Gambit memory manager

### 3.1 Three allocation strategies [V] (lib/mem.c:58-186)

- **Permanent.** Never moved, never reclaimed, never scanned. All pointers inside a permanent object must point to permanent objects. Used for C globals and module literals. **Interned symbols must be permanent** (mem.c:211-212).
- **Still.** Allocated from the C heap, never moved, reclaimed by mark-sweep. Four words precede the header: `link` (list of all still objects), `refcount` (count of references hidden in C), `length` and `mark` (−1 at GC start, then a scan-list link). Objects bigger than `___MSECTION_BIGGEST` = 255 words are always still (mem.h:37-38, 67). The FFI uses still objects for anything C must hold. `___still_obj_refcount_inc/dec` (mem.c:1443-1480) lets C keep an object alive and in place.
- **Movable.** One header word `[length | subtype:5 | head-tag:3]`. Collected by **stop-and-copy**. A forwarded object's head becomes a pointer tagged `___FORW` (mem.c:120-186).
- **Reference-counted root blocks.** `___alloc_rc` blocks are C-heap blocks with a `data` slot that is a GC root. They live on a doubly linked list scanned by `mark_rc` (mem.c:668-800, 3969-3987). This is Gambit's "persistent handle".

### 3.2 msections, stack and heap [V] (mem.c:393-488; mem.h:9-80)

- **Memory comes in `msection`s of 131072 words (1 MiB),** each split into from-space and to-space halves. A processor gets an msection for its **stack** (continuation frames, allocated by decrementing `fp`) and msections for **small objects** (`hp` incremented). One msection may serve both, with the stack growing down from its end.
- **Fudge areas.** A fudge area ends each allocation zone: `___MSECTION_FUDGE` = max frame slots + 2 = 8194 words. Generated code may overshoot `heap_limit` by up to the fudge before checking.
- **Heap sizing** targets `___DEFAULT_LIVE_PERCENT` = **50%** live after GC (mem.h:53-55, 70). `adjust_heap` keeps a history of target sizes (mem.c:4454-4523).
- **The GC is one non-generational STW collection.** Strong mark: VM objects, symbol tables, rc blocks, globals, the continuation, registers, saved slots and caches, then a transitive closure (mem.c:6813-6870). A weak phase processes wills, then `move_continuation`. Cleanup handles GC hash tables and frees unmarked still objects (mem.c:6872-6935).
- **Address-hashed tables.** After objects move, eq-tables with memory-allocated keys are flagged `___GCHASHTABLE_FLAG_NEED_REHASH` (gambit.h.in:6055-6060; `process_gc_hash_tables`, mem.c:5364ff). This is the standard fix for address-based `eq?` hashing under a moving collector.

### 3.3 Allocation checks and polls emitted by the compiler [V]

- **`___CHECK_HEAP(n,m)`** is `if (hp > ps->heap_limit) { temp1 = label n; jump handler_heap_limit; }` (gambit.h.in:8529-8546).
- **The C back end amortises checks.** `targ-heap-reserve-and-check` accumulates the words each basic block allocates. It emits a check only when the reservation exceeds `targ-msection-fudge` = **4096 words**, or at the end of the block (gsc/_t-c-2.scm:345-413, 1120-1141; _t-c-1.scm:116-117). Up to 4096 words of allocation thus share one check, relying on the fudge zone. OCaml's Comballoc does the same within a block.
- **Interrupts reuse the stack check.** `___POLL(n)` is `if (fp < ps->stack_trip) jump handler_stack_limit`. `___STACK_TRIP_ON()` sets `stack_trip = stack_start`, so the *next* poll fails (gambit.h.in:8483-8520). This is OCaml's `young_limit = MAX` trick applied to the stack pointer.
- **Poll placement is analysed.** The front end bounds the instructions executed between polls: `poll-period` (Lmax) = 40000, `poll-head` (E) = 200, `poll-tail` (R) = 200 (gsc/_front.scm:1307-1340). This is the scheme of Feeley's "Polling efficiently on stock hardware" (FPCA 1993).
- **Generational GC is not there yet.** The compiler has plumbing for a **sequential store buffer** (`targ-ssb-reserve`, `CHECK_SSB`, "TODO: remove when transition to combined checks", _t-c-2.scm:257, 365-404), but neither `mem.c` nor `gambit.h.in` contains SSB runtime support (grep finds nothing). [I] Gambit appears to be partway into adding a generational write barrier, using a bounded per-block SSB reservation checked together with heap and poll checks. That is a good shape for JIT barriers too: reserve barrier-log space per block and check once.

### 3.4 Frames and GC maps keyed by the return point [V]

- **The descriptor lives in the return point.** Every return point is a permanent `___sRETURN` object whose field 0 is a frame descriptor (mem.c:237-241). An inline descriptor packs `kind:2 | fs:5 | link:5 | gcmap` into one word (`___IFD`). Larger frames use an out-of-line array: header word, then `gcmap` words, one bit per slot (gambit.h.in:9193-9310).
- **Lookup is O(1),** from return address to label descriptor, with no hashing (`___LABEL_DESCR_GET`).
- **The `link` slot** is the frame slot holding the caller's return address, so frames form a linked chain through slot numbers rather than through a frame pointer.
- **Internal return points (RETI).** For runtime calls such as GC, the *callee* saves all GVM registers into a frame instead of each call site pushing its live ones (_kernel.scm:1285ff; `___RETI_*` macros). OCaml's `caml_call_gc` + `gc_regs` does the same.
- **All five GVM registers are marked unconditionally** at GC (`mark_registers`, mem.c:6511-6522). They must always hold valid tagged objects, possibly stale. That trades a little retention for simplicity.
- **Walking frames.** `mark_frame` walks the gcmap bit runs and calls `mark_array` on runs of live slots (mem.c:3817-3896). `mark_continuation` walks frames from `fp` to `stack_break` (mem.c:3898-3966).

### 3.5 Continuations: O(1) capture, lazy copy-on-return, GC migration to the heap [V]

**Capture** (`___CONTINUATION_CAPTURE`, gambit.h.in:6577-6593):
- If `R0` (the return address) is already the break handler, the current frame is the one the break frame points to. Otherwise save `R0` into the frame, set `R0 := handler_break`, and take `frame := fp`.
- **Allocate the 2-field continuation object (frame, dynamic env) on the stack itself**, then push a break frame pointing at `frame` and set `ps->stack_break = fp`.
- No frames are copied. Frames below the break are now shared and immutable.

**Return past the break.** The break handler (_kernel.scm:1077-1270) copies the caller's frame to the top of the stack, re-installs the break below it, and jumps to the caller's return address. The caller's frame may be in the stack or in the heap (tag `___tFRAME`). This is lazy, one-frame-at-a-time reinstatement: the "incremental stack/heap" strategy, related to Clinger, Hartheimer and Ost's comparison of continuation strategies and to the Hieb–Dybvig–Bruggeman segmented stacks used by Chez.

**Invocation** (`___CONTINUATION_GRAFT_NO_WINDING`, gambit.h.in:6641-6651): set `fp := stack_break`, point the break frame at the continuation's frame, restore the dynamic environment, and return through `handler_break`. This is also O(1).

**At GC.** `mark_captured_continuation` (mem.c:3667-3814) **copies each captured stack frame into the heap as a `___sFRAME` object.** It leaves a fixnum-tagged forwarding pointer in the stack frame's link slot and rewrites links so that heap frames point to heap frames. `move_continuation` (mem.c:6223-6254) then copies only the *topmost* contiguous frames (`fp` up to `stack_break`) into the fresh stack msection.
- After a GC, everything older than the latest break is ordinary heap data. The next GC therefore scans only frames pushed since the last break: generational stack scanning for free.
- **Deep recursion.** When the contiguous frames on a stack section take more than 2/3 of it, `___stack_limit` adds a break frame and triggers a GC, which moves those frames to the heap (mem.c:7180-7230). Otherwise it starts a new stack msection with a "first break frame". Stack depth is bounded only by the heap.

### 3.6 GC safety of the C back end [V/I]

- Gambit compiles to C with GVM registers, `fp` and `hp` held in C locals. GC is reachable only through `___JUMPEXTPRM` to `handler_heap_limit` or `handler_stack_limit`, after `___W_ALL` writes the locals back to `___ps` [V: macros cited above; `___heap_limit` recovers `ps->fp`/`ps->hp`, mem.c:7441-7446].
- At those points every live value is in a GVM register (marked), in a stack frame (described by its return point's gcmap) or in the heap. C code never holds a movable object across a GC point, because C primitives allocate still objects, which do not move [I from the still-object design and the FFI rules in mem.c:1485-1497].

---

## 4. What transfers to Patina (interpreter now, Cranelift JIT later)

### 4.1 Representation prerequisites [I]

- **Moving the young generation conflicts with three Patina facts:**
  1. raw `HeapIndex`/raw bits escape into Rust maps (GC_DESIGN.md §3.4);
  2. off-heap `Rc` structures hold `TaggedValue`s;
  3. deferral relies on "no collection while Rust holds values".

  Gambit's answer is to keep moving objects out of those places: escaped references must point to still or permanent objects. For Patina:
  - symbols and core-syntax markers become permanent or old (non-moving);
  - code-object constants and macro literals are **pretenured** (allocated old), like OCaml's static data and Gambit's permanent literals;
  - raw-bits-keyed diagnostics maps key only on old, non-moving objects, or on a stable id;
  - `eq?` hash tables either hash a stable per-object id or rehash after a minor GC moves keys (Gambit `NEED_REHASH`).
- **Object headers.** Move from `Vec<HeapObjectData>` (72-byte enum slots, `Rc` payloads) to headered objects with an OCaml-like `[size | colour | tag]` word in a pooled old generation. That gives inline allocation of closures and vectors (no per-object `Vec` malloc) and lets the JIT read lengths and tags at fixed offsets.
- **`Is_young` must be a range test.** Reserve one virtual range for the nursery (OCaml address_class.h:50-58) so the barrier and promotion test is two compares. With 32-bit compressed references, "young" can be an offset range inside the heap reservation.

### 4.2 Allocation fast path and safepoint word [I, modelled on §2.2/§2.4/§3.3]

- **VM state.** Keep `{alloc_ptr, alloc_limit}` in a `#[repr(C)]` VM-state struct. Cranelift cannot pin a global register the way OCaml pins `r15`/`r14`. Pass a `vmctx` pointer as an argument, keep `alloc_ptr` in a CLIF variable within a function, and write it back before every call or safepoint.
- **Batch allocations.** Merge allocations in a basic block into one bump and check (Comballoc, or Gambit's reservation). An optional fudge zone at the nursery end allows checking once per several allocations.
- **Slow path.** An out-of-line cold block calls a runtime `gc_alloc_slow(vmctx, words)`. That call is a Cranelift safepoint with a user stack map (or, under §4.3 option A, a VM-register-file spill). OCaml's `caml_garbage_collection` shows the slow path can also serve asynchronous work: "limit = MAX" means poll, not allocate.
- **Interpreter.** Replace the per-instruction `gc_pending` check with the same limit test at allocating instructions, calls and backward jumps. Interrupts and `(gc)` requests set the limit to max.

### 4.3 Root discovery for JIT frames: two options [I]

**A. VM register file as the GC-visible frame store** (recommended first).
- JIT'd Scheme procedures keep values in Cranelift SSA variables between safepoints. At each safepoint (calls, the allocation slow path, polls) they write live values back to their `registers[base..base+n]` window, and reload after.
- Roots then stay exactly as today: `registers` plus `frames`, filtered by `register_roots` at the CallFrame's pc (gc_roots.rs:47-68), where the JIT records a "resume pc" per safepoint.
- This is the Gambit model (`___W_ALL`, write GVM registers back to `___ps` before entering the runtime) and the OCaml bytecode model (`Setup_for_gc`). It also keeps **continuation capture unchanged**: no native frames ever need copying.
- Cost: stores and loads at safepoints, which Cranelift's alias analysis partly removes (fitzgen notes the mid-end eliminates redundant reloads).
- One hazard: if `registers` is a `Vec` that may reallocate, the JIT must reload its base pointer after every safepoint. A segmented stack (§4.5) avoids reallocation.

**B. Native stack with Cranelift user stack maps.**
- Declare GC values with `declare_value_needs_stack_map`, collect the `UserStackMap` per safepoint, and build a PC-sorted table per code object (Wasmtime-style binary search, or an OCaml-style hash on return address). Walk frames by frame pointer, register and unregister tables on code load and unload (OCaml's zombie protocol), and mark JIT-to-runtime transitions as "return-to-C" boundaries like OCaml's `0xFFFF` descriptors.
- This gives faster frames but makes **first-class continuations** hard. Capture would have to copy native frames, which requires knowing every frame's layout and having no interior pointers (OCaml fibers manage this with a dedicated stack per fiber). Reinstatement would need underflow handling on the native stack.
- Recommendation: defer B until profiling shows the register-file traffic matters, and limit it to leaf or non-capturing code. Escape analysis could prove "no call/cc reachable", but Scheme makes that rare.

### 4.4 Write barrier for the interpreter and JIT [I, from §2.3]

- **Combined barrier** on `set-car!`/`set-cdr!`/`vector-set!`, cell writes, closure-slot writes, record-field and global-binding writes: `if is_old(obj) { if marking && is_ptr(old) && !is_young(old) {darken(old)}; if is_young(new) {log(&slot)} }`.
- **In JIT code,** inline the `is_old(obj)` test (often known statically: a freshly allocated object is young, so no barrier) and call a **non-safepoint** slow path. In Cranelift, a call that is known never to GC needs no stack map. Mark it so the frontend does not spill around it, matching OCaml's `alloc=false` external call.
- **Remembered set:** a growable slot-address buffer (OCaml `Ref_table`) or an SSB with per-block reservation (Gambit's in-progress design). Duplicates are fine.
- **Globals.** If globals stay in Rust `Environment` maps, they need either a barrier into a "young globals" list (OCaml generational global roots) or a full scan of all globals at each minor GC. Moving globals into heap cells, as OCaml and Gambit do, makes `StoreGlobal` an ordinary barriered store and makes the JIT's global access a load from a fixed cell. That is also what a JIT wants for inline caching.

### 4.5 Continuations and stacks [I, from §2.9 and §3.5]

- **Replace "clone all frames and registers"** (types/continuation.rs:159-196) with **segmented register stacks and Gambit-style O(1) capture**:
  - capture seals the current segment (a break marker) and returns an object referencing it;
  - frames above the break stay mutable;
  - returning across a break copies one frame (or a segment) back to the active segment, Gambit's break handler;
  - invocation sets the active stack to the break and points it at the continuation's frames.

  This makes `ctak`-style programs O(1) per capture instead of O(depth). It should also remove the need for weak continuation side tables: the frames become heap objects the GC already traces.
- **GC interaction without a stack barrier:**
  - Sealed segments are immutable.
  - Allocate the capture object in the nursery. When it is promoted, scan its segment (OCaml `Cont_tag` promotion, minor_gc.c:270-288). Or follow Gambit and have the GC migrate captured frames into the old heap as frame objects, so later GCs only scan frames pushed since the last break.
  - On reinstatement during incremental marking, darken the segment first (`caml_continuation_use_noexc`).
  - Only the active segment is a mutable root, scanned at every GC (minor and major).
- **Frame layout must be self-describing from the frame alone:** code object plus resume pc gives the size and liveness bitmap (Gambit `fs`/`link`/`gcmap`, OCaml descriptor). Patina's `CallFrame` already has `code: Rc<CodeObject>` and `pc`. For a heap-resident frame, the code reference must itself be traceable (today it is an `Rc`).
- **Delimited continuations and `abort`** map naturally to segments: a prompt is a segment boundary. One-shot uses (generators, green threads) can use OCaml-fiber-style dedicated segments.

### 4.6 Rust-side rooting and the deferral rule [I]

- A moving nursery invalidates the "Rust locals may hold values while deferred" assumption only if a collection happens. Keep the deferral rule but give it OCaml's escape hatch: **while deferred, nursery exhaustion switches allocation to the old generation (`caml_alloc_shr`) and initialising stores use `caml_initialize`-style logging**. Nested loops then never move objects under Rust's feet, and the nursery cannot overflow.
- For code that must hold values across a *collecting* call, add a shadow-stack root scope (the `CAMLparam` analogue). Gambit's rc handles cover long-lived Rust structures holding Scheme values (`SourceMap`, `CompiledMacro`). Either form is updatable by a moving collector.

### 4.7 Pacing, sizing and observability [I]

- Pace the old generation in bytes promoted, with a `space_overhead` knob (OCaml default 120) or Gambit's simpler "resize so live = 50%".
- Account for off-heap memory: `Vec` payloads, strings and bignums, the `caml_alloc_dependent_memory` analogue.
- Watch for ramp-up overwork, which needed OCaml 5.4's `Gc.ramp_up`.
- Start with a STW old-generation collector. Incremental slices need the SATB half of the barrier and the "darken on reinstatement" rule, both cheap to keep in the design from day one.

---

## 5. Constraints and lessons for the redesign (summary list)

1. GC only at explicit safepoints: allocation slow path, polls, calls into the runtime. Never asynchronous. (Both systems.)
2. One limit word that is both "nursery full" and "please stop" (OCaml `young_limit`, Gambit `stack_trip`). Interrupts store MAX into it.
3. Poll placement for tail-call languages: every loop and every potentially recursive tail call path contains an allocation or a poll (OCaml polling.ml PRTC rule), or use a bounded instruction count between polls (Gambit Lmax 40000).
4. Stack maps keyed by resume/return pc. At call safepoints live values sit in memory slots (OCaml: calls destroy all registers; Cranelift: user stack maps spill), so maps name slots only.
5. Combined barrier (remember old-to-young slot + SATB darken while marking) as a non-allocating, non-safepoint slow call. No barrier on initialising stores into fresh young objects.
6. `Is_young` is an address- or offset-range test, which requires a contiguous nursery reservation.
7. Promote directly to a non-moving size-class old generation. Lazy sweep amortised into allocation. Colour rotation or bitmap flipping instead of clearing marks.
8. Continuation segments are immutable once captured. Handle their young pointers by promoting the capture object (OCaml) or migrating frames to the heap at GC (Gambit). Darken on reinstatement under incremental marking. No per-store stack barrier.
9. O(1) capture with a lazy underflow handler (Gambit) instead of O(depth) cloning.
10. Escaped raw references must point to non-moving objects: permanent and still in Gambit, pretenured or old in Patina. Address-hashed tables need rehash or stable ids.
11. A deferred, no-collect region allocates in the old generation with initialising-store logging (OCaml `alloc_shr` + `caml_initialize`).
12. JIT code tables register and unregister with deferred reclamation (OCaml zombie frametables).
13. Explicit compaction can come later (OCaml 5.2), if every reference is collector-visible.

---

## 6. Open questions

1. **Option A or B for JIT roots?** Option A spills to the VM register file; Option B uses native-stack Cranelift stack maps. How much do spills and reloads cost in a Patina JIT prototype? Does Patina's `call/cc` and prompt usage rule out native frames outright? (Patina's suites use `call/cc` heavily: control_flow_matrix.rs, `dynamic-wind`.)
2. **Reference width.** Should compressed 32-bit references (offsets in one reservation, like Wasmtime and V8) replace `u32` arena indices? That keeps `TaggedValue` at 8 bytes with room for NaN-boxing flonums, which would remove the 72-byte boxed `Real`.
3. **Off-heap holders.** For `Environment` maps, `CodeObject.constants`, `CompiledMacro` literals and `SourceMap` raw-bits keys: which become heap objects, which become pretenured or old, and which use generational-root registration?
4. **The tree-walker's roots.** The tree-walker's `CpsContinuation` (Rc graph) has no frame maps. Does it get a separate rooting discipline (handles), or is it retired from the moving design, with the nursery used only when the VM is outermost?
5. **Old-generation marking.** Is incremental marking worth it for Patina's workloads, which are single-threaded and mostly short scripts plus benchmarks? Or is a STW old generation with a 2-8 MiB nursery enough? OCaml's measured pause improvements (1125 to 689 ms max on menhir) come mainly from incrementality plus generations.
6. **Segment granularity.** Should continuation segments use Gambit's one-frame-per-underflow copy or Chez-style fixed-size segment splitting? This interacts with `dynamic-wind` re-entry (VM_RUNTIME.md §5.6) and the existing `reentry` boundary model.
7. **Weak and ephemeron semantics** across generations (the OCaml `ephe_ref` analogue), and SRFI 124 `ephemeron-broken?` timing when keys die young.
8. **Finalisation of off-heap resources** (ports, `Rc` payloads now dropped by sweep tombstoning) in a nursery that frees objects without visiting them. Such objects need a "custom table" (OCaml `caml_custom_table`, caml/minor_gc.h:58-62) or must be allocated old.

---

### Primary sources consulted (beyond local code)

- Sivaramakrishnan et al., "Retrofitting Parallelism onto OCaml", ICFP 2020: https://arxiv.org/abs/2004.11663 (HTML: https://ar5iv.labs.arxiv.org/html/2004.11663)
- Sivaramakrishnan et al., "Retrofitting Effect Handlers onto OCaml", PLDI 2021: https://arxiv.org/abs/2104.00250 (HTML: https://ar5iv.labs.arxiv.org/html/2104.00250)
- N. Fitzgerald, "New Stack Maps for Wasmtime and Cranelift", 2024-09-10: https://fitzgen.com/2024/09/10/new-stack-maps-for-wasmtime.html
- Cranelift `ir/user_stack_maps.rs`: https://docs.wasmtime.dev/api/src/cranelift_codegen/ir/user_stack_maps.rs.html
- `cranelift_frontend::FunctionBuilder` (`declare_value_needs_stack_map`): https://docs.rs/cranelift-frontend/*/cranelift_frontend/struct.FunctionBuilder.html
- Wasmtime `wasmtime_environ/stack_map.rs`: https://docs.wasmtime.dev/api/src/wasmtime_environ/stack_map.rs.html
- M. Feeley, "Polling efficiently on stock hardware", FPCA 1993 (the parameters cited are those in gsc/_front.scm:1307-1340).
