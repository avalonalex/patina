# Garbage Collection Design

**Status:** Approved design, not yet implemented
**Date:** 2026-07-31 (file/line references are as of this date)
**Supersedes:** `PRD/ARCHIVE/phase1_optimization_2026_02/GC_DESIGN.md` (pre-TaggedValue, proposed `rust-gc` over the deleted `Value` enum)
**Extends:** `PRD/TRACK_P_PERFORMANCE_PRD.md` §P6 (VM-only mark-and-sweep sketch)

---

## 1. Problem Statement

Neither backend reclaims heap memory. The `Heap` has per-arena free lists that
every `alloc_*` drains, but **nothing ever pushes to them** — long-running
programs grow the arenas monotonically. Additionally, R7RS allows cyclic
structures (`set-cdr!`, closures capturing their own environment), which the
`Rc`-based ownership of environments and heap payloads can never reclaim even
after the whole cluster becomes unreachable.

Goals:

1. Both backends collect garbage — the tree-walker favoring simplicity and
   obvious correctness, the VM with headroom for future performance work.
2. Cycles are reclaimed, including closure ↔ environment cycles.
3. The collector is pluggable: the algorithm can be swapped (e.g. generational
   for the VM later) without touching backend root enumeration, and vice versa.
4. The default build is unaffected until the GC is proven (Cargo feature flag).

Non-goals (v1):

- **Moving/compacting collection.** Ruled out by the current architecture — see §3.4.
- **GC-managing environments.** They stay `Rc`; the collector traces *through*
  them. §8 explains why cycles are still fully reclaimed.
- **Concurrent or incremental collection.** Stop-the-world only.
- **Weak symbol interning.** Symbols are immortal in v1 (§9.2).

---

## 2. Key Decisions (summary)

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | Non-moving stop-the-world mark-and-sweep | Indices escape `TaggedValue` (§3.4); free lists + in-place slot reuse already exist |
| 2 | **One collector implementation shared by both backends** | Same `SharedHeap`, same object graph; backends differ only in roots + safe points |
| 3 | Pluggability = `Collector` trait (algorithm) × `GcRoots` trait (root providers) | The seam between "how to collect" and "what is live" is the stable boundary |
| 4 | Mark state = side bit-vectors per arena, owned by the visitor per collection | No `TaggedValue` bloat, no object header changes, no idle state on `Heap` |
| 5 | Sweep pushes to the existing free lists **and tombstones the slot** | Allocation path unchanged; tombstoning drops `Rc` payloads eagerly → breaks env cycles (§8) |
| 6 | Safe point = top of each backend's driver loop, guarded by a re-entrancy defer counter | All Rust-stack temporaries are dead or restored there (§7) |
| 7 | Environments traced via new `Environment::for_each_value`, deduped by `Rc` pointer | Globals/bindings live in Rust `HashMap`s outside the arenas |
| 8 | VM continuation side tables traced by `VmState`'s root provider, not the heap tracer | Heap only holds opaque `VmContinuationRef(u64)` — heap-only tracing under-approximates |
| 9 | Feature flag `gc`, off by default until stage 4 | Baseline builds must be bit-identical until differential testing passes |

---

## 3. Ground Truth: Current Memory Architecture

### 3.1 Heap layout

`Heap` (`crates/patina-core/src/heap/mod.rs:241-268`) is four typed `Vec`
arenas plus an intern table and four free lists:

```rust
pairs:   Vec<(TaggedValue, TaggedValue)>,
vectors: Vec<Vec<TaggedValue>>,
strings: Vec<Vec<char>>,
objects: Vec<HeapObjectData>,            // 26 variants, mod.rs:119-180
symbol_table: HashMap<String, HeapIndex>, // intern table, mod.rs:255
free_pairs / free_vectors / free_strings / free_objects: Vec<HeapIndex>,
```

The free lists are **drained** by `alloc_pair` (:307), `alloc_vector` (:357),
`alloc_string_chars` (:417), `alloc_object` (:874) — but never filled. The
sweep phase slots directly into this: reclaimed indices go onto the free lists
and the allocation path needs zero changes.

`SharedHeap = Rc<RefCell<Heap>>` (`mod.rs:48`). One heap instance is shared by
the parser, macro expander, environments, and both backends. Primitives take
short-lived `borrow()`/`borrow_mut()` guards constantly — any GC entry point
must hold no outstanding borrow.

### 3.2 Value encoding

`TaggedValue` (`crates/patina-core/src/tagged_value.rs:57`) is a 64-bit word
with a **low-3-bit tag and 61-bit payload** (`tagged_value.rs:9-21`). Despite
older doc comments saying "NaN-boxed", it is not — it is a tagged integer.
Heap references are **arena indices** (`HeapIndex = u32`, `tagged_value.rs:28`),
e.g. a pair is `(index << 3) | 0b011`. Index reuse is invisible to holders, so
a non-moving collector requires no handle rewriting.

### 3.3 Environments live outside the heap

`Environment` (`crates/patina-core/src/environment.rs:43-53`):

```rust
heap: SharedHeap,
bindings: Rc<RefCell<FxHashMap<String, TaggedValue>>>,
scoped_bindings: Rc<RefCell<FxHashMap<String, Vec<ScopedBinding>>>>,
parent: Option<Rc<Environment>>,
```

Environments are `Rc`-linked Rust structs, **bidirectionally entangled** with
the heap:

- **env → heap:** bindings hold `TaggedValue`s (bare arena indices, no ownership).
- **heap → env:** `HeapObjectData::EnvironmentSpecifier{env}`,
  `VmClosure{globals}`, `Procedure` (CPS lambda env), `CpsContinuation.env` all
  hold owning `Rc<Environment>`.

There is no traversal API today; the tracer needs a new
`Environment::for_each_value(&self, f: &mut dyn FnMut(TaggedValue))` that walks
`bindings` + `scoped_bindings` + the parent chain.

### 3.4 Why moving/compacting GC is off the table

Raw indices escape `TaggedValue` into places a relocator cannot see or would
have to be taught about:

1. `Heap::symbol_table` maps names to bare `HeapIndex` (`heap/mod.rs:255`).
2. `SourceMap` keys source locations by `tv.raw_bits()`
   (`crates/patina-frontend/src/source_map.rs:17-19,67,72`).
3. `eq?`/`eqv?`/hashing compare raw bits (`heap/mod.rs:1474,1507,1593,1719`).
4. `CallFrame.closure: Option<HeapIndex>` (`crates/patina-vm/src/types/mod.rs:52`).
5. `CodeObject.constants: Vec<TaggedValue>` in every compiled code object.
6. `CompiledMacro` captures literal `TaggedValue`s at macro-compile time
   (`crates/patina-core/src/compiled_macro.rs:461-465`).
7. Environment binding maps reachable only through `Rc` graphs.

Non-moving is therefore a hard constraint, and the `Collector` trait should
assume it (future implementations may be generational or incremental, but not
moving).

---

## 4. Architecture

Three layers. The tracer lives with the heap (it is intrinsic to heap layout);
roots live with their owners; policy is swappable.

```
┌────────────────────────────────────────────────────────┐
│ Collector trait (patina-core)                          │  policy: when / how
│   └─ MarkSweepCollector (v1, shared by both backends)  │
├────────────────────────────────────────────────────────┤
│ GcRoots trait (patina-core)                            │  what is live
│   ├─ impl for VmState            (patina-vm)           │
│   ├─ tree-walker loop roots      (patina-tree-walker)  │
│   └─ impl for LibraryRegistry    (patina-runtime)      │
├────────────────────────────────────────────────────────┤
│ Heap tracing + sweep (patina-core)                     │  mechanism
│   ├─ mark bit-vectors per arena                        │
│   ├─ GcVisitor (worklist) + per-variant trace rules    │
│   └─ sweep → free lists + tombstone                    │
└────────────────────────────────────────────────────────┘
```

### 4.1 Traits

```rust
// patina-core::heap::gc

/// Marking front-end handed to root providers and used internally by tracing.
pub struct GcVisitor<'h> { /* &mut mark bitmaps, worklist, env-dedup set */ }

impl GcVisitor<'_> {
    /// The normal edge: mark + enqueue any heap reference; no-op for immediates.
    pub fn visit(&mut self, v: TaggedValue);
    /// For bare object-arena indices (CallFrame.closure).
    pub fn visit_object_index(&mut self, i: HeapIndex);
    /// Trace through an environment chain; deduped by Rc::as_ptr so the
    /// global env is not re-walked once per closure.
    pub fn visit_env(&mut self, env: &Rc<Environment>);
    /// Trace a continuation / expression tree held outside the arenas.
    pub fn visit_continuation(&mut self, k: &Rc<CpsContinuation>);
    pub fn visit_expr_literals(&mut self, expr: &CpsExpr);
    /// Shared trace rules, so each is authored once rather than per backend.
    pub fn visit_promise(&mut self, p: &RefCell<PromiseState>);
    pub fn visit_wind(&mut self, w: &DynamicWindRecord);
    pub fn visit_winds(&mut self, w: &[DynamicWindRecord]);
    pub fn visit_library(&mut self, l: &Library);
    /// Dedup hook for root providers' own Rc-shared structures (§9.4).
    /// Returns false if this identity was already traced this collection.
    pub fn visit_once(&mut self, identity: usize) -> bool;
}

/// Implemented by anything that owns live values: backend state, registries,
/// and per-collection transient roots (the tree-walker's current StepResult).
pub trait GcRoots {
    fn trace_roots(&self, v: &mut GcVisitor<'_>);
}

/// Swappable algorithm. Non-moving is a contract: implementations may not
/// relocate live slots. Automatic policy is expressed as an allocation
/// threshold the controller installs into the heap (§6), not a per-query
/// method — the safe point never asks the collector anything.
pub trait Collector {
    fn collect(&mut self, heap: &mut Heap, roots: &[&dyn GcRoots]) -> GcStats;
}

pub struct GcStats {
    pub collections: u64,
    pub last_marked: usize,      // live slots per arena
    pub last_swept: usize,       // freed slots per arena
    pub last_pause: Duration,
}
```

Notes:

- `GcVisitor::visit_env` exists because environments are not heap objects; the
  dedup set (keyed by `Rc::as_ptr`) is essential — every closure points at the
  globals env, and without dedup marking would be quadratic.
- `collect` takes a slice of root providers: the driving backend passes itself
  plus the shared registries plus any loop-local roots.
- Borrow discipline: `collect` needs `&mut Heap`, so the caller must hold the
  single `borrow_mut()` for the whole collection — which is exactly why safe
  points must be borrow-free (§7). But `visit_env` and object-variant tracing
  read `Rc<RefCell<...>>` interiors (env binding maps, `Record.fields`,
  `Promise` state); those are *separate* RefCells from the heap's, so tracing
  them under the heap borrow is fine. The one self-referential case is
  `MutableCell(RefCell<TaggedValue>)`, which lives *inside* the object arena —
  the tracer reads it directly through the `&mut Heap` it already holds, not
  through a second borrow.

