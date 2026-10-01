# JIT readiness: the Patina VM as a Cranelift target, and what the GC must promise a JIT

Scope: survey of Patina's JIT plans and of the register VM as a compilation target, ending in a numbered **GC <-> JIT contract**. Repo state: `main` at `28a94f8` (2026-09-30). Labels: **[V]** = verified in source, by measurement, or in a primary source; **[I]** = inference or design proposal.

Measurements made for this report (probes in `PRD/study/gc/probes/jit-readiness/`; nothing in the repo modified):
- Type sizes, via a throwaway crate depending on `patina-core`/`patina-vm` by path: `TaggedValue` 8 B, `HeapObjectData` **72 B**, `Instruction` 48 B, `CallFrame` 40 B, `CodeObject` 160 B, `PromptFrame` 48 B, `DynamicWindRecord` 40 B, `VmContinuation` 152 B, pair slot `(TV,TV)` 16 B, `Vec<TV>` header 24 B.
- Dynamic opcode histograms: release `patina --trace` built from HEAD into a separate target dir, run on the repo's bench programs (§2.3).
- Deep recursion probe (§2.8).

---

## 1. What the existing plans say

| Doc | What it says about a JIT | GC content |
|---|---|---|
| `PRD/VM_OPTIMIZATION_ROADMAP.md` §10 (lines 134-142) | "Baseline JIT compiling hot bytecode to native code. Potentially tiered: interpreter -> baseline JIT -> optimizing JIT." Ranked P10 (last), "Enormous engineering effort. Long-term goal." Its prerequisites appear only implicitly: §2 flat encoding, §6 threaded dispatch, §9 stack slicing. | §3 GC "Done", with generational "later". Nothing on barriers, safepoints or stack maps for compiled code. |
| `PRD/TRACK_P_PERFORMANCE_PRD.md` §3 (line 430) | JIT is a **non-goal** of Track P: "If revisited, the readable match-based loop must remain as a documented reference path." | P6 points at GC_DESIGN. §1.6/§1.8 profiles: dispatch is 66-76% of samples; register-window `memset` 2-7.5%; alloc+GC 2.6-3.1% on deriv/nboyer. |
| `PRD/phase2/reference/01_META_TRACING.md` | A PyPy/LuaJIT-style tracing JIT on hot tail-recursive loops. Cranelift is the "recommended" backend. Guards, and deopt that restores `vm.registers`. Written for an older VM model (`Opcode::Add(dst,src1,src2)`, `Value`). | **None.** It never mentions allocation inside traces, barriers, GC during a trace, or roots held in native registers. This is the main gap in the existing plans. |
| `PRD/phase2/reference/ARCHITECTURE_LESSONS.md`, `VM_BACKEND_DESIGN.md` | Historical. They predate the register VM (a stack-machine sketch; a "rust-gc" recommendation at ARCHITECTURE_LESSONS §3). Both are superseded. | Superseded |
| `PRD/future/TREE_WALKER_HOOK_SYSTEM.md` §10/§10.1 (lines 591-645) | "Hooks attached ⇒ don't tier up." Events that funnel through runtime helpers (`Define`, `Raise`, `Apply`/`Return`) can keep firing from JIT code; per-form `Step` pins the tier. | "**Safepoint metadata is designed once.** A Cranelift tier needs GC safepoints and stackmaps regardless … Design the stackmap story for GC; do not promise it for debugging." |
| `PRD/future/GENERAL_TAIL_CALL_OPTIMIZATION.md` | Phase-1 tree-walker material (`EvalResult::TailCallPrimitive`). Obsolete for the VM, which has had a `TailCall` ISA since Phase 2A. | — |
| `crates/patina-vm/src/runtime/control.rs` module doc | **The only place that anticipates a compiled tier concretely [V]:** "A compiled driver must use the same guard/safe-point discipline and the existing `VmState` root provider, and publish all live Scheme values before servicing a safe point" (lines 27-30). "A compiled tier must participate in this protocol, not catch the sentinel and continue the native computation that was abandoned" (81-82). "Native Rust stack frames are not captured" (104). | Rooting and deferral rules (lines 23-33) |
| `docs/GC_DESIGN.md` §3.4, `PRD/ARCHIVE/GC_STAGE5_PRD.md` | — | Moving GC "ruled out permanently" (raw indices escape). Generational is planned as non-moving with sticky mark bits and barriers on `set-car!`/`set-cdr!`/`vector-set!`/`MutableCell`. "the barrier … taxes the mutator's hottest stores". |

`git grep -i "cranelift|\bjit\b"` finds nothing else beyond archived notes. **There is no concrete baseline-JIT design in the repo [V].**

---

## 2. The VM as a JIT target (verified facts)

