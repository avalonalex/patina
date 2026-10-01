# Continuation representation under a moving GC and a Cranelift JIT

Scope: `crates/patina-vm` at `28a94f8`, plus the tree-walker's continuation types, mapped onto three
GC-visible representations. This is a read-only study. New measurements:
- Scheme probes run with a release `patina` built from `28a94f8` into a separate target directory
  (`PRD/study/gc/probes/continuation-representation/probes/*.scm`).
- A toy Rust prototype of today's shape against designs A and C′
  (`PRD/study/gc/probes/continuation-representation/src/main.rs`; its `results.txt` was not retained).

Both ran on the development Mac (arm64, macOS 27.2). They are single runs (the prototype is best of 5), so read
them as orders of magnitude, not A/B results. Tags: **[V]** means verified in source or by measurement; **[I]**
means inference.

---

## 0. Summary

1. **[V] The cost of today's capture scales with the *whole* stack, and the trigger cannot see it.** The
   known figure of about 4 µs and 17 KB per capture holds only at 100 frames. Measured at depth 1000:
   - **24 µs and about 171 KB per capture**, with 20 k captures reaching **3.4 GB RSS**;
   - **5.7 GB at 80 k captures**, so the "590 MB plateau" is a property of depth 100, not a bound;
   - **27 µs per coroutine switch**;
   - `ctak 18 12 6` takes 0.06 s and 98 MB on its own, and **1.04 s and 4.0 GB when run 500 frames
     deep**.

   (§2.)
2. **[V] Patina's control machinery is already representation-friendly.** Prompts, handlers and winds
   address frames by *depth integers*, never by pointers (`types/continuation.rs:15-48,147-153`). "Work owed
   after a call" is always a stub frame (`control.rs:91-104`). Relocation is integer arithmetic
   (`execution_state.rs:319-424`). What blocks a moving GC is the *container*:
   - `Rc` payloads in weak `FxHashMap` side tables (`vm_state.rs:196-199`);
   - `CallFrame.code: Rc<CodeObject>` (`types/mod.rs:59`);
   - `closure: Option<HeapIndex>` (`:50`);
   - `Rc<[u64]>`/`Rc<[ExceptionHandler]>` snapshots;
   - code liveness by counting (`vm_state.rs:542-575, 1003-1008`).
