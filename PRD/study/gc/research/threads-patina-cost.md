# Threading models: what each would cost Patina

Task key: `threads-patina-cost`. Every claim below is checked against `main` at `28a94f8` (2026-09-30). File:line references are to that tree. Effort figures are in focused engineer-weeks for one developer who knows the codebase. They are estimates, not measurements. The only measurement taken here is the isolate spawn proxy in §4.1.

## 0. Bottom line

| | A. Green threads (SRFI 18, M:1) | B. Shared-heap OS threads | C. Isolated heaps per OS thread |
|---|---|---|---|
| Satisfies SRFI 18 (shared mutable state, mutexes) | yes | yes | **no** (no shared mutable data) |
| Real parallelism | no | yes | yes (share-nothing) |
| Effort | **8–12 wk** VM core; +2–3 wk tree-walker; +2–4 wk non-blocking I/O | **9–18 engineer-months** | **3–5 wk** basic; +4–8 wk cross-isolate code sharing |
| Risk | medium (dynamic-state transfers, parameterize semantics) | high (pervasive `Rc`→`Arc`, races, 787 heap-borrow sites) | low |
| Single-thread cost when unused | ~0 (reuses the existing safe-point load) | est. 5–15% VM, 15–40% tree-walker (unmeasured) | 0 |
| GC impact | enumerate N execution states; nothing else | TLABs, handshakes, safe regions, precise rooting replacing deferral, atomic/parallel marking, race-tolerant barriers | none (each heap single-threaded) |

Recommendation: implement **A inside each heap**, with **C** later for parallelism. Racket CS has threads plus places, Ruby has Threads inside Ractors, and JS has async inside workers. Write the GC so that B remains a later engineering project and not a rewrite (§6). Most of what keeps B open is also what a Cranelift JIT needs anyway: a per-mutator context, a single polled interrupt word, stable arena addresses, a narrow store/barrier funnel, and precise roots instead of deferral.

---

## 1. Ground truth: the runtime today

**Ownership graph.** `pub type SharedHeap = Rc<RefCell<Heap>>` (`crates/patina-core/src/heap/mod.rs:51`). `Heap` (`heap/mod.rs:304`) has four growable `Vec` arenas: `pairs: Vec<(TaggedValue,TaggedValue)>` (:307), `vectors: Vec<Vec<TaggedValue>>` (:310), `strings: Vec<Vec<char>>` (:313) and `objects: Vec<HeapObjectData>` (:316). It also holds the intern table `symbol_table: HashMap<String, HeapIndex>` (:319), global free lists (:367–376) and **mutator-ish state**: `allocs_since_gc` (:379), `gc_threshold` (:387), `gc_pending: Rc<Cell<bool>>` (:395) and `gc_defer_depth` (:417). `TaggedValue` is a `u64` with a 3-bit tag and a `u32` arena index (`tagged_value.rs:28,57,70–84`).

**`HeapObjectData` has 28 variants** (`heap/mod.rs:143–229`). 14 of them carry `Rc` payloads: `Symbol(Rc<str>)`, `Procedure`, `Port`, `Macro`, `RecordType`, `Record{record_type, fields: Rc<RefCell<Vec<_>>>}`, `Identifier{name: Rc<str>}`, `Continuation`, `Parameter{values: Rc<RefCell<Vec<_>>>}`, `Promise(Rc<RefCell<_>>)`, `Library`, `EnvironmentSpecifier{env: Rc<Environment>}`, `PromptTag` and `VmClosure{globals: Rc<Environment>}`. Five use interior mutability: Record fields, Parameter, Promise, `MutableCell(RefCell<TaggedValue>)` (:190) and `Ephemeron(RefCell<…>)` (:203). `Procedure::Primitive.registry_index` is a `Cell` (`procedure.rs:49`).

**Environments** are `Rc`-linked Rust structs outside the heap (`environment.rs:487–547`). Each holds its own `heap: SharedHeap` clone, `bindings: RefCell<Bindings>`, `scoped_bindings: RefCell<…>`, `alias_bindings: RefCell<…>`, `Cell<bool>` flags (:531–532) and `parent: Option<Rc<Environment>>`. The tree-walker allocates one per call (`cps_eval/application.rs:79`, `step.rs:154`).

**Usage counts** (`rg -c` over `crates/`, tests included):