### 2.1 Machine model
- One growable register file, `registers: Vec<TaggedValue>`. Frames are kept separately in `frames: Vec<CallFrame>`. Prompts, winds and handlers make three more stacks. All five sit in `ExecutionState` (`runtime/execution_state.rs:17-23`). [V]
- `CallFrame { pc: usize, register_base: usize, num_regs: u16, closure: Option<HeapIndex>, return_reg: u16, code: Rc<CodeObject> }` (`types/mod.rs:41-60`), 40 B. `return_reg` lives in the **callee** frame and names a register in the caller's window. [V]
- Call: `push_frame` does `registers.resize(base+num_regs, NULL)` and pushes the frame (`execution_state.rs:55-74`). Return: `pop_frame` truncates the file (`:78-82`). **Growing the file can relocate the whole register array.** The same happens at `tail_replace` (`:106`) and `scratch_return_reg` (`:158`). [V]
- Register windows are small. Mean/max `num_regs` per code object: fib 6/13, tak 8/17, nqueens 10.4/22, deriv 11.1/48. [V, `--dump`]

### 2.2 Instruction encoding
- `enum Instruction` has 56 variants (`types/instruction.rs:77-590`). Size is asserted ≤ 48 B (`:595`). Registers are `u16`, jump targets are `usize` instruction indices, and call arguments are a `Vec<Reg>`. There is no binary encoding. Dispatch is `match *instr` in `dispatch_one_instruction` (`vm_state.rs:1397`). [V]
- **Codegen emits forward jumps only [V].** Every jump is emitted as a placeholder and patched to a later pc (`pass5_codegen.rs:68-87`, `:767-777`, `:1059-1062`). Loops are therefore always calls or tail calls. The only "go round again" constructs are runtime stubs (`ResumeForce`, `ResumePrimitive`), which restart their own frame at pc 0.
- Never emitted by the compiler [V, grep]: `StoreClosure`, `AbortToPrompt`, `CaptureComposable`, `InvokeContinuation` (dead, or reachable only through runtime paths), plus `Resume*`, which appears only inside runtime-built stubs (VM_ISA §4.8).
- Inline primitive opcodes (`Add`…`VectorSet`, the `*Imm` forms, `TestJumpUnless`) each carry `(func_id, name)` and check `is_primitive_shadowed` first. On a miss they fall back to `exec_call_primitive` (`inline_primitive!`, `vm_state.rs:1374-1393`; `exec_call_primitive`, `control.rs:3458`).

### 2.3 Hot instructions (measured)
Dynamic counts of dispatched VM instructions, grouped by category, in % of all dispatches. "call" = Call/TailCall/Return/Apply. "cl+cell rd" = LoadClosure+ReadCell. "alloc" = Cons/MakeClosure/AllocCell. "heap store" = WriteCell/VectorSet.

| workload (dispatches) | call | branch | move/imm/const | global | cl+cell rd | inline prim | alloc | heap store | CallPrim |
|---|---|---|---|---|---|---|---|---|---|
| fib 20 (142k) | 30.8 | 15.4 | 0 | 15.4 | 0 | 38.5 | 0 | 0 | 0 |
| tak 18 12 6 (445k) | 25.0 | 14.3 | 21.4 | 14.3 | 0 | 25.0 | 0 | 0 | 0 |
| sum 5000 (35k) | 14.3 | 14.3 | 0 | 0 | 28.6 | 42.8 | 0 | 0 | 0 |
| nqueens 8 (377k) | 10.6 | 18.3 | 14.1 | 0 | 14.2 | 40.2 | 2.0 | 0.0 | 0.5 |
| primes 500 (79k) | 14.5 | 14.0 | 22.2 | 0.8 | 13.8 | 20.2 | 7.2 | 0.1 | 7.2 |
| deriv ×200 (79k) | 22.1 | 14.5 | 20.8 | 3.8 | 11.7 | 15.5 | 7.9 | 1.0 | 2.8 |
| map 1000 (21k) | 28.6 | 9.5 | 9.5 | 4.8 | 14.3 | 23.8 | 9.5 | 0 | 0 |
| **nboyer 0 (9.69M)** | 16.4 | 15.6 | 11.8 | 0.0 | **27.7** | 21.3 | 3.2 | **2.2** | 1.8 |

Top single opcodes in nboyer: LoadClosure 16.0%, TestJumpUnless 12.6, ReadCell 11.7, Car 9.0, Return 6.0, Call 6.0, Cdr 5.6, Move 5.1, LoadImm 5.0, PairP 4.8, TailCall 4.5, Cons 2.6, WriteCell 2.2, CallPrim 1.8.

What a JIT and the GC should take from this:
1. Calls and returns are 10-31% of dispatches, and Track P's profiles put call setup at about 7% of samples. The **call sequence** (closure test, code lookup, frame push) is the JIT's biggest target, and it is where safepoints and frame layout live.
2. **Boxed captured variables are hot.** LoadClosure+ReadCell is 28% of nboyer and 29% of sum. Each `ReadCell` goes through a 72-byte `HeapObjectData::MutableCell(RefCell<TV>)` behind `heap.borrow()` (`heap/mod.rs:1253-1290`). `WriteCell` is the dominant heap-store (barrier) site at 2.2% of nboyer. Most of these are `letrec*` internal defines, boxed because they are captured (VM_COMPILER §10.3).
3. Allocation runs at 2-10% of dispatches. Cons and MakeClosure are the inline-allocation candidates.
4. Barrier-site stores are about 1-2% of dispatches, roughly 10× rarer than heap loads. This favors a store-barrier-only design over read barriers.

