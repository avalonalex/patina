# The VM runtime as the GC sees it, and as a JIT will see it

Scope: `crates/patina-vm` at `28a94f8` (after #602, "Encapsulate VM execution state").
Method: read the source; disassembled code with `(vm-compile …)`; measured type sizes in a
throwaway crate (`PRD/study/gc/probes/vm-runtime`, `size_of`); timed small programs and took one `sample` profile, using a release
build of `patina` at `28a94f8`, on macOS 27.2 arm64 (12 logical CPUs; the CPU model was not
reported). Tags: **[V]** means verified in source or by measurement, **[I]** means inference.

---

## 0. Executive summary

1. **[V] The VM keeps Scheme frames as data, not on the native stack.** `ExecutionState` holds five
   `Vec`s: registers, frames, prompts, winds, and handlers (`runtime/execution_state.rs:17-23`). A
   Scheme call pushes a `CallFrame` (40 B) and resizes a flat register file. It never recurses in
   Rust. A 10,000,000-deep non-tail recursion runs (0.50 s, 1.37 GB RSS, about 137 B/frame). There
   is no stack limit: `VmError::StackOverflow` exists but nothing raises it.
2. **[V] Root discovery is simple and precise for the VM stack.** At a safe point, the GC traces
   the whole register file as one slice (`gc_roots.rs:75`). First it overwrites dead temporaries with
   `UNSPECIFIED`, using per-pc may-root bitsets that the compiler emits (#423, `gc_roots.rs:47-68`,
   `pass5_codegen.rs:223-284`). Because `TaggedValue`s carry their own tags, a liveness bit is
   enough; no pointer-vs-non-pointer map is needed. Guile uses the same "dead slot → unspecified"
   technique.
3. **[V] The safe point is one `Cell<bool>` load at the top of every dispatched instruction**
   (`vm_state.rs:1204-1209`). Allocation never collects: it only sets the flag
   (`heap/mod.rs:571-585`). Collection runs only in the outermost dispatch loop. Every nested loop
   defers through `GcDeferGuard` (`vm_state.rs:1151`).
4. **[V] Nearly every heap-touching instruction borrows `Rc<RefCell<Heap>>`.** That includes
   `LoadClosure`, `ReadCell`, the closure probe on every `Call`/`TailCall`, `Cons`, `Car`, `Eq`,
   `VectorRef`, `MakeClosure`, and a closure frame's `LoadGlobal`. A named-let loop iteration costs
   seven instructions and four heap borrows, plus one inner `RefCell` borrow for the cell (§5). On
   that loop the measured cost is about 31 ns per iteration, or about 4.4 ns per instruction.
5. **[V] Continuations copy the stack into off-heap side tables.** `call/cc` clones all five
   components into a `VmContinuation` held in `FxHashMap<u64, Rc<VmContinuation>>`
   (`vm_state.rs:196`). The heap holds only an opaque `VmContinuationRef(u64)`, and an
   ephemeron-style fixpoint makes the table weak. **Measured:** about 3-4 µs and about 17 KB per
   capture of a 100-frame stack. The trigger counts *objects*, not bytes, so these payloads are
   invisible to it. A program that holds no continuations still plateaus at **590 MB RSS**.
6. **[V] Closures cost two allocations.** A `VmClosure` takes a 72-byte enum slot in `objects`,
   plus a separately `malloc`ed `Vec<TaggedValue>` for its free variables, plus an
   `Rc<Environment>` clone (`heap/mod.rs:205-213`). In a closure-allocating loop, **12.4% of
   samples were in malloc/free**. Closures are tagged `TAG_OBJECT`. The dedicated `TAG_CLOSURE`
   (0b110) is never constructed, so "is this a closure?" needs a memory load plus an enum
   discriminant check.
7. **[V] The GC must notify the VM when a closure dies.** Code objects are freed when their units
   are released. That depends on `live_closures` counters, which are decremented from
   `gc_freed_closure_code_ids`, a list that sweep fills for each dead closure (`vm_state.rs:542-575`,
   `heap/gc.rs:905`). A copying or generational collector that never visits dead objects breaks this
   protocol.
8. **[I] The biggest JIT/GC decision this exposes:** keep the "frames are VM data" model, which
   suits a Sparkplug-style baseline JIT that preserves the frame layout, or move to native frames
   (needs Cranelift stack maps plus Chez-style stack-segment continuations). Today's continuation,
   `dynamic-wind`, raise and resumable-primitive machinery all assume that the remaining work is a
   **pc in a VM frame** (`control.rs:91-104`). A baseline JIT that keeps that layout inherits all of
   it unchanged.

---

## 1. `VmState` anatomy (what holds values, and how the GC sees it)

`size_of::<VmState>() = 832` B **[V]**. Fields (`runtime/vm_state.rs:44-219`):

| Field | Holds `TaggedValue`s? | GC treatment | JIT relevance |
|---|---|---|---|
| `execution: ExecutionState` | registers, frames' closure, prompts, winds, handlers | strong roots (`gc_roots.rs:75-105`) | the "stack" a JIT reads and writes |
| `pending_escape: Option<TV>` | yes | root (`:85`) | escape protocol state |
| `pending_transfer`, `reentry: Vec<u64>`, `next_reentry`, `reentry_kept` | no | – | re-entry boundary bookkeeping (§8) |
| `code_store: Vec<Rc<CodeObject>>` (+`empty_code`, `free_code_ids`, `code_units`) | `constants` | each loaded code's `constants` traced (`gc_roots.rs:97-99`) | code lookup by `CodeObjectId` slot+generation |
| 9 stub ids (`wind_jump_code` … `resume_codes[5]`) | no | – | runtime-built bytecode stubs (§7.4) |
| `globals: Rc<Environment>` | yes (env bindings) | `visit_env` (`:101`) | top-level code's global env |
| `heap: SharedHeap` = `Rc<RefCell<Heap>>` | – | – | one borrow per heap op |
| `primitive_registry: Rc<PrimitiveRegistry>` | no | – | primitive dispatch by index |
| `shadowed_primitives: Vec<u64>`, `shadowed_controls: u8` | no | – | deopt guards for inlined primitives (§6) |
| `scratch_args: Vec<TV>` | yes (transiently) | root (`:76`); empty at safe points | arg buffer |
| `continuation_store`, `delimited_continuation_store`: `RefCell<FxHashMap<u64, Rc<…>>>` | yes, off-heap | **weak**: `trace_weak_ids`, `sweep_weak` (`gc_roots.rs:117-139`) | continuation payloads |
| `tracer` | register snapshots | root (`:112-114`) | – |
| `library_registry`, `loader_registry`, `fs` | via `Library` | `LibraryRegistry` is a separate root provider | – |
| `gc: RefCell<GcController>`, `gc_pending: Rc<Cell<bool>>` | – | – | safe-point flag |

**[V]** The top-level backend keeps `VmState` in a `RefCell` and borrows it mutably for each form
(`backend.rs:276-282`). Each top-level form is compiled, loaded as a unit, executed with
`execute`, and released if unused (`backend.rs:272-283`).

---

## 2. Register file and frames

### 2.1 Layout **[V]**
- `registers: Vec<TaggedValue>` is one flat array. Each frame owns the window
  `[register_base, register_base+num_regs)`. `push_frame` does
  `registers.resize(base+num_regs, NULL)`; `pop_frame` truncates to `register_base`
  (`execution_state.rs:55-82`). So **the register file can reallocate on any call**. Every
  `&[TaggedValue]` or raw pointer into it is invalid after a call, and a JIT must reload the base
  pointer after every call or allocation that can grow it.
- `CallFrame` (40 B, `types/mod.rs:41-60`): `pc: usize`, `register_base: usize`, `num_regs: u16`,
  `closure: Option<HeapIndex>` (a **bare u32 index, not a `TaggedValue`**, so it gets a special trace
  path, `gc_roots.rs:154-161`), `return_reg: u16` (the callee records where in the *caller's*
  window the result goes), and `code: Rc<CodeObject>`.
- `Reg = u16`: up to 65,535 registers per frame. `num_regs` is the linear-scan high-water mark
  (`VM_COMPILER.md §9.2`). Parameters are r0..r(n-1), then internal defines, then temporaries.
- Tail calls reuse the window. If the callee needs more registers it grows the window; it never
  shrinks (`execution_state.rs:95-114`). Self tail calls skip the code lookup entirely
  (`control.rs:3428-3441`).
- `dispatch_frame` (`execution_state.rs:134-142`) reads `pc`/`base` and post-increments `pc` in the
  frame **in memory** on every instruction. `cur_code` is a loop-local `Rc<CodeObject>` that is
  re-cloned only when the frame's code changes. That still costs **up to four non-atomic `Rc`
  count updates per call/return pair**: `code_object()` clones, `pop_frame` drops, and the loop
  re-clones `cur_code` in each direction.

### 2.2 Costs of this representation **[V]/[I]**
- **[V]** `registers.resize` with `NULL` fill appears as `memset_pattern16` (2% of samples in the
  closure profile). `push_frame` and `pop_frame` themselves are another 5.6%.
- **[V]** Deep recursion is bounded by memory, not by the native stack. A JIT that puts Scheme
  frames on the native stack would lose this unless it adds segment overflow handling (Chez's
  `S_split_and_resize`, `ChezScheme/c/schsig.c:93`) or a spill-to-heap mechanism.
- **[I]** For a JIT this layout is close to a shadow stack. Values are already in memory at known
  offsets from `base`, so GC root discovery and continuation capture need no native stack walking.
  The price is a load and store per register access, unless the JIT caches values in machine
  registers between safepoints.

---

## 3. Code objects and constants

**[V]** `CodeObject` is 160 B (`types/code_object.rs:127-174`):

- `instructions: Vec<Instruction>`. `Instruction` is a Rust enum of at most **48 B**, asserted at
  `instruction.rs:595`. Operand lists are `Vec<Reg>` (`Call`, `TailCall`, `MakeClosure`,
  `CallPrimitive`), and names are `Symbol = Rc<str>` (`core_expr.rs:7`).
- `constants: Vec<TaggedValue>`. **Only** non-immediates go here. `LoadImmediate` embeds only
  immediates (`pass5_codegen.rs:697-707`) and `*Imm` opcodes embed only fixnums. So
  **instructions never embed heap references**. Every heap literal goes through the pool, which
  makes constant relocation for a moving GC tractable (one `Vec` per code object, or a flat table).
- `register_roots: Option<Vec<Vec<u64>>>` is a **per-pc** may-root bitset (#423), with one heap
  `Vec` per instruction. `None` for runtime stubs (all slots conservative).
- `global_cache: Vec<Cell<GlobalCacheEntry>>` (16 B per pc, or empty if the code has no global
  ops).
- `live_closures: Cell<u32>` counts how many live closures name this code.

**Lifetime [V].** A compilation is loaded as a unit (`load_unit`, `vm_state.rs:430-463`). A unit is
released when no frame or continuation holds its `Rc` (`Rc::strong_count > 1`) and no closure
counts it (`live_closures > 0`) (`vm_state.rs:1003-1009`). Closures name code by
`CodeObjectId(u64)` (low 32 bits = slot, high 32 bits = generation), not by pointer. So a
closure's death reaches the code store only through the GC: sweep records the `code_id` of every
freed `VmClosure` (`heap/gc.rs:905-955`), and `after_collection` decrements the counters and
releases units (`vm_state.rs:542-575`). `eval`'s one-shot closures are retired eagerly
(`retire_vm_closure`, `heap/mod.rs:1351`).

**GC root cost [V]:** `trace_roots` walks every loaded code object's `constants`
(`gc_roots.rs:97-99`). GC_DESIGN §9.5 measured this walk at 57% of root tracing before #338 bounded
the store. It is still a walk over scattered `Rc` allocations.

---

## 4. Closures, free variables, and `MutableCell`

**[V] Representation.** Heap variant `HeapObjectData::VmClosure { code_id: u64, free_vars: Vec<TaggedValue>, globals: Rc<Environment> }` (`heap/mod.rs:205-213`). It lives in the `objects`
arena, whose slot size is `size_of::<HeapObjectData>() = 72` B. The free variables are a second,
malloc'ed buffer. `types::VmClosure` (32 B, `types/mod.rs:74-80`) is a stale compile-time mirror
that the heap does not use.

**[V] Flat closures.** `MakeClosure { free_vars: Vec<Reg> }` snapshots register values
(`vm_state.rs:1538-1553`). Globals are never captured; they are looked up by name. A variable that
is captured **and** mutated anywhere becomes a `MutableCell(RefCell<TaggedValue>)`, also a 72 B
object slot (`pass2_closure.rs:276-290`). **Internal defines are always treated as mutated**
(`pass1_analysis.rs:121-127`), so every recursive local procedure that a lambda captures is boxed.
That includes the **named-let loop variable**:

```
;; (let loop ((i 0) (s 0)) (if (= i n) s (loop (+ i 1) (+ s i))))   — loop body, CodeObject #112
0  LoadClosure  r6 ← closure[1]          ; n           (heap borrow)
1  TestJumpU    r3 ← =(r0, r6) → 5
2  JumpUnless   r3 → 5
5  LoadClosure  r5 ← closure[0]          ; the cell for `loop` (heap borrow)
6  ReadCell     r5 ← *r5                 ; heap borrow + cell RefCell borrow
7  AddImm       r6 ← +(r0, 1)
8  Add          r8 ← +(r1, r0)
9  TailCall     r5(r6, r8)               ; heap borrow for closure code-id probe → self_tail_call
```

A JIT would want letrec/named-let to resolve to a known code object, with no cell and no probe. That
is a compiler change (Waddell-Sarkar-Dybvig "Fixing Letrec"), but it removes a heap object per loop
entry and two borrows per iteration.

**[V] Free-variable access path.** `LoadClosure` reads `frames.last().closure`, borrows the heap,
indexes `objects[idx]`, matches the enum, then indexes `free_vars[slot]`
(`vm_state.rs:1451-1468`, `heap/mod.rs:1378-1390`). That is three dependent loads plus a borrow
check.

---

## 5. Global variable access path

**[V]** `LoadGlobal { dst, name: Rc<str> }` (`vm_state.rs:1492-1506`) runs these steps:
1. `frame_globals(state)` (`vm_state.rs:1357-1364`): if the frame has a closure, it borrows the heap,
   matches `VmClosure`, and **clones its `Rc<Environment>`**; otherwise it clones `state.globals`.
   That is a refcount increment and decrement on every global access, and 2.6% of samples in the
   closure profile.
2. `GlobalCacheEntry::probe(&code.global_cache[pc], &globals, name)` (`code_object.rs:229-238`):
   compares the cached `env_id` with `globals.env_id()`. On a hit it returns the slot. On a miss it
   does `local_slot(name)`, a linear scan when there are 8 or fewer bindings and an FxHashMap
   lookup above that.
3. `globals.slot_value(slot)` (`environment.rs:605-613`): borrows `bindings: RefCell<Bindings>`
   (a SmallVec of `(Rc<str>, TV)`). If the slot holds `TaggedValue::FORWARDED` (an imported
   binding, #406), it calls `shared_value`, which follows an `Owner` link into the exporting
   library's environment and repeats (`environment.rs:655-662`; "about 2 ns more" per the comment).
4. Parent-resolved and alias-resolved names are **never cached**. They take `Environment::get`
   every time.

**[V]** `StoreGlobal`/`Define` also call `mark_if_shadowing_primitive*` (`vm_state.rs:2440-2478`),
which borrows the heap and inspects the old value. That keeps the `CallPrimitive`/inline opcode
deopt bits correct.

**JIT implications [I].** There is no binding *cell* that compiled code could hold a stable
pointer to. Slots live in a growable `SmallVec` behind a `RefCell`, environments are Rust `Rc`
structs outside the GC heap, and imported bindings add an indirection. The usual JIT-friendly shape
is a heap-allocated, GC-traced binding cell: Chez keeps a symbol's top-level value in the symbol
object, and V8 uses `PropertyCell`. With that, `LoadGlobal` becomes one load from a constant cell,
and redefinition invalidates through the cell rather than a per-site `env_id` check. Primitive
inlining guards (`shadowed_primitives`) would become cell-identity guards or code invalidation.

---

## 6. How each instruction touches the heap (dispatch-loop audit)

**[V]** From `dispatch_one_instruction` (`vm_state.rs:1397-2431`). "Borrow" means `heap.borrow()` or
`borrow_mut()` on the `Rc<RefCell<Heap>>`. "Rust alloc" means `malloc` outside the GC heap.

| Instruction | Heap borrows | GC-heap alloc | Rust alloc |
|---|---|---|---|
| `LoadImmediate`, `LoadConst`, `Move`, `Jump*`, `Not`, `NullP`/`PairP`/`VectorP`, arithmetic fast paths | 0 | – | – |
| `LoadClosure` / `StoreClosure` | 1 | – | – |
| `LoadGlobal` / `StoreGlobal` | 1 if closure frame (+ env `RefCell`) | – | – (`Rc` clone) |
| `Define` | 1–2 | – | possibly a new binding |
| `MakeClosure` | 2 (`frame_globals`, `alloc_vm_closure`) | 1 object (72 B) | `Vec` of captured values |
| `Call` | 1 (closure probe) | rest list if variadic | `Vec` args on the non-closure path |
| `TailCall` | 1 | rest list if variadic | `Vec` if more than 16 args or non-closure |
| `Apply`/`TailApply` | 1+ | – | spread `Vec` |
| `Return` | 0 | – | – |
| `Cons` | 1 (`borrow_mut`) | 1 pair (16 B) | – |
| `Car`/`Cdr`/`VectorRef` | 1 | – | – |
| `VectorSet` | 1 (`borrow_mut`) | – | – (**no write barrier**) |
| `Eq` / `TestJumpUnless Eq` | 1 (`values_eq` compares `Rc` identity for `Procedure`/`Record`, `heap/mod.rs:2118-2146`) | – | – |
| `AllocCell` | 1 | 1 object (72 B) | – |
| `ReadCell`/`WriteCell` | 1 + cell `RefCell` | – | – |
| `CallWithValues` | 1 | – | `Vec` of unpacked values |
| `PushWind` | 0 | – | `Rc<[ExceptionHandler]>` if any handlers are installed |
| `CaptureComposable` / `call/cc` / abort | 1–2 | 1 ref object | full or partial stack copies + `Rc` + hash insert |
| `CallPrimitive(Direct)` | primitive-defined | primitive-defined | none at depth 1 (scratch buffer) |
| error routed to a handler | 1 | exception object | `String` message |

Observations:
- **[V] No write barrier exists anywhere.** Mutation sites are `VectorSet`, `WriteCell`,
  `StoreClosure`, `StoreGlobal`/`Define` (env slots), `set-car!`/`set-cdr!`/`vector-set!`/
  `string-set!` in primitives, `Record` field `RefCell`s, parameter value stacks, and promise
  state. A generational or incremental collector needs barriers in the interpreter, in every Rust
  primitive, and in the JIT.
- **[V] `eq?` is not bit identity.** Two distinct heap slots that wrap the same `Rc<Procedure>` or
  `Rc` record payload compare `eq?` (`heap/mod.rs:2129-2141`). A JIT cannot compile `eq?` to a
  single compare until the representation guarantees one heap object per identity.
- **[V] Pairs:** `pairs: Vec<(TV,TV)>`, so `car` is `pairs.ptr + idx*16`. Vectors:
  `vectors: Vec<Vec<TV>>`, so `vector-ref` is two dependent loads plus a bounds check. Arena `Vec`s
  reallocate when they grow, so the **arena base pointers move** even though indices stay valid. A
  JIT must reload `heap.pairs.as_ptr()` after any allocation.

---

## 7. Control machinery

### 7.1 Full continuations (`call/cc`) **[V]**
- Capture is `capture_full` (`execution_state.rs:239-251`). It clones `frames` (one `Rc<CodeObject>`
  increment per frame), `winds`, `prompts`, `handlers`, and the **entire register file**. Then
  `alloc_vm_continuation` (`vm_state.rs:636-643`) clears dead temporaries in the copy, allocates a
  `VmContinuationRef(id)` heap object, and inserts `Rc<VmContinuation>` (152 B header) into
  `continuation_store`. `dst` is cleared before capture to break retention chains: there is a 296 MB
  leak story in a comment (`control.rs:641-654`).
- Invocation is `step_wind_jump` (`control.rs:1201-1308`). It computes one wind step with the shared
  `next_wind_step` policy. If there is a step, it pushes a `wind_jump_stub` frame (registers TARGET,
  VALUE, ENTERING, THUNK_RESULT) and calls the thunk as an ordinary VM call. On arrival it does
  `restore`, which **clones all five vectors back** (`execution_state.rs:255-261`), pops resolved
  prompts, and writes the value into `deliver_reg`. The caller then returns the
  `ContinuationEscape` sentinel with the value parked (`park_escape`, `control.rs:3113`).
- **Measured:** 20,000 captures of a ~100-frame stack cost ~4.2 µs each (0.17 s versus 0.08 s for
  the same program without `call/cc`). Retained memory is **~17.4 KB per capture**: RSS 101 MB at
  5k captures and 362 MB at 20k, then a 590 MB plateau at 60k and 200k. A collection fires only
  after ~65k *allocations* (`DEFAULT_MIN_THRESHOLD = 65_536` or 2×live, `heap/gc.rs:969`), and each
  capture counts as one allocation however large its off-heap payload is.

### 7.2 Delimited continuations **[V]**
`capture_delimited` (`control.rs:2840-2918`) copies the frame and register slice above the prompt,
plus the winds, prompts, and handlers inside it, and records `base_at_capture`, `depth_at_capture`,
`wind_depth_at_capture`, and `handler_depth_at_capture`. `append_delimited`
(`execution_state.rs:319-410`) **relocates** on invoke: it extends the registers, shifts each
appended frame's `register_base`, and re-bases all three prompt depths and the handler depths with
`relocate_depth`. Abort (`abort_to_prompt`, `control.rs:2541-2630`) has an in-place fast path
(`truncate_to_prompt`) when no wind extent intervenes. Otherwise it builds a whole landing
`VmContinuation` and travels to it.

### 7.3 Dynamic-wind, handlers, prompts **[V]**
- Wind record: `WindRecord<ExceptionHandler> { id, before, after, handlers: Rc<[ExceptionHandler]> }`
  (40 B). It is the immutable, shared handler snapshot of its own `dynamic-wind` call
  (`patina-core/src/continuation.rs:173-198`).
- Handlers: `{ handler: TV, stack_depth: usize }` (16 B), popped when the frame depth drops below
  `stack_depth`.
- Prompts: `{ tag, handler, dst, stack_depth, dynamic_wind_depth, exception_handler_depth }` (48 B).
- All three are **depth-indexed into the frame/wind/handler vectors**. They contain no pointers into
  frames, so continuation relocation is integer arithmetic. That suits a design where frames stay
  VM data.

### 7.4 "Remaining work is a pc": runtime stubs **[V]**
Every operation that must finish after a Scheme call is a hand-built bytecode stub frame whose
registers carry the state, so that a continuation captured inside the callee carries the remainder
(`control.rs:91-104`).

| Stub | Instructions | Regs | Purpose |
|---|---|---|---|
| `wind_jump_stub` | `ResumeWindJump` | 4 | step a full jump through wind thunks |
| `invoke_step_stub` | `ResumeComposableInvoke` | 5 | re-enter a delimited capture's extents |
| `value_wind_stub` | Call/PushWind/Call/PopWind/Call/Return | 5 | value-form `dynamic-wind` |
| `value_cwv_stub` | Call/TailCallWithValues | 3 | value-form `call-with-values` |
| `abort_handler_stub` | Call/Return | 4 | call a prompt handler on abort |
| `raise_step_stub` | Call/ResumeRaise/Return | 5 | raise bookkeeping after the handler returns |
| `force_stub` | Call/ResumeForce/Return | 3 | promise forcing, constant space for `delay-force` |
| `resume_stub` ×5 (by argc) | Call or Apply/ResumePrimitive/Return | 8 | resumable Rust primitives (`Step::Call`/`Step::Eval`) |

Stubs are loaded into `code_store` (`runtime_stub`, `control.rs:1418-1447`) and have
`register_roots = None`, so all their slots are conservative. **[I]** For a JIT these are just more
bytecode functions. They need no special treatment if the JIT can compile, or interpret, any code
object and resume at any pc.

### 7.5 Re-entry and nested run loops **[V]**
- Resumable primitives return `Step::Call`/`Step::Eval` and are resumed from `resume_stub`'s frame
  (`control.rs:1823-1994`). `Step::Eval` compiles the datum into a 0-arity closure, `eval_closure`
  (`vm_state.rs:972-986`), that the stub calls. No Rust frame waits across the call.
- Nested Rust-driven loops still exist. `VmApplyContext::apply_proc` → `run_apply_proc` →
  `run_loop_until(depth_before)` (`control.rs:2331-2353`), `eval_expr` → `vm_eval_expr` →
  `execute_nested`, library loading (`vm_evaluate_parsed_library`), and `Step::Eval` expansion
  (which can load libraries) all go through `across_reentry` (`control.rs:2169-2227`). That
  function pushes a boundary id onto `state.reentry`, and continuations record those ids
  (`VmContinuation::reentry: Rc<[u64]>`) to tell an escape from a return. **`apply_proc` call sites
  remain in Rust primitives**: `values.rs:35,48`, `lists.rs:440,587`, `parameters.rs:250,273`,
  `lazy.rs:125`, `io/ports.rs:666`, `io/file.rs:216,263`, plus `registry.rs:112` for the
  synchronous driver of resumables. The VM intercepts `call-with-values` and `force`, and
  `member`/`assoc` are Scheme (`lib/scheme/base/higher_order.scm`), so on the VM these are mostly
  unreachable. `%parameterize-swap!` still reaches `apply_proc` for non-object "parameter-like"
  procedures.
- `VmApplyContext` holds a **raw `*mut VmState`** and dereferences it while a primitive also holds
  `&`-derived state (`control.rs:2130-2133`, `2253-2322`). That is sound only because of
  single-threaded, synchronous use.
- **Every nested loop defers GC** (`GcDeferGuard`, `vm_state.rs:1151`). Their Rust callers hold
  unrooted `TaggedValue`s: argument vectors, `saved_globals`, and `ParsedLibrary.body`. A long
  library body loaded through `eval`, or a long callback, never collects (`GC_DESIGN.md §7`, known
  limitation).

### 7.6 Errors and escapes **[V]**
Errors are `Result<_, VmError>`, a Rust enum with `String` payloads. Continuation transfers use the
same channel: `VmError::ContinuationEscape` with the value parked in `pending_escape`. The driver
checks `pending_escape` **before** classifying an error, because primitive error conversion can wrap
the sentinel (`vm_state.rs:1218-1244`). A catchable error is converted into a heap exception object
and raised through the handler stack *inside the driver loop* (`vm_state.rs:1249-1278`).

---

## 8. GC integration in the VM

- **Safe point [V].** At the top of `run_loop_until_outcome`'s loop, before each instruction
  (`vm_state.rs:1203-1209`): read `gc_pending`; `maybe_collect` returns immediately unless the flag
  is set and the loop is outermost (`vm_state.rs:1287-1310`); `after_collection` runs if the flag
  went from set to cleared. The fast-path cost was measured at about 1% versus a control binary with
  no safe point (`GC_DESIGN.md §6.1`). When a collection runs, it first calls
  `retire_registers` over **all frames** (O(stack depth × window), `gc_roots.rs:47-68`), then
  `GcController::safe_point` → `collect(&[state, registry])` under one `heap.borrow_mut()`
  (`heap/gc.rs:381-409`). Collection is refused while the library registry is mutably borrowed
  (`vm_state.rs:1300-1303`).
- **Invariant [V].** Collection happens only at a safe point. Allocation never collects. Therefore
  **any Rust local holding a `TaggedValue` across an allocation is safe**, which the whole
  primitive library and the control code rely on (`control.rs:11-33`, "These helpers do not
  collect"). That invariant is the single most important constraint on any redesign: making
  allocation able to collect would require handles or rooting in all Rust code (in the style of
  V8 `Handle`/`HandleScope` or SpiderMonkey `Rooted`).
- **Roots [V].** Covered in §1. Hidden roots are handled by deferral, not tracing.
- **Register precision [V].** These are may-root maps per pc, computed by a forward dataflow over
  final bytecode (`pass5_codegen.rs:223-284`). Locals stay conservative for the whole frame. Only
  finished expression temporaries are retired. The maps clear slots in place
  (`TaggedValue::UNSPECIFIED`) rather than skipping them, so continuation snapshots and the tracer
  never hold deliberately untraced pointers. Snapshots are cleaned at capture
  (`vm_state.rs:637`, `647`). Suspended frames use `frame.pc`, which points after their call; that
  is exactly a return-address stack map.
- **Weak continuation tables [V].** These are ephemeron-like: marking records continuation ids it
  reaches, `trace_weak_ids` traces payloads for new ids until a fixpoint, and `sweep_weak` prunes
  the rest and shrinks the table when it is less than 1/8 full (`gc_roots.rs:117-152`). Soundness
  depends on "capture and invoke touch the store within one dispatch, and nested loops defer"
  (`gc_roots.rs:21-24`).

---

## 9. Measurements (release build at `28a94f8`, arm64 macOS; single runs, wall time including ~10 ms bootstrap)

| Program | Result | Time | Max RSS | Derived |
|---|---|---|---|---|
| empty `(display 1)` | – | 0.01 s | 11.7 MB | bootstrap |
| named-let sum, 20M iterations (7 instr/iter) | ok | 0.62 s | 10.9 MB | ~31 ns/iter, ~4.4 ns/instr |
| cons churn, 20M iterations | ok | 1.20 s | 13.0 MB | ~60 ns/iter |
| `fib 30` / `fib 32` (1.35M / 3.5M calls; ~10 instr/call) | ok | 0.11 / 0.27 s | 10.9 MB | ~38 ns/call |
| make + call a closure, 5M iterations | ok | 0.55 s | 19.2 MB | ~110 ns/iter; 12.4% malloc/free in `sample` |
| `deep 100` × 20k, no call/cc | ok | 0.08 s | 10.9 MB | ~3.5 µs per 100-deep call chain |
| same + `call/cc` at the bottom, 5k / 20k / 60k / 200k | ok | 0.05 / 0.17 / 0.46 / 1.36 s | 101 / 362 / 590 / 591 MB | ~3-4 µs and ~17 KB per capture |
| 10,000,000-deep non-tail recursion | ok | 0.50 s | 1.37 GB | ~137 B/frame |

Type sizes (`size_of`, **[V]**): `TaggedValue` 8, `CallFrame` 40, `Instruction` 48, `CodeObject`
160, `GlobalCacheEntry` 16, `PromptFrame` 48, `ExceptionHandler` 16, `DynamicWindRecord` 40,
`VmContinuation` 152, `VmDelimitedContinuation` 160, **`HeapObjectData` 72** (so every
`MutableCell`, `VmClosure`, continuation ref, and flonum `Real` costs a 72 B slot), `Environment`
224, `VmState` 832.

Profile of the closure loop (2181 samples): `dispatch_one_instruction` (with inlined arms) 54%;
`run_loop_until` 6%; `call_closure_from_regs` 4%; `push_frame` 3.6%; malloc/free family 12.4%;
`frame_globals` 2.6%; `pop_frame` 2.0%; memset 2.0%; sweep + `HeapObjectData` drop glue 3.1%;
`Environment::slot_value` 0.9%; `read_mutable_cell` 0.7%.

Repo baselines (`benchmark_reports/performance.md`, 2026-09-30): VM "sum 100" 3.06 µs,
"list 256" (with GC) 14.9 µs, fresh bootstrap 7.15 ms.

---

## 10. Prior art for runtime and JIT co-design

- **Chez Scheme.**
  - *Frame metadata:* return points carry an `rp-header` with `frame-size` and `livemask`
    (`ChezScheme/s/cmacros.ss:1758-1762`; compact form at `c/types.h:329-337`). The GC walks stack
    frames by return address (`s/mkgc.ss:1040-1075`).
  - *Continuations:* a continuation is a GC object with `stack`, `stack-length`, `clength`, `link`,
    `winders`, and `attachments`. One-shot continuations are flagged by `scaled-shot-1-shot-flag`,
    and the GC copies only the live `clength` (`s/mkgc.ss:220-262`). Stack overflow splits segments
    (`c/schsig.c:86-93`). These follow Hieb/Dybvig/Bruggeman PLDI'90 and Bruggeman/Waddell/Dybvig
    PLDI'96 (one-shot continuations).
  - *Polling:* `np-insert-trap-check` decrements a dedicated `%trap` register and calls `event` on
    zero, inside a `pariah` (cold) arm (`s/cpnanopass.ss:3997-4012`). A GC request sets
    `something-pending` (`s/library.ss:1220-1238`). The poll costs a decrement and a branch, with no
    memory load.
- **OCaml 5.**
  - *Stack scanning:* frame descriptors per return address (`caml_find_frame_descr`,
    `ocaml/runtime/frame_descriptors.c:383`) drive `scan_stack_frames` (`runtime/fiber.c:262-319`).
    An `Already_scanned` mark lets minor GCs skip old frames, which bounds stack-scan cost under
    generational GC.
  - *Fibers:* stacks are heap-allocated, linked by `Stack_parent`, which is the effect-handler
    analogue of segmented stacks.
  - *Polling:* `asmcomp/polling.ml` inserts polls in loops and function prologues; allocations are
    polling points too.
- **Guile VM.** `scm_i_vm_mark_stack` marks the VM stack precisely and sets dead slots to
  `SCM_UNSPECIFIED`, the same technique as Patina's #423. Guix bug threads document a race where a
  frame-pointer update before the dynamic link was set hid objects from marking
  (https://lists.nongnu.org/archive/html/bug-guix/2018-05/msg00074.html; stack layout:
  https://www.gnu.org/software/guile/manual/html_node/Stack-Layout.html). Lesson: the frame-push
  order must be GC-consistent at every safepoint.
- **V8 Sparkplug.** A non-optimizing baseline JIT that compiles bytecode straight to machine code
  and keeps the interpreter's frame layout, so OSR/deopt and stack walking are unchanged
  (https://v8.dev/blog/sparkplug). This is the closest model for a first Cranelift tier over
  `ExecutionState`.
- **Cranelift.**
  - *User stack maps:* `FunctionBuilder::declare_value_needs_stack_map` makes the frontend spill
    GC values to stack slots around safepoints, reload them after, and attach stack-map entries.
    Reference types and regalloc-generated maps were removed
    (https://fitzgen.com/2024/09/10/new-stack-maps-for-wasmtime.html;
    https://docs.rs/crate/cranelift-codegen/0.134.3/source/src/ir/user_stack_maps.rs, "a safepoint
    is a program point … where it must be safe to run GC"). **[I]** Spill-then-reload means a moving
    collector that rewrites the slot is observed after the call.
  - *Calls and exceptions:* `CallConv::Tail` supports guaranteed tail calls, and exception payload
    registers are defined for `try_call`/exception resume
    (https://docs.rs/cranelift-codegen/latest/cranelift_codegen/isa/enum.CallConv.html). Proper
    tail calls are mandatory for Scheme.
- **HotSpot.** Thread-local handshakes (JEP 312, https://openjdk.org/jeps/312) poll a per-thread
  word. ZGC concurrent thread-stack processing (JEP 376, https://openjdk.org/jeps/376) uses stack
  watermarks so a GC never scans a whole deep stack in a pause. That is relevant because Patina
  allows 10M-frame stacks and `retire_registers`/`trace_roots` are O(depth) per collection.

---

## 11. What blocks or complicates a Cranelift JIT and a faster GC

Severity: **H** = must be redesigned, **M** = workable but costly, **L** = cleanup.

1. **H: `Rc<RefCell<Heap>>` as the access path.** Every heap read checks and updates a borrow flag,
   and `RefCell` exists precisely so Rust code can hold `&Heap`. JIT code cannot take part in
   borrow tracking. It must bypass it (unsafe) while guaranteeing no outstanding Rust borrow, or the
   heap must become a raw-pointer allocator with explicitly managed access. The global
   `objects: Vec<HeapObjectData>` with 72 B enum slots and `Rc` payloads is not something JIT code
   can allocate into inline.
2. **H: Index-based references into reallocating arenas.** A JIT can read `car` only by loading
   `heap.pairs.ptr` from memory and reloading it after any allocation. Per-type arenas mean a
   `TaggedValue` encodes the arena (tag) and an index, not an address. An address-based encoding
   with object headers lets JIT code do `ld [v - tag + off]`. Several other components' notes argue
   for this change too.
3. **H: Allocation never collects, polls are per instruction.** This is a strength to keep for
   simplicity: no handles in Rust code, and GC only at polls. A JIT needs polls at function entry and
   loop back-edges (tail calls) in the Chez/OCaml style, plus stack maps only at polls and calls.
   If the redesign moves to "collect on allocation failure", every Rust primitive and control
   helper that holds a `TaggedValue` across an allocation becomes a bug (`control.rs:11-33`).
4. **H: Off-heap, byte-blind continuation payloads.** Side tables, `Rc` sharing, weak fixpoint
   rounds, and a trigger that cannot see payload bytes (measured 590 MB plateau). A redesign
   should make continuations, or stack segments, real GC objects whose size counts. `CallFrame` holds
   `Rc<CodeObject>`, so capture and restore do per-frame refcounting.
5. **H: GC to code-store feedback through per-object sweep.** `live_closures` is decremented from
   the sweep's list of freed closures. Copying or nursery collection frees dead objects without
   visiting them. Code objects (or at least their liveness) should become GC-traced, with the
   closure pointing to its code, so code is collected like any other object (or with a finalizer
   list).
6. **H: Immutable `Rc`-shared Rust structures holding `TaggedValue`s.** These include
   `Rc<VmContinuation>`, `Rc<VmDelimitedContinuation>`, `Rc<[ExceptionHandler]>` in wind records,
   `Rc<CodeObject>.constants`, environments, `Rc<CompiledMacro>`, and parameter value stacks. A
   moving GC must update these slots in place, needing interior mutability or GC ownership, or must
   pin every object they reference, as Bartlett mostly-copying does.
7. **M: Bare `HeapIndex` in `CallFrame.closure`.** A special trace path today. Under a moving GC it
   must be updated in both live frames and snapshots. Making it a `TaggedValue`, or folding
   closure, code, and constants into one heap pointer, simplifies both the GC and the JIT frame
   layout.
8. **M: Globals are name-keyed Rust structures.** `Rc<str>` names, per-site `env_id` caches,
   `RefCell<Bindings>` SmallVec slots, `FORWARDED` indirections, and a per-closure
   `Rc<Environment>` cloned on every access. A JIT wants GC-heap binding cells referenced from the
   constant pool. Environments living outside the GC heap also means a moving GC cannot relocate
   the values they reference without tracing them as updatable roots (it does trace them via
   `visit_env`).
9. **M: Rust-stack VM entry and nested loops.** `across_reentry`, raw `*mut VmState` aliasing, and
   GC deferral in nested loops. JIT code calling Rust primitives that call back (`apply_proc`) would
   interleave native JIT frames, Rust frames, and VM frames. Today continuations cannot carry Rust
   frames: they abandon them, and `reentry` ids detect this. Native JIT frames would be in the same
   position unless JIT frames are VM frames (Sparkplug model) or are copyable (Chez model). Keep
   the rule "anything that calls back is Scheme or a resumable primitive" and extend it to JIT code.
10. **M: The error and escape channel is `Result<_, VmError>` with `String`s.** JIT code needs a
    cheap status return such as "continue / escape / error" with no allocation, plus the
    `pending_escape` check ordering. Cranelift exception support (`try_call`) is an alternative, but
    unwinding through Rust primitives is not.
11. **M: Polymorphic call protocol.** The `Call` probe order is closure, then control primitive,
    primitive, resumable, parameter, full continuation, delimited continuation
    (`control.rs:3172-3226`). Each probe is a heap borrow and an enum match on a `TAG_OBJECT` slot.
    The unused `TAG_CLOSURE` tag plus a uniform "callable header with entry pointer" (an
    applicable-struct or entry-point word, as Chez closures have a code pointer in word 0) would
    give JIT call sites one load and an indirect jump.
12. **M: `eq?` depends on the heap** (`Rc::ptr_eq` for `Procedure`/`Record`). Fix the representation
    so identity is the reference.
13. **L: Per-pc `Vec<Vec<u64>>` root maps** allocate one `Vec` per instruction. A JIT and a
    redesigned interpreter need maps only at safepoints (calls and polls): compact, deduplicated, and
    keyed by bytecode pc for the interpreter and by return address for native code.
14. **L: Instruction encoding.** The `Vec<Instruction>` enum (48 B) with `Vec<Reg>`/`Rc<str>`
    operands is fine as JIT *input*. A JIT wants a stable pc → native-address table for
    resume-at-pc (continuation restore, `ResumeWindJump`, stubs).
15. **L: `retire_registers` and root tracing are O(stack depth).** Deep stacks (Patina allows 10M
    frames) make every collection pay for the whole stack. Generational GC wants a stack watermark
    (OCaml `Already_scanned`, ZGC JEP 376) or a stack barrier.

---

## 12. Constraints and lessons the redesign must respect

- Keep a **single, explicit safepoint discipline**: GC happens only at polls, and allocation
  never collects (or collects only where every live value is published). Rust primitives and
  control code are written against it.
- **Continuation semantics depend on "remaining work = (frame, pc, registers)".** That covers
  `dynamic-wind` travel, raise bookkeeping, `force`, resumable primitives, `eval`, and abort
  landings (`control.rs:91-104`, `VM_RUNTIME.md §4.6`). Any JIT tier must be able to resume at an
  arbitrary bytecode pc of any frame, or it breaks `control_flow_matrix.rs` and
  `escape_from_primitive.rs`.
- **Proper tail calls in every path**, including deopt from inlined primitives (`exec_call_primitive`
  checks whether the next instruction is `Return` to keep a rebound primitive in tail position,
  `control.rs:3477-3490`).
- **Deep, unbounded non-tail recursion works today.** A native-stack JIT must not regress it
  silently.
- **Precise roots are cheap here.** Self-tagged values mean stack maps are liveness bitsets, not
  type maps, as long as JIT code keeps boxed `TaggedValue`s in GC-visible slots at safepoints.
  Unboxed floats or fixnums in JIT frames would need typed maps.
- **Dead-slot clearing in snapshots** (call/cc `dst` clearing, delimited hole clearing, #423
  retirement at capture) prevents real leaks that were measured at 296 MB. Keep it under any
  continuation representation.
- **Shadow-bit deopt** for inlined primitives and for `call-with-values`/`dynamic-wind` sequences
  must survive into JIT code. That means a global-rebinding invalidation channel, cell-based or
  through code invalidation.
- Code lifetime must stay bounded (#338: about 600 B leaked per form before the fix).

---

## 13. Open questions

1. Should the first JIT tier keep `ExecutionState`'s frame layout (Sparkplug-like, trivially
   compatible with continuations and GC), or move Scheme frames to the native stack (needs
   Cranelift stack maps, frame walking, Chez-style segment copying for `call/cc`, and handling of
   Rust frames in between)?
2. If frames stay VM data, should the register file become GC-managed stack segments (Chez model:
   a continuation is a heap object pointing to a segment, captured by splitting rather than
   copying), which would make `call/cc` O(1) and its memory visible to the trigger?
3. Can `globals` move from the closure (`Rc<Environment>` per `VmClosure`) to the code object or the
   constant pool? In practice each code unit is compiled against one environment (`load_unit` after
   `compile_with_qq_resolving(.., env, ..)`). That needs confirming for `with_globals` library
   bodies.
4. Which `apply_proc` call sites are still reachable on the VM? §7.5 lists them; `%parameterize-swap!`
   is reachable. Can the nested-loop path, and the GC deferral it forces, be removed entirely?
5. Is the 72 B `HeapObjectData` slot (and its `Rc` payloads) going away in the heap redesign? Most
   per-instruction costs above (closure slot, cell slot, continuation ref) follow from it.
6. Should a closure's identity, code entry, and free variables be one contiguous heap object (Chez
   layout: code pointer in word 0, free variables inline), so a JIT call is a tag check, a load, and
   an indirect jump?
7. How should the GC count off-heap bytes (continuation payloads, `Vec` element buffers, `Vec<char>`
   strings) until they become heap objects? Byte-based allocation accounting is needed either way.
8. Does `retire_registers` need to run per collection over the whole stack, or can a watermark
   limit it to frames touched since the last GC (relevant to generational GC)?