| crate | `Rc<` | `RefCell<` | `Rc::new` | `Cell<` | `thread_local!` | `borrow_mut()` | `.borrow()` | heap `.borrow*` (non-test) | LOC |
|---|---|---|---|---|---|---|---|---|---|
| patina-core | 233 | 36 | 73 | 10 | 4 | 90 | 72 | 15 | 23.4k |
| patina-frontend | 78 | 18 | 54 | 5 | 0 | 85 | 183 | 198 | 17.1k |
| patina-vm | 85 | 16 | 69 | 6 | 2 | 53 | 82 | 58 | 16.2k |
| patina-primitives | 22 | 13 | 12 | 3 | 1 | 170 | 280 | **406** | 15.4k |
| patina-tree-walker | 62 | 10 | 22 | 3 | 2 | 37 | 47 | 30 | 6.3k |
| patina-macros | 38 | 2 | 6 | 0 | 1 | 28 | 51 | 61 | 8.2k |
| patina-runtime | 40 | 9 | 10 | 0 | 1 | 8 | 5 | 6 | 3.9k |
| patina-interpreter/repl/ir/tests/compat | 30 | 11 | 61 | 1 | 0 | 13 | 41 | 13 | — |
| **total** | **588** | **115** | **305** | **28** | **11 blocks / 19 statics** | **484** | **761** | **787** | |

Other totals: `SharedHeap` is named 598 times, `Rc<Environment>` 173 times, `Rc<CodeObject>` 27 times, `Rc<Port>` 32 times, and there are 437 `.alloc_*(` call sites.

**`thread_local!` inventory (19 statics).**
- core: `EMPTY_CONT_ENV` (`cont_value.rs:56`), `SCOPE_ORIGINS` (`scope.rs:39`), `PHASE` (`scope_trace.rs:117`), and the stdin and output-file state `STDIN_UNREAD/POSITION/FOLD_CASE/CARRY` and `OUTPUT_FILES` (`port.rs:157–179`).
- primitives: `CURRENT_INPUT/OUTPUT/ERROR_PORT` (`io/ports.rs:41–44`).
- tree-walker: `PENDING_ESCAPE`, `ACTIVE_TRAMPOLINES`, `NEXT_TRAMPOLINE` and `UNHANDLED_IN_CALLBACK` (`cps_eval/types.rs:20,66–69`).
- VM: `EMPTY_REENTRY` and `EMPTY_HANDLERS` (`runtime/control.rs:1657,2114`).
- `TRACER` (`patina-macros/src/tracer.rs:14`) and diagnostics `OUTPUT` (`patina-runtime/src/diagnostic.rs:82`).

**Send/Sync bounds that already exist** are few, all at I/O and error edges:
- `FileSystem: Send + Sync` (`vfs.rs:19`); `ReadPort`/`WritePort: Send` (`vfs.rs:86,90`).
- `Backend::Error: … + Send + Sync + 'static` (`patina-runtime/src/backend.rs:38`).
- `SourceLocation` is asserted `Send + Sync` (`source_document.rs:159`), with `Arc<SourceDocument>` (:145,150).

**Every id counter is already a process-global atomic.** That is favourable for all three options:
- environment ids (`environment.rs:443`), `ScopeId` (`scope.rs:36`), record types (`record_type.rs:5`)
- dynamic-wind and prompt ids (`continuation.rs:140,143`), CPS gensyms (`cps_expr.rs:69`, `cps_transform.rs:50`)
- desugarer (`desugarer/mod.rs:196`), source documents (`source_document.rs:41`)

**Machine state.** The VM keeps the five dynamic-extent components in `ExecutionState` (`runtime/execution_state.rs:17–23`), encapsulated by #602. Per-machine control fields sit beside it on `VmState`: `pending_escape`, `pending_transfer`, `reentry`, `next_reentry`, `reentry_kept` (`vm_state.rs:54–106`) and `scratch_args` (:188). The shared machinery is `code_store: Vec<Rc<CodeObject>>` (:110), the weak continuation tables as `RefCell<FxHashMap<u64, Rc<…>>>` (:196–199), `globals`, `heap`, `primitive_registry: Rc<…>` (:168), `gc: RefCell<GcController>` (:213) and `gc_pending` (:218). `VmBackend` holds `state: RefCell<VmState>` (`backend.rs:126`). The tree-walker's entire machine state is one `StepResult` (`cps_eval/types.rs:181–212`): expression, env, `ContEnv`, prompt stack, winds and handlers.

**Safe points and deferral.**
- VM: every dispatch loop takes a `GcDeferGuard` (`vm_state.rs:1151`). It polls `gc_pending` once per instruction (:1204–1208) and collects only when the guard is outermost (`maybe_collect`, :1287–1311).
- Tree-walker: `run_trampoline` does the same (`cps_eval/mod.rs:213–230`).
- Rust-stack temporaries are protected by *deferral*, not rooting (`GC_DESIGN.md` §7; `heap/gc.rs:232–268`).
- The poll costs about 1% versus a no-check control (`GC_DESIGN.md` §6.1).

---

## 2. Option A: green threads (SRFI 18 multiplexed on one OS thread)

