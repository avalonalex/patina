# The CPS tree-walker as the GC sees it, and its options under a redesigned collector

Repo state: `main` at `28a94f8`. Read-only study. Measurements were taken with a throwaway probe crate at
`PRD/study/gc/probes/tree-walker/` (`src/main.rs`), built in release with a counting global allocator. Numbers come from single runs on the development Mac, so they show orders of magnitude and are not A/B results.

Notation: **[V]** marks a fact verified in source or by measurement. **[I]** marks inference or assessment.

---

## 0. Summary

1. **[V] The tree-walker's runtime state lives almost entirely outside the GC heap.** Environments, continuation chains, closure bodies, wind/handler/prompt stacks and captured continuations are `Rc`/`Box`/`Vec` Rust structures. They hold `TaggedValue`s, and heap objects hold owning `Rc`s back to them. In `(fib 25)` the probe counted **3.16 M Rust allocations, 479 MB, about 13 per Scheme call, against 43 GC-heap allocations in total**. Of those, 1.09 M were `Environment` boxes, about 4.5 per call. A `sample` profile of `(fib 32)` puts **malloc/free at about 22%** of samples and Rust drop glue at about 6%. Name lookup accounts for about 34%. GC never ran.
2. **[V] Collection happens at exactly one place.** That place is the top of `run_trampoline`, and only in the outermost trampoline (`cps_eval/mod.rs:219-230`). Everything live is in the `current_step: StepResult` local plus the entry `expr`. Every nested trampoline defers through `GcDeferGuard`. No primitive calls back into Scheme on the hot path any more (#471-#478), so in practice nesting remains only for library loading, `environment`/eval-time imports and embedding calls.
3. **[V] Within a step, live values sit in Rust locals and in function parameters that have been moved out of the `StepResult`.** Examples: `proc`/`arg_values` in the `App` arm (`step.rs:236-259`); `cont`, `cont_env` and the three stacks passed by value into `apply_cps_step` and onward; the owned `args` `Vec` moved into primitives (`application.rs:949`). The shared `patina-primitives` crate (15.4 k LOC, about 117 direct allocation call sites) does the same, as do helpers like `Heap::list_from_iter_with_tail` (`heap/mod.rs:2934-2944`). **If collection could happen at allocation, all of these would be unrooted.**
4. **[V] Several mechanisms depend on today's non-moving, sweep-visits-the-dead design:**
   - Sweep tombstoning drops the `Rc` payloads of dead `Procedure::CpsLambda` and `Continuation` objects. This is how environment cycles get reclaimed (GC_DESIGN §8).
   - `Rc<CpsExpr>` literal trees, `ContValue` fields, `WindRecord`/`PromptFrame`/`ExceptionHandler` fields are immutable `Rc`-shared Rust data. A moving collector cannot update them without interior mutability.
   - The `LetVal` frame-sharing optimisation keys on `Rc::strong_count(&env) == 1` (`step.rs:151`). The safe-for-space behaviour of SRFI 45 depends on it: 39 MB vs 895 MB peak, per the code comment.
5. **[I] Recommendation:**
   - Keep the tree-walker on **safe-point-only collection (option d)**, and make "allocation never collects synchronously" a heap-wide contract, the way Chez does it.
   - Run tree-walker heaps under a **non-moving policy, or treat all tree-walker-held references as pinning roots (options b and e)**. This is feasible because each backend constructs its own heap; no constructor shares one (verified).
   - Remove the drop-at-sweep dependency by turning tree-walker payload objects into **id handles into weak side tables**. That reuses the existing `trace_weak_ids`/`sweep_weak` seam, which is compatible with copying.
   - **Do not** make tree-walker frames or continuations heap objects (option c) as part of the GC project. It is a rewrite of the evaluator. Its upside, mostly removing malloc/free (about 28% of call-heavy time), is real but separate from the GC.
   - **Do not** attempt full precise rooting with collection at allocation (option a).

---

## 1. The machine (verified map)

### 1.1 Driver loop and safe point

- **One loop for every run.** `CpsEvaluator::run_trampoline` (`crates/patina-tree-walker/src/eval/cps_eval/mod.rs:213-357`) is used for top-level forms, primitive callbacks and embedding applies. It:
  1. Takes a `GcDeferGuard` (`:219`) and hoists `is_outermost` (`:221`).
  2. Pushes a `TrampolineGuard` that gives the run a unique id (`types.rs:87-133`).
  3. Then loops `maybe_collect(is_outermost, &current_step, expr)` → `match current_step { Continue | InvokeContinuation | ApplyProc | Done }` (`mod.rs:227-305`).
- **Safe point.** `maybe_collect` (`mod.rs:114-133`) calls `GcController::safe_point` (`patina-core/src/heap/gc.rs:381-405`). When `!is_outermost || !pending` that is one flag load. On the cold path it builds a **closed** root array `[evaluator, &*registry, &EscapeRoots, &StepRoots{step, expr}]` (`mod.rs:130`). It aborts without collecting if the `library_registry` is mutably borrowed (`:126`).
- **Trigger.** `Heap::note_alloc` raises a shared pending flag when allocations cross the threshold (`heap/mod.rs:581-584`, gc.rs header `:20-27`). Allocation itself never collects.
- **Deferral.** `GcDeferGuard` (`gc.rs:232-275`) is a heap-wide counter. Every trampoline takes one, so only a run entered with depth 0 may collect. `ParsedLibrary` also holds a guard for its whole lifetime (`patina-runtime/src/library_loader.rs:162-216`). As a result, **an entire library body evaluates with no collection.** In the probe, importing `(scheme list) (scheme vector) (scheme sort) (scheme hash-table)` left about 184 k object slots and about 300 k pair slots of garbage, all allocated under deferral, and the arenas never shrink.
- **Granularity.** One safe point per `StepResult` (one control transfer). [I] GC latency is bounded by the longest single step: the inner `eval_one_step` loop over `LetVal`/`LetCont`/`If`/`Set`/`Define` (`step.rs:78-397`) is bounded by expression size, and primitive work is bounded because primitives no longer re-enter Scheme.

### 1.2 `StepResult`: the register file

`StepResult` (`cps_eval/types.rs:181-214`) has four variants: `Done(TV)`, `Continue{expr: Rc<CpsExpr>, env: Rc<Environment>, cont_env: ContEnv, prompt_stack: Vec<PromptFrame>, dynamic_winds: Vec<DynamicWindRecord>, exception_handlers: Vec<ExceptionHandler>}`, `InvokeContinuation{cont: ContValue, value, …}` and `ApplyProc{proc, args: Vec<TV>, cont, …}`.

[V] The `ApplyProc` payload is about 184 B and is moved by value on every step. **There is no persistent machine-register struct.** Each arm destructures the step and moves its fields into `eval_one_step`, `apply_cps_step` or `invoke_continuation_step` as owned parameters. From then on the only owners are Rust stack frames. This is the single most important fact for any "collect anywhere" design.

### 1.3 Continuations

- **`ContEnv`** (`patina-core/src/cont_value.rs:38-48`) is a persistent `Rc` cons list of `(Rc<str>, ContValue)`. The probe measured 104 B per node. Empty lists share a thread-local node (`:56-65`). A lambda body starts with a fresh `ContEnv` binding only `cont_param` (`application.rs:160`). The dynamic "stack" is therefore `ContValue::Local → cont_env → Local → …`, and capture is O(1) with respect to Scheme depth.
- **`ContValue`** (`cont_value.rs:220-403`, 64 B) has 16 variants:
  - `Local{param, body: Rc<CpsExpr>, env: Rc<Environment>, cont_env}`, `Halt`, `Captured(Rc<CpsContinuation>)` (never constructed, kept for Q2).
  - Effect wrappers chained by **`Box<ContValue>`**: `CallWithValuesConsumer`, `ForceCache`, `ResumePrimitive{index, state: TV, original_cont}`, `DynamicWindSetup/Cleanup/AfterDone`, `Jump{entered, value, target: Rc<CpsContinuation>}`, `ExceptionHandlerCleanup`, `RaiseHandlerReturn`, `PromptBoundary{id}`, `AbortLanding`, `ExitLanding`, `ComposableInvokeStep`.
  - About a dozen of these fields are bare `TaggedValue`s (consumer, promise, state, after, body, result_value, value, original_exception, handler, delimited, …).
  - [V] `cont_env.get(k).clone()` deep-copies the `Box` wrapper chain down to the first `Local` on every `App`/`Continue` (`step.rs:245-248`).
- **`CpsContinuation`** (`continuation.rs:22-137`, 208 B) holds:
  - `body`, `param`, `env`, `boundary`, and `trampoline` (an id, since a chain may end in a nested run's `Halt`), plus `crosses_callback`;
  - **copies** of `dynamic_winds`, `prompt_stack` and `exception_handlers` (`Vec`s cloned at capture, `continuation.rs:183-185` in the tree-walker);
  - `captured_cont_env` and `resume: Option<ContValue>`.

  Reified into the heap as `HeapObjectData::Continuation(Rc<CpsContinuation>)` (`heap/mod.rs:173`, `continuation.rs:191-211`).
- **Invocation mutates environments.** Invoking a `Local` does `captured_env.define(param, value)` (`continuation.rs:239`). That writes the returned value into the `LetCont`'s existing `Environment` and does not allocate a new frame. [I] This is an old→young store site for any generational scheme.
- **Escapes.** A jump into a continuation owned by another run parks `(value, Rc<CpsContinuation>)` in the `PENDING_ESCAPE` thread-local and unwinds the Rust stack as `Err(ContinuationEscape)` (`wind.rs:111-115`, `types.rs:20-30`). The loop catches it (`mod.rs:312-353`). `EscapeRoots` roots it (`gc_roots.rs:40-46`).

### 1.4 Environments

`Environment` (`patina-core/src/environment.rs:487-547`, 224 B, guarded by the `environment_size_is_watched` test) holds:

- `heap: SharedHeap` (an `Rc<RefCell<Heap>>` clone per frame);
- `bindings: RefCell<Bindings>`, where `Bindings` is a `SmallVec<[(Rc<str>, TV); 3]>` plus a hash index above 8 entries (`:208-219`);
- `scoped_bindings: RefCell<FxHashMap<Rc<str>, SmallVec<[ScopedBinding;1]>>>`, `alias_bindings`, two latch flags, `rare: OnceCell<Box<RareTables>>` (import links/owners, introduced globals), and `parent: Option<Rc<Environment>>`.

[V] Lookups are **by name with scope sets at run time**: `lookup_var_tagged` walks parents with `env.get`/`get_with_scopes` (`tree-walker eval/cps_eval/environment.rs:79-103`).

Frames are created:
- per call (`application.rs:79`);
- per `LetVal` run, unless the frame-sharing condition holds (`step.rs:151-158`);
- for source-written parameters, `define_scoped_definition` also allocates a hashbrown table. [I] The 364 B allocation seen once per call (4 buckets × 88 B plus control bytes) matches that.

The same `Environment` type is the VM's global environment (`VmClosure.globals`, per-site global caches keyed by `env_id`).

### 1.5 Closures and code

- `make_cps_closure_tagged` (`tree-walker environment.rs:137-173`) allocates `HeapObjectData::Procedure(Rc<Procedure::CpsLambda{params: Vec<ScopedParam>, variadic, cont_param, body: Rc<CpsExpr>, env: Rc<Environment>, binding_scopes}>)`. Sizes: heap slot 72 B, `Procedure` 120 B (the probe saw 136 B `RcBox`es once per closure), plus a `Vec` of params. [I] That is about 300 B of Rust memory per closure.
- **Literals live inside code.** `CpsExprKind::Literal(TaggedValue)` is stored in immutable `Rc<CpsExpr>` trees (`CpsExpr` 176 B). The tracer walks whole trees with a per-collection node-address dedup set (`cps_expr.rs:143-198`, `gc.rs:642-646`). The VM instead lifts constants into `CodeObject.constants` (GC_DESIGN §4.4).
- There is no CpsExpr cache: each top-level form is re-transformed (`mod.rs:448-483`).

### 1.6 Dynamic state: `call/cc`, `dynamic-wind`, prompts, handlers

- **Claimed at apply time.** Control primitives are recognised by the *primitive's* name, not the spelling at the call site (`application.rs:176` `match *name`). Covered: `call/cc`, `dynamic-wind`, `with-exception-handler`, `raise*`, `error`, `apply`, `call-with-values`, `force`, `exit`, `call-with-continuation-prompt` and `abort-current-continuation`.
- **`dynamic-wind`.** Builds `DynamicWindRecord` (`continuation.rs:173-219`, 40 B: id, before/after `TV`, `handlers: Rc<[ExceptionHandler]>`). It is pushed by `DynamicWindSetup` and popped by `DynamicWindCleanup` (`application.rs:515-581`, `continuation.rs:404-479`).
- **Jumps.** Jumps travel one thunk per step through `ContValue::Jump` (`wind.rs:47-116`, `next_wind_step` in `continuation.rs:232`). Each thunk is an ordinary traced step, not a nested run.
- **Aborts.** An abort captures two continuations, `delimited` (reified with `alloc_continuation`, `prompts.rs:265`) and a landing. It then jumps.
- **Stack copies.** The three stacks are `Vec`s copied into every captured continuation and into callback contexts (`callback.rs:41-47`, `:113-119`).

### 1.7 Resumable primitives and the remaining nesting

- **Resumable primitives** return `Step::Call`/`Step::Eval`. The tree-walker turns a `Call` into an `ApplyProc` whose continuation is `ContValue::ResumePrimitive{index, state, original_cont}` (`application.rs:1031-1069`), so the primitive's state is a traced `TaggedValue` and nothing sits on the Rust stack. `Step::Eval` expands the datum (desugaring may load libraries, which nest) and continues on the same trampoline under `EVAL_K` (`:1071-1129`).
- **Non-resumable primitives** run inside `apply_cached_owned(args, &CallbackContext)` (`:942-955`). The context carries `&` borrows of the step's stacks, while `cont` and `cont_env` sit in the caller's Rust frame.
- **Where nested runs still occur:**
  - `CallbackContext::apply_proc`/`eval_expr`/`eval_core` (`callback.rs:32-131`), used by `run_synchronously` (`patina-primitives/src/registry.rs:98-121`) for callers with no machine;
  - library initialisation (`eval/mod.rs:768-…`, `:518`);
  - embedding `apply_from_direct_tagged` (`wind.rs:245-288`).

### 1.8 Hidden roots and thread-locals

| Item | Holds values? | Rooted by |
|---|---|---|
| `PENDING_ESCAPE` (`types.rs:20-30`) | TV + `Rc<CpsContinuation>` | `EscapeRoots` |
| `ACTIVE_TRAMPOLINES`, `NEXT_TRAMPOLINE`, `UNHANDLED_IN_CALLBACK` (`types.rs:66-70`) | ids/flags only | — |
| `EMPTY_CONT_ENV` (`cont_value.rs:56-65`) | none | — |
| Suspended outer `StepResult` parts in nested runs | yes | deferral only (GC_DESIGN §5.1/§7) |
| Embedder-held results of `eval` | yes | **nothing**; there is no handle API (grep found no pin/root registration) |

### 1.9 Heap-side obligations specific to the tree-walker

- **[V] Two tree-walker-only object kinds carry Rust ownership:**
  - `Procedure(Rc<Procedure>)` / CpsLambda, which owns `Rc<CpsExpr>` and `Rc<Environment>`;
  - `Continuation(Rc<CpsContinuation>)`.

  They are released only when the sweep tombstones the slot (`HeapObjectData::Free`, `heap/mod.rs:224-228`; GC_DESIGN §4.5, §8). This is load-bearing: environment↔closure cycles are broken that way.
- **[V] The VM has a parallel dependency.** Code-unit release counts down freed closures through `take_gc_freed_closure_code_ids` (`heap/mod.rs:636`, `vm_state.rs:533-560`).
- **[I] Both dependencies break under any collector that does not visit dead objects** (a copying nursery, a semispace). Dead closures would leak their environments, code trees and code units.

### 1.10 What is traced today

`gc_roots.rs:29-134`, plus `gc.rs:1137-1292` for the `ContValue`/`ContEnv`/prompt/handler walkers:

- `global_env`, the registry and the escape slot;
- the step: value/proc/args, expression literals, env chain (deduplicated per env), `ContEnv` (deduplicated per chain head and queued iteratively since the Larceny family 6 fix), each `ContValue`'s `Box` chain iteratively, and the stacks;
- reached `CpsContinuation`s, through `trace_continuation_children` (`gc.rs:796-826`).

Dedup is mandatory: without it the trace is exponential (6.8 s at depth 26, GC_DESIGN §9.4).

---

## 2. Measurements

### 2.1 Type sizes

[V] `std::mem::size_of`, probe crate:

| Type | Size (bytes) |
|---|---|
| `TaggedValue` | 8 |
| `ContValue` | 64 |
| `ContEnv` | 8 (node 104) |
| `CpsContinuation` | 208 |
| `PromptFrame` | 112 |
| `DynamicWindRecord` | 40 |
| `ExceptionHandler` | 8 |
| `Environment` | 224 (`RcBox` 240) |
| `CpsExpr` | 176 |
| `CpsExprKind` | 112 |
| `Procedure` | 120 |
| `HeapObjectData` | 72 |
| `Option<SourceLocation>` | 64 |

### 2.2 Allocation profile

[V] Release build, `PATINA_GC=0` for exact heap counts; default GC gives the same Rust counts. "Envs" counts 240-byte `Environment` boxes.

| Program | Time | Rust allocs | Rust bytes | Envs | GC-heap allocs | Collections with GC on |
|---|---|---|---|---|---|---|
| `(fib 25)` (242,785 calls) | 219 ms | 3,156,671 | 479 MB | 1,092,538 | **43** | 0 |
| named-let loop to 1e6 | 1056 ms | 14.0 M | 2.1 GB | 5.0 M | 419 | 0 |
| build 1e5-element list ×10 | 1040 ms | 14.0 M | 2.1 GB | 5.0 M | 1.0 M | 10 |
| 1e5 closure creations | 165 ms | 2.5 M | 0.40 GB | 0.8 M | 100 k | 1 |
| 1e5 `call/cc` escapes | 180-193 ms | 2.7 M | 0.42-0.47 GB | 0.9 M | 200 k | 3 |
| 1e5 `dynamic-wind` | 180-189 ms | 3.3 M | 0.47-0.54 GB | 1.1 M | 300 k | 5 |
| `map` over 1e5 ×10 | 1920 ms | 26.4 M | 4.1 GB | 9.5 M | 1.1 M | 4 |

[V] **The GC trigger sees about 1 allocation in 10⁴-10⁵ of what the tree-walker actually allocates.** The rest is `Rc`/malloc managed.

### 2.3 Profile of `(fib 32)`

[V] macOS `sample`, 2,765 samples:

- malloc/free family: 618 (**22%**);
- drop glue (`Environment`, `ContEnvNode::drop_slow`, `ContValue`, scoped/alias hash tables, `RareTables`): 158 (**6%**);
- name lookup (`Bindings::slot_of` + `memcmp`, `Environment::get`, `visible_scoped_index`, …): about 952 (**34%**);
- `run_trampoline` and `eval_one_step` self time: 481.

About 0.9 µs per Scheme call. No collection ran.

### 2.4 Collection cost

- **Full collection with a big live set.** [V] `(gc)` with 300 k live pairs plus 300 k vectors: **about 3.1 ms** with GC on (heap arenas compact), and 7.8-8.7 ms after a `PATINA_GC=0` run had grown the arenas. The difference is the sweep over never-shrunk arenas.
- **Fresh session.** `(gc)` after bootstrap: 0.09 ms (about 390 live objects). After loading four large libraries: **1.73 ms**, almost all sweeping 185 k object slots and 300 k pair slots of load-time garbage.
- **Cost per suspended frame.** [V] Collection under deep non-tail recursion: depth 10 k adds about 1.4 ms and depth 50 k adds about 6.9 ms beyond the 3.07 ms baseline. That is **about 140 ns per suspended Scheme frame**, against about 5 ns per heap object marked [I, from 3 ms / 600 k objects]. Off-heap continuation chains are about 30× costlier to trace than heap data, because of the hash-set dedup inserts per env, chain and expression node.

---

## 3. What a redesigned collector would collide with

Each item below was found in the code above. Each is classified by the feature that triggers it.

1. **Collection at allocation.** No persistent register file exists (§1.2). Live values sit in moved parameters and locals throughout `cps_eval` (4,613 LOC across 11 files), and in `patina-primitives` (shared with the VM). Rooting them needs HandleScope/CAMLlocal-style registration, plus a refactor that keeps the machine state in a GC-visible struct.
2. **Moving or compaction.**
   - Shared immutable Rust structures hold bare `TaggedValue`s: about 20 field sites across `ContValue`, `PromptFrame`, `ExceptionHandler`, `WindRecord`, `StepResult`, `CpsExprKind::Literal` and `PENDING_ESCAPE`. A moving collector can update these only if they become `Cell<TaggedValue>`, or if the GC writes through `UnsafeCell`.
   - Every *copy* must be visited, not just one per object. Marking tolerates aliased copies and the code says so (`gc.rs:806-819`); moving does not.
   - `Environment` slots are already `RefCell`, so they can be updated at a safe point with no outstanding borrows.
3. **A nursery that does not visit dead objects (copying or semispace).** The drop-at-sweep obligations in §1.9 leak: the environment, code tree and continuation payloads of dead closures and continuations. The VM's code-unit release has the same problem.
4. **Generational barriers.**
   - Heap-reference stores into Rust structures bypass any heap barrier: environment `define`/`set` (including continuation-return `define`, `continuation.rs:239`), the `LetVal` frame extension (`step.rs:152`) and `Parameter` stacks.
   - Off-heap environments have no age. A minor GC must either scan every reachable environment, which is the whole static chain including the global and library environments with hundreds of bindings each, or keep a dirty/remembered mechanism.
5. **Pointer encoding or header changes for the JIT.** [V] The tree-walker never touches raw indices or bits: grep finds no `heap_index`, `raw_bits` or `HeapIndex` in the crate, so an encoding change is transparent to it. It does go through the heap `RefCell` 84 times (37 `borrow_mut`, 47 `borrow`). If the JIT forces a borrow-free mutator API, the tree-walker's call sites are mechanical to migrate.
6. **Safe-for-space.** The `strong_count == 1` frame-sharing test (`step.rs:151`) relies on `Rc` refcounts. A heap-resident environment would need a "captured" bit set by closure and continuation creation to keep SRFI 45 leak test 3 bounded (39 MB vs 895 MB per the code comment).

---

## 4. Options

### (a) Same heap, full precise rooting, collection may happen at allocation

**What it takes:**
- Persistent machine registers (`StepResult` fields stored in an evaluator-owned struct rather than moved through calls).
- Shadow-stack or handle rooting for every Rust local holding a `TaggedValue` across an allocation, in the tree-walker and in all shared primitives.
- `Cell`-ified `TaggedValue` fields so the collector can update them.
- An updatable `PENDING_ESCAPE`.
- A slot-based visitor API.

**Prior art:**
- OCaml `CAMLparam`/`CAMLlocal` local-root blocks (`~/Project/reference/ocaml/runtime/caml/memory.h:290-347`), with minor GC possible at any `Alloc_small` (`:211-216`).
- Racket BC "3m": the `xform` source-to-source pass maintains `GC_variable_stack` for the C runtime (`racket/src/bc/gc2/README`, "At any point when the allocator/collector is invoked, Racket will have set the GC_variable_stack…"). Racket later moved to Chez.
- V8 `HandleScope`.
- SpiderMonkey `Rooted<T>`, plus a static rooting-hazard analysis that exists because hand rooting is error-prone (https://firefox-source-docs.mozilla.org/js/HazardAnalysis/index.html).

**Cost:** [I] about 6-10 engineer-weeks if limited to the tree-walker, but it cannot be limited: the primitives are shared, so the VM pays the same rooting tax. **Risk: high.** A missed root is a use-after-free found only by stress lanes, Rust's borrow rules fight shared mutable roots, and allocation-heavy primitives get slower.

**Benefit for the tree-walker:** collection inside long steps and nested runs. [I] Low value, because steps are already short (§1.1).

**Verdict:** not worth it for the tree-walker. A design that forces it on the shared primitives is a strong argument against collecting at allocation anywhere.

### (b) Precise enumeration, no update: pin everything the tree-walker's Rust structures reference

Keep today's tracer (enumerate only) and report those references as **non-transitively pinning roots**. Objects reachable only through other heap objects (list tails, vector elements) can still move.

Note that this is *precise* pinning. True conservative stack scanning (Boehm/Whippet style) does **not** fit a Rust host:
- live values sit in malloc'd `Vec`/`SmallVec`/`Box` buffers (`args`, stacks, `ContValue` chains), not on the machine stack;
- small arena indices are fixnum-like words, so false positives would be rampant until a pointer encoding lands.

**Prior art:**
- MMTk's `RootsWorkFactory::create_process_pinning_roots_work`: "useful for conservative stack scanning, or VMs that cannot update some of the root slots". It also has a transitively pinning variant (https://github.com/mmtk/mmtk-core/blob/master/src/vm/scanning.rs).
- Chez `lock-object` ("prevents the storage manager from reclaiming or relocating the object", `csug/smgmt.stex:1023-1041`). Chez's collector **marks in place** on segments holding immobile or locked objects and promotes those segments (`c/gc.c:35-79`).
- Whippet mmc: referents of conservative roots are pinned, while others may still be evacuated (https://github.com/wingo/whippet/blob/main/doc/collector-mmc.md).

**Cost:** [I] about 1-2 weeks in the tree-walker: a root-kind flag on `GcRoots`, plus routing `visit` calls from Rust-side structures to "pin". The real cost lands on the collector. With a copying nursery, a pinned nursery object must be promoted in place, which needs Immix-style line or block marking.

**Risk:** medium-low for correctness, since today's enumeration already works. Space risk too: nearly every live young object is directly referenced from some environment binding, so tree-walker nurseries would barely evacuate. [I] That is acceptable for a non-performance tier.

### (c) Make environments, continuations and closures heap objects

**What it takes:**
- Frames become heap records: parent plus (symbol, value) pairs, with names as immortal interned symbols. The runtime-scoped lookup (hygiene families 36/38) moves onto those records.
- `ContValue`/`ContEnv` and `CpsContinuation` become heap objects.
- Code trees are referenced by id from a code store, as the VM does.
- `StepResult` becomes a GC-visible register file.

**Prior art:**
- SML/NJ's heap-allocated CPS frames.
- Appel 1987, "Garbage collection can be faster than stack allocation" (IPL 25(4)).
- Chez represents control as heap-allocated stack segments (Hieb, Dybvig and Bruggeman, PLDI 1990).
- GHC treats stack chunks as heap objects with a `dirty` flag (`StgStack.dirty`, https://github.com/ghc/ghc/blob/master/rts/include/rts/storage/TSO.h).

**Upside:**
- One allocator. Frames are bump-allocated and die young, which removes the malloc/free/drop share (about 28% of fib time, §2.3).
- Uniform barriers.
- Moving becomes possible.
- Continuations become first-class heap data, which suits the debugger's continuation ids and green threads.

**Cost:** [I] 6-12 engineer-weeks. It touches all 4,613 LOC of `cps_eval`, `cont_value.rs` (403 LOC) and a split of `environment.rs` (3,389 LOC, which is shared with VM globals).

**Risk: high.**
- Runtime hygiene resolution.
- The `strong_count` space trick needs a replacement.
- The 24-shape `control_flow_matrix`, prompts and winds.
- Lookup cost (34%) is untouched unless lexical addressing is added, which turns the backend into the VM.

**Verdict:** a separate project, if tree-walker performance ever matters. It is not a prerequisite for the GC redesign.

### (d) Collect only at the tree-walker's existing safe points; the VM/JIT may collect more freely

**This is today's model.** The prior art matches it exactly:
- **Chez.** `S_get_more_room` finds room (a new segment) and only *queues* a collect request (`c/alloc.c:207-219`, `:500-556`). `S_fire_collector` sets `collect-request-pending` and every thread's `something-pending` (`c/schsig.c:574-592`). The collection runs later in the `event` handler, which also serves timer (engines), signals and keyboard interrupts (`s/library.ss:1194-1238`).
- **gc-arena.** "`Arena::mutate` must *return* for collection to be performed" (https://github.com/kyren/gc-arena).
- **piccolo's stackless `Sequence` callbacks.** Control "will always continuously return outside the GC arena" (https://github.com/kyren/piccolo). Patina's resumable `Step::Call`/`Step::Eval` (#477, #478) is the same move.

**Cost:** near zero for the tree-walker. The heap must be able to over-allocate between safe points (a soft limit), with a hard limit that raises out-of-memory, as Chez's `find_room` does.

**Risk:** low. Known gap: long nested runs (library loads, debugger pauses) cannot collect. The fix is to root those boundaries rather than defer:
- make `ParsedLibrary`'s `body: Vec<TaggedValue>` a `GcRoots` provider;
- record the suspended step's `cont`, `cont_env` and stacks in a boundary frame when a primitive's callback starts a run.

[I] The JIT does not need allocation-site collection either. Its allocation slow path can grab a fresh block and set the flag, with polls at back-edges, calls and returns, as Chez does. The cost is temporary heap overshoot.

### (e) A simpler separate collector for the tree-walker

[V] The backends are independent crates. `TreeWalker::new`/`with_fs`/`from_evaluator` and `VmBackend::new`/`with_fs` each create their own `Environment::new()` heap, and no constructor accepts a heap (`crates/patina-*/src/backend.rs`). So a **per-heap collector policy** is safe today.

A literally separate GC implementation is not viable: the object model and `patina-primitives` are shared, and two heaps would double primitive maintenance. **The practical form is (e′): one object model and allocator, with the tree-walker's heap configured non-moving.** Examples:
- an Immix or mark-region space with evacuation disabled;
- non-moving generational collection with sticky mark bits: MMTk StickyImmix, where nursery copying is optional (`prefer_copy_on_nursery_gc`); Whippet mmc "generational tracing via the sticky mark-bit algorithm" with card marking;
- or simply full-heap-only collections. At 3 ms for 600 k live objects that is acceptable for this tier.

**Cost:** [I] low for the tree-walker (1-3 weeks, mostly the payload-handle change below). It is a requirement on the collector: a non-moving mode and pinning must be first-class. **Risk:** low-medium.

### Cost and risk summary [I]

| Option | Tree-walker effort | Effect on the shared design | Risk | Fit |
|---|---|---|---|---|
| a | 6-10 weeks, plus VM primitives | Forces rooting on all primitives | High | Poor |
| b | 1-2 weeks | Collector needs pinning and in-place promotion | Medium-low | Good |
| c | 6-12 weeks | Splits `Environment`, new object kinds | High | Later, optional |
| d | ~0 | Contract: allocation never collects | Low | **Yes** |
| e′ | 1-3 weeks | Collector needs a non-moving mode | Low-medium | **Yes, combined with d and b** |

---

## 5. Recommended tree-walker shape under the redesign [I]

1. **(d) Safe-point-only collection.** Generalise the pending flag into an *event word*, Chez's `something-pending`, covering GC requests, debugger breaks, engine fuel and interrupts. The JIT polls the same word.
2. **(e′) plus (b).** The tree-walker heap runs non-moving. Its Rust-side references are reported through a "pinning root" visitor category, so the same collector code serves both backends and pinning stays available for embedder handles and debugger-retained values.
3. **Kill drop-at-sweep.** Replace `Procedure(Rc<Procedure>)` (CpsLambda) and `Continuation(Rc<CpsContinuation>)` with small heap objects that carry an **id into an evaluator-owned side table**. Mark them through `GcRoots::trace_weak_ids` and prune them with `sweep_weak` (the §9.5 pattern already used for `VmContinuationRef`).
   - It works for copying and non-copying plans alike.
   - It removes Rust `Drop` payloads from the heap object model, which a JIT-friendly headered layout wants anyway.
   - It preserves the §8 cycle argument: an unreached id drops the `Rc<Environment>`.
   - The alternative is a per-nursery finalisation table, like OCaml's custom table (`runtime/minor_gc.c:785-806`).
4. **Per-form constants table** for `CpsExprKind::Literal`. Collect a form's literals into one slice at CPS transform time and have nodes index it. This replaces the per-collection tree walk with hash dedup (`gc.rs:642`), and the slice becomes updatable if moving is ever allowed. It mirrors the VM's `CodeObject.constants`.
5. **If tree-walker heaps go generational:**
   - `Environment` gets a dirty bit and a remembered list (`Weak`), set on stores of heap references (the `Bindings::insert`/`write_slot` choke points). This is GHC's dirty `STACK` idea.
   - Immutable nodes (`ContEnvNode`, `CpsContinuation`) carry a creation epoch, so a minor GC scans only nodes created since the last GC: an older immutable node cannot point to a younger one.
   - This cuts the 140 ns/frame full re-trace (§2.4) down to new frames only.
   - [I] Optional. Full-heap collection is acceptable for this tier.
6. **Open root registration** on `Evaluator`, replacing the closed array at `mod.rs:130`. It is needed for debugger hooks, green-thread schedulers, embedder handles and `ParsedLibrary`.
7. **Account for off-heap memory in the trigger**, like OCaml's `caml_alloc_custom_mem` or V8's external-memory adjustment. Closure↔environment cycles are reclaimed only when the heap trigger fires, and the trigger sees almost none of the tree-walker's allocation (§2.2).

---

## 6. Constraints from `PRD/future/TREE_WALKER_HOOK_SYSTEM.md` and `VISUAL_DEBUGGER_DESIGN.md`

### What the hook layer requires

- **H1 makes hook storage a root.** The hook design is `DebugHook: GcRoots`, with `Evaluator`'s `GcRoots` forwarding to the installed hook (§5.2). A closed root array blocks this. Retained values are watchpoint targets (`Watch … Becomes(TaggedValue)`), recorded macro datums (debugger §5, "must be traced and bounded") and trace logs. **Under moving these must be updatable, or pinned.**
- **No GC while paused (§5.2).** A hook that evaluates Scheme (`p <expr>`, conditional breakpoints) re-enters `eval_in_env` under the outer guard. A client evaluating in a loop while paused grows the heap.
  - Fixing it means rooting the paused boundary. The `DebugEvent` payload borrows parts of the live step (`&CpsExpr`, `&Rc<Environment>`, `args: &[TaggedValue]`, `&ContValue`).
  - **With a moving collector, those borrowed values go stale across a nested collection.** So either keep "no collection during a pause", or pin the event payload for the pause's duration.
- **Fire sites assume today's step structure.** The sites are the top of the `eval_one_step` inner loop, `apply_cps_step`, `invoke_continuation_step`, the `Set`/`Define` arms, three `Raise` sites and the escape catch at `mod.rs:312`. The `Unwind` event carries `(value, Rc<CpsContinuation>)`. **If continuations become heap handles (§5.3), the event carries a handle that must stay rooted while the hook runs.**
- **Identity must not be address- or bit-based.**
  - Debugger D0 requires a `u64` id stamped on continuations, mirroring `DynamicWindRecord.id`, because `Rc::ptr_eq` on bodies conflates activations.
  - `SourceMap.locations` is keyed by raw `TaggedValue` bits and pruned at form boundaries; the hook doc tells clients to store `SourceLocation` values and never those keys.
  - [I] A moving collector invalidates raw-bit keys *within* a form, so this rule becomes mandatory.

### Interactions with threads, the JIT and performance

- **Green threads (§9.2).** "A green thread *is* a `StepResult` plus its dynamic state", held by a scheduler. That needs a root provider over the scheduler's suspended steps. All-stop debugging parks threads "the way it parks them for a collection: one flag, two clients", so the safe point should be a general event check (see the Chez `event` entry above). Engines and step budgets also hook the same point.
- **Abort and unwinding.** `DebugDecision::Abort` is an `EvalError` that unwinds by `?` without running wind thunks. That is compatible with deferral. Any shadow-stack rooting (option a) would need RAII pops.
- **Tier policy (§10.1).** Hooks attached ⇒ don't tier up. The JIT's GC stack maps and safepoints are "designed once"; debugging deopt may piggyback on them but is not promised. [I] So the tree-walker and the VM must remain safepoint-polling tiers. A JIT design must not make hooked execution impossible, for example by eliminating the interpreter's ability to stop at safe points.
- **Time travel stays excluded.** Environments are mutable and `Rc`-shared (debugger §8). A heap-object environment (option c) would not change that by itself.
- **Hot-loop budget (§7).** The hook adds one `Option` check per inner-loop iteration. A GC event-word check adds one more per step. The existing safe point is one load plus a branch (`gc.rs:381-397`).

### Stale statements in the docs (for whoever updates them)

- GC_STAGE5 Priority 2 and the `collection_inside_higher_order_primitive` test comment still describe `map` as a nested trampoline. `map` is Scheme now (#471).
- Hook doc §4(c) says re-entry resets prompt and handler stacks. `resume_step` now restores both (`continuation.rs:32-42`).
- Hook doc §7 says `eval_one_step` deep-clones the expression. It is an `Rc` clone now (`step.rs:32`).
- GC_DESIGN §3.3 shows an old `Environment` layout.

---

## 7. Lessons and constraints for the redesign

- Design the heap contract as **"allocation never collects synchronously; collection only at polled safe points."**
  - The tree-walker has no register file.
  - Shared primitives hold `TaggedValue`s in locals and `Vec`s across allocation.
  - Chez, gc-arena and piccolo all work this way.
- **Provide a non-moving mode and/or pinning roots as first-class collector features.** MMTk-style non-transitively pinning roots, and Chez/Whippet-style in-place marking of segments with pinned objects.
- **Make the visitor slot-based from day one**, even if the first plan is non-moving. Every Rust-side `TaggedValue` copy must be reachable as an updatable slot, or reported as pinning. Marking-only tracers hide skipped aliases.
- **No collector may rely on visiting dead objects.** Convert drop-at-sweep obligations (CpsLambda environments and code, `CpsContinuation`, VM `take_gc_freed_closure_code_ids`, ports) into weak-id side tables or finalisation tables.
- **Keep the tree-walker's representation decisions out of the JIT's critical path.** It treats `TaggedValue` opaquely, so it survives an encoding change. It does need a borrow-free heap API migration (84 sites) if the heap's `RefCell` goes.
- **Preserve safe-for-space:**
  - frame sharing must not capture later bindings (SRFI 45 test 3);
  - continuation-chain tracing must stay iterative and deduplicated (Larceny family 6, GC_DESIGN §9.4).
- **Replace deferral with explicit boundary roots** where it hurts: library loading (about 485 k garbage slots loading four libraries, measured), debugger pauses, embedding calls.
- **Keep the differential lanes**, and add a "move everything every GC" stress mode if any moving is allowed anywhere. Stress lanes are the only net for missed roots.
- **Expose rooted handles for embedders.** `Interpreter::eval*` returns raw `TaggedValue`s that are unrooted between forms, in both backends.

---

## 8. Open questions

1. **Is the tree-walker a long-term production backend, or the reference, oracle and debugging tier?** This decides whether option (c) is ever worth funding.
2. **Will the chosen collector have a copying nursery with no in-place promotion?** If so, the tree-walker needs a separate non-moving heap configuration (e′) or `Cell`-ification of about 20 field sites plus updatable literals.
3. **Should a per-heap policy be codified?** Today it is safe only because no API shares a heap across backends. Should "one backend per heap" be documented as an invariant?
4. **Should tree-walker heaps be generational at all,** or full-heap only (3 ms per 600 k live objects measured; about 140 ns per suspended frame)?
5. **Should the GC trigger account for off-heap (`Rc`) bytes?** If so, how, without per-allocation cost on the 13-mallocs-per-call path?
6. **How should a paused debugger collect** (pin the event payload vs a boundary root frame)? Is that needed for H1, or deferred?
7. **For heap-resident environments (if option c ever happens): what replaces `Rc::strong_count` for frame sharing?** For example, a capture bit, or static escape analysis in the CPS transform.

---

## References

**Repository files**
- `crates/patina-tree-walker/src/eval/cps_eval/{mod,types,gc_roots,step,application,continuation,wind,prompts,callback,environment}.rs`
- `crates/patina-core/src/{cont_value.rs, continuation.rs, environment.rs, cps_expr.rs, procedure.rs, heap/mod.rs, heap/gc.rs}`
- `crates/patina-runtime/src/library_loader.rs`
- `crates/patina-primitives/src/registry.rs`
- `crates/patina-vm/src/runtime/vm_state.rs:500-565`
- `docs/GC_DESIGN.md` (§3-§9)
- `PRD/ARCHIVE/GC_STAGE5_PRD.md`
- `PRD/future/TREE_WALKER_HOOK_SYSTEM.md`
- `PRD/future/VISUAL_DEBUGGER_DESIGN.md`
- `crates/patina-tests/tests/gc_tree_walker.rs`
- `scripts/run_gc_differential.sh`

**Chez Scheme** (local checkout)
- `~/Project/reference/ChezScheme/c/alloc.c:207-219,500-556`
- `c/schsig.c:574-592`
- `c/gc.c:23-110`
- `s/library.ss:1194-1238`
- `csug/smgmt.stex:30-60,331-360,1022-1041`
- `csug/foreign.stex:262-275`

**OCaml** (local checkout)
- `~/Project/reference/ocaml/runtime/caml/memory.h:211-347`
- `runtime/minor_gc.c:785-806`

**Racket BC** (local checkout)
- `~/Project/reference/racket/racket/src/bc/gc2/README`

**Web sources**
- gc-arena README: https://github.com/kyren/gc-arena
- piccolo README: https://github.com/kyren/piccolo
- MMTk `memory_manager.rs` (`pin_object`, `alloc`) and `vm/scanning.rs` (pinning roots), plus `plan/sticky/immix`: https://github.com/mmtk/mmtk-core
- Whippet `doc/collector-mmc.md`: https://github.com/wingo/whippet
- SpiderMonkey rooting hazard analysis: https://firefox-source-docs.mozilla.org/js/HazardAnalysis/index.html
- GHC `StgStack.dirty`: https://github.com/ghc/ghc/blob/master/rts/include/rts/storage/TSO.h

**Literature**
- Appel, "Garbage collection can be faster than stack allocation", IPL 1987.
- Hieb, Dybvig and Bruggeman, "Representing control in the presence of first-class continuations", PLDI 1990.