### 4.2 Mark state

Side bit-vectors, one per arena (`MarkBits`), sized to arena length at
collection start. Owned by the `GcVisitor` and created fresh per collection —
no persistent mark state on `Heap`, so there is no clearing step and no idle
memory between collections. No `TaggedValue` or `HeapObjectData` change.
The visitor also roots the intern table at construction (a dangling
`symbol_table` index would break *any* collector — heap invariant, not
policy), marking symbols without a worklist round-trip since they are leaves
by construction.

### 4.3 Trace rules

Iterative worklist; immediates (fixnum, special, char) are skipped by `visit`.

| Slot | Children |
|------|----------|
| Pair | car, cdr |
| Vector | each element |
| String | leaf |

Object arena, by `HeapObjectData` variant (`heap/mod.rs:119-180`):

| Variant | Children |
|---------|----------|
| `BigInt`, `Rational`, `Real`, `Symbol`, `Bytevector`, `Port`, `RecordType`, `Identifier`, `Library`*, `PromptTag`, `LabelPlaceholder` | leaf |
| `Complex` | `real`, `imag` |
| `Exception` | each of `irritants` |
| `Record` | `record_type` value + each of `fields` (through the `Rc<RefCell<Vec<_>>>`) |
| `Parameter` | each of `values` + `converter` |
| `Promise` | delayed thunk / forced value (through `Rc<RefCell<PromiseState>>`) |
| `Values` | each element |
| `MutableCell` | inner value |
| `VmClosure` | each of `free_vars` + **`visit_env(globals)`** |
| `Procedure` | captured env (`visit_env`) + **body-expression literals** (§4.4) |
| `Macro` | `CompiledMacro` pattern/template literal `TaggedValue`s (`compiled_macro.rs:78,:280`) |
| `Continuation` | `CpsContinuation`: env (`visit_env`), `dynamic_winds` before/after thunks, `captured_cont_bindings` (recursive), body literals (§4.4) |
| `EnvironmentSpecifier` | `visit_env(env)` |
| `VmContinuationRef`, `VmDelimitedContinuationRef` | **leaf for the heap tracer** — the real payload lives in `VmState`'s side tables and is traced by the VM's `GcRoots` impl (§5.2) |

\* `Library` heap objects wrap `Rc<Library>` whose `exports` map and `env` are
also reachable via `LibraryRegistry`; tracing them from the registry root
(§5.3) suffices, but tracing from the heap variant too is harmless and more
robust — do both.

### 4.4 Expression-tree literals

Live procedures keep their code alive, and code embeds heap values:
`CpsExprKind::Literal(TaggedValue)` / `Quasiquote{template}`
(`crates/patina-core/src/cps_expr.rs:158,:324`), analogous nodes in `CoreExpr`.
When the tracer reaches a `Procedure` or `Continuation`, it must trace the
literals in the body `Rc<CpsExpr>`.

- **Tree-walker:** walk the expression tree, memoized per collection by
  `Rc::as_ptr` (an expression shared by many closures is walked once).
- **VM:** not needed — literals are lifted into `CodeObject.constants`, which
  the VM roots directly (§5.2). This asymmetry is one of the few places
  "tree-walker simple" vs "VM fast" shows up in v1.

### 4.5 Sweep and tombstoning

For each arena, every unmarked, not-already-free slot (sweep pre-marks
free-list indices in the mark bits rather than building a separate
already-free set):

1. push its index onto the arena's free list;
2. **tombstone the slot** — overwrite with a payload-free value:
   vectors/strings → `Vec::new()` (drops element storage); objects → the
   dedicated `HeapObjectData::Free` variant. Pairs are `Copy` with nothing to
   drop, so release builds skip the store entirely; debug builds write a
   reserved poison value (`TaggedValue::GC_POISON`) instead.

Tombstoning is not just hygiene: dropping the old `HeapObjectData` releases its
`Rc` payloads (environments, ports, procedures) at sweep time rather than at
some future reuse of the slot. §8 shows this is what makes cycle reclamation
work. It also closes resources (a swept `Port` drops promptly).

Sweep completion is the "collection happened" boundary: it resets the
allocation counter and clears any pending `(gc)` request, so alternative
collectors composing `GcVisitor` + `sweep` get the trigger bookkeeping for
free.

Arena `Vec`s are never shrunk; a "free list ratio" stat can inform future
shrink heuristics but v1 does not shrink.

**Use-after-free detectability by arena** (debug builds): object-arena UAF
panics via the `Free` assert in `get_object`; pair UAF panics via the poison
assert in `get_pair`/`set_car`/`set_cdr`; vector/string tombstones (empty) are
legal values, so UAF in those two arenas goes undetected — the differential
stress lane is the safety net there.

---

## 5. Root Inventory

This section is the checklist implementations must satisfy. Every entry was
verified against the source on 2026-07-31. Missing any "yes" row is a
use-after-free.

### 5.1 Tree-walker

The evaluator itself holds almost nothing: `Evaluator`
(`crates/patina-tree-walker/src/eval/mod.rs:42`) has `global_env` and the
registries; `CpsEvaluator` is a stateless borrow. **The live machine state is
the `current_step: StepResult` local** in `eval_in_env`'s trampoline loop
(`eval/cps_eval/mod.rs:123`, loop at `:155`).