### 2.1 What a thread is, per backend

- **VM.** A thread is `ExecutionState` plus the five re-entry and escape fields plus `scratch_args`. In code, that means moving `vm_state.rs:46,54,83,97,99,106,188` into a `ThreadCtx` (or `Mutator`) struct. #602 already made `ExecutionState` the only module that may resize or replace these stacks (`execution_state.rs:1–3`), so the refactor has a natural seam. `code_store`, the continuation tables, `globals`, the registries and `gc` stay on the shared machine.
- **Tree-walker.** A thread is one suspended `StepResult`. `PRD/future/TREE_WALKER_HOOK_SYSTEM.md` §9.2 already says the same. The thread-locals that describe "the running machine" (`PENDING_ESCAPE`, `ACTIVE_TRAMPOLINES`) remain valid because switches happen only at the outermost trampoline.

### 2.2 Switch mechanism: continuations give the semantics, not the cost

`capture_full`/`restore` clone every component (`execution_state.rs:239–261`): `registers.clone()`, `frames.clone()` and so on. A continuation-based switch, which is how Chicken builds SRFI 18, is therefore O(stack depth) per switch on this VM.

The O(1) equivalent is `mem::swap(&mut vm.execution, &mut thread.execution)` plus the scalar fields. That is a one-shot hand-off, in the same spirit as Chez's `call/1cc`, which Racket CS engines build on. What VM continuations *do* prove is that the five components plus the re-entry fields are a self-contained, restorable machine image. A continuation captured in one thread can therefore be invoked in another, which SRFI 18 requires to be well defined. Under A this works for free, because all threads share one `VmState` and therefore one `continuation_store`.

A thread switch needs a new row in the dynamic-state matrix (`docs/VM_RUNTIME.md` §5.6): **save/restore all five, run no wind thunks.** SRFI 18 states that scheduler switches do not run `dynamic-wind`, and Racket's engines make the same choice ("doesn't run winders when suspending or resuming", `racket/src/cs/rumble/engine.ss:3`). Thread start becomes a second new row: an empty `ExecutionState` with an inherited dynamic environment and the handler stack reset. `control_flow_matrix.rs` needs rows for switch-inside-wind, abort across a switch and cross-thread continuation invoke.

### 2.3 Scheduler and preemption