Caveats: histograms count VM dispatches only, not work inside Rust primitives or control primitives (`call/cc` is a `Call` to an intercepted procedure). The ctak trace (2.2k dispatches) was too short to rank.

### 2.4 Calling convention [V]
- `Call` (`vm_state.rs:1580-1599`). `heap.borrow().get_vm_closure_code_id(func)` checks `is_object()` and then matches the 72-byte enum, because **compiled closures carry `TAG_OBJECT`, not `TAG_CLOSURE`**. `call_closure_from_regs` (`control.rs:169-208`) then looks up `code_object(id)` (Vec slot plus generation check plus an `Rc` clone), checks arity, pushes the frame (resize and zero-fill), and copies the args into callee r0..rn. For a variadic callee it conses the rest list directly from the caller's registers. Any non-closure callee collects its args into a `Vec` and goes through `call_value_with_probe`, whose probe order is control primitive → primitive → parameter → continuation (VM_RUNTIME §4.5).
- `Return` (`:1667-1678`) pops the frame, writes the caller's `return_reg`, and runs `pop_resolved_extents` (the prompt and handler sweep by depth).
- `CallPrimitive` pushes no frame. The handler ABI is `fn(&SharedHeap, &[TaggedValue]) -> Result<TV, EvalError>` (`patina-primitives/src/registry.rs:14`). Higher-order and resumable primitives use `ApplyContext` and `Step::Call`/`Step::Eval`, which push `resume_stub`. A JIT can call heap-tier handlers directly through an `extern "C"` shim, because the Rust ABI is unstable.
- `TailCall` (`:1601-1640`) stages args through a 16-slot stack buffer. For a self-call it reuses the window and resets `pc=0` (`self_tail_call`, `control.rs:3428`). For a different callee, `tail_replace` grows the window. After a shadow-bit deopt, a tail-position primitive site keeps proper tail calls by reading the next instruction (P8.2, `control.rs:3471-3492`).

### 2.5 Continuations [V]
- `call/cc` (`control.rs:635-662`) does `capture_full`: it **clones the entire frame stack and the entire register file**, plus the wind, prompt and handler stacks (`execution_state.rs:239-251`). That is O(stack depth) per capture. `alloc_vm_continuation` retires dead temporaries in the copy, then stores it in a **weak side table** keyed by a heap-minted u64 id (`vm_state.rs:635-642`, `gc_roots.rs:117-139`).
- Reinstatement replaces all five components (`restore`, `execution_state.rs:255-261`). It travels the winds one thunk at a time through `ResumeWindJump` stub frames, so "the rest of the jump" is a pc, not Rust state.
- A delimited capture copies a frame slice and relocates `register_base` and three depths when invoked (`append_delimited`, `:319-409`).
- Escapes are signalled by `park_escape`/`park_transfer`, which set `pending_escape`/`pending_transfer` and return `VmError::ContinuationEscape`. The owning loop checks this **before** classifying errors (`control.rs:56-84`). Rust callbacks sit behind `across_reentry` ids (`control.rs:2169-2229`).
- Because the dispatch loop is flat, non-tail recursion uses no native stack. `(bl 2000000)` building a 2M-element list by non-tail recursion ran in **0.33 s at 394 MB RSS** [V, measured]. `VmError::StackOverflow` is never constructed (`error.rs:58` is its only definition).