| Root | Location | Notes |
|------|----------|-------|
| `current_step: StepResult` | Rust local in the trampoline loop | Carries value/proc/args, env, `ContEnv` continuation chain, `prompt_stack`, `dynamic_winds`, `exception_handlers` (`eval/cps_eval/types.rs:250-272`). Passed to `collect` as a transient root at the safe point. |
| `ContValue` chain | inside `StepResult` | Recursive via `Box<ContValue>`; variants embed `TaggedValue`s (`types.rs:168`) — needs its own trace impl |
| `Evaluator.global_env` | `eval/mod.rs:43` | `visit_env` |
| `LibraryRegistry` | `eval/mod.rs:47` | §5.3 |
| `PENDING_ESCAPE` thread-local | `eval/cps_eval/types.rs:21-31` | Holds `(TaggedValue, Rc<CpsContinuation>)` between set and take — a genuine hidden root |
| Suspended outer `StepResult`s in nested trampolines | `apply_from_direct_tagged`, `eval/cps_eval/wind.rs:163` | **Not rooted** — handled by deferral (§7), not by tracing |
| `Parser.labels` | `crates/patina-frontend/src/parser/mod.rs:47` | Datum labels during parse; GC never runs mid-parse (deferral), listed for completeness |
| Macro-expansion `MatchEnv` / `Matcher` / `Expander` state | `crates/patina-core/src/pvref.rs:236` etc. | Live only during expansion; covered by deferral |

### 5.2 VM

Everything hangs off `VmState` (`crates/patina-vm/src/runtime/vm_state.rs:29`),
which makes `impl GcRoots for VmState` natural:

| Field | Root? | Notes |
|-------|-------|-------|
| `registers` | **yes** | Whole vector, conservatively — including slots past a frame's live range (they hold `NULL` or stale-but-valid values) |
| `frames[*].closure` | **yes** | **Bare `Option<HeapIndex>`, not a TaggedValue** (`types/mod.rs:52`) — use `visit_object_index` |
| `value_buffer` | **yes** | Multi-value side channel |
| `scratch_args` | yes | Empty at safe points (`mem::take`n during primitive calls), but rooting it is free and future-proof |
| `prompt_stack`, `dynamic_winds`, `exception_handlers` | **yes** | `tag`/`handler`/`before`/`after` values (`types/continuation.rs:14,:39,:86`) |
| `code_store[*].constants` | **yes** | Effectively immortal (code objects are never evicted); candidate for a mark-once immortal set later |
| `globals` | **yes** | `visit_env` |
| `continuation_store` / `delimited_continuation_store` | **yes — the big one** | `VmContinuation` snapshots hold full `registers` copies, frames (each with a bare closure index), wind/prompt/handler stacks (`types/continuation.rs:59,:101`). Heap-side `VmContinuationRef(u64)` is opaque; only this impl reaches the payload. |
| `tracer` | yes | `StepTracer.pre_regs`/`pre_all_regs` (`crates/patina-vm/src/tracer.rs:270-272`) |
| `library_registry` | yes | §5.3 |
| `primitive_registry`, `shadowed_primitives`, `next_cont_id`, `fs` | no | No TaggedValues |

Rust-stack temporaries (continuation-capture register clones, the
`saved_globals` swap windows, primitive args in `VmApplyContext` callbacks):
**not rooted — handled by safe-point placement + deferral (§7).**

**Implementation note (stage 3):** every dispatch loop takes a
`GcDeferGuard`, so any nested `run_loop_until` — reached via `execute_nested`,
a re-entrant primitive, or `eval` — is deferred by construction.

Library loading is the case that needed more. The predicate is **"does this
Rust frame hold heap values that must survive across an evaluation call?"** —
*not* which entry point it uses. `execute` versus `execute_nested` is a red
herring: `vm_load_library_from_parsed` calls `execute_nested` and still needed
deferral, because `run_loop_until` guards unconditionally, so a nested call
reached from outside any dispatch loop is equally "outermost".

Three code paths evaluate a `ParsedLibrary` (VM backend, VM state,
tree-walker), each a near-copy of the others, and when the guard lived at the
call sites one of them was missed — bootstrap died on the first collection.
The guard therefore lives on **`ParsedLibrary` itself**: it holds unevaluated
`body` forms that no root provider can see, so it carries a `GcDeferGuard` for
as long as it exists. A fourth loading path is now safe by construction, and
the placement is also correct for a `ParsedLibrary` held beyond a single
loading call, which a call-site guard would get wrong. See §11 on why the
debug build is the lane that localizes failures like this one.

### 5.3 Shared (both backends)

| Root | Location | Notes |
|------|----------|-------|
| `LibraryRegistry.libraries[*]` | `crates/patina-runtime/src/library_registry.rs` | Each `Library` has `exports: HashMap<String, TaggedValue>` **and** `env: Rc<Environment>` — two root sets per library. **`impl GcRoots for LibraryRegistry`** lives in `patina-runtime` so both backends pass it as a root rather than restating the rule; the per-library walk is `GcVisitor::visit_library` |
| `ParsedLibrary.body` | `crates/patina-runtime/src/library_loader.rs:122` | Unevaluated forms during loading; covered by deferral |
| `Heap.symbol_table` | `heap/mod.rs:255` | Treated as a root set in v1 → symbols immortal (§9.2) |
| `CompiledMacro` literals | `compiled_macro.rs:78,:280,:439` | Reached via the `Macro` heap-variant trace rule when the macro binding is live |
| In-flight `ExceptionObject.irritants` | `crates/patina-core/src/error.rs:44` | Lives in a propagating `Err` on the Rust stack; covered by deferral (GC never runs during unwinding — safe points are at loop tops, not in error paths) |