- **Preemption piggybacks on the existing per-instruction poll.** Generalize `gc_pending: Rc<Cell<bool>>` into a per-mutator interrupt word with bits for GC, preempt, signal, debugger and handshake. A timer thread (Gambit's heartbeat, `lib/_kernel.scm:243–246,1480`) or a fuel counter (chibi: `SEXP_DEFAULT_QUANTUM 500`, `include/chibi/features.h:341`; `vm.c:1113`) sets the preempt bit.
- **The word must be `Arc<AtomicU32>` to be settable from a timer thread.** A relaxed load compiles to the same plain load as `Cell::get`, so the measured ~1% does not move. OCaml 5 goes one step further and folds the interrupt into `young_limit`, so the allocation check *is* the poll (`runtime/caml/domain_state.tbl:17`, `signals.c:295`).
- **Blocking SRFI 18 operations must be machine-level control primitives**, not Rust primitives, because they swap the execution state. This applies to `thread-yield!`, `thread-sleep!`, `thread-join!`, `mutex-lock!`, `mutex-unlock!` with a condition variable, and `thread-terminate!`. They go through `VmControlPrimitive`, as `call/cc` and `dynamic-wind` do (`VM_RUNTIME.md` §5.1), and through the matching intercept in the tree-walker.
- **New heap object types:** Thread, Mutex, ConditionVariable and Time. Each follows the four-step checklist in AGENTS.md (variant, predicate, display, GC trace). If wait queues are heap objects, a deadlocked unreachable thread becomes collectable. That is optional; Gambit and chibi simply keep every thread as a root.

### 2.4 The hard constraint: no switch across a Rust re-entry boundary

A thread can be switched out only when its Rust stack holds none of its state. In practice that means the outermost loop with `reentry` empty, which is the same condition GC already uses (`is_outermost`, `vm_state.rs:1151–1153`). The project has spent #471–#478 removing Rust callbacks into the program (AGENTS.md, "A procedure that calls back into the program…"). That direction is exactly what green threads need. Residual boundaries remain:
- `ApplyContext::{apply_proc, eval_expr, load_scheme_library}` (`control.rs:2253–2300`);
- `Step::Eval`'s import loading (`control.rs:1849`);
- library bodies (`vm_state.rs:847`);
- Rust fallbacks that still call `ctx.apply_proc` (`primitives/lazy.rs:125`, `values.rs:35,48`, `io/file.rs:216,263`, `io/ports.rs:666`, `lists.rs:440,587`).

Inside one of these boundaries, preemption is postponed, which is acceptable. A *blocking* SRFI 18 call there cannot switch, so it must raise an error. This is Lua's "attempt to yield across a C-call boundary". Expected incidence is low: a library body that locks a mutex, or a callback-driven `force` fallback.

### 2.5 State that is global today but must become per-thread

1. **Parameters use shallow binding.** `Parameter{values: Rc<RefCell<Vec<TaggedValue>>>}` (`heap/mod.rs:174–177`) is one process-wide stack. `parameterize` is `dynamic-wind` plus `%parameterize-swap!` (`lib/scheme/base/parameters.scm:36–49`; `primitives/parameters.rs:195`). Because switches do not run winds, one thread's `parameterize` would be visible in every other thread. There are two fixes:
   - (a) **Deep binding.** Chibi's threaded `parameterize` sets a per-thread `thread-parameters` alist inside the wind (`chibi lib/srfi/39/syntax.scm` vs `syntax-no-threads.scm`). The cost is a lookup per parameter read.
   - (b) Swap on switch. This is fragile.

   (a) is the right choice. Cache the three current ports in `ThreadCtx` so `display` stays O(1).
2. **Current ports** are `thread_local!` (`io/ports.rs:41–44`). This is already a latent bug: two interpreters on one OS thread share them. They move into the thread's dynamic environment and are inherited at `make-thread`, as SRFI 18 requires.
3. **Exception handler stack.** It is already per-`ExecutionState`. It is reset at thread start.
4. **Tree-walker thread-locals** stay as they are, provided switches occur only at the outermost trampoline.

### 2.6 Blocking I/O

Ports are synchronous `Box<dyn ReadPort>` behind the VFS (`vfs.rs:86`; `PortData`, `port.rs:250`). A `read-line` on stdin blocks every green thread. There are three established answers:
- Non-blocking fds plus `poll` in the scheduler. Chibi does this: `sexp_maybe_block_port` in `sexp.c:2072` and `sexp_blocker` with `sexp_insert_pollfd` in `lib/srfi/18/threads.c:400`. Chicken's scheduler does the same.
- Suspendable ports with read/write waiters, as in Guile fibers.
- **A helper OS-thread pool** that performs the blocking read and posts bytes back. This fits the VFS abstraction, since payloads are `Vec<u8>` and therefore `Send`. It needs no fd-level redesign and also covers `MemoryFs` and file I/O. It is the cheapest path.

This work can ship after the first release, which would document I/O as blocking the whole process.

### 2.7 GC with many stacks

- **Root enumeration.** `impl GcRoots for VmState` (`vm_state/gc_roots.rs:70–118`) traces `self.execution`. Move the per-thread part into `impl GcRoots for ThreadCtx` and trace the thread table. Each suspended state keeps `pc` per frame, so `retire_registers` (`gc_roots.rs:47–68`) applies unchanged to every thread. Weak continuation tables (`trace_weak_ids`/`sweep_weak`, :120–145) are untouched. Cost: about 3–5 days.
- **Scaling.** A full STW mark scans every thread's registers, which is O(sum of stacks). For a future sticky-mark-bit generational collector (`GC_STAGE5_PRD.md` Priority 3), add a **per-thread "ran since last GC" bit**. Every survivor of a collection is old and only running threads allocate, so a stack untouched since the last collection holds only old references and can be skipped by a minor GC. This is the same insight as Loom's stack chunks and HotSpot's stack watermarks (JEP 376): suspended stacks are heap-like data processed lazily.
- **Thread objects as heap objects.** Optionally make a suspended `ExecutionState` an object reachable from its Thread object, as a Loom `StackChunk` is. Reachability then decides what to scan.

### 2.8 JIT implications

Green threads, snapshot continuations and precise stack roots all assume that **Scheme frames live in `ExecutionState`, not on the native stack.** Keep this property for the Cranelift JIT:
- JIT code addresses the register window through a base pointer.
- Calls return to a dispatcher or trampoline instead of nesting native frames.
- Code polls the same interrupt word at loop back-edges and function entries.
- `register_roots` per-PC maps (`code_object.rs:158`) stay the stack maps.

LuaJIT (Lua stack in memory) and Chez (its own segmented stack) follow the same principle. If the JIT used the native stack, every green thread would need its own mmap'd native stack with stack switching, as in Go or Wasmtime fibers. Continuations would need native-stack copying, and the GC would need Cranelift user stack maps for every frame on every stack. Each of those costs months.

### 2.9 Effort and risk (A)

| Item | Weeks |
|---|---|
| Extract `ThreadCtx` out of `VmState`; interrupt word as `Arc<AtomicU32>` | 1–2 |
| Scheduler, Thread object, start/yield/sleep/join/terminate as control primitives | 2–3 |
| Mutex, condition variable, time, SRFI 18 exception kinds | 1–2 |
| Deep-bound parameters; ports into the dynamic environment | 1–2 |
| GC: thread table roots, a deterministic scheduler mode for the GC differential lanes | 1 |
| Matrix rows, chibi/Gauche SRFI 18 test import | 1–2 |
| **VM total** | **8–12** |
| Tree-walker parity (thread = `StepResult`) | +2–3 |
| Non-blocking I/O via helper pool | +2–4 |

Risk is **medium**. Historically, defects in this codebase arrive in exactly the dynamic-state transfers (`VM_RUNTIME.md` §5.6 lists #157–#163). The `parameterize` change touches a hot, well-tested path. GC risk is low.

---

## 3. Option B: shared-heap OS threads

### 3.1 What must change, by structure

1. **`SharedHeap = Rc<RefCell<Heap>>`.** A global `Mutex<Heap>` is simply a GIL. Real parallelism needs a heap that is `Sync` for reads and field writes, with structural mutations (allocation, growth) moved to per-thread contexts.
   - All **787 heap-borrow sites** change API. Primitives account for 406 and the frontend for 198.
   - The `Vec` arenas reallocate on `push` (`alloc_pair`, `heap/mod.rs:703–714`). A concurrent `car` would read freed memory. Arenas must become stable-address blocks: a chunk table, or reserved virtual memory with commit on demand.
   - Slot access must use atomic words (`AtomicU64` relaxed loads and stores, which are plain `mov`/`ldr` on x86-64 and arm64). Rust's memory model makes a racy plain access undefined behaviour even when the Scheme program is the one that raced.
2. **Allocation.** The global free lists and `allocs_since_gc` on `Heap` become per-thread TLABs plus per-thread counters. Chez gives each thread context its own allocation pointer and takes `S_alloc_mutex` only for refills (`c/globals.h:43`). OCaml 5 gives each domain a minor heap.
3. **Interning.** `symbol_table: HashMap<String, HeapIndex>` (`heap/mod.rs:319`, `intern_symbol` :981) and `core_syntax_table` become a concurrent interner (sharded lock). `Symbol(Rc<str>)` becomes `Arc<str>`.
4. **Environments.** `RefCell<Bindings>` and its siblings (`environment.rs:492–513`) become `RwLock` or per-slot atomic cells, with a lock only for inserts. Every `Environment` clones `SharedHeap`, and the tree-walker creates one per call. Under `Arc` that is two or more atomic RMWs per call plus the drops.
5. **VM inline caches.** `global_cache: Vec<Cell<GlobalCacheEntry>>` is `{env_id: u64, slot: u32}` (`code_object.rs:165,196–201`). That does not fit one atomic word, so it needs packing (32-bit env ids) or a seqlock. `live_closures: Cell<u32>` (:173) becomes atomic.
6. **`code_store` is per-`VmState`, but a closure names its code by `code_id` only** (`HeapObjectData::VmClosure{code_id}`, `heap/mod.rs:205–213`). With one `VmState` per OS thread, a closure made in thread A is meaningless in thread B. The code store and the continuation tables (`vm_state.rs:196–199`) must become machine-global and lock-protected or append-only.
7. **The 14 `Rc` payloads and 5 `RefCell`s in `HeapObjectData`** become `Arc` with `Mutex`/`RwLock` or atomics. The store funnel is narrow:
   - `set_car`/`set_cdr`/`vector_set` (`heap/mod.rs:743,755,804`)
   - `vector_slice_mut` (:816, one user at `vm_state.rs:2380`)
   - `write_mutable_cell(&self …)` (:1273)
   - record fields under a *shared* heap borrow (`primitives/records.rs:251–253`)
   - parameter, promise and ephemeron internals.
8. **Ports** become `Arc<Mutex<PortData>>`, and need to lock per operation anyway. `STDIN_*` and `OUTPUT_FILES` (`port.rs:157–179`) become process-global under locks.
9. **Primitive registry.** It is immutable after setup and holds `fn` pointers, so `Arc` is enough. `registry_index: Cell` becomes `AtomicUsize`.
10. **`GcDeferGuard` counts on the shared heap** (`gc.rs:232–268`, `heap/mod.rs:417`). It must become per-mutator, since one thread's nesting depth means nothing for another.

### 3.2 GC features that become mandatory

- **TLABs.** With index-based arenas, a TLAB is a claimed contiguous *index run* inside a block. Pairs are fixed-size, so bumping an index is ideal for inline JIT allocation. Sweep must therefore produce free runs (Immix-style blocks and lines, lazy sweep per block) rather than scattered free-list entries.
- **Stop-the-world handshake.** A collection request sets the GC bit in every mutator's interrupt word, as Chez's `S_fire_collector` does (`c/schsig.c:574–592`). Each mutator parks at its next poll.
- **Safe regions.** Threads blocked in native code, on I/O or on `mutex-lock!` declare themselves parked. Chez calls this `Sdeactivate_thread` (`c/thread.c:234`) and tracks an `active_threads` count (:166,296); OCaml calls it `caml_enter_blocking_section`; HotSpot calls it `_thread_in_native`.
- **Precise rooting must replace deferral.** This is the decisive item. Today a nested loop simply refuses to collect (`is_outermost`). With several mutators, the collector must stop *all* of them. A thread inside a nested loop holds live `TaggedValue`s in Rust locals, `mem::take`n buffers and argument `Vec`s (`gc_roots.rs:26–28`). It cannot park safely, so every other thread waits on it. If it is waiting on a lock a parked thread holds, the system deadlocks.
  - Conservative native-stack scanning (BDW in Gauche and Guile; JSC Riptide) does **not** rescue this. Rust temporaries live in heap-allocated `Vec`s, not on the stack.
  - What works is handle scopes, as in V8 HandleScope, SpiderMonkey `Rooted` and JNI local references. Concretely, `Mutator` would own a root stack that the remaining re-entry paths push onto. This is `GC_STAGE5_PRD.md` Priority 2 ("root the re-entrancy boundary"), which would move from desirable to mandatory.
- **Parallel or concurrent marking.** STW marking by a single thread can keep non-atomic `MarkBits` (`gc.rs:88–93`). Parallel marking needs `fetch_or` bitmaps and work-stealing, and must replace the `FxHashSet` dedup sets over `Rc` graphs (`GcVisitor`, `gc.rs:422–440`). Concurrent marking needs SATB, which means per-thread barrier buffers; OCaml 5's `caml_modify` darkens the old value.
- **Barriers that are correct under races.** Card marking is an idempotent byte store, so it is race-safe. SATB and remembered sets need per-mutator buffers flushed at handshakes. The store funnel in §3.1.7 must be complete.

### 3.3 Single-thread tax and precedent

PEP 703 reports 5–6% single-thread overhead (Skylake 6%, Zen 3 5%). Reaching that took biased reference counting, immortalization, deferred reference counting, mimalloc and per-object critical sections. It also dropped generational GC, because frequent young collections mean too many stop-the-world pauses. Patina has more `Rc` traffic per operation than CPython has refcount traffic per bytecode: the tree-walker clones `Rc<Environment>` and `SharedHeap` per call and carries `Rc<CpsExpr>` in every `StepResult`. A naive `Arc` port would land well above that figure, and only an equivalent engineering campaign would bring it down. The 5–15% (VM) and 15–40% (tree-walker) figures above are estimates by analogy and are unmeasured. Gauche and Guile avoid the problem with a conservative BDW collector. Chez avoids it with a GC designed from day one around thread contexts.

### 3.4 Effort and risk (B)

- Heap and arena redesign with TLABs: 6–10 weeks.
- `Rc`→`Arc` and `RefCell`→locks or atomics across about 600 `Rc<` sites and 115 `RefCell<` types: 8–14 weeks.
- 787 heap-access call-site migrations: 4–6 weeks.
- Precise rooting to remove deferral: 4–8 weeks.
- Handshake, safe regions, per-mutator GC state: 3–5 weeks.
- Shared code store and caches: 2–4 weeks.
- Concurrency testing (loom/TSan lanes) and performance recovery: 8–16 weeks.

**Total: 9–18 engineer-months. Risk: high.** It touches every crate. Heisenbugs would land in an area that has relied on byte-identical differential lanes for its correctness. It would also conflict with ongoing correctness work for many months.

---

## 4. Option C: isolated heaps per OS thread (places, isolates)

### 4.1 Patina is already most of the way there

Each `Interpreter` owns its own `SharedHeap`, environments, registries and `GcController` (`VmBackend`, `backend.rs:125–133`; `Evaluator`, `eval/mod.rs:43–65`). Ids are process-global atomics, so they never collide across isolates. An isolate is therefore `std::thread::spawn(|| Interpreter::new_vm()…)` today. The `!Send` interpreter is created on its thread and never leaves it.

`patina-compat` runs packages in parallel but uses subprocesses (`compat/src/run.rs:150,225`). In-process isolates have not been exercised.

**Measured spawn proxy.** I timed a release build on this machine. A program importing `(scheme base) (scheme write) (scheme char) (scheme lazy) (scheme case-lambda)` and running a `map` finished with median **11.1 ms** wall and **~12 MB** max RSS (VM, 15 runs). The tree-walker took 10.9 ms. Process startup is included, so this is an upper bound on per-isolate bootstrap cost.

### 4.2 What is per-heap and what is shared

| Thing | Per-isolate | Shareable | Note |
|---|---|---|---|
| Heap arenas, symbols, environments, libraries | ✓ | | `TaggedValue` indices are heap-relative |
| `CodeObject.constants`, inline caches | ✓ | | constants are heap indices (`code_object.rs:140`) |
| Instructions, register maps, source maps | | ✓ (`Arc`) | needs a split between immutable code and per-heap constant templates, like V8's code cache |
| Primitive registry | | ✓ | `fn` pointers |
| `SourceDocument` | | ✓ | already `Arc` |
| `FileSystem` | | ✓ | already `Arc<dyn FileSystem + Send + Sync>` |
| stdin buffer, open-output-file flush list | | **must be process-global** | today per OS thread (`port.rs:157–179`). Two isolates reading stdin would split lookahead; exit flush would miss other isolates' files |
| exit and error status | process | | `exit_status.rs:30,33` are process statics. Semantics are needed for `exit` inside an isolate |

### 4.3 Messages

Messages are copied. Use the heap's existing iterative graph copier, which already preserves sharing and cycles for syntax stripping (`GC_DESIGN.md` §9.1). Run it into a `Send` intermediate form and rebuild in the receiving heap. Symbols travel by name. Procedures, ports, environments and continuations are refused, which matches Racket place messages and JS structured clone. Record types need a prefab-like global registry. Large bytevectors can be shared zero-copy as `Arc<[u8]>`, which opens a SharedArrayBuffer path. Each isolate's GC runs unchanged, single-threaded, with its own pauses.

### 4.4 Limits

Isolates **do not implement SRFI 18**. SRFI 18 assumes shared mutable state guarded by mutexes. They are the parallelism complement, as in Racket places, Ruby Ractors and JS workers.

Racket CS places and Ruby Ractors are logically isolated but share one physical heap and one stop-the-world GC. Racket CS docs: Chez threads "share a global allocation space". That shared-heap variant is option B's GC with C's language semantics. It is not cheaper than B for the GC.

### 4.5 Effort and risk (C)

- Spawn, channels, the message codec, join and `exit` semantics, and moving the process-global leftovers: 3–5 weeks.
- A shared compiled-library cache across isolates: 4–8 weeks. Optional; bootstrap is already about 11 ms.

Risk: **low**. There is no GC change and no single-thread cost.

---

## 5. Hybrid: green threads within isolates

Racket CS, Gambit and Ruby all converge on this shape. Racket CS runs green threads plus places. Gambit runs green threads on "processors", with `___MAX_PROCESSORS` per VM (`include/gambit.h.in:1484–1490`), and is single-processor by default. Ruby runs Threads inside Ractors. For Patina:
- A first gives SRFI 18 compliance at about 0 cost to code that does not use it.
- C gives multicore parallelism for share-nothing work.
- B remains open if the GC follows §6.

---

## 6. GC design choices that keep every option open cheaply

Each item lists its approximate cost now and what it later buys.

1. **A per-mutator `Mutator` (allocation context) object, even with one mutator.** Move `allocs_since_gc`, the threshold check, `gc_pending`, `gc_defer_depth` (`heap/mod.rs:379–417`) and the allocation cursors off `Heap` into it. `alloc_*` takes `&mut Mutator`, or the mutator owns the heap handle. *Cost: one API pass over 437 alloc sites, mostly mechanical. Buys:* A gets a context per thread and B gets TLABs. The JIT gets one register-resident context pointer for inline bump allocation.
2. **One polled interrupt word per mutator** (`AtomicU32` bitset of GC, preempt, signal, debugger, handshake), loaded relaxed at the existing safe points. *Cost: about 0; same load as today. Buys:* A gets timer preemption and B gets STW handshakes. The debugger's all-stop (`TREE_WALKER_HOOK_SYSTEM.md` §9.1) and Ctrl-C get a home. JIT poll sites read one word. Optionally fold it into the allocation limit, as OCaml 5 does.
3. **A safe-point protocol phrased over "all mutators".** Request, then every mutator acknowledges (parked at a poll, or in a declared safe region), then collect, then release. With one mutator the acknowledgement is immediate. Make mutator states explicit: Running, AtSafepoint, InSafeRegion. *Cost: small. Buys:* B's handshake without redesign. A's blocking primitives also mark the thread as in a safe region.
4. **Precise roots for Rust-held values instead of deferral.** Give `Mutator` a root stack (handle scope) used by the residual re-entry paths, and make nested loops collectable (`GC_STAGE5_PRD.md` Priority 2). *Cost: the 4–8 weeks counted in §3.4, and worth it on its own merits (nested-map churn). Buys:* B becomes possible at all. A allows preemption inside callbacks. The JIT can call runtime helpers that allocate.
5. **Stable-address arenas.** Use blocks behind a chunk table, or reserved virtual memory with commit on demand, instead of growable `Vec`s. With reserved memory, index→address stays `base + i*size`, a single `lea`. *Cost: moderate, mostly platform code. Buys:* JIT code can keep the base in a register across calls and allocation. B gets race-free concurrent reads. Per-block mark bitmaps, lazy sweep and TLAB index runs follow. Block-structured arenas are also the natural unit for generational and sticky-mark metadata.
6. **Per-block mark bitmaps behind one `mark(tv) -> bool` function**, rather than a whole-arena `BitSet` sized per collection (`gc.rs:88`). *Cost: low. Buys:* switching to `fetch_or` for parallel marking is a local change.
7. **A complete, single store funnel with a barrier hook.** Route `set_car`, `set_cdr`, `vector_set`, `vector_slice_mut`, `write_mutable_cell`, record, parameter and promise stores, and environment slot writes through one inline barrier function that takes the mutator. *Cost: low; about 8 funnels and 34 call sites. Buys:* generational (card), concurrent (SATB, per-mutator buffer) and race-tolerant barriers. The JIT inlines the same sequence. Remove `vector_slice_mut`-style bypasses.
8. **Per-execution-context root providers.** Split `impl GcRoots for VmState` into shared roots (code store, globals, continuation tables) and per-thread roots (`ExecutionState`, the re-entry fields), each with a "ran since last GC" bit. *Cost: low. Buys:* A's many stacks, B's per-thread scans, and skipping clean stacks in minor GCs.
9. **New runtime data goes in the heap, not in `Rc<RefCell<…>>` side structures.** This covers Record fields, Parameter values, Promise state and new thread/mutex objects. *Buys:* no atomic refcounting under B, simple copying for C's messages, no `seen_*` dedup sets in parallel marking, and inline JIT field access. Each new `Rc` payload in `HeapObjectData` adds to B's cost.
10. **Dynamic environment per execution context.** Use deep-bound parameters and current ports in the context, not `thread_local!`. *Cost: 1–2 weeks, counted under A. Buys:* correctness for A and B, and for C on a thread pool. It also fixes today's latent sharing between interpreters on one OS thread.
11. **Split each code object into immutable code and per-heap constants and caches.** Make inline caches single-word (pack `env_id` to 32 bits). *Buys:* B's shared code store, C's cross-isolate code cache, and JIT code that can be shared.
12. **JIT: Scheme frames stay in `ExecutionState`, and native frames are transient.** Polls go at back-edges and entries; `register_roots` serve as stack maps. *Buys:* O(1) green-thread switches, snapshot continuations unchanged, and precise GC without Cranelift user stack maps on arbitrary native stacks.

None of 1–3 and 5–8 changes single-thread behaviour or cost measurably if done carefully. All of them can be gated by the existing byte-identical GC differential lanes.

---

## 7. Sequencing

1. During the GC redesign, adopt §6 items 1, 2, 3, 5, 6, 7 and 8 as structural rules, and item 4 as the stage-5 Priority 2 work.
2. Ship A on the VM: `ThreadCtx`, scheduler, SRFI 18 core, deep-bound parameters, then tree-walker parity, then non-blocking I/O.
3. Add C when a parallel workload appears.
4. Treat B as a separate, explicitly approved program. Revisit it only if shared-memory parallelism is demonstrably needed. With §6 in place, its cost falls mainly to the `Rc`/`RefCell` migration (about 8–14 weeks), call-site churn, and concurrency testing.

## Sources

- SRFI 18 text: https://srfi.schemers.org/srfi-18/srfi-18.html
- PEP 703: https://peps.python.org/pep-0703/
- Racket CS internals (places share Chez's allocation space): https://docs.racket-lang.org/inside/cs-overview.html
- Local sources:
  - ChezScheme `c/schsig.c:574–592`, `c/thread.c:166–328`, `c/globals.h:39–43,133`
  - racket `racket/src/cs/rumble/engine.ss:1–10`, `place.ss`
  - gambit `include/gambit.h.in:1463–1490`, `lib/_kernel.scm:243,1480`
  - chibi `vm.c:1086–1156`, `include/chibi/features.h:341`, `sexp.c:2072`, `lib/srfi/18/threads.c:400–430`, `lib/srfi/39/syntax.scm`
  - ocaml `runtime/caml/domain_state.tbl:17`, `runtime/signals.c:295–349`, `runtime/domain.c:1949`
