# How a Cranelift JIT integrates with a GC (2024 to 2026), and what that means for Patina's GC redesign

Scope: Cranelift's GC-facing surface as of `bytecodealliance/wasmtime` `main` on 2026-09-30 (Cranelift 0.136.1, Wasmtime 49.0.1). That surface is user stack maps, frame walking, Wasmtime's collectors and barriers, the `tail` convention, `try_call` exceptions, `stack_switch`, pinned registers, vmctx and alias regions. The report then compares other root-finding and safepoint strategies, and gives concrete guidance for a baseline JIT over Patina's register-bytecode VM.

Tags: **[V]** means checked in source, release notes or a primary document (source path given). **[I]** means my inference. **[M]** means measured here.

Sources were fetched with `curl` from `raw.githubusercontent.com/bytecodealliance/wasmtime/main/...` and cached locally (the cache was not retained). Paths below are repo-relative to wasmtime unless prefixed `patina:`.

---

## 0. Summary

1. **Cranelift's GC contract is narrow and explicit [V].** Safepoints are exactly the non-tail calls (`Opcode::is_safepoint` = `is_call() && !is_return()`, `cranelift/codegen/src/ir/instructions.rs:319-321`). The CLIF producer declares which SSA values or variables are GC references. `cranelift-frontend` then spills them to stack slots before each safepoint, reloads them at their uses, and attaches a `UserStackMap` of `(type, SP offset)` entries to each call. Nothing else in Cranelift knows about GC: there are no reference types, no read or write barriers, no polling, and no trap-based safepoints.
2. **Wasmtime's own path is evidence about what works [V].**
   - It started with deferred reference counting (DRC), which needs read and write barriers and cannot collect cycles.
   - In 2026 it added a Cheney semi-space copying collector: bump allocation inlined into compiled code, **no read or write barriers**, precise roots from stack maps.
   - The copying collector became the default in Wasmtime 46 (2026-06-22, PR #13439), described as "more performant in most situations".
3. **For Patina, the simplest and fastest baseline keeps every Scheme value in VM-managed memory at every safepoint [I].** That memory is the existing register file plus `CallFrame`s. The JIT then needs **no Cranelift stack maps, no native frame walking, and no unwinder**. The GC's root set is unchanged, and `call/cc` capture stays a VM-level operation.
   - This is the design of Guile 3's JIT ("the same stack reads and writes", `libguile/jit.c` header), V8's Sparkplug, and in spirit LuaJIT.
   - Patina already has the metadata this needs: per-pc register liveness maps, `CodeObject::register_roots` (patina:`crates/patina-vm/src/types/code_object.rs:151-158`), applied by `retire_registers` (patina:`crates/patina-vm/src/runtime/vm_state/gc_roots.rs:47-68`).
4. **Choices that most simplify a Cranelift JIT** (details in §6):
   - raw tagged addresses, or one never-moving reserved region, instead of per-type growable `Vec` arenas;
   - one bump pointer plus limit word for allocation, GC requests and interrupts (OCaml's `young_limit`);
   - every runtime call is a GC safepoint, and GC never runs inside a Rust primitive frame;
   - no read barriers;
   - no movable object addresses embedded in machine code;
   - `#[repr(C)]` object headers instead of Rust enums and `Rc` payloads.
5. **Choices that would hurt the JIT:**
   - growable-`Vec` arenas, whose base pointer must be reloaded after every allocation;
   - `RefCell`/`Rc` in fast paths;
   - ZGC/Shenandoah-style load barriers;
   - deferred RC;
   - conservative stack scanning combined with index encoding;
   - precise native-frame roots in the baseline tier;
   - relying on Cranelift `stack_switch` for continuations (lowered only on x64, one-shot);
   - code patching on macOS arm64.

---

## 1. Patina facts this analysis depends on

- **Values [V].** `TaggedValue(u64)` has a low 3-bit tag. Heap tags are pair 011, vector 100, string 101, closure 110 and object 111 (patina:`crates/patina-core/src/tagged_value.rs:9-19,77-84`). The payload is a **u32 arena index**, not an address: `pair(index) = (index << 3) | TAG_PAIR` and `heap_index = bits >> 3` (`tagged_value.rs:376-378,410-413`).
- **Heap [V].** `pairs: Vec<(TV,TV)>`, `vectors: Vec<Vec<TV>>`, `strings: Vec<Vec<char>>` and `objects: Vec<HeapObjectData>` (patina:`crates/patina-core/src/heap/mod.rs:307-316`), behind `SharedHeap = Rc<RefCell<Heap>>` (`heap/mod.rs:51`). `alloc_pair` calls `note_alloc`, pops a free list or pushes onto the `Vec` (`heap/mod.rs:703-714`). Every `push` can reallocate the arena, so the base pointer is not stable.
- **VM state [V].** `ExecutionState { registers: Vec<TaggedValue>, frames: Vec<CallFrame>, prompt_stack, dynamic_winds, exception_handlers }` (patina:`crates/patina-vm/src/runtime/execution_state.rs:17-23`).
  - `push_frame` resizes `registers` (`:55-75`), so the register file can also move.
  - `CallFrame { pc, register_base, num_regs, closure: Option<HeapIndex>, return_reg, code: Rc<CodeObject> }` (patina:`crates/patina-vm/src/types/mod.rs:41-60`).
- **Continuations [V].** Full `call/cc` capture clones all five components, including the whole register `Vec` (`capture_full`, `execution_state.rs:239-252`; `restore`, `:255-261`). Delimited invoke relocates captured windows onto the live array (`append_delimited`, `:319-345`).
- **Safepoint [V].** There is one at the top of `run_loop_until_outcome`'s loop: a `Cell<bool>` pending flag, honoured only by the outermost dispatch loop (`GcDeferGuard`) (patina:`crates/patina-vm/src/runtime/vm_state.rs:1151-1209,1287-1300`). Its measured residual cost is about +1.4% against a no-safepoint control (patina:`docs/GC_DESIGN.md` §6.1).
- **Liveness [V].** The VM already computes per-pc register-root bitsets (`register_roots`). The GC clears dead slots of each frame using `frame.pc` before tracing (`gc_roots.rs:47-68`). These are, in effect, **stack maps for VM frames**.
- **Callbacks [V].** AGENTS.md: "No primitive calls back into the program from Rust any more". Callbacks run as VM frames through `Step::Call`/`Step::Eval`. Nested dispatch loops remain for `execute_nested` (library loading, `vm_state.rs:1084-1094`) and synchronous calls (`control.rs:2335-2352`).
- **Globals [V].** `LoadGlobal` probes an inline cache against an `Rc<Environment>` obtained via `frame_globals`, which borrows the heap and clones an `Rc` (`vm_state.rs:1357-1364,1492-1506`).
- **Sizes [M].** These come from re-running the sibling probe binary `jit-readiness-sizes` (crate `PRD/study/gc/probes/jit-readiness/`), built against the clean `main` tree: `CallFrame` 40 B, `HeapObjectData` 72 B, `Instruction` 48 B, `VmContinuation` 152 B, `CodeObject` 160 B.
- **Deep recursion [M].** `(define (f n) (if (= n 0) 0 (+ 1 (f (- n 1)))))` with `(f 10000000)` runs on the current release VM in 0.52 s at 1.37 GB max RSS. Patina therefore supports 10M-deep non-tail recursion today, about 137 B of RSS per level. A JIT that maps each Scheme frame to a native frame cannot match this on an 8 MiB native stack (§5).
- **Host [M].** The development machine is `arm64` (Apple M4 Pro, macOS). This matters for `stack_switch` (§2.6) and for W^X (§6.4).

---

## 2. Cranelift's GC-relevant surface, verified

### 2.1 User stack maps (the 2024 redesign)

**History [V].**
- In Wasmtime ≤ 24, stack maps were produced by the register allocator from special `r32`/`r64` reference types.
- Nick Fitzgerald lists the problems (https://fitzgen.com/2024/09/10/new-stack-maps-for-wasmtime.html):
  - missed optimizations, because `is_null` is a distinct opcode;
  - **miscompiles**, because the mid-end could not see the spills and reloads inserted later, so it deduplicated field-address computations across safepoints. Under a moving GC that is a use-after-move;
  - references forced to pointer width, which ruled out 32-bit GC references on 64-bit hosts;
  - a run of security advisories.
- Release notes: "CLIF frontends can now define their own stack maps" (Wasmtime/Cranelift 23.0.0, 2024-07-22). "Wasmtime now uses the new 'user' stack maps … rather than the old regalloc-based stack maps" (25.0.0, 2024-09-20).

**API [V]** (`cranelift/frontend/src/frontend.rs`):
- `declare_var_needs_stack_map(var)` (`:426-448`): "All values that are uses of this variable will be spilled to the stack before each safepoint and reloaded afterwards … Between spilling the stack and it being reloading again, the stack can be updated to facilitate moving GCs." It must be called before any definition, and the type may be at most 16 bytes.
- `declare_value_needs_stack_map(val)` (`:545-566`): same, per SSA value; the size must be a power of two and at most 16 bytes.
- `make_stack_map_alias_region(cb)` (`:681-698`): tags the inserted spill and reload memory operations with an alias region (PR #13769, 2026-06-30).
- `finalize()` runs the spiller **only if** some value was declared (`:735-742`). A JIT that declares nothing pays no compile-time cost for it.

**Mechanics [V]** (`cranelift/frontend/src/frontend/safepoints.rs`):
- A backward liveness fixpoint over declared values records the live set at each safepoint (`record_safepoint`, `:378-393`).
- Each value live across any safepoint gets a stack slot. Slots are reused through a free list and sized by type (`SlotSize` 1 to 16 B).
- The spiller then:
  - stores the value to its slot right after its definition;
  - appends one `UserStackMapEntry { ty, slot, offset }` per live value to each safepoint (`rewrite_safepoint`, `:698-726`);
  - replaces **every use** of a value live across any safepoint with `stack_addr` + `load` (`rewrite_use`, `:734-770`). The mid-end's alias analysis is left to forward those loads.
- Tail calls are not safepoints, and nothing is live after one (test `needs_stack_map_and_tail_calls`, `:1327`).

**Format and limits [V]** (`cranelift/codegen/src/ir/user_stack_maps.rs:1-90`).
- A stack map is a table from return address to SP-relative offsets, with an `ir::Type` per entry.
- The module states its limits: "Currently all non-tail call instructions are considered safepoints." It does **not** allow "skipping safepoints for calls that are statically known not to trigger collections, or to have a safepoint on a volatile load to a page that gets protected when it is time to GC".
- Consequences:
  - a write-barrier slow-path call is still a safepoint and forces spills around it;
  - HotSpot-style trap polls are not expressible.
- Compiled maps are read from `MachBuffer::user_stack_maps()` (`cranelift/codegen/src/machinst/buffer.rs:2002-2010`).
- `cranelift-jit`'s `JITModule` does **not** surface stack maps. A grep of `cranelift/jit/src/backend.rs` finds no `stack_map`. An embedder that wants them must drive `cranelift_codegen::Context` itself and keep the per-function tables [V by absence, I for the workaround].

**Cost model [I].** Every GC value live across a call is spilled once at definition and reloaded at its uses. It can never stay in a callee-saved register across a call, which is the same as OCaml, where every call destroys every register (see sibling report `ocaml-gambit.md` §2.5). Under `opt_level=speed`, alias analysis removes redundant reloads between safepoints. `enable_alias_analysis` is "Only effective when `opt_level` is `speed`" (`cranelift/codegen/meta/src/shared/settings.rs:61-68`).

### 2.2 Frame walking and root discovery in Wasmtime

- **Frame-pointer walk [V].** Stack walking uses frame pointers, not unwind tables: "only works on code that has been compiled with frame pointers enabled (`preserve_frame_pointers`) … follows the singly-linked list of saved frame pointer and return address pairs" (`crates/unwinder/src/stackwalk.rs:1-21`).
  - Entry and exit trampolines bracket each contiguous run of Wasm frames: the exit trampoline provides the start FP and the entry trampoline the end FP.
  - This replaced unwind-info-based walking in Wasmtime 0.40 (PR #4431): "stack walking is much faster than before".
- **Root tracing [V]** (`crates/wasmtime/src/runtime/store/gc.rs:713-809`). `trace_roots` visits, in order: Wasm stack frames; suspended continuation stacks (stack-switching); vmctx roots (globals and tables); instance roots; user (host) roots; and the pending exception. For each frame:
  - the PC maps to a module and code offset;
  - `StackMap::lookup(offset, stack_map_data)` finds the map;
  - `sp = stack_map.sp(fp)` locates the frame;
  - for each live slot that holds a non-null, non-i31 `u32`, `add_wasm_stack_root(slot_ptr)` registers the **slot address**, so a moving collector rewrites it in place.
- **Host-side roots [V].** Host roots use indirection (`crates/wasmtime/src/runtime/gc/enabled/rooting.rs:1-90`): "all of our rooting types below use indirection … Logically, the `Store` 'owns' the actual pointers; and rooting types own the slots". Scoped `Rooted<T>` (LIFO, bulk-unrooted) and `OwnedRooted<T>` exist specifically so that "moving collectors (such as generational and compacting collectors)" stay sound.
- **Design rule from the RFC [V]** (`bytecodealliance/rfcs accepted/wasm-gc.md` §"Defining the Pluggable GC Interface"): "GC roots on the Wasm stack will always be precisely traced in a stop-the-world phase. We won't do conservative stack scanning, nor will we reference count Wasm-stack roots, nor will we incrementally trace stack roots."

### 2.3 Wasmtime's collectors, barriers and allocation fast paths

All are behind the `GcCompiler` (compile-time hooks: allocation, read and write barriers, `is_moving_collector`) and `GcRuntime` traits (`crates/cranelift/src/func_environ/gc.rs:33`; RFC §"Defining the Pluggable GC Interface").

| Collector | Collects cycles | Barriers emitted in compiled code | Allocation in compiled code | Status |
|---|---|---|---|---|
| Null | no (traps when full) | none | inline bump (`gc/null.rs`) | added 27.0.0 (2024-11-20) as a "speed of light" baseline |
| DRC | **no** | **read** barrier on every GC-ref load into a stack value: if not already in the over-approximated stack-root set (header bit), link it and `inc_ref` (`gc/drc.rs:510-540`). **Write** barrier on stores to heap, tables and globals: `inc_ref(new)`, `dec_ref(old)`, cold `drop_gc_ref` at zero (`:647-687`) | libcall `gc_alloc_raw` (free list) | was the default until 46.0 |
| **Copying** (Cheney semi-space) | yes | **none**: "No read barrier needed … but we do need stack maps" and "No write barrier needed" (`gc/copying.rs:364-411`) | **inline bump**, cold libcall fallback (`gc/copying.rs:91-216`) | initial 45.0.0 (2026-05-21, #13093/#13107); inline bump and **default** 46.0.0 (2026-06-22, #13323/#13439); multi-increment collection (#13491) |

(`gc/*.rs` paths are under `crates/cranelift/src/func_environ/`. The runtime halves live in `crates/wasmtime/src/runtime/vm/gc/enabled/{null,drc,copying}.rs`.)

**Inline bump allocation, verbatim structure [V]** (`gc/copying.rs:97-216`):
1. Load `bump_ptr` and `active_space_end` (both `u32`) through a pointer to `VMCopyingHeapData`.
2. Compute `end = bump + aligned_size` in i64 and branch on `end <= active_space_end`.
3. The **cold** block (`builder.set_cold_block`) calls `gc_alloc_raw`, which "will collect or grow the GC heap".
4. The fast block stores the new bump pointer, computes `obj_ptr = heap_base + gc_ref`, and writes three header words (kind with inline trace bits, type index, object size).
5. The merged `gc_ref` is declared `declare_value_needs_stack_map`. The bump state lives in memory that "is written to by compiled Wasm code" (`runtime/vm/gc/enabled/copying.rs:252-256`). It is not pinned in a register.

**GC references are 32-bit offsets into one reserved region [V].**
- `VMGcRef(NonZeroU32)` is "not actually a pointer, but a compact index into a Wasm GC heap". `i31ref` is distinguished by a low discriminant bit (`runtime/vm/gc/gc_ref.rs:93-120,358-361`).
- The heap base is loaded from the store context. When the heap cannot move, the load is marked `readonly` and `can_move`, so LICM hoists it out of loops (`func_environ/gc.rs:1679-1705`).
- Defaults: a 4 GiB reservation plus 32 MiB guard on 64-bit when the `gc_heap_*` tunables are set; `gc_heap_may_move` defaults to `true` (`crates/wasmtime/src/config.rs:2114-2178`). This is Wasm's sandboxing and compression choice.
- [I] Patina is not a sandbox, so raw tagged addresses are cheaper (§6.2). The **stable-base** lesson still applies to anything addressed by base plus offset, including Patina's register file.

**Header-resident layout info pays [V].** Storing struct and array pointer bitmaps in the header's 26 reserved bits instead of a hash-map side table gave **1.07x to 1.08x** on `splay.wasm` (PR #13495).

**Collector trade-off table [V]** (`config.rs:3577-3608`): copying has bad latency and heap utilisation, good throughput and allocation speed. DRC has good latency, poor throughput and does not collect cycles. Wasmtime's docs say copying "should be more performant in most situations".

### 2.4 Proper tail calls

- **The `tail` convention [V].** `CallConv::Tail` "Supports tail calls, not ABI-stable … basically sys-v except that callees pop stack arguments" (`cranelift/codegen/src/isa/call_conv.rs:17-31`).
- **Instructions [V].** `return_call` and `return_call_indirect` require "the caller and callee calling conventions must be the same, and must be a calling convention that supports tail calls". The indirect form takes a native address (`meta/src/shared/instructions.rs:235-270`).
- **Timeline [V].** The tail convention gained callee-saved registers in 20.0.0 (2024-04-22, #8246). Wasm tail calls became default-on in 21.0.0/22.0.0 (2024-05/06, #8540/#8682). s390x support is complete as of 24.0.0.
- **Frame layout [V]** (RFC `accepted/tail-calls.md` §"New Wasm Calling Conventions"). Stack arguments sit above the return address and saved FP, and "The callee must clean up its own stack space and on-stack arguments". The RFC also requires that "We must maintain frame pointers … for fast, DWARF-less stack walking".
- **GC interaction [V].** From the RFC: "we don't need to worry about anything as long as callee stack maps 'own' stack arguments".

### 2.5 Exceptions: `try_call` (2025)

- **Instructions [V].** `try_call` and `try_call_indirect` are block terminators with a normal successor (`retN` block args) and an exception table of `tag: block(exnN)` (`meta/.../instructions.rs:307-360`). Initial PR #10510 merged 2025-04-08 for x64, aarch64, riscv64 and pulley.
- **ABI [V].** "all registers are clobbered" at `try_call` sites, so the unwinder never restores callee-saved registers. Payloads arrive in two fixed registers (x0/x1 on aarch64; `call_conv.rs:19-27`).
- **Unwinding [V].** The unwinder walks the FP chain and maps return PCs to exception tables. `resume_to_exception_handler` sets SP and FP and jumps (Chris Fallin, https://cfallin.org/blog/2025/11/06/exceptions/). The happy path has zero cost.
- **Wasmtime timeline [V].** Wasm EH was fully implemented in 37.0.0 (2025-09-20, #11326). In 38.0.0, Wasmtime's own trap handling dropped `setjmp`/`longjmp` for Cranelift EH (#11592). EH became default-on in 47.0.0 (2026-07-20).
- **Use outside Wasmtime [V].** `cranelift-jit` has a `wasmtime-unwinder` feature that builds exception tables per compiled function and exposes `JITModule::lookup_wasmtime_exception_data(pc)` (`cranelift/jit/src/backend.rs:394-415,501-511`). The unwinder crate is published as `wasmtime-internal-unwinder` 49.0.1, self-described "INTERNAL", so its API is unstable.
- [I] Clobbering all registers at handler sites costs nothing extra if Scheme values never live in machine registers across calls anyway (§6.1).

### 2.6 Stack switching (typed continuations) and `call/cc`

- **The instruction [V].** `stack_switch(store_context_ptr, load_context_ptr, in_payload)` exists, but "The instruction is experimental and only supported on x64 Linux at the moment", and it is **one-shot**: "performing two `stack_switches` using the same `load_context_ptr` causes undefined behavior" (`meta/.../instructions.rs:981-1030`).
  - [V] I checked `cranelift/codegen/src/isa/{aarch64,x64,riscv64}/lower.isle`: only x64 matches `stack_switch` (4 hits; aarch64 has 0). Wasmtime's config accepts Linux and macOS with the `basic` model and Windows with `update_windows_tib` (`config.rs:3096-3104`), but the arm64 lowering is absent.
- **Wasmtime continuations [V].** Each continuation is a full native stack of `async_stack_size` (default `2 << 20` = 2 MiB, `config.rs:301`). Allocation says "we currently don't support deallocating them. Instead, all continuations remain allocated throughout the store's lifetime" (`crates/wasmtime/src/runtime/store.rs:465-469,2135-2160`).
  - GC traces suspended continuation stacks with the same per-frame stack-map routine, and payload buffers with a per-slot GC-ref marker (`store/gc.rs:838-898`).
  - Stack switching is off by default; "Work continues" per 48.0.0 notes.
- **Relevance to Patina [I].**
  - It is useless for multi-shot `call/cc`.
  - It is a poor fit even for one-shot uses (generators, green threads): 2 MiB per continuation, no arm64 lowering, and it is experimental.
  - Patina's `call/cc`, `dynamic-wind` and prompts are already VM-level operations on VM frames. A JIT should keep them there.
- **Evidence from Hoot [V].** Guile→Wasm (Hoot) without stack switching uses a "tailify" pass: every call becomes `return_call`, and three explicit stacks (numeric, tagged references, return continuations) are saved and restored. Wingo reports "10x penalties in some cases" (https://wingolog.org/archives/2024/05/27/cps-in-hoot). That cost comes from Wasm's type system forcing three stacks and boxing. A native Cranelift JIT can address one untyped VM register array directly.

### 2.7 Other relevant knobs

- **`enable_pinned_reg` [V].** "This register is excluded from register allocation, and is completely under the control of the end-user", accessed with `get_pinned_reg`/`set_pinned_reg` (`meta/src/shared/settings.rs:113-121`). It is **x21 on aarch64** and **r15 on x64**, chosen to match SpiderMonkey's HeapReg (`isa/aarch64/inst/regs.rs:17-20`; `isa/x64/inst/regs.rs:73-78`).
  - Wasmtime itself does not use it. vmctx is passed as an `ArgumentPurpose::VMContext` parameter (`crates/cranelift/src/func_environ.rs:1483,1979,2461`).
  - (This corrects the sibling note's "Cranelift cannot pin a global register": it can pin exactly one.)
- **`preserve_frame_pointers` [V]** is required for `get_frame_pointer`/`get_return_address` and for Wasmtime's walker (`settings.rs:226-236`).
- **Alias regions [V]** are user-defined entities since 2026-05 (#13354): "Two memory operations in different alias regions are known not to alias" (`ir/memflags.rs:25-55`). They let a JIT tell Cranelift that VM-register-file traffic, heap fields and vmctx fields do not alias.
- **`preserve_all` convention and patchable calls [V].**
  - `PreserveAll` clobbers no registers, takes register arguments only, has no return values and no tail calls (`call_conv.rs:50-61`; #12061, #12160, #12447).
  - Any non-tail, non-indirect call to an `ExtFuncData { patchable: true }` callee "will emit additional metadata indicating how to patch them in or out" (`ir/extfunc.rs:316-323`).
  - [I] This suits rare slow paths (barrier buffer full, interrupt poll). It is still a safepoint for user stack maps (§2.1).
- **Debug tags and `sequence_point` [V].** These exist "to allow perfect reconstruction of original (source-level) program state … preserving 'virtual' frames across an inlining transform" (`ir/debug_tags.rs:1-33`). This is the closest Cranelift gets to deoptimization metadata. [I] It matters only for a future optimizing tier.
- **Compile-speed knobs [V].** `regalloc_algorithm = single_pass` gives "quick compilation but … more register spills and moves". `opt_level = none` disables alias analysis (`settings.rs:34-68`). Cranelift has supported cross-function inlining since Wasmtime 36 (2025-08-20).
- **`cranelift-jit` W^X [V].** Finalized code is made read+execute with `region::protect` (`cranelift/jit/src/memory/system.rs:98-109`). Patching code after finalization means re-protecting pages.

---

## 3. Root strategies for JIT frames

### 3.1 Precise native-stack maps

This is the Wasmtime, OCaml and Chez family.

- **Wasmtime [V]:** §2.1 and §2.2. Frame-pointer walk, per-return-address maps, slots updated in place.
- **Chez [V].** Each non-tail call's return point carries an inline `rp-header { mv-return-address, livemask, toplink, frame-size }` (ChezScheme `s/cmacros.ss:1758-1770`). The collector walks frames using `ENTRYFRAMESIZE(ret)` and `ENTRYLIVEMASK(ret)` read **from the code at the return address**. It uses no frame pointers (`s/mkgc.ss:1036-1060`).
  - Chez runs Scheme on its own stack: `%sfp` = r13 on x86_64 (`s/x86_64.ss:21`).
  - It **returns by an indirect jump through the frame slot**, `%ref-ret` = `sfp[0]`: "All functions currently stash the ret register in sfp[0] and return to sfp[0]" (`s/np-register.ss:126-139`).
  - That is what makes stack-segment continuations cheap: frames are self-describing and position-independent.
- **OCaml:** see the sibling `ocaml-gambit.md` §2.5. Return-address-keyed descriptors; every call clobbers every register.
- **Costs for Patina [I].**
  - It needs a frame walker plus code-range registry and unregistration when code dies.
  - Every Scheme value live across a call is spilled anyway, so it is no better than a VM register file for values that cross calls. It only helps values that do not cross calls.
  - Cranelift frames are linked by **absolute** FP chains. Copying native frames for multi-shot `call/cc` to a different address therefore needs FP relocation and has no support from Cranelift. Chez avoids this by design; Cranelift cannot.

### 3.2 Shadow stacks

- **LLVM's `shadow-stack` strategy [V]** "carefully maintains a linked list of stack roots" that "mirrors the machine stack". It "requires no special support from the target code generator", but has "High overhead per function call". LLVM says "most new development work is focused on `gc.statepoint`" (https://llvm.org/docs/GarbageCollection.html). This is Henderson's ISMM 2002 technique.
- [I] Patina's VM register file *is* a shadow stack whose frames are pushed anyway because they are the VM's frames. A JIT that reuses them pays nothing extra for root registration. A separate shadow stack would only make sense for an optimizing tier with its own frame format.

### 3.3 Conservative native-stack scanning

- **Evidence [V].** Wingo argues that conservative root finding avoids stack-map metadata and forced spills, and that "Conservative Immix … can beat precise scanning in some cases … maybe a percent". He notes "Apple's JavaScriptCore uses conservative stack scanning, and V8 is looking at switching to it". Whippet's `stack-conservative-mmc` pins conservatively referenced objects and moves the rest (https://wingolog.org/archives/2024/09/07/conservative-gc-can-be-faster-than-precise-gc).
- **For Patina [I]:**
  - With **index** encoding, a stack word `(i << 3) | 011` with `i < pairs.len()` is indistinguishable from a real pair. False retention would be high, and pinning would apply to arbitrary arena slots.
  - With raw addresses plus an object-start map (Immix line metadata), it becomes workable. It could replace `GcDeferGuard` for Rust frames, but Rust/LLVM may keep only derived values, such as an index times 16 or an interior pointer. That breaks the "a pointer to the start of the object is on the stack" assumption unless the collector accepts interior pointers.
  - Not recommended for the baseline: the VM-register-file model (§6.1) has no Scheme values on the native stack at safepoints, so there is nothing to scan.

### 3.4 Interpreter-managed frames

This is the LuaJIT, Guile 3, V8 Sparkplug and (in spirit) Chez family.

- **Guile 3 [V]** (`libguile/jit.c` header comment, fetched from savannah cgit).
  - "The generated code performs the same operations on the Guile program state the VM interpreter would: the same stack reads and writes, the same calls, the same control flow."
  - Calls: "it makes a new frame in just the same way the VM would, with the difference that it also sets the machine return address (mRA) … If the callee has mcode, then the caller jumps to the callee's mcode. It's a jump, not a call, as the stack is maintained on the side."
  - Result: "systems-oriented Guile Scheme can walk stacks, throw errors, reinstate partial continuations, and so on without being aware of the existence of the JIT".
  - The frame layout keeps both vRA and mRA, and the dynamic link is stored as an offset "because the stack can move at runtime as it expands or during partial continuation calls" (Guile manual, Stack Layout).
  - It tiers up on a per-function counter (default threshold 1000), with `instrument-loop` entry points for OSR in loops. The JIT holds no Scheme values in registers between bytecodes.
- **V8 Sparkplug [V]:** "Sparkplug intentionally creates and maintains a frame layout which matches the interpreter's frame; whenever the interpreter would have stored a register value, Sparkplug stores one too". This makes OSR "trivial" and leaves "the debugger, the profiler, exception stack unwinding, stack trace printing" unchanged. Most work is done by calling builtins. Reported gains are 5 to 15% on browsing benchmarks (https://v8.dev/blog/sparkplug).
- **LuaJIT [V].**
  - The Lua stack is a slot array inside the `lua_State` object, and `gc_traverse_thread` marks `[stack, top)` like any object (`src/lj_gc.c:309-321`).
  - Traces keep values in machine registers and reconstruct the Lua stack from snapshots on exit.
  - JIT code runs incremental GC steps (`asm_gc_check` compares `gc.total` against `gc.threshold` and calls `lj_gc_step_jit`), but **forces a trace exit if the GC reaches the atomic or finalize phase**: "Exit trace if in GCSatomic or GCSfinalize. Avoids syncing GC objects." (`src/lj_asm_arm64.h:1845-1869`; `lj_gc.c:768-777`).
  - So with a non-moving collector, LuaJIT never needs maps for trace registers. The final marking only happens once the trace has written its state back.
- **For Patina [I].** This is the natural fit: `ExecutionState.registers` and `frames` already are the "stack maintained on the side", and `register_roots` already provides per-pc liveness.

### 3.5 Comparison for a Patina baseline JIT [I]

| Strategy | Cranelift features needed | GC root code | Moving GC | `call/cc`, `dynamic-wind`, prompts | Main cost |
|---|---|---|---|---|---|
| VM register file at safepoints (Guile/Sparkplug) | none (optional alias regions) | unchanged (`GcRoots for VmState`) | yes: JIT reloads from VM memory after every runtime call | unchanged VM-level capture | loads and stores to VM memory across calls; Cranelift cannot keep values in registers across calls (they would be spilled under 3.1 anyway) |
| Native frames + user stack maps (Wasmtime) | `declare_*_needs_stack_map`, frame pointers, own map registry, walker | new frame walker plus code registry | yes | must materialize or deoptimize native frames into VM frames, or copy native stacks (absolute FP chains) | spills plus metadata plus walker; much harder continuations |
| Shadow stack (LLVM) | none | registry of frame records | yes | as above | per-call registration overhead |
| Conservative native scan (Whippet/JSC) | none | conservative scanner plus object-start map | only unpinned objects | as above | needs address encoding and pinning; false retention |

---

## 4. Safepoint polling strategies

- **HotSpot [V].**
  - JEP 312 (thread-local handshakes, JDK 10) replaced the global polling page with "an indirection through a per-thread pointer". Compiled code polls with a load from the thread-local polling address, and arming swaps the pointer to a guarded page.
  - JEP 376 (ZGC concurrent stack processing, JDK 16) extends the same poll into a **stack watermark** check on method return, so frames are fixed up lazily: "Less than one millisecond should be spent inside ZGC safepoints".
  - These are multithreaded mechanisms. Cranelift **cannot** express a trap-based poll as a safepoint (§2.1).
- **OCaml [V]** (sibling `ocaml-gambit.md` §2.4; `runtime/caml/domain_state.tbl:17-29`; `asmcomp/polling.ml`).
  - Allocation compares `young_ptr` against `young_limit`, and anything can interrupt by setting `young_limit = UINTNAT_MAX`.
  - Explicit `Ipoll` instructions go only at loop heads that can loop without allocating and at prologues of possibly-recursive functions. `polling.ml` computes "safe" loop heads with a backward dataflow analysis: a head is safe if "every path … goes through an Ialloc, Ipoll, Ireturn, Itailcall".
- **V8 [I, widely documented but not re-fetched here].** The stack-limit check at function entry doubles as an interrupt check by lowering the limit. This is the same merge as OCaml, applied to stack overflow instead of allocation.
- **Wasmtime epochs [V].** Wasmtime compares a global epoch counter against a deadline at function entries and loop headers. Its docs say epochs can be up to 2-3x faster than fuel, "because epoch-based interruption does less work: it only watches for a global rarely-changing counter" (`config.rs:690-788`).
- **Measured yieldpoint costs [V]** (Lin, Wang, Blackburn, Hosking, Norrish, "Stop and Go", ISMM 2015, text extracted from the PDF):
  - Java benchmarks execute about 100M yieldpoints/s, of which about 1 in 20,000 are taken.
  - Untaken thread-local geometric-mean overheads: **1.9%** conditional, **1.2%** load trap, **1.5%** store trap; a code-patching NOP fast path costs **0.3%**.
  - Patching itself is costly when frequent: 13.4% if every yieldpoint is patched every timer tick.
- **Patina today [V].** One `Cell` load per dispatched instruction costs about 1.4% against a no-safepoint control in the interpreter (GC_DESIGN §6.1).
- **For a single-threaded Patina JIT [I].**
  - GC is requested only by allocation. Every allocation slow path and every runtime call can therefore be the collection point, and **loops that never allocate or call do not need a GC poll at all**.
  - Polls are needed only for asynchronous interrupts (Ctrl-C, timers, profilers). If Patina wants those, put OCaml-style polls at non-allocating loop heads and at the entry of self-recursive fragments, as a load-compare-branch to a cold `preserve_all` call.
  - Merge the GC request into the allocation limit: Rust-side allocations that cross the threshold set `alloc_limit = 0`, so the next inline allocation takes the slow path.
  - Merge the interrupt request into the register-file limit checked on every Scheme call (`base + num_regs <= reg_limit`, which a JIT needs anyway; §6.1).

---

## 5. Continuations across JIT frames

What a continuation must restore, and where the JIT keeps it [I]:

1. **Everything in VM memory (Guile/Chez style).** Capture equals today's `capture_full` (or a future segmented VM stack, as in the sibling's Gambit-style O(1) capture proposal). Reinstatement equals `restore`. Each restored frame resumes at `(code, pc)`, either in the interpreter or at a JIT resume entry. No native frame is ever captured.
2. **Native frames hold Scheme state (Wasmtime-style S3 below).** Capture must deoptimize each native frame into a VM frame. That needs per-safepoint maps of *all* Scheme values, not just GC references: Cranelift's debug tags or user stack maps declared for every value. Otherwise the native stack must be copied, with absolute FP chains and stack-slot addresses to relocate.
3. **`stack_switch`.** One-shot, x64 only (§2.6).

**Native stack depth is the hidden constraint [M/I].** The VM handles 10M-deep non-tail recursion (§1). If each Scheme call is a native `call`, each level costs at least 16 B on aarch64 (FP/LR pair) plus callee-saves and spill slots. At about 48 B/level, 10M levels need about 480 MB of native stack, against the 8 MiB macOS main-thread default. A native-call JIT must either:
- run on a large reserved stack, as Wasmtime fibers do but bigger;
- bail out to the interpreter beyond a native depth limit; or
- use the Guile/Chez "jump, not call" discipline (S1 below), which keeps native depth constant.

---

## 6. Guidance: GC choices that make a Cranelift baseline JIT simplest and fastest

### 6.1 The JIT frame model the GC should assume [I]

Two viable baselines. Both keep **all Scheme values in the VM register file at every safepoint**, store `frame.pc` before every runtime call (so `register_roots` liveness is correct), and declare **no** Cranelift stack maps.

- **S1 – fragments plus tail calls (Guile/Chez control).**
  - Compile each bytecode function into Cranelift functions ("fragments") with the `tail` convention, split at every non-tail Scheme call. Each return point becomes an entry.
  - A Scheme call writes arguments into the new window, pushes a `CallFrame` (vRA = pc, and either an mRA field or `code.jit_entry[pc]`), checks `top <= reg_limit`, then `return_call_indirect`s the callee's entry.
  - A Scheme return pops the frame and `return_call_indirect`s the caller's resume fragment. If the resume fragment has no code, it returns to the interpreter trampoline (tier-down). Scheme tail calls reuse the window, as `tail_replace` does today.
  - Native depth stays constant: trampoline, current fragment, runtime callee. GC never sees a JIT native frame holding Scheme values. Escapes and `call/cc` invocation become "runtime returns a new target fragment" with no unwinding. Deep recursion is bounded only by VM memory.
  - Costs:
    - returns are indirect jumps, predicted by the indirect predictor rather than the return-stack buffer. Chez makes the same trade (§3.1);
    - fragments are smaller compilation units with more prologues;
    - Cranelift cannot optimize across Scheme call boundaries. That is mostly moot, since values cross them in memory anyway.
- **S2 – native call/ret with VM frames (Sparkplug style).**
  - The same VM frames, but Scheme calls are native `call`s, so returns use the return-stack buffer.
  - The GC still sees only VM state.
  - It needs:
    - a way to discard native JIT frames when a continuation escapes: either status-code checks after each call, or Cranelift `try_call` at the entry trampoline plus the internal unwinder (§2.5);
    - resume entries, or tier-down, for frames reinstated mid-function;
    - a large reserved native stack or a depth cap (§5).
- **S3 – optimizing tier, later.** Values stay in machine registers. Use user stack maps only for code proven not to capture continuations, or add deoptimization metadata (debug tags). Defer it.

[I] S1 is simplest for the GC and for continuations. Whether S2's return prediction outweighs S1's simplicity on Apple M-series needs a prototype measurement (open question).

**Caching values within a fragment.** A template-style baseline can still keep a VM register's value in an SSA variable *between* safepoints. The rule: flush dirty cached values before any runtime call, and reload after it. Tag VM-register loads and stores with a `vm-regs` alias region so Cranelift forwards them between safepoints; this needs `opt_level=speed`. A runtime call is a full memory clobber, which is exactly the semantics a moving GC needs.

### 6.2 Requirements on the new heap and GC [I unless marked]

1. **References must be cheap to dereference in machine code.**
   - Today `car` is `pairs.as_ptr() + ((v >> 3) << 4)` (shift, shift, add, and a base that can move on any `push`), then a load.
   - With raw tagged addresses (8-byte aligned objects, low tag 011 for pairs) it is one load at offset −3: `load.i64 v-3`.
   - If 32-bit compressed fields are wanted, use one reserved region with a fixed base (Wasmtime §2.3, V8 pointer compression's base register; V8 reports "V8 heap size up to 43%" smaller [V], https://v8.dev/blog/pointer-compression). The base then becomes a `readonly`/`can_move` load or the pinned register. Note that compressing to 32 bits conflicts with 61-bit fixnums in heap slots.
2. **Bases never move.** Reserve virtual address space up front for the heap spaces and for the VM register stack, and commit lazily. JIT code can then hoist bases out of loops, exactly as Wasmtime does when `memory_may_move` is false [V §2.3]. Growable `Vec` arenas force a reload after every allocation or call and block LICM.
3. **One allocation fast path.** A bump `alloc_ptr` and `alloc_limit` in a `#[repr(C)]` vmctx: load, add, compare, cold call (Wasmtime copying §2.3; OCaml). The limit word doubles as the GC-request and interrupt flag (§4). A semi-space or nursery is the natural partner; per-type free lists are possible but cost more instructions and more state.
4. **Every runtime call is a safepoint, and collection happens only there.** This keeps the existing "GC never runs while Rust holds values" rule, now enforced structurally.
   - The JIT-to-runtime wrapper runs a pending collection **after** the primitive's Rust frame has returned and **before** returning to JIT code. At that point all live Scheme values are in VM memory.
   - Inline-allocation slow paths can collect directly, as Wasmtime's `gc_alloc_raw` does [V].
   - Rust code that must hold values across a collecting call uses handles or root scopes, the Wasmtime `Rooted` indirection model [V §2.2].
5. **No read barriers.** Patina is single-threaded with stop-the-world collection. Load barriers (ZGC/Shenandoah) buy concurrency Patina does not need, and would add a check to every `car`, `cdr`, `vector-ref` and closure-slot load. Wasmtime's DRC read barrier is part of why copying became the default [V/I].
6. **Write barriers designed for an inline fast path.** For generational or incremental collection, emit only a cheap filter inline. Use either a card mark (`card_table[addr >> k] = 1`, which needs address-based objects) or an inline sequential-store-buffer bump. Call the runtime only when the buffer is full. A barrier slow-path *call* is a Cranelift safepoint (§2.1), and `preserve_all` can make it cheap at the call site. Stores into freshly allocated objects take no barrier.
7. **Object layout readable at fixed offsets.** `#[repr(C)]` headers (type or size word, inline pointer-map bits as Wasmtime does for its 7-8% [V]) instead of a 72-B `HeapObjectData` enum with `Rc` payloads. JIT code cannot read Rust enum layouts or touch `RefCell`/`Rc`.
8. **No movable addresses in machine code.**
   - Symbols, global cells, quoted constants and code objects go in an immortal or non-moving space, or are loaded from the code object's constant table (one load off a register).
   - Do not patch code on GC. On macOS arm64 every patch toggles W^X, and `cranelift-jit` uses `mprotect` [V §2.7].
9. **Globals in heap cells.** `LoadGlobal` should become `load [cell]`, with the cell address taken from the constant table, instead of `frame_globals` (a heap borrow plus an `Rc` clone). Under a moving or generational GC, `StoreGlobal` is then an ordinary barriered store.
10. **Frames are GC-plain data.** Replace `CallFrame.code: Rc<CodeObject>` (refcount writes per call) with a raw pointer to an immortal or GC-managed code object. `closure` stays a plain reference the GC can update in place. Code lifetime: unregister and free JIT code only when no frame or continuation snapshot can reference it (an epoch or zombie list, as OCaml does).
11. **Derived pointers never cross safepoints.** Interior pointers (field addresses, `obj+8`) are recomputed from the reloaded reference after any call. This was the source of Cranelift's old miscompiles under moving GC [V §2.1].
12. **Rust panics never unwind through JIT frames.** Catch them at the runtime-call boundary. Cranelift frames carry no DWARF registration by default, and Wasmtime itself dropped `setjmp`/`longjmp` for EH [V].

### 6.3 Sketches (CLIF-ish) [I]

```
; car with raw tagged pointers (pair tag = 3)
t   = band_imm v, 7
ok  = icmp_imm eq t, 3
brif ok, fast, slow_cold        ; slow: call runtime (type error / boxed pair)
fast: car = load.i64 notrap aligned region=heap v-3

; cons: bump allocation; vmctx = first argument (Wasmtime style) or the pinned reg
p   = load.i64 region=vmctx vmctx+ALLOC_PTR
lim = load.i64 region=vmctx vmctx+ALLOC_LIMIT     ; 0 => GC/interrupt requested
np  = iadd_imm p, 16
brif (icmp ule np, lim), fast, slow_cold
fast: store a,[p]; store d,[p+8]; store np -> vmctx+ALLOC_PTR; r = bor_imm p, 3
slow_cold: store pc_const -> frame.pc; flush dirty VM regs
           r = call gc_alloc_pair(vmctx, a_reg_idx, d_reg_idx) ; operands are re-read from VM regs after any GC
           reload cached VM regs

; S1 Scheme call (non-tail): callee fn in r_f, args already in regs[new_base..]
store {code, pc=k+1, base, closure, ret_reg} -> frames[top]   ; or mRA = fragment k+1
brif (icmp ugt new_top, reg_limit), grow_or_interrupt_cold, go
go: return_call_indirect sig_tail, entry(r_f), (vmctx)
```

### 6.4 Choices that would hurt the JIT [I]

- Keeping **per-type growable `Vec` arenas with u32 indices**: multi-instruction address computation, bases reloaded after every allocation or call, and no single address space for card marking or nursery range tests.
- **`Rc<RefCell<Heap>>` on hot paths**: JIT code must call into Rust for every heap touch.
- **Read barriers** (load-barrier concurrent GC) or **deferred RC**: DRC needs both read and write barriers and leaks cycles. Wasmtime moved off it [V].
- **Precise native-stack roots in the baseline**: spills at every call, a frame walker and code registry, and continuations that need deoptimization.
- **Conservative stack scanning with index encoding**: high false retention and pinning of arbitrary arena slots.
- **GC at arbitrary instructions** (asynchronous preemption, as Go 1.14 does with signals): needs per-instruction maps that Cranelift cannot express. Cranelift safepoints are calls only [V].
- **Embedding object addresses in code, or patching code on GC or IC updates** (W^X on macOS arm64).
- **Relying on Cranelift `stack_switch`** for continuations or generators: x64 only, one-shot, a 2 MiB stack each in Wasmtime, never freed there [V].
- **Native-call frames without a depth plan**: the current VM supports 10M-deep recursion [M].

---

## 7. Verified and inferred, and open questions

**Verified from primary sources:**
- the Cranelift APIs and their limits (§2);
- Wasmtime's collectors, barriers, allocation fast path, root tracing, rooting design and release timeline;
- the `stack_switch` restrictions and arm64 absence;
- the pinned-register identity;
- the Guile, Sparkplug, LuaJIT and Chez frame and return mechanisms;
- the yieldpoint overhead numbers;
- the Patina line references.

**Measured here:** Patina deep-recursion depth and RSS; structure sizes from the sibling probe.

**Inferred:** the S1/S2/S3 analysis, the costs in §3.5, and the guidance in §6.

**Open questions:**
1. S1 vs S2 on Apple M-series: return-stack-buffer vs indirect-predictor cost for deep Scheme recursion and mutual recursion, and Cranelift per-fragment prologue cost. This needs a prototype of about 20 bytecodes.
2. Does Patina want asynchronous interrupts (Ctrl-C, timers, a sampling profiler) inside JIT code? If not, GC needs no loop polls at all (§4).
3. Should `call/cc` move to a segmented VM stack (Gambit/Chez-style O(1) capture, see sibling report) before the JIT? Both S1 and S2 inherit whatever the VM does. Today capture is O(depth) (`capture_full` clones the whole register `Vec`).
4. Value encoding: keep 61-bit fixnums with low-tag raw pointers, or move to NaN-boxing (`PRD/VM_OPTIMIZATION_ROADMAP.md` lists NaN-boxed floats)? This changes the JIT's tag checks and how the GC identifies references in VM slots.
5. Code-object lifetime: GC-managed vs immortal vs epoch-freed, given that continuation snapshots can hold frames referencing code indefinitely.
6. If S2 is chosen: status-code propagation vs Cranelift EH with `wasmtime-internal-unwinder`, whose API is unstable and internal.
7. If an optimizing tier (S3) is pursued: user stack maps must be obtained outside `cranelift-jit` (§2.1), and deoptimization metadata built from debug tags. Both are unproven outside Wasmtime.