---

## 6. Trigger Policy

- `Heap` gains an `allocs_since_gc` counter, incremented in each `alloc_*`
  via `note_alloc`, which **raises a collection-pending flag** (an
  `Rc<Cell<bool>>` shared with the dispatch loops) when the counter crosses
  `Heap.gc_threshold`. The threshold is the mode made concrete —
  `GcController::current_threshold`, the single owner of that mapping:
  `usize::MAX` for Off, the collector's adaptive `max(GC_MIN_THRESHOLD, 2 ×
  live_after_last_gc)` for On, `n` for Stress. The backend installs it when
  heap and controller are paired (a bare heap defaults to the inert
  `usize::MAX` — policy stays in the controller, mechanism in the heap), and
  `GcController::collect` re-installs it after each collection — the only
  point the adaptive term changes. `request_gc` raises the same flag, which
  is how `(gc)` is honored in every mode; sweep lowers it.
- The *decision* is therefore made at allocation time, but **collection still
  happens only at backend safe points** (§7) — inside `alloc_*` the heap is
  re-entrantly borrowed and mid-operation temporaries would be unrooted. The
  safe point reads the flag (one load, no `RefCell` borrow) and collects when
  it is raised. §6.1 has the measurements that forced this shape.
- Collection is **off by default** until stage 4: the baseline build must
  behave exactly as it did before the collector existed. The mode table and
  its environment grammar live in `patina-core` (`GcMode::from_env`) because
  the variables are process-global and both backends must agree on them:
  | Mode | Selected by | Behavior |
  |------|-------------|----------|
  | Off (default) | — | collect only when `(gc)` has been called |
  | On | `PATINA_GC=1` | adaptive threshold above |
  | Stress | `PATINA_GC_STRESS[=n]` | collect once `n` allocations (default 1) have happened, **bypassing the adaptive `2 × live` floor** |

  Stress deliberately ignores the adaptive floor: after bootstrap the live set
  is large enough that `2 × live` would almost never fire, which is the
  opposite of what a stress lane wants.
- Manual entry points for testing and users: `(gc)` and `(gc-stats)`
  primitives, honored in **every** mode. `(gc)` records a request; the next
  safe point services it. This is what makes collection testable without
  process-global environment variables.

### 6.1 Trigger cost — measured, redesigned (stage 4a), re-measured

The stage-3 safe point re-derived policy per dispatched instruction. Measured
on the VM (M1 Max, release, interleaved A/B/C against `main`, with a control
binary that deletes *only* the `maybe_collect` call):

| Lane | Cost vs `main` | Where it went |
|------|----------------|---------------|
| GC **off** (default) | **−2.5 to −3.5%** | `mode == Off && !heap.borrow().gc_requested()` per dispatched instruction ≈ 0.24–0.34 ns (~1 cycle) |
| GC **on**, zero collections | **−13.7%** | `should_collect` per instruction: a non-inlined call plus two `RefCell` borrows ≈ 1.75 ns |

The control binary reproduced `main` to within noise on every workload, so the
safe point *was* the whole cost — nothing else in the GC integration is
measurable.

The flaw was architectural, not micro: **the safe point asked a question whose
answer only changes when something allocates.** Stage 4a therefore moved the
decision to where allocation happens — `Heap::note_alloc` raises a pending
flag when `allocs_since_gc` crosses the mode-derived threshold (§6), and every
mode's safe point collapsed to a single flag test. The flag lives outside the
heap's `RefCell` (an `Rc<Cell<bool>>`; dispatch loops hoist a handle at
entry), so the fast path has no borrow either.

Re-measured after the redesign (same rig and methodology; `fib 33` for
dispatch, a 10M-cons churn for the allocation path; 7 interleaved rounds,
medians, round-to-round spread 1–3%):

| Lane | vs `main` | vs control | Reading |
|------|-----------|------------|---------|
| GC **off**, dispatch | **−0.4%** | +1.4% | parity with `main`; the residual vs control is the bare load-and-branch |
| GC **off**, alloc-heavy | **−1.3%** | +1.1% | `note_alloc`'s compare-and-branch is not measurable over the old bare increment |
| GC **on**, zero collections | **−12.2%** (i.e. the penalty is gone) | +0.8% | on-lane now costs the same as off-lane: on-vs-off is −0.2% on the branch, +13.2% on `main` |

The on/off convergence is the acceptance-relevant result: enabling collection
no longer has a standing per-instruction cost, only the pauses themselves.
The remaining ~1% vs control is the flag load and branch, removable only by
specializing the dispatch loop — noted as a stage-5 option, not pursued while
it sits at the edge of the noise band.

---

## 7. Safe Points and Re-entrancy

**Invariant: GC runs only when every live value is reachable from a registered
root, and no heap `RefCell` borrow is outstanding.**

Both backends stash live values in Rust-stack locals mid-operation (VM
continuation-capture temporaries, `mem::take`n buffers, `saved_globals` swap
windows, primitive argument vectors; tree-walker nested trampolines). Rather
than shadow-stack rooting every such site (invasive, error-prone), v1 uses
placement + deferral:

1. **Safe point = top of the driver loop iteration**:
   - VM: top of `run_loop_until` (`vm_state.rs:685`) before
     `dispatch_one_instruction`. At that point all state is in `VmState`
     fields; capture temporaries are dead; `value_buffer`/`scratch_args` are
     restored.
   - Tree-walker (**stage 2, implemented**): top of the trampoline loop in
     `eval_in_env`. The entire machine state is `current_step`, which the
     safe point passes as a transient root along with the `expr` the
     trampoline was entered with (its literals stay live for the call).
2. **`gc_defer_depth` counter** on the shared heap, managed by the
   `GcDeferGuard` RAII type so early returns and `?` propagation cannot leak
   an increment.

   The tree-walker takes a guard **on every trampoline entry** (`eval_in_env`
   *and* `apply_from_direct_tagged`) rather than instrumenting each re-entrant
   call site. This inverts the failure mode: a nested trampoline is deferred
   *by construction* (whatever route reached it — a higher-order primitive,
   `eval`, quasiquote), instead of relying on someone having remembered to
   guard that route.

   A safe point asks **its own guard** — `GcDeferGuard::is_outermost()`, true
   when nothing was deferring at the moment it was taken — rather than
   comparing the depth to a literal. The "outermost" depth differs per backend
   (the tree-walker guards every trampoline; the VM guards only its re-entrant
   paths) and both share one counter through `SharedHeap`, so a hardcoded
   number would be correct for at most one of them.

   Additional guards cover Rust-side scopes that hold values across an
   evaluation call:
   - library loading's `for tv in &parsed.body` loop (`eval/mod.rs`) — the
     unevaluated forms are TaggedValues no root provider can see.
   - VM (stage 3): `execute_nested`, `VmApplyContext` primitive callbacks,
     the globals-swap windows.

   The tree-walker safe point additionally refuses to collect when
   `library_registry` is already mutably borrowed: rooting must walk it, and
   a partial root set is a use-after-free. Parsing needs no guard — safe
   points exist only inside the trampoline, so GC cannot fire mid-parse.

**Known limitation (accepted for v1):** a long-running nested execution — e.g.
`(map f huge-list)` where each `f` call is a nested trampoline, or a library
loading a large body — cannot collect until it returns to the outermost loop.
The mitigation path (stage 5+) is to root the re-entrancy boundary explicitly
(the suspended `StepResult` / the boundary argument slice) and let nested loops
collect; the `GcRoots` trait already accommodates this (transient providers).

**Backend coexistence:** both backends share one `SharedHeap` in mixed
scenarios. The deferral rule generalizes: whichever backend is *outermost*
owns the safe point; any nested execution (either backend) runs at
`gc_defer_depth > 0` and never collects. This replaces P6's blunter
"GC runs only from the VM driver" rule.

---

## 8. Why Cycles Are Fully Reclaimed Without GC-Managing Environments

The worry: closures create heap ↔ environment cycles
(`env binding → TaggedValue → VmClosure slot → Rc<Environment> → …`), `Rc`
can't collect cycles, and the collector doesn't manage environments — so do
env cycles leak?

No, because the two edge directions have asymmetric ownership:

- **env → heap** edges are bare indices (`TaggedValue` in binding maps) — no
  ownership, invisible to `Rc`.
- **heap → env** edges are owning `Rc<Environment>` held **inside heap slots**
  (`VmClosure.globals`, `Procedure`'s captured env, `CpsContinuation.env`,
  `EnvironmentSpecifier.env`).

Every environment cycle must route through a heap slot, because binding maps
hold `TaggedValue`s, never `Rc<Environment>` directly, and parent chains are
acyclic trees. When a closure cluster becomes unreachable from roots:

1. the tracer never marks the `VmClosure`/`Procedure`/`Continuation` slot;
2. sweep tombstones the slot, dropping its `HeapObjectData` — including the
   `Rc<Environment>`;
3. the environment's refcount falls; if that was the last strong ref, the env
   drops, dropping its binding maps (which held only non-owning indices);
4. anything those bindings pointed at was likewise unmarked and swept in the
   same collection (reachability is transitive).

The same argument covers `set-cdr!` pair cycles trivially (pairs own nothing)
and `Rc`-payload variants like `Promise`/`Record` (their `Rc<RefCell<…>>`
payloads drop at tombstone time unless shared with a live holder — and a live
holder means they were correctly marked).

**Consequence:** tombstoning at sweep (§4.5) is load-bearing, not cosmetic.
Deferring payload drop to slot reuse would keep env cycles alive indefinitely
on quiet arenas.

---

## 9. Known Hazards and Policies

### 9.1 SourceMap raw-bits keying (pruned since stage 4b)

`SourceMap` keys `HashMap<u64, SourceLocation>` by `tv.raw_bits()`
(`source_map.rs`). Once slots are reused, a new object could inherit a stale
source location — misattributed diagnostics, never unsoundness.

**Implemented (stage 4b)** as the recording flavor: sweep pushes each
reclaimed slot's raw bits into a capped buffer on the heap
(`Heap::take_gc_freed_bits`), recorded only once a source-mapped session has
called `enable_gc_freed_tracking` (done by `Parser::new_with_source_map`, so
plain backend use pays nothing). The run loops in `patina-interpreter` and
`patina-repl` drain via `prune_freed_locations` at the top of each
parse–eval iteration — sweeps happen only during evaluation and raw-bits
lookups only during a later form's desugaring, so pruning at the form
boundary closes the window. Overflowing the buffer cap degrades the next
drain to "clear all locations": a missing location degrades a diagnostic, a
stale one misattributes it. Residual (accepted): with several live maps at
once — nested tracked evals — a drain consumes bits for all of them, leaving
possibly-stale entries in the map not being pruned; no worse than the
unpruned status quo, and lookups by raw bits happen only at desugar time.

### 9.2 Symbol table

`symbol_table: HashMap<String, HeapIndex>` is traced as a root set → interned
symbols are immortal. This is correct (symbols are `eq?`-identity-bearing) and
cheap (symbols are small). A weak intern table with post-sweep pruning is
future work; nothing in the design blocks it.

### 9.3 Transient raw-bits sets

Cycle-detection helpers key transient `HashSet`s by `tv.raw()`
(`heap/mod.rs:2163,:2190`, parser `:1055,:1083`, datum writer `:427-637`).
These are within-call only; safe because GC never runs mid-traversal
(deferral / no safe point inside them).

### 9.4 `Rc`-shared structures need dedup or tracing goes exponential

Any persistent structure whose nodes capture the tail below them must be
memoized by pointer identity, or a single trace is exponential rather than
linear. The tree-walker's `ContEnv` is exactly this shape — every
`ContValue::Local` captures the chain below it, so node *k* holds a chain of
length *k−1* and an un-memoized walk costs `2ⁿ − 1` node visits. Measured
before the fix: **6.8 s for one collection at nesting depth 26**, roughly
1.9× per added level.

This is why every `Rc`-shared structure the visitor walks has a dedup set
(`visit_env`, `visit_continuation`, `visit_expr_literals`), and why
[`GcVisitor::visit_once`] exists — root providers must be able to dedup their
own shared structures. **Stage 3 check:** `VmContinuation` snapshots and
`CodeObject` sharing have the same potential; if a VM root provider walks an
`Rc` graph, it must route through `visit_once`.

### 9.5 Root sets that grow without bound (measured)

Two roots scale with *everything ever created* rather than with live data, so
the pause grows monotonically in a long-running process. Both are stage 5
work; recorded here because they are invisible until a session runs long.

**`code_store` constants.** Code objects are never evicted, so every compiled
top-level form adds roots permanently. Instrumented over one 130 ms chibi run
(17 collections), `code_store` grew **356 → 4,660 code objects** and the scan
grew **8.8 µs → 107–151 µs per collection** — by the last collection, **57% of
the entire root-tracing phase** and ~19% of the pause. The cost is the hash-map
walk and chasing scattered `Rc<CodeObject>`s, not the marking (only ~1.9
constants each). Fix: append constants to one flat `Vec<TaggedValue>` at load
time and `visit_slice` it once, or mark them into a persistent immortal bitmap
seeded into `MarkBits::for_heap`. The same argument applies to the symbol
table, which `GcVisitor::new` re-marks every collection; one immortal set
covers both.

**Continuation side tables.** Nothing ever removes entries from
`continuation_store` / `delimited_continuation_store`. With 20 000 `call/cc`
captures whose continuations are immediately discarded, a collection spends
**1 456 µs of a 1.79 ms pause (81%) in root tracing** — and every register and
frame snapshot those dead continuations pin stays unreclaimable. Fix: treat the
stores as weak, dropping an entry when its `VmContinuationRef` object did not
survive marking. That is a sweep-side change and needs care with the
`is_outermost` protocol, hence stage 5.

### 9.6 `CompiledMacro.heap`

Compiled macros hold a `SharedHeap` clone for their literal values
(`compiled_macro.rs:461-465`). As long as it is the same heap instance
(the field comment already requires this), the `Macro` trace rule covers the
literals. A debug assertion that `Rc::ptr_eq(macro.heap, heap)` at trace time
would catch violations.

---

## 10. Staging

Collection is gated at runtime rather than by a Cargo feature (§6): the
default build never collects unless `(gc)` is called, so the baseline lane is
unaffected without a second compilation configuration to maintain.

| Stage | Deliverable | Acceptance |
|-------|-------------|------------|
| **1. Core infra** ✅ *(merged 2026-07-31, PR #4)* | Mark bit-vectors, `GcVisitor`, per-variant trace rules, tombstoning sweep, `Collector`/`GcRoots` traits, `MarkSweepCollector`, alloc counter, `Environment::for_each_local_value`, `(gc)`/`(gc-stats)` primitives | Unit tests with synthetic roots: reachability, tombstone drops `Rc` payloads, free-list reuse, poison-mode assertions |
| **2. Tree-walker integration** ✅ *(2026-07-31)* | Root providers for `Evaluator` (global env), `LibraryRegistry` (shared, in `patina-runtime`), `StepRoots` (`StepResult` + `ContValue`/`ContEnv` chains + entry `expr`) and `EscapeRoots` (`PENDING_ESCAPE`); safe point at the trampoline loop top; `GcDeferGuard` on both trampolines and the library-body loop; `GcMode` policy | **Met:** chibi suite (1194 test expressions) on the tree-walker under `PATINA_GC_STRESS=1` produced byte-identical output to baseline. Reclamation proven: a 20 000-cons workload holds ~4 000 pairs under stress vs ~24 000 unreclaimed without it. 14 integration tests in `patina-tests/tests/gc_tree_walker.rs` cover cycles, closures, continuations, `dynamic-wind`, records, nested trampolines, and a regression guard for §9.4 |
| **3. VM integration** ✅ *(2026-08-01)* | `impl GcRoots for VmState` (registers, frames' bare closure indices, `value_buffer`, `scratch_args`, wind/prompt/handler stacks, `code_store` constants, globals, **both continuation side tables**, tracer register snapshots); safe point at the top of `run_loop_until`; `GcDeferGuard` on every dispatch loop and on both library-loading paths; `GcController` lifted to `patina-core` and shared | **Met:** VM chibi suite under `PATINA_GC_STRESS=1` byte-identical to baseline in **both** release and a debug build with the poison/`Free` assertions active (the strongest check — a missed root panics rather than passing silently). 20 004 collections on a 20 000-cons workload, arena 4 039 vs 24 047 pairs. 14 VM integration tests in `patina-tests/tests/gc_vm.rs` |
| **4a. Trigger redesign** ✅ *(2026-08-03)* | Safe-point trigger redesign (§6.1) — the stage-4 gating item: collection decision moved to `Heap::note_alloc` (pending flag + mode-derived threshold), safe point collapsed to one flag load, `GcController::collect` re-arms the adaptive term | **Met:** interleaved A/B/C — GC-off at parity with `main` (−0.4% dispatch, −1.3% alloc-heavy) and the GC-on zero-collection penalty eliminated (−0.2% on-vs-off, was −13.7%); ~1% residual vs no-safe-point control (§6.1). Chibi suite byte-identical across baseline/stress/on lanes, both backends, release + debug poison builds |
| **4b. Groundwork** ✅ *(2026-08-03)* | Two CI lanes (`gc-differential` release + debug-poison jobs running `scripts/run_gc_differential.sh`: stress + adaptive vs baseline, both backends, plus a reclamation proof so the lane cannot pass vacuously), SourceMap pruning hook (§9.1), liveness stress (100k-element list survives collection) and arena-reuse plateau as shared integration tests | **Met:** all lanes byte-identical locally and in CI; `cargo clippy`/`fmt` clean |
| **4c. Default-on** | Adaptive threshold on by default (`PATINA_GC=0` opt-out) — the 4a numbers support it: enabling GC costs only the pauses themselves | Chibi suite green under the new default; interleaved sanity check; own PR for easy revert |
| **5+. Future** | VM-only `Collector` upgrades: immortal set for `code_store` constants, lazy sweep, non-moving generational (sticky mark bits + write barriers on `set-car!`/`set-cdr!`/`vector-set!`/`MutableCell` stores); explicit rooting of re-entrancy boundaries so nested loops can collect; weak symbol table | Each behind the trait, benchmarked interleaved |

Rationale for full-arena tracing from stage 1 (vs P6's pairs+vectors-first):
the tracer must understand every `HeapObjectData` variant *anyway* to be safe
(an untraced variant holding a `TaggedValue` is a use-after-free, not a
smaller scope). Restricting the *sweep* to some arenas saves little once the
visitor exists, and the stress lane is the real safety net.

---

## 11. Testing Strategy

1. **Differential lanes:** the default (non-collecting) build must be
   bit-identical to today; `PATINA_GC_STRESS=1` must produce identical output
   on the full chibi suite for **both** backends. Tree-walker:
   ```bash
   ./target/release/patina --tree-walker scheme_tests/chibi/r7rs-tests.scm > base.txt
   PATINA_GC_STRESS=1 ./target/release/patina --tree-walker scheme_tests/chibi/r7rs-tests.scm > stress.txt
   diff base.txt stress.txt   # must be empty
   ```
   Run the stress lane in a **debug build too**, not just release. Release
   tolerates a use-after-free silently (the swept slot reads back as a
   `Free`/poisoned value and surfaces later as a confusing type error at an
   unrelated call site); the debug build panics at the exact accessor with the
   slot number, and the backtrace names the code that lost the root. Stage 3's
   missed library-loading guard was diagnosed this way in one run.
2. **Poison mode (debug):** tombstoned slots hold sentinels; accessors assert.
   Any missed root becomes a deterministic panic under stress, not a
   heisenbug.
3. **Reclamation proofs:** cycle tests (`set-cdr!` self-loop, closure
   capturing its own env, `call/cc` captured and dropped); arena-length
   plateau test (allocate-and-drop in a loop; assert arena `len()` stabilizes).
4. **Pause/overhead:** interleaved A/B benchmark runs (main / branch / main)
   per the project's established methodology; record `GcStats.last_pause`
   distribution on allocation-heavy benchmarks.
5. **Paranoid pre-sweep assertion (debug):** after marking, assert no free-list
   slot is marked and no marked slot is on a free list.

---

## 12. Relationship to Prior Plans

- **P6 (`PRD/TRACK_P_PERFORMANCE_PRD.md:147-156`):** this design keeps P6's
  core (side mark bits, sweep into existing free lists, non-moving, safe-point
  trigger, feature flag, stress lane) and extends it with: tree-walker as a
  first-class client (P6 was VM-driver-only), the `Collector`/`GcRoots`
  pluggability seam, tombstoning-as-cycle-breaker (§8), the full root
  inventory (§5 — P6's root list missed the continuation side tables' opacity,
  `CallFrame.closure`, `PENDING_ESCAPE`, tracer buffers, macro literals), and
  full-arena tracing from the start.
- **Archived `GC_DESIGN.md` (2026-02):** superseded; its `rust-gc` approach
  targeted the deleted `Value` enum. Its correctness criteria (cycle
  reclamation, no >20% regression, all tests pass) carry over.