### 2.6 Today's GC interface [V]
- **Allocation never collects.** `note_alloc` raises `gc_pending: Rc<Cell<bool>>` when the threshold is crossed (`heap/mod.rs:570-584`). Collection happens only at the safe point at the top of **every dispatched instruction** of the outermost loop (`vm_state.rs:1197-1208`, `maybe_collect` at `:1287-1309`). Nested loops hold a `GcDeferGuard` and never collect (`:1151-1154`).
- Roots (`gc_roots.rs:71-115`): the whole register file (after `retire_registers` clears dead temporaries using per-pc bitsets), `scratch_args`, `pending_escape`, each frame's `closure: HeapIndex`, the constants of every loaded `CodeObject`, the globals `Environment` graph, prompts, winds, handlers, and tracer snapshots. Continuations are weak, reached through `trace_weak_ids`/`sweep_weak`.
- **Per-pc register liveness maps already exist.** `CodeObject::register_roots: Option<Vec<Vec<u64>>>` holds "bitsets of possible register roots before each instruction" (`code_object.rs:152-158`; VM_COMPILER §10.4, #423). For a suspended frame, `frame.pc` is the return point, so these are **return-point stack maps** for VM windows: the same idea as Chez's `rp-header` livemask and Gambit's frame-descriptor gcmap.
- **Code lifetime depends on sweep enumerating dead closures.** `live_closures` counts up at `MakeClosure` and down when sweep reports a freed closure's code id (`take_gc_freed_closure_code_ids`; `vm_state.rs:533-573`). A copying nursery never visits dead objects, so this mechanism cannot survive a move to evacuation [I].

### 2.7 Representation obstacles to inline code [V]
- `SharedHeap = Rc<RefCell<Heap>>` (`heap/mod.rs:51`). Every inline opcode takes `state.heap.borrow()`/`borrow_mut()`, a flag check and a potential panic.
- Arenas are Rust `Vec`s: pairs `Vec<(TV,TV)>`, vectors `Vec<Vec<TV>>` (two indirections), strings `Vec<Vec<char>>`, objects `Vec<HeapObjectData>` (`heap/mod.rs:304-316`). An index-to-address step needs the arena base, which moves when the arena grows.
- `HeapObjectData` is a 72-byte Rust enum with `Rc`, `RefCell` and `Vec` payloads. Its discriminant layout is unspecified, so JIT code cannot test it safely.
- **`TAG_CLOSURE` (0b110) is never constructed in production [V].** `TaggedValue::closure(` appears only in `tagged_value.rs` tests, even though `tree-walker/eval/application.rs:46` claims "CPS lambdas use this tag". One of the eight primary tags is free.
- Fixnum tag is 0, but `fixnum_add` untags, adds and retags (`tagged_value.rs:165-175`). A JIT can do `iadd` with the overflow flag directly on the tagged words.
- `LoadGlobal` calls `frame_globals` → `heap.borrow().get_vm_closure_globals()` → an `Rc<Environment>` clone on every execution (`vm_state.rs:1357-1365`). It then probes `GlobalCacheEntry` by `env_id` and reads `Bindings.slots` (`SmallVec` in a `RefCell`, plus a `FORWARDED` import indirection; `environment.rs:208-217, 596-632`). No binding has a stable address.
- `value_buffer` is gone. `values` allocates a `#<values>` heap object (`control.rs:696-705`), so VM_ISA §4.6/§8 and VM_RUNTIME §5.4/§6 are stale on this point [V].

---

## 3. What a baseline Cranelift JIT would compile to [I]

**Recommended shape: a frame-compatible template JIT**, as in V8 Sparkplug and the Guile 3 JIT. One Cranelift function per `CodeObject`, compiled from bytecode in one pass. It operates on the **same VM register window in memory**, keeps `CallFrame` as the activation record, and calls shared runtime helpers for anything complex.
- Sparkplug keeps Ignition's frame layout so that "as far as they're concerned, all they have is an interpreter frame", and can "swap between the interpreter and Sparkplug code with almost zero frame translation overhead" (https://v8.dev/blog/sparkplug).
- Guile frames always hold a bytecode "virtual return address (vRA)". The "machine return address (mRA) is only present when a call is made from a function with machine code" (Guile manual, Stack Layout).

Signature, sketched: `extern "C" fn(vm: *mut VmCtx, base: *mut TV, entry: u32) -> Status`. Here `entry` selects pc 0 or a return-point resume block through a `br_table`. Lowering:

| Bytecode | Native lowering | GC touchpoint |
|---|---|---|
| Move/LoadImm | load/store `base[r]` (SSA-cached within a block) | none |
| LoadConst | load from code's constant vector (not an immediate) | constant may move (R24) |
| Add/Sub/AddImm… | `(a\|b)&7==0`; `sadd_overflow` on tagged words; cold helper on miss | helper may allocate a bignum (not a safepoint, R8) |
| Lt/NumEq/TestJumpUnless | tag test + `icmp` + `brif` | none |
| Car/Cdr/PairP/NullP | tag test + one load at a fixed offset | needs stable layout (R2/R3) |
| VectorRef | tag, header-length bounds check, load | R3 |
| **VectorSet, WriteCell** | store + **write barrier** | R11-R13 |
| ReadCell/LoadClosure | 1-2 loads | box/closure layout (R3/R4) |
| **Cons, MakeClosure, AllocCell** | **inline bump allocation**, init stores without a barrier | R7-R10 |
| LoadGlobal/StoreGlobal/Define | load or store through a stable binding cell, plus a shadow check | R5, barrier on store |
| Call (closure) | tag test → code descriptor → arity → push VM frame (bump register-stack top, limit check) → return to the driver, or call the callee's mcode | **safepoint** (R15-R18) |
| TailCall (self) | write args, jump to the loop header (**poll**) | safepoint poll |
| CallPrimitive | heap-tier: direct call to an `extern "C"` shim with `&base[r..]`; HO/resumable: may-GC helper | R18 |
| PushWind/PopWind/CallWithValues, Resume* | helpers; stubs stay interpreted | may-GC |

The fib body in this shape: entry poll → `LtImm`+branch → `Return r0`, or → `LoadGlobal` (cell load) → `SubImm` (tagged add with overflow) → `Call` (push frame for fib, resume block #1) → … `Add` → `Return`. Every Scheme value is live in the window at each call, so **GC needs no native stack maps**.

**Two options for Scheme→Scheme calls.** They have different GC consequences:
- **(A) Trampoline through the driver.** A `Call` pushes the VM frame and returns `Status::Enter`. The driver runs the callee (JIT or interpreter). `Return` pops the frame and re-enters the caller at its resume entry. The native stack stays flat, so continuations, escapes, unbounded recursion and GC all work as they do now. Cost: about two indirect branches per call.
- **(B) Native call with a mirrored VM frame.** Faster (predicted call/ret), but the native stack now holds JIT activations. Reinstating a continuation must abandon them (unwind or longjmp to the driver), and resume blocks are still needed because a continuation restores VM frames, not native ones. Deep recursion now consumes native stack, against today's 2M-deep behaviour (§2.5), unless a depth guard falls back to (A). Chez avoids the problem by keeping its own segmented stack and using jumps, not machine call/ret ("Scheme code does not use the C stack", ChezScheme `IMPLEMENTATION.md` "Functions and Calls").

Option (A) with a self-tail-call fast path inside one function covers the hot loops in §2.3 (sum, vecsum, tak's inner TailCall). (B) can follow once profiles show the trampoline cost.

---

## 4. Prior art that shapes the contract

- **Chez Scheme** (local `~/Project/reference/ChezScheme`).
  - Allocation is inline bump on `%ap`/`%eap`, with a `get-room` slow path (`s/cpnanopass.ss:6745-6781`).
  - Allocation never collects. `maybe_queue_fire_collector` → `S_fire_collector` sets `SOMETHINGPENDING` (`c/alloc.c:207-219`, `c/schsig.c:574-592`), and the collect request runs at the next **event trap check**: decrement `%trap`, call `event` on zero (`s/cpnanopass.ss:3996-4011`). Trap checks are placed at procedure entry and loop headers (`np-place-overflow-and-trap`, `:2964-3143`).
  - The write barrier is store + "remember" into a buffer growing down from `%eap`. It is skipped statically for known immediates and dynamically for fixnums (`s/cpprim.ss:665-723`, `cpnanopass.ss:6810+`).
  - Return points carry `rp-header` frame size plus livemask (`s/cmacros.ss:1758-1784`). Frames live on heap-allocated stack segments, which give O(1) continuation capture (Hieb/Dybvig/Bruggeman 1990, cited in `IMPLEMENTATION.md`).
- **Gambit.** Return points carry frame descriptors with a gcmap (`include/gambit.h.in:9229-9292`, `___IFD(kind,fs,link,gcmap)`).
- **Guile 3 JIT.** Template JIT over the VM stack, with vRA plus an optional mRA. Counters on calls and loop iterations trigger compilation.
- **V8 Sparkplug.** Interpreter-compatible frames; complex operations go to builtins. Main-thread gains of 5-15%.
- **Cranelift 0.136.** User stack maps: "all non-tail call instructions are considered safepoints". The producer must "identify and spill the live GC-managed values". `declare_value_needs_stack_map` values are "spilled to the stack before each safepoint and reloaded afterwards … the stack can be updated to facilitate moving GCs" (docs.rs `cranelift-frontend` `FunctionBuilder`; `cranelift-codegen/src/ir/user_stack_maps.rs`). `CallConv::Tail` + `return_call` gives proper tail calls. Wasmtime moved off regalloc-integrated stack maps because the old design caused misoptimizations and CVEs (fitzgen.com 2024-09-10, "New Stack Maps for Wasmtime and Cranelift").
- **HotSpot.**
  - G1 barriers are a pre-barrier for concurrent marking and a post-barrier for the remembered set. Each "tests whether the barrier is actually needed; if so … calls into the JVM". Since JDK 24 they are expanded late in C2 (JEP 475).
  - Generational ZGC (JEP 439) moves marking and remembered-set work into **store** barriers because "load barriers are often more frequently executed". Stack references are "colorless". The remembered sets are field bitmaps with an "act-once" slow path.
  - C2 loop strip mining polls every `LoopStripMiningIter` (1000) iterations.
  - JEP 376 stack watermarks: the safepoint processes few frames, and "returning to a frame that has not yet been fixed up" takes the poll slow path.
  - nmethod entry barriers heal oops embedded in compiled code lazily.
- **MMTk.** Bindings inline the `BumpPointer {cursor, limit}` fast path at an offset from the mutator, and must sync the cached pointer around `alloc_slow` (docs.mmtk.io porting guide).
- **Whippet** (`api/gc-attrs.h`) is the closest prior art for a GC↔JIT contract written as an API. It exports `gc_inline_allocator_kind` (bump pointer or freelist), `gc_allocator_allocation_pointer_offset`/`limit_offset`, `gc_write_barrier_kind` (none/field/slow) with `gc_write_barrier_field_table_offset`/`fields_per_byte`/`first_bit_pattern`, `gc_safepoint_mechanism` (cooperative mutator or heap flag), and `gc_can_move_objects`/`gc_can_pin_objects`.

---

## 5. GC <-> JIT contract requirements

Each item states the requirement, then **why** (repo evidence or prior art) and today's **status**.

### A. Value encoding and object layout
1. **Frozen tag ABI before any JIT code exists.** Keep: fixnum tag 0, so tagged `iadd`/`isub` with the overflow flag work; `#f` as a single-word compare; one primary tag each for pair and **compiled closure**; a header word for everything else. *Why:* every inline opcode in §3 starts with a tag test. *Status:* tags exist (`tagged_value.rs:76-84`), but closures use `TAG_OBJECT` and `TAG_CLOSURE` is unused. Assign it to `VmClosure`. Any NaN-boxing decision (Roadmap §7) must be made before this freeze.
2. **One-step address computation, no borrow.** A heap reference must become an address either as `v - tag`, or as `base + idx*size` with a `base` that never moves (reserved virtual range, base kept in `VmCtx`). *Why:* JIT code cannot take `RefCell` borrows or reload a `Vec` pointer after every allocation. *Status:* unmet (`Rc<RefCell<Heap>>`, Vec arenas).
3. **`#[repr(C)]` layouts with fixed offsets** for every object the JIT touches: pair, vector (length in header, elements inline), box (`MutableCell` as a 1-field object), closure, flonum, global binding cell, values. No Rust-enum discriminants and no `Rc`/`RefCell`/`Vec` payloads in those objects. *Status:* unmet. `HeapObjectData` is 72 B; `MutableCell(RefCell<TV>)`; `VmClosure{Vec, Rc<Environment>}` (`heap/mod.rs:205-212`).
4. **Closure = header + code descriptor pointer + inline free variables.** The descriptor exposes arity, `num_regs`, and the machine-code entry (null means interpret). *Why:* calls are 10-31% of dispatches, and today each one costs a borrow, an enum match, a code-store lookup and an `Rc` clone. *Status:* unmet.
5. **Stable global binding cells.** Each binding has a GC-traced cell with a stable address. LoadGlobal is one load plus an unbound check; StoreGlobal/Define is a store plus a barrier plus the shadow-bit update. *Why:* globals are 14-15% of fib/tak dispatches, and today each pays `frame_globals` (a heap borrow and an `Rc` clone) plus a `SmallVec` slot read in a `RefCell`. *Status:* unmet. Slot indices are stable but addresses are not, and the import `FORWARDED` indirection must become the cell itself.

### B. Allocation
6. **Inline bump fast path exported as data.** `VmCtx` holds `alloc_ptr`/`alloc_limit` at fixed offsets, as in Chez `%ap/%eap`, MMTk `BumpPointer`, and Whippet `*_offset()`. The JIT inlines add, compare, branch, header store and field stores for fixed small sizes: pair (Cons 2-9.5% of dispatches), box (AllocCell), closure (MakeClosure up to 6%), flonum.
7. **The slow path never collects.** `rt_alloc_slow(vm, bytes, kind)` refills or extends the allocation buffer and raises "collection pending". Collection happens only at safepoints. *Why:* it keeps allocation sites from being GC points, so no stack maps or publishing is needed there, and SSA temporaries such as a half-built list survive. This matches today's invariant (`heap/mod.rs:570-584`; `control.rs:23-25`; P10 relies on it for back-to-front consing) and Chez's queue-fire design. *Open:* what happens at hard heap exhaustion.
8. **Initializing stores are barrier-free.** New objects are born young, or "allocated black" if incremental marking is ever added. Cons, MakeClosure and AllocCell then emit plain stores.
9. **Variable-size and large objects go through helpers** (make-vector, strings, long rest lists). A helper that only allocates is still not a safepoint (item 7).

### C. Write barriers
10. **JIT barrier sites:** `VectorSet`, `WriteCell`, StoreGlobal/Define into heap cells, and any future inline `set-car!`/`set-cdr!`/record setter. Not needed for register-file stores (roots), initializing stores, or stores of immediates (skipped statically, else tested at runtime as Chez does). `StoreClosure` is never emitted. *Frequency:* WriteCell 2.2% of nboyer dispatches; barrier sites are about 1-2% overall.
11. **Barrier shape exported as data,** following Whippet's `gc_write_barrier_kind` and table offset/shift. The kind is none, card, or field-log. The fast path is about 3-5 instructions: value-is-heap-ref test, then card mark or young/log-bit test; the slow path is a cold helper call. Store barriers only, **no read barriers.** Loads (ReadCell, LoadClosure, Car, Cdr) are about 10× more frequent than barrier sites (§2.3), which is the same argument Generational ZGC uses for moving work into store barriers.
12. **One Rust choke point.** Every Rust-side mutation (`set_car`/`set_cdr`, `vector_set`, record fields, parameter value stacks, promises, ephemerons, hashtables, environment stores) applies the same barrier. *Status:* `Heap::vector_slice_mut` hands out `&mut [TaggedValue]` (`heap/mod.rs:816`; 8 primitive call sites plus the `VectorSet` arm). It must become barrier-aware, for example by remembering the whole object after the borrow ends, or be removed.
13. **Fewer barriers through the compiler.** `letrec*` internal defines that are written only by their initializer should not be boxed. That removes `WriteCell`s (barriers) and up to 28% of nboyer dispatches spent on cell and closure reads. This is a compiler item, but it shifts the barrier cost the GC design must budget for.

### D. Safepoints
14. **Poll placement:** at function entry, which is also the loop header for self tail calls, and on every return to the driver. *Why sufficient:* compiler-emitted jumps are forward-only (§2.2), so every loop passes through a call or tail call. This matches Chez's entry/loop trap checks. *Status:* today every dispatched instruction is a safepoint (`vm_state.rs:1204`), which a JIT must not replicate.
15. **Poll cost: one load and one branch on a word in `VmCtx`.** Today that word is `gc_pending: Rc<Cell<bool>>`, hoisted into the loop (`vm_state.rs:215-218`). It may become a counter (Chez `%trap`) to also drive timers, engines or interrupts, or be folded into `alloc_limit=0` for allocating code.
16. **Publish before, reload after.** At every safepoint (poll slow path, may-GC helper, return to driver), every live Scheme value is in its frame's register window, and every cached `base`/heap pointer is reloaded afterwards. *Why:* the GC traces windows, not native frames, and a moving GC (or a relocating register file, item 22) changes addresses. This is exactly the existing rule "publish all live Scheme values before servicing a safe point" (`control.rs:27-30`).
17. **Every helper declares a class.** (a) *leaf*: no GC, no Scheme, no transfer (fixnum-overflow promotion, `values_eq`, `rt_alloc_slow`), so not a safepoint. (b) *may-GC / may-run-Scheme / may-transfer* (`call_value`, higher-order or resumable `CallPrimitive`, raise, call/cc, wind ops), which is a safepoint with publish, reload and escape check. *Status:* implicit in the `control.rs` contract table (lines 36-47); it needs to be explicit and machine-checked, for example as a helper attribute.
18. **Nested activations obey deferral.** JIT code entered under a Rust re-entry boundary (`across_reentry`) runs inside a `GcDeferGuard`. Its poll slow path must consult `is_outermost` (`vm_state.rs:1151-1154`) until GC stage-5 P2 roots boundary temporaries.

### E. Roots and metadata for JIT frames
19. **Baseline JIT frames are VM frames.** Same `CallFrame` and window layout, so `impl GcRoots for VmState` stays unchanged (`gc_roots.rs:71-115`). JIT native frames are transient and own no roots at a safepoint. **No Cranelift stack maps are needed in the baseline tier.**
20. **Per-pc liveness maps are the stack maps.** `register_roots` (#423) describes VM windows for interpreted and JIT frames alike, as Chez's rp-header livemask and Gambit's gcmap do. JIT code must not keep a value in a slot the map retires. The GC must never trace a slot the map excludes, and today it overwrites such slots with `UNSPECIFIED` first (`gc_roots.rs:47-68`). If window zeroing (2-7.5% `memset`, Track P §1.6/§1.8) is dropped for speed, the maps become load-bearing for correctness, not just precision.
21. **Optimizing tier (later).** For values held in SSA or unboxed across calls, use Cranelift user stack maps: `declare_value_needs_stack_map`, with spill and reload at every non-tail call, which are its only safepoints. Walk native frames with frame pointers and look up maps by return address. **No derived (interior) pointers may be live across a safepoint.**
22. **Code liveness is traced, not counted.** Closure→code, frame→code and continuation→code edges must be visible to the collector, and machine code is freed only when its code object is unreachable and no native activation is inside it. *Why:* `live_closures` relies on sweep reporting each dead closure (`vm_state.rs:533-573`), which an evacuating nursery cannot do.
23. **Constants: no movable heap pointers baked into machine code** unless the GC can patch them (HotSpot nmethod oops plus entry barriers). Load from the code object's traced constant vector, or allocate code constants in a pinned or immortal space. The latter also serves GC_STAGE5 P1's immortal-set item.

### F. Register stack
24. **A register stack that never relocates.** Reserve virtual address space and commit on demand with an explicit limit check, or use Chez-style segments. *Why:* JIT code wants `base` in a machine register across a basic block. Today `Vec::resize` can move the file on any call (`execution_state.rs:63,106,158`).
25. **Keep unbounded recursion.** Non-tail recursion must stay limited by heap, not by the native stack (§2.5: 2M deep in 0.33 s). This forbids native recursion per Scheme call without a fallback (§3 option B).
26. **Deep stacks and minor GCs.** If collection becomes generational, scanning a 2M-frame register file on every minor GC is O(depth). Keep a low-water mark of frames touched since the last GC (frames below the top cannot be mutated while suspended), checked on `Return`: a stack watermark (JEP 376) or generational stack scanning. JIT `Return` must then perform the watermark compare, which costs about one compare.

### G. Continuations and control transfer
27. **Capture is a may-GC helper.** Since item 16 has published all state into VM frames, `capture_full` works unchanged on JIT frames (`execution_state.rs:239-251`).
28. **Resume at any return point.** Every JIT function provides an entry for each pc after a Call/Apply/CallWithValues/stub-pushing CallPrimitive. Otherwise the driver interprets that frame until it returns, following Guile's "vRA always valid, mRA optional". JIT code never relies on a native return address after reinstatement, abort, raise, or `exit` travel.
29. **Escapes return immediately.** On an escape status from a helper, JIT code returns to the driver without writing `dst` or running cleanup (the `control.rs:56-84` protocol). The driver alone consumes `pending_escape`.
30. **Continuations become heap objects.** Make them real GC objects, either copied frame vectors or stack segments, instead of weak u64-keyed side tables whose soundness rests on "every store touch confined to one instruction dispatch" (`gc_roots.rs:21-24`). JIT helpers would otherwise inherit that confinement rule, and a moving GC cannot relocate Rc'd side-table payloads.
31. **Cheaper capture (optional).** Capture is O(depth) today. Chez-style segmented stacks make it O(1), but require the GC to scan segments as objects using the frame metadata of items 20 and 22. Design the frame header (code, pc, size) so a segment walker can find maps without the `frames` side vector.

### H. Tiering and invalidation (GC-adjacent)
32. **Tier-up and OSR.** Tier up on counters at entry and self-tail-call. Because frames are shared, OSR is just "enter mcode at the next activation, or at the pc-0 loop header".
33. **Shadow bits.** JIT inline primitive fast paths check the same shadow bits (`is_primitive_shadowed`, `shadowed_controls`) with one load and test, or register dependencies that invalidate code. Code invalidation must not free machine code that is still on a native stack (item 22).
34. **Hooks and tracer pin the interpreter tier** (TREE_WALKER_HOOK_SYSTEM §10.1). The tracer's register snapshots stay roots (`gc_roots.rs:110-114`).
35. **`VmCtx` is the single ABI object.** A `#[repr(C)]` struct pinned for the life of the VM holds: alloc ptr/limit, poll word, card or log table base and shift, register-stack top and limit, code-descriptor table, shadow bitsets, and the escape/status slot. Whippet's `gc-attrs.h` is the model: the GC publishes offsets and kinds, and the JIT emits code from them, so the collector can change (non-moving to moving, card to field log) without rewriting the JIT.

---

## 6. Lessons and constraints for the GC redesign
- The interpreter **already has the right shape for a JIT-friendly GC** [V]: allocation never collects, collection happens at cheap cooperative polls, per-pc liveness maps exist, all Scheme state lives in VM frames, and stubs make "work owed after a call" a pc. Preserve these properties; they are what make Chez's and Guile's designs work.
- Per-frame metadata decides moving vs non-moving more than the JIT does [I]. With frames as the only home of values (item 19), a moving nursery costs the baseline JIT almost nothing (reload after safepoints). The real blockers are the raw-index escapes in GC_DESIGN §3.4 (symbol table `HeapIndex`, raw-bits keyed source maps and eq-hashing, `CallFrame.closure: HeapIndex`, `CompiledMacro` literals, constants), plus sweep-enumeration dependencies such as `live_closures` and `gc_freed_bits`.
- Representation work comes before the JIT: closure tag, `#[repr(C)]` objects, a box object, global cells, and a non-relocating register stack. All of it also speeds up the interpreter, for example by removing `heap.borrow()` from every inline opcode.
- Barriers on the order of 1-2% of dispatches are cheap enough only with a 3-5-instruction inline fast path and **no read barriers**. Measure with interleaved A/B (GC_STAGE5 P3).
- Benchmarks available [V]:
  - Criterion `crates/patina-tests/benches/scheme_benchmarks.rs` covers 41 workloads in `bench_programs/workloads.json` (40 criterion, 18 quick, 25 compare). Run with `scripts/run_benchmarks.sh [--quick] --backend vm` and `scripts/bench_compare.sh --quick`.
  - Phase lanes include `phases/allocation_gc/list_256`, with a fixed probe of 1000×256 pairs that records collection counts (`benches/scheme_benchmarks.rs:177-268`). VM baseline: 14.9 µs (`benchmark_reports/performance.md`).
  - The external ecraven r7rs-benchmarks sweep is `~/Project/r7rs-benchmarks`, subset `tak ctak deriv diviter divrec nboyer maze slatex compiler matrix` (TRACK_P §1.2).
  - **GC-heavy:** nboyer/sboyer ("GC-active"), deriv and diviter (cons churn), primes, `data/lists/*`, `allocation_gc/list_256`, and ctak plus `continuations/callcc/loop/*` (capture-heavy; ctak was a 4 GB blowup before weak tables, 227 MB after).
  - There is no pause-latency benchmark (performance.md says so explicitly), and the JIT/GC work will need one.

## 7. Open questions
1. Trampolined calls (A) or native calls with mirrored frames (B)? (B) needs native-stack unwinding on every continuation transfer and a depth guard to keep item 25.
2. Freeze the value encoding: low-tag pointers or NaN-boxing (Roadmap §7)? Floats are heap `Real` objects today, which makes float code allocation-heavy.
3. Should environments, and hence globals, become GC heap objects? Item 5 says yes for the JIT. Today they sit outside the heap behind `Rc`, with a GC design (§8) that depends on that.
4. Behaviour on hard allocation failure when the slow path cannot extend (item 7).
5. The tree-walker shares the heap and keeps values in Rust locals and `Rc` CPS continuations. Does a moving collector keep supporting it, or does the tree-walker get a non-moving space, or conservative treatment of its roots?
6. Machine-code memory management: W^X, code-cache eviction, and how it interacts with code-unit release (#338/#352).
7. Threads: if SRFI 18 ever lands, polls become handshakes and allocation buffers become thread-local. The `VmCtx` design (item 35) should not preclude it.
8. Whether `register_roots` should become precise last-use liveness (it is "expression lifetime, not last-use" today, VM_COMPILER §10.4) once the JIT also relies on it.