3. **[V/I] Design A (today's snapshot as an ordinary traced, byte-accounted heap object) is low-risk.** It is
   needed in any case: it removes the weak tables, makes captures visible to the trigger and makes them
   movable, and it changes no control semantics. In the toy it cuts per-capture overhead about 5.8× at
   d=100 (755 → 130 ns over a 430 ns no-capture baseline). It remains **O(stack depth)** per capture.
4. **[V, toy] Design C′ makes capture and invoke independent of depth.** C′ is Loom-style freeze into
   immutable chunks, combined with a *frozen watermark* so that only frames resumed since the last freeze
   are copied (Larceny's stack-cache flush discipline), and lazy thaw on underflow (Chez/Gambit). In the toy:
   - same-depth capture 57 ns vs 1.3 µs (A) and 5.0 µs (today) at d=1000;
   - escape 101 ns vs 4.7/8.9 µs;
   - ping-pong 65 ns vs 7.8/7.5 µs;
   - retained memory 150-250 B per capture vs 64-80 KB.

   Its weak spot is re-entering a deep continuation and returning through all of it: 1.6× (thaw 8
   frames) to 4.4× (thaw 1) slower than A's bulk copy. **Design B** (Chez/Gambit seal-in-place segments) has
   the same asymptotics but makes the live stack's lower part a GC object, which fits a non-relocating
   JIT register stack worse.
5. **[I] Recommendation, in stages.**
   - (0) Route every `frames().len()` (37 sites) through one depth accessor, and add the depth probes.
   - (1) Design A plus a byte-accounted trigger, which deletes the VM weak tables.
   - (2) Code liveness by tracing: non-moving code descriptors marked from frames, closures, chunks and
     parent code. This deletes `live_closures` and `gc_freed_closure_code_ids`, a prerequisite for any
     copying nursery.
   - (3) A non-relocating live register stack with `Copy` frames.
   - (4) C′, with winds, handlers, prompts and re-entry ids as persistent heap lists, and one Return-path
     watermark shared with generational stack scanning.
   - (5) The baseline JIT on the same frames.
6. **[I] The tree-walker disagreement dissolves once two questions are kept apart.** The weak-id table is a
   *host-payload* mechanism (finalising Rust-owned data). VM continuations should leave it because their
   payload is plain data that belongs in the heap. Tree-walker `CpsContinuation`/`Procedure` payloads own
   `Rc<Environment>`/`Rc<CpsExpr>` and should enter it. Both may stay off-heap only under a non-moving or
   pinning policy for the tree-walker heap.

---

## 1. Inventory of VM control state

Legend: **H** = holds heap references (TaggedValue or HeapIndex); **C** = holds a code reference;
**R** = Rust-owned shared allocation (`Rc`). "Depth" fields are integers into frame, wind or handler
stacks.

### 1.1 Live machine (`ExecutionState`, `execution_state.rs:17-23`, plus `VmState` control fields)

| Component | Element type (size) | Refs | Notes |
|---|---|---|---|
| `registers: Vec<TV>` | 8 B | **H** | flat file; frames own windows `[register_base, +num_regs)`; `resize` may reallocate on any call (`:63,106,158`) |
| `frames: Vec<CallFrame>` | 40 B (`types/mod.rs:41-60`) | **H** `closure: Option<HeapIndex>` (bare u32, special trace path `gc_roots.rs:154-161`); **C/R** `code: Rc<CodeObject>` | `pc`, `register_base` (absolute index), `num_regs`, `return_reg` (slot in *caller's* window) |
| `prompt_stack: Vec<PromptFrame>` | 48 B | **H** `tag`, `handler` | `dst` (Reg in establishing frame), `stack_depth`, `dynamic_wind_depth`, `exception_handler_depth` (`types/continuation.rs:15-48`) |
| `dynamic_winds: Vec<WindRecord<ExceptionHandler>>` | 40 B (`patina-core/src/continuation.rs:173-198`) | **H** `before`, `after`; **R** `handlers: Rc<[ExceptionHandler]>` (each holds **H** `handler` and a `stack_depth`) | `id: u64` from a global counter, compared positionally by `next_wind_step` (`:232-245`) |
| `exception_handlers: Vec<ExceptionHandler>` | 16 B | **H** `handler` | `stack_depth` = `frames.len()` at install (`execution_state.rs:184-191`) |
| `VmState.pending_escape: Option<TV>` | — | **H** | rooted (`gc_roots.rs:85-87`) |
| `pending_transfer`, `reentry: Vec<u64>`, `next_reentry`, `reentry_kept` | — | — | re-entry boundary protocol (`vm_state.rs:83-111`, `control.rs:2169-2227`) |
| `scratch_args`, tracer snapshots | — | **H** | roots (`gc_roots.rs:76,112-114`) |

**[V] Stub frames are ordinary frames whose registers carry the transfer.** Their code objects have
`register_roots: None` (`control.rs:1418-1447`), so all of their slots are conservatively live. Heap
references inside stub windows:
- `wind_step` TARGET (a full-continuation ref), VALUE (`control.rs:1151-1164`);
- `invoke_step` CONT (a delimited ref), VALUE, and DST/INDEX as fixnums (`:2701-2718`);
- `abort_step` HANDLER, VAL, CONT (`:2777-2789`);
- `raise_step` HANDLER, EXCEPTION, and DEPTH_BELOW as a **distance**, not an index (`:2639-2662`, `:952-959`);
- the value-form `dynamic-wind`/`call-with-values` thunks, `force`'s promise, and `resume_stub`'s primitive
  state.

**[V] Orphan windows.** `finish_wind_step` pops a wind stub's *frame* but deliberately leaves its 4-register
*window* on the file (`execution_state.rs:84-91`; rationale at `vm_state.rs:1863-1874`: the window is the only
root for a weak-store TARGET). The register file is therefore not always the exact union of frame windows
during travel, and any segmented or chunked design must decide what to do with these windows (§3).

### 1.2 What each continuation kind captures and restores

| Field | Full `VmContinuation` (`types/continuation.rs:159-196`) | Delimited `VmDelimitedContinuation` (`:66-138`) | Abort landing / `exit` target |
|---|---|---|---|
| frames | all, `frames.clone()` (one `Rc` increment each); **H C R** | `frames[depth_at_capture..]` (`control.rs:2858`) | `frames[..landing_depth]` + `abort_step` stub frame (`:2602-2614`); `exit` target: empty (`:676-690`) |
| registers | **entire file** including orphan windows, `retire_registers` at alloc (`vm_state.rs:636-643`); **H** | `registers[base_at_capture..]`, hole cleared in the copy (`control.rs:2895-2903`), retire relative to `base_at_capture` (`vm_state.rs:646-655`) | `registers[..registers_end]` + stub window holding HANDLER, VAL, CONT (**H**) |
| winds | all (**H**, **R** handler snapshots) | `winds[wind_depth_at_capture..]`, or none if the region is empty | `winds[..wind_depth]` |
| prompts | all (**H**) | `prompts[prompt_idx+1..]` (inner prompts, #163) | `prompts[..prompt_idx]` |
| handlers | all (**H**) | `handlers[handler_depth_at_capture..]` | `handlers[..handler_depth]` |
| delivery | `deliver_reg: u16` = the call/cc `dst`, cleared in the live frame first (`control.rs:641-654`, 296 MB story) | `deliver_reg: Option<u16>` = hole; `None` means the identity continuation | `deliver_reg = abort_step::RESULT` (never read) |
| depth bookkeeping | none: arrival replaces everything (`execution_state.rs:255-261`); `pop_resolved_prompts` on arrival (#176, `control.rs:1278-1297`) | `base_at_capture`, `depth_at_capture`, `wind_depth_at_capture`, `handler_depth_at_capture`, all used by `append_delimited` via `relocate_depth` (saturating, `execution_state.rs:422-424`) | `abort_landing: true`, read at every `ResumeWindJump` step to `park_transfer` (`vm_state.rs:1904-1923`) |
| re-entry | `reentry: Rc<[u64]>` boundary ids (`captured_reentry`, `control.rs:1650-1655`), consumed by `step_wind_jump`'s `reentry_kept` (`:1250-1276`) | none (composable invoke extends; it is not an arrival) | landing keeps every boundary (`:2622-2625`) |
| `exit_status` | `None` | — | `Some(status)` ends the process on arrival (`:1247-1249`) |
| store | `continuation_store: RefCell<FxHashMap<u64, Rc<VmContinuation>>>` — **weak** | `delimited_continuation_store` — weak | same store as full |
| heap handle | `VmContinuationRef(u64)` (72 B enum slot, `heap/mod.rs:215-219`) | `VmDelimitedContinuationRef(u64)` | same |

**[V] Invocation paths.**
- *Full jump.* `step_wind_jump` (`control.rs:1201-1308`) computes one `WindStep`.
  - Exit: pop the record, then push a stub and call `after` under the record's handlers (`push_wind_step`,
    `:1345-1366`; `install_thunk_handlers` clamps depths, `execution_state.rs:227-235`).
  - Enter: run `before`; the record is pushed by `ResumeWindJump` (`vm_state.rs:1876-1898`).
  - Arrive: `restore` clones all five vectors back.
- *Composable invoke.* `invoke_delimited` (`control.rs:2966-2990`) re-enters the captured winds one at a time
  through `invoke_step` stubs, under the **invoke site's** handlers. It then runs `append_delimited`, which
  extends registers and frames and shifts `register_base`, prompt depths (three each) and handler depths. It
  re-points the outermost frame's `return_reg` to `dst` and writes the value into the hole
  (`execution_state.rs:319-409`).
- *Abort.* `abort_to_prompt` (`control.rs:2541-2630`) always captures the delimited continuation first.
  - No wind extent to leave: `truncate_to_prompt` plus `push_abort_stub` in place (`:2591-2597`). This fast
    path was measured about 20% faster on 300-frame aborts.
  - Otherwise: build the landing as a `VmContinuation` and travel to it.

### 1.3 Where heap and code references hide today, and how each is kept alive

| Reference | Today's keep-alive | Problem for moving / generational / JIT |
|---|---|---|
| live registers, prompt/wind/handler TVs | strong roots (`gc_roots.rs:71-115`) | none for moving (slots are updatable); `Vec` reallocation is a JIT problem |
| `CallFrame.closure: HeapIndex` | `visit_object_index` | a bare index, not updatable through the TV visitor |
| `CallFrame.code: Rc<CodeObject>` | `Rc` count keeps the unit in `code_store` (`unit_in_use`, `vm_state.rs:1003-1008`); constants of **every loaded code** are traced strongly (`gc_roots.rs:97-99`) | refcount traffic per call (4 `Rc` operations per call/return pair, vm-runtime.md §2.1); a dead continuation's code stays loaded until the weak prune, plus one collection |
| snapshot TVs (registers, frames, records) | weak-id fixpoint `trace_weak_ids` / `sweep_weak` (`gc_roots.rs:117-152`, `heap/gc.rs:1013-1093`), nested with ephemerons (`ephemerons.rs:183-213`) | immutable `Rc` payloads cannot be updated by a mover; payload bytes are invisible to the trigger; soundness rule "every store touch confined to one dispatch, nested loops defer" (`gc_roots.rs:21-24`) |
| `WindRecord.handlers: Rc<[ExceptionHandler]>` | traced through `visit_winds_with` | a fresh `Rc` copy of the **whole handler stack per `dynamic-wind` call** (`captured_handlers`, `control.rs:2107-2112`); immutable |
| `reentry: Rc<[u64]>` | no heap refs | `Rc` in what should be plain data |
| closures' code | `live_closures` counter: incremented by `MakeClosure`, decremented from `gc_freed_closure_code_ids` reported by sweep (`vm_state.rs:542-575`) | requires sweep to visit **dead** objects, which a copying nursery never does |

---

## 2. Measurements

### 2.1 Scheme probes (release, built from `28a94f8`; files in `probes/`) [V]

| Probe | Shape | Time | Max RSS | Per op |
|---|---|---|---|---|
| `base` | 20 k × 100-deep recursion, no capture | 0.08 s | 11 MB | — |
| `dead100` | same + `call/cc` at the bottom, never invoked | 0.14 s | 345 MB | ~3 µs, ~17 KB retained |
| `sameloop1000` / `samedepth1000` | loop at depth 1000, 20 k iterations; the second captures each iteration | 0.01 / 0.49 s | 14 MB / **3,433 MB** | **24 µs, ~171 KB** per capture; only 1 collection ran |
| `samedepth1000`, 5 k / 80 k iterations | | 0.12 / 1.60 s | 875 MB / **5,720 MB** | the plateau scales with depth × allocation-count threshold |
| `samedepth10` | the same at depth 10 | 0.02 s | 60 MB | ~3 KB per capture |
| `escape1000` | at depth 1000: capture, descend 10 frames, escape | 0.65 s | 3,517 MB | **32 µs** per capture plus escape |
| `pingpong1000` | two `call/cc` coroutines over a 1000-deep base, 40 k switches | 1.08 s | 3,797 MB | **27 µs** per switch |
| `abort100` | prompt, 100 frames, abort (fast path) | 0.13 s | 235 MB | ~2.5 µs, ~12 KB per delimited capture |
| `resume100` | the same + resume `k` | 0.15 s | 235 MB | ~3.5 µs |
| `ctak` / `ctakdeep` | `(ctak 18 12 6)`, alone / under 500 frames | 0.06 / 1.04 s | 98 MB / **4,033 MB** | 17× slower, 41× the RSS, for the same captures |

The cost is proportional to the depth of the *entire* stack, not of the region a program cares about. A
library body, REPL, test harness or green-thread scheduler that sits a few hundred frames deep multiplies
every capture.

### 2.2 Toy prototype (`src/main.rs`; best of 5; ns/op) [V for the toy, I for transfer to Patina]

Each machine pushes 4-7-register frames. The prototype checks that all machines produce identical results.
- **T** = today's shape: `Rc` code, `Vec` clones, a retire pass, an `Rc<payload>` in an `FxHashMap`, and
  clone-back on invoke.
- **A** = one flat `Box<[u64]>`: a 3-word header, 24 B `Copy` frames and registers, memcpy in and out.
- **C′** = live stack plus frozen watermark plus immutable chunk chain plus lazy thaw (thaw k frames per
  underflow).

| Workload | d | T | A | C′ | Notes |
|---|---|---|---|---|---|
| dead capture (recurse d, capture, return) | 100 | 1216 | 567 | 607 | no-capture baseline 430 |
| | 1000 | 11963 | 5635 | 5803 | baseline 4256; C′ copies d new frames, as A does |
| same-depth loop capture | 10 | 112 | 28 | 57 | |
| | 1000 | 4974 | 1345 | **57** | |
| escape (capture, push 10, invoke) | 10 | 274 | 67 | 101 | |
| | 1000 | 8877 | 4714 | **101** | |
| ping-pong coroutines | 10 | 197 | 43 | 66 | |
| | 1000 | 7536 | 7828 | **65** | |
| reinstate d-deep, return through all | 100 | 352 | 132 | 831 / 281 (thaw 1 / 8) | lazy thaw's worst case |
| | 1000 | 3934 | 1796 | 7897 / 2830 | |
| retained bytes per kept capture | 100 | 8208 | 6452 | 184 | |
| | 1000 | 80244 | 64081 | 242 | |

The toy's T capture overhead at d=100 is about 0.8 µs against Patina's measured ~3-4 µs. The difference is the
real VM's larger windows, the heap-ref allocation, the `call/cc` dispatch, and retention without collection
(cache misses). Ratios between designs are the transferable part.

---

## 3. The three designs against the inventory

### 3.1 Definitions

- **A — traced snapshot object.** Same algorithms as today, but a continuation is one variable-size heap
  object: a header (kind, byte size, flags `abort_landing`/`exit`, `deliver_reg`, depth bookkeeping), inline
  `Copy` frames, the register slice, and wind, prompt and handler arrays (or pointers to persistent lists).
  It is traced like a vector, counted in bytes, and updatable by a mover. `VmContinuationRef(u64)` and both
  side tables are deleted. Frames carry `code` as a non-moving code-descriptor index or pointer and `closure`
  as a TV.
- **B — segmented register stack, O(1) capture (Chez/Gambit).**
  - The live stack is a chain of fixed segments.
  - `call/cc` *seals* the used part of the current segment in place: the continuation points at
    `[base, top)` plus a link. Chez `reify-cc-help`, `ChezScheme/s/cpnanopass.ss:5147-5222`, sets
    stack/clength/link/winders and installs `dounderflow` as the return address.
  - The live stack continues above the seal.
  - Returning across a seal *underflows*: it copies a bounded piece back. Chez `S_split_and_resize`,
    `c/schsig.c:93-141`, splits at frame boundaries so that about `underflow_limit` (128 B) plus one frame is
    copied.
  - Overflow seals everything below into a continuation (`S_overflow`, `:197-300`).
  - Gambit does the same with a break frame (`___CONTINUATION_CAPTURE`, `gambit.h.in:6577-6593`; break
    handler, `lib/_kernel.scm:1077-1180`) and migrates captured frames into heap `___sFRAME` objects at GC.
- **C — Loom chunk freeze and thaw.**
  - *Freeze* copies frames from the live stack into a `StackChunk` heap object.
  - *Thaw* copies them back lazily through a return barrier: "Below this heuristic [500 words] we thaw the
    whole chunk, above it we thaw just one frame" (`continuationFreezeThaw.cpp`, openjdk/jdk master).
  - The fast path requires `!chunk->requires_barriers()`. Old chunks get a GC-mode bitmap. Native frames
    pin.
  - Loom continuations are one-shot. Thawed frames leave the chunk.
- **C′ — the multi-shot form for Patina (recommended end state).**
  - Freeze **copies** (the live stack keeps running) only frames above a *frozen watermark* `wm`: the
    lowest frame index resumed since the last freeze, updated by one compare on `Return`.
  - The new chunk links to the previous frozen chain, truncated to `wm`, so it is shared with no copy.
  - Chunks are immutable. Invoke grafts the chain in O(1) and thaws a few frames, and the rest thaw on
    underflow.
  - This is Larceny's discipline: `stk_flush` turns only the stack cache into heap frames, and
    `stk_restore_frame` copies one frame back (`larceny/src/Rts/Sys/stack.c:129-254`). It is also Loom's
    implicit "re-freeze only what was thawed", without consuming the chunk.

### 3.2 Criterion-by-criterion

| Criterion | A | B (seal in place) | C′ (freeze above watermark) |
|---|---|---|---|
| Capture cost | O(total depth + winds + prompts + handlers); ~5.8× cheaper constant than today in the toy | O(1) for frames; O(new) if winds etc. stay `Vec`s | O(frames resumed since last freeze), O(1) in loops; same caveat for dynamic stacks |
| Invoke cost | O(total depth) bulk copy | O(1) graft + bounded underflow per later return | same as B |
| Return-path cost | none | an underflow at each seal crossing (free check via an underflow-stub frame) | one compare-and-store (`wm`), plus underflow only below thawed frames |
| Delimited capture | as today: copy `[depth_at_capture..]` | O(1) if a prompt starts a segment (Racket CS: "every prompt is on a boundary between metacontinuations", `racket/src/cs/rumble/control.ss:1-5`); otherwise split | freeze + view with a `floor` depth: O(new) |
| `append_delimited` relocation | unchanged: memcpy + shift `register_base` and three depths | thaw rebases per frame; carried prompts/handlers still relocated at invoke | same as B; eager append first (§6) |
| Wind travel through stub frames | unchanged | unchanged (stubs are frames in the live segment) | unchanged; travel stubs are frozen like any frame |
| Abort fast path (`truncate_to_prompt`) | unchanged, O(1) once frames hold no `Rc` | O(1) drop of segments above a prompt boundary | truncate live, or if the prompt lies in the thawed-out chain, re-point `below` to the chain truncated at the prompt depth (O(chunks)) |
| Abort landing (winds to leave) | copy `[..landing_depth]` as today | landing = view of sealed chain + 1-frame stub | freeze + truncated view + 1-frame stub chunk: O(new) |
| Re-entry ids | inline `u64` array in the object | inline array or persistent list | inline in the continuation object (not per chunk) |
| `deliver_reg`, hole clearing | unchanged; hole cleared in the copy | **must clear in place** before sealing (safe: the hole is dead in the live frame as well) | clear in the live frame before freezing; the capturing frame is always above `wm`, so it is always re-copied |
| Per-pc `register_roots` | retire at capture as today, or map-guided scan at GC | map-guided frame walk at GC (Chez `trace-stack` livemask, `s/mkgc.ss:1037-1095`) | retire only newly frozen frames, or at GC; GC rewriting dead slots to UNSPECIFIED is idempotent for shared chunks (same pc, same map) |
| Moving GC must update | object slots (registers, closures, record TVs); the object itself moves freely | segment contents, links, the segment objects; live segment pinned while code holds `base` | chunk slots, chain links, view objects, the live-stack root array, `frozen`/`below` views; no interior pointers anywhere (frames use `(code, pc)`, not return addresses) |
| Generational | object allocated young; immutable afterwards, so no barrier | sealed segments immutable; the live segment is a root | chunks immutable; a large chunk allocated old must be remembered at creation (Loom `requires_barriers`; OCaml scans a `Cont_tag` stack at promotion, `runtime/minor_gc.c:270-288`) |
| JIT: non-relocating register stack | orthogonal (needs Stage 3) | yes (fixed segments), but sealing moves the live base upward | yes: one fixed reserved buffer; overflow = freeze all but the top k frames and slide them down (Gambit `___stack_limit`, Chez `S_overflow`), so unbounded recursion survives |
| JIT: resume-at-pc entries | required (restore lands mid-function) | required | required |
| JIT: frames as plain data with GC-visible code ref | required (frames are copied into objects) | required (segments scanned by the GC) | required (chunks scanned by the GC) |
| Weak side table | gone | gone | gone |
| Trigger visibility | full byte size | segment bytes | chunk bytes, which are small |
| Risk | low: same control code, new container | high: in-place sealing, depth virtualisation, underflow on every crossing | medium-high: depth virtualisation, watermark invariant, lazy thaw |

**[I] B vs C′.** Their asymptotics are equal. B pays the copy on return and C′ pays it at capture. C′ fits
Patina better for three reasons:
- **The live stack stays simple.** It is one VM-owned, non-GC, non-relocating buffer and one root array,
  which is the JIT contract of jit-readiness.md F.24. B makes the lower part of the live buffer an object
  owned by the continuation, so either the stack buffer must be GC-managed (Chez's stack segments are
  objects copied by `copy_stack`, `c/gc.c:803-856`) or sealed regions must be copied out at GC.
- **Chunks are exact-size immutable objects.** That rules out the clength/length split, one-shot promotion
  (`S_promote_to_multishot`) and stack-cache invalidation.
- **Normal execution after a capture runs on the live stack.** B forces every return past the seal through
  underflow.

### 3.3 The control-flow matrix, row family by row family

`control_flow_matrix.rs` has **64 rows**: 2 extent forms (head `PushWind`/`PopWind` vs value-form
`value_wind_stub`) × 2 positions × 16 transfers (`:116-121`). `AGENTS.md`'s "24 transfer shapes" is stale.
Every row depends on four representation properties:
- **P1:** stub frames are ordinary captured frames.
- **P2:** captures are immutable and re-invocable (multi-shot).
- **P3:** winds, prompts and handlers are captured as of capture time, with depths re-interpretable in the
  target stack.
- **P4:** value delivery into `deliver_reg` or the hole, and re-pointing of the outermost `return_reg`.

A preserves all four trivially: same algorithm, different container. B and C′ preserve P1 trivially, P2 by
chunk immutability with copy-on-thaw, P3 only with **virtual depths** (`below_depth + live frames`) used
consistently in place of `frames.len()` (37 call sites: control.rs 21, execution_state.rs 8, vm_state.rs 8),
and P4 by an underflow stub that carries `deliver`/`dst`.

| Transfer (×4 rows each) | Mechanisms exercised | A | B / C′ specifics |
|---|---|---|---|
| none | `PushWind`/`PopWind` or the value stub, normal `Return` | ✓ | C′: watermark update only |
| escape | full capture outside, `Exit` step via `wind_jump_stub`, arrival | ✓ | arrival = drop live stack + graft + thaw; travel stubs discarded in O(1) |
| reenter | capture in body; `Enter` step runs `before`, `ResumeWindJump` pushes the record | ✓ | the target's winds must be a captured value: a persistent list pointer makes this O(1) |
| reenter-before / reenter-after | capture inside thunks during an ordinary call (value form: inside `value_wind_stub`'s window) | ✓ | stub windows (`register_roots: None`) are frozen and traced conservatively |
| abort | abort crossing an extent, so the **slow** landing path | ✓ (copies `[..landing_depth]`) | landing = freeze + truncated view + 1-frame stub chunk; `abort_landing` flag in the continuation header |
| resume | abort's delimited continuation resumed: `invoke_step` stub, `before` re-entry, `append_delimited` | ✓ | delimited = view + floor; eager append keeps today's relocation code; lazy append needs an underflow stub holding `dst` (§6) |
| resume+thunk-reenter | full capture inside a `before` run by `invoke_step`; re-enter later | ✓ (today the stub's CONT keeps the delimited payload via the weak fixpoint; under A it is a plain strong edge) | same |
| jump+before-reenter | capture during a full jump's travel; the snapshot holds `wind_jump_stub` with TARGET | ✓ (orphan windows harmlessly included) | stub frame frozen; orphan windows can be freed once TARGET is a strong object (§1.1) |
| escape+after-reenter | capture inside an `after` run by an escaping jump; the record was already popped | ✓ | the captured wind list excludes the popped record, so it is a correct value |
| abort+after-reenter | capture inside an `after` run by abort travel; TARGET is the landing | ✓ | the landing object must survive via the stub register: strong edge |
| resume+after-reenter | capture inside an `after` of a resumed composable region | ✓ | with lazy append the frozen state includes the underflow stub, so replay needs its view/floor/dst operands |
| abort-from-before / abort-from-after | delimited capture inside thunks of an ordinary `dynamic-wind`; record not yet pushed / already popped, so the captured winds are empty | ✓ | the empty-region identity rule (`control.rs:2859-2890`) must be kept by construction |
| resume-from-before / resume-from-after | resuming those: the appended frames include the value stub (value form) | ✓ | as for resume |

[I] Tail-position rows matter: a tail call to a control primitive pops the body frame first, which produces
empty delimited regions (`deliver_reg = None`) and prompts at `exit_depth`. These are the shapes behind
#162, #163 and #176. Under C′ they become "view whose floor equals its top", and must still yield the
identity continuation with no dynamic environment.

---

## 4. Code objects become GC-traced (#338/#352)

**Today [V].**
- A frame holds `Rc<CodeObject>` (`types/mod.rs:59`). A closure names its code by `code_id: u64`
  (`heap/mod.rs:205-213`).
- A unit (a top-level form plus its nested lambdas, `code_units`) is in use while any of its code has
  `live_closures > 0` or `Rc::strong_count > 1` (`vm_state.rs:1003-1008`).
- `live_closures` goes up at `MakeClosure` and down only from sweep's dead-closure report
  (`after_collection`, `:542-575`) or `retire_eval_closure` (`:521-531`).
- Constants of every *loaded* code are strong roots (`gc_roots.rs:97-99`).
- Slots are recycled by generation (`CodeObjectId`; #352).

**Change [I].**
1. **Code descriptors live in a non-moving code space**, an immortal or non-moving large-object space in the
   MMTk/Immix sense, and are *marked*, not counted. Frames carry a `u32` table index or a stable pointer,
   with no refcount. That removes the four `Rc` operations per call/return pair and lets frames be `Copy`
   (A's memcpy capture depends on it).
2. **Edges.** frame→code, chunk-frame→code, closure→code, and code→nested-code (a `MakeClosure` operand
   makes a parent's reachability keep its lambdas, which replaces the "unit kept whole" rule), plus
   code→constants.
   - Constants become per-code traced slots, updatable in place because the descriptor does not move.
   - Machine code never embeds a movable constant (jit-readiness.md E.23).
3. **Release only after a complete mark** (full or major GC), as HotSpot unloads classes only after a full
   marking cycle. A minor or copying collection never decides code liveness. This deletes `live_closures`,
   `gc_freed_closure_code_ids` and `RETIRED_VM_CLOSURE_CODE` bookkeeping. It also removes the sweep-visits-the-
   dead dependency (vm-runtime.md §0.7).
4. **Generation-checked ids** remain only for *external* weak references to code, such as debugger ids and
   global-cache entries keyed by code. A frame's or chunk's reference keeps the slot from being reused.
5. **Per-pc `register_roots`.** A GC scanning a chunk reads `frame.code → register_roots[frame.pc]`.
   - The descriptor must be marked no later than the frame that names it, and must never be freed during
     the collection that discovers it. Non-moving code space gives both for free.
   - Pack the maps (today `Vec<Vec<u64>>` per pc, `code_object.rs:158`) so that a chunk walk is one load per
     frame.
6. **JIT machine code.** It is freed with its descriptor, and only when no native activation is inside it.
   Under the S1 fragment model (cranelift-gc.md §6.1) no JIT native frame survives a safepoint, so release
   at a safepoint is safe without an epoch scheme.

---

## 5. Tree-walker: off-heap `CpsContinuation`/`ContEnv`, and the weak-id disagreement

**[V] Facts** (tree-walker.md §1.3, §1.9).
- `CpsContinuation` (208 B, `Rc`) holds `Rc<Environment>`, `Rc<CpsExpr>`, a `ContEnv`, a `resume:
  Option<ContValue>`, and **`Vec` copies of the three dynamic stacks** taken at capture.
- `ContEnv` is a persistent `Rc` cons list, so capture is O(1) in Scheme depth but O(winds + prompts +
  handlers) in stack copies.
- Liveness of `Procedure(Rc<…>)` and `Continuation(Rc<…>)` payloads depends on sweep tombstoning dead slots.

**[I] Can they stay off-heap?** Yes, under a non-moving policy for the tree-walker heap, or under precise
**non-transitive pinning** of everything they reference (MMTk `create_process_pinning_roots_work`; Chez
`lock-object`). The reason: their TVs sit in immutable `Rc` structures that a mover cannot update.
- Environments are `RefCell`-updatable.
- `ContValue`/`CpsContinuation`/`WindRecord` fields are not, which is about 20 field sites
  (tree-walker.md §3.2).
- Each backend constructs its own heap, so a per-heap policy is safe.
- The rule must cover the weak-fixpoint path too: a payload traced via `trace_weak_ids` reports its TVs as
  **pinning** edges.

**[I] Reconciling "add weak tables" (tree-walker) with "remove weak tables" (VM).** The two proposals are
about different payloads.

| | VM continuation | Tree-walker `CpsContinuation`/`CpsLambda` |
|---|---|---|
| Payload | plain data (frames, registers, records) | Rust-owned graph (`Rc<Environment>`, `Rc<CpsExpr>`, `Box<ContValue>`) with `Drop` |
| Right home | **the heap** (A/C′); weak table deleted | **host-payload table** keyed by a heap handle, traced when the handle is marked and dropped when it is not |
| Why | movable, byte-accounted, no soundness side-rule | the GC cannot represent `Drop`; a copying nursery never visits dead slots, so tombstone-drop must go |

Keep **one** collector mechanism: today's `trace_weak_ids`/`sweep_weak`, renamed to a host-payload table and
generalised. Precedents are OCaml custom blocks with finalisers (`runtime/minor_gc.c:785-806`), Wasmtime
`ExternRef` host data and V8's external pointer table. Its contract must be stated generally:
- At a safe point a payload is reachable only through its handle.
- Rust code holding a payload `Rc` across a safe point also roots the handle. `invoke_step::CONT`
  (`control.rs:2702-2706`) is the existing example.
- Payload TVs are pinning edges unless the payload makes them updatable.

The VM keeps none of these tables after Stage 1. The tree-walker gains them for `CpsLambda` and
`CpsContinuation`, plus `Continuation`'s stack copies. Those copies would shrink if `WindRecord`'s
`Rc<[H]>` handler snapshot and the three stacks became shared persistent lists, a change in `patina-core`
that serves both backends.

**Generational note [I].** A payload table is old, unbarriered storage. Immutable payloads, such as
`CpsContinuation` and `ContEnvNode`, need scanning only in the first minor GC after their creation (the
OCaml `Cont_tag` promotion argument). Mutable `Environment`s need a dirty bit (tree-walker.md §5.5).

---

## 6. Staged recommendation

**Stage 0 — groundwork, no behaviour change.**
- Introduce `ExecutionState::depth()` and use it at all 37 `frames.len()` sites. Introduce one
  `register_window(frame)` accessor.
- Add `samedepth1000`, `escape1000`, `pingpong1000`, `ctakdeep` and `abort100` to `bench_programs` with a
  retained-bytes metric. Today's harness has no capture-at-depth or pause benchmark.
- Matrix and `escape_from_primitive.rs` stay 64/64 and green.

**Stage 1 — design A (pairs with the byte-accounted trigger).**
- Continuations become variable-size heap objects; frames become `Copy` (code index plus `closure` as a TV).
- Delete `continuation_store`, `delimited_continuation_store`, `VmContinuationRef`, and the VM's
  `trace_weak_ids`/`sweep_weak`.
- Free wind-step stub windows with their frame, since TARGET is now a strong object.
- Rewrite `weak_continuation_tests.rs` (`vm_state/weak_continuation_tests.rs`: pruning, chains, cross-
  references, bounded store) as plain reachability tests. Keep `gc_vm.rs:58-101` and the ephemeron +
  continuation test (`ephemerons.rs:195-213`). The latter becomes an ordinary ephemeron-fixpoint case,
  because the second weak kind disappears.
- Expected [I]: a 2-4× cheaper capture (toy: 5.8× on the capture overhead), about 20-25% fewer retained
  bytes, and the GB plateaus replaced by the byte trigger's policy.
- **Risk: low.** No control algorithm changes.

**Stage 2 — traced code liveness (§4).** Required before any copying nursery. **Risk: medium.** Unit
release semantics (#338) move from "counted at sweep" to "marked at full GC", and code may survive longer
between full collections.

**Stage 3 — non-relocating live register stack.**
- Reserve VA once and commit on demand, with an explicit limit check on push. jit-readiness.md F.24.
- Optionally interleave frame headers with windows (`[closure][code|pc][nregs|ret|prev]` + registers), so
  that a frozen chunk is one word array and a frame walk needs no side vector (jit-readiness.md G.31).

**Stage 4 — C′.**
- Frozen watermark updated on `Return`. It is shared with the minor-GC stack watermark proposed in
  java-hotspot.md §3.12: one `low = min(low, depth)` on `Return`, folded into each consumer's own mark at
  capture and at GC.
- Immutable chunks; continuation = {view, depth, deliver, flags, re-entry}.
- Graft-and-thaw invoke through an **underflow stub frame**. It is just another runtime stub (`Underflow`
  instruction), so neither the interpreter's nor the JIT's `Return` needs a check (Chez `dounderflow`).
- Adaptive thaw: the whole chunk below a few hundred words, otherwise one frame, after Loom.
- Overflow = freeze and slide, which keeps 10M-deep recursion on a fixed buffer.
- Winds, handlers, prompts and re-entry ids become persistent heap lists, as Chez `winders`
  (`s/mkgc.ss:233`) and Gambit's `denv` (`gambit.h.in:6641-6651`) are. The threads report already asks for
  deep-bound dynamic state as heap data (threads-recommendation.md item 9).
  - `next_wind_step` gains a list form: walk the longer list to equal length, then compare by `id`.
  - `install_thunk_handlers` clamping and `relocate_depth` stay integer arithmetic.
  - Handler and prompt depths stored as **distances from the delimiting prompt**, as `raise_step::DEPTH_BELOW`
    already does, would make relocation free.
- Delimited continuations: freeze + view with a floor; **append eagerly first**, which keeps
  `append_delimited` as written. Go lazy only if measurements ask for it, because lazy append needs an
  underflow stub carrying `dst` and re-pointing the outermost `return_reg`.
- **Risk: medium-high.**
  - Virtual-depth mistakes reproduce the #162/#163/#176 family, so the matrix is the gate.
  - The watermark invariant must hold across every "resume frame i" path: `Return`, value delivery,
    `ResumeWindJump`, raise stubs and loop exits.
  - Lazy-thaw regressions on reinstate-and-return-all (toy 1.6-4.4×).

**Stage 5 — baseline JIT on these frames.** Resume entries per return point; underflow and overflow stubs
as ordinary frames; the `Return` sequence = pop + watermark compare + indirect jump to the caller's resume
entry (S1) or `ret` (S2).

**Cross-cutting risks [I].**
1. **Stale Rust views.** Rust code holding `&` into a continuation object or a chunk across a safe point
   would go stale after a move. Today's "helpers do not collect" rule (`control.rs:23-33`) covers it; the
   JIT must reload after safepoints (jit-readiness.md D.16).
2. **Retention in C′.**
   - The machine's `frozen` view can retain the stale top of its last chunk until the next freeze or GC.
     Truncate it to `wm` at safe points.
   - Chunks shared by many continuations retain the registers live at freeze time. This is semantically
     required.
3. **Large chunks in a generational heap** must be remembered on allocation.
4. **The tree-walker keeps its own representation.** A `patina-core` change to `WindRecord`/`next_wind_step`
   must keep a slice form for it, or move both backends together.

---

## 7. Lessons and constraints for the redesign

- **Continuations must be heap objects measured in bytes.** A one-allocation-per-capture trigger turned a
  1000-frame capture loop into 3.4-5.7 GB.
- **Capture cost must not depend on the depth below the region of interest.** Library loading, REPLs,
  harnesses and schedulers run hundreds of frames deep.
- **Keep the depth-integer discipline.** No pointer into a frame from a prompt, handler or wind record.
  Prefer distances relative to a prompt or segment base. This is what makes moving and chunking cheap.
- **Frames must be plain `Copy` data:** `(code ref, pc, base, nregs, return_reg, closure TV)`. Code refs point
  into a non-moving, marked code space. No return-address interior pointers.
- **Keep "work owed after a call is a stub frame" as a hard invariant.** Underflow and overflow become two
  more stubs, and the JIT then needs no special frames.
- **Captured state is immutable.** Re-entry copies (thaws) and never runs in place. This removes write
  barriers on chunks and makes multi-shot safe.
- **One host-payload mechanism, not a VM-specific weak table.** Its soundness rule must be written down for
  every user.
- **Code liveness is decided only by complete marking.** Never by counting at sweep.

## 8. Open questions

1. Does any program shape in the suites rely on delimited continuations deep enough for lazy *append* to
   matter, or is eager append permanent?
2. Thaw policy: fixed k, Loom's 500-word threshold, or adaptive per continuation? This needs Patina-level
   measurement, not the toy.
3. Should a prompt start a new chunk boundary (the Racket CS metacontinuation model), making abort and
   delimited capture O(1) regardless of the watermark?
4. Do C′'s frame copies below `wm` (live and frozen) materially raise peak memory for deep recursion with
   periodic capture? Bounded by one live-stack copy [I], but unmeasured.
5. With winds as persistent heap lists, should `parameterize` move to deep binding at the same time (threads
   report item 9), so that `dynamic-wind` stops being the parameter mechanism?
6. Where should the tree-walker's continuation stack copies go? Shared persistent lists would require the
   tree-walker to adopt heap records for winds and handlers, which is a small slice of tree-walker.md
   option (c).
