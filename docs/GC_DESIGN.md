# Garbage Collection Design

**Status:** Describes the collector as built: stages 1–4c (PRs #4–#11, merged
2026-08-01 to 2026-08-04 UTC) and the weak continuation tables of 2026-08-05
(#19). The redesign that replaces it is designed and planned in
[`PRD/GC_PRD.md`](../PRD/GC_PRD.md); this file is rewritten as that plan's
stages land. Stage numbers here are this document's own staging (§10), not
the redesign's.
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
| 8 | VM continuation side tables traced by `VmState`'s root provider, not the heap tracer — **weakly** since stage 5: payloads trace only for marked ref objects, dead entries are pruned | Heap only holds opaque `VmContinuationRef(u64)` — heap-only tracing under-approximates, strong table tracing over-approximates into a monotonic leak (§9.5) |
| 9 | Runtime mode gate (no Cargo feature); off by default until stage 4c, adaptive-on since | Baseline behavior stayed bit-identical through differential testing at every stage; the CI lanes enforce it permanently |

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
e.g. a pair is `(index << 3) | 0b011`, the index in bits 3–34 of the word. A
non-moving collector requires no handle rewriting. In a plain release build
index reuse is invisible to holders. In a check build (debug, or release with
`patina-core`'s `gc-check` feature; #621) bits 40–55 of the word also carry the
slot's 16-bit allocation generation, which `heap_index()` does not read: the
heap stamps it into every reference it makes, bumps a slot's generation when
sweep frees it, and refuses a reference whose stamp no longer matches its slot
(`heap/check.rs`, §4.5). So a holder of a reference to a reused slot panics
instead of reading the new tenant. Every copy of a value carries the same
stamp, so `eq?` and the other raw-bit consumers still see one key per
object.

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

*As built:* `Environment::for_each_gc_edge` reports one environment's edges,
naming every field of the struct and of its side tables (#623): the value in
each slot, plain and scoped, and two kinds of edge that leave the parent
chain, both an `Rc<Environment>` in a side table rather than a value in a
slot: a macro-expansion alias into the environment the macro was defined in,
and the owner of an imported binding, whose slot here holds only a marker —
the value is in the slot of the library that owns the location (#406). It
returns the parent. `GcVisitor::visit_env` follows the parent chain by
reference and walks the other environments from a worklist, all deduplicated
by `gc_identity`, without recursing. Anything that gives an environment
another way to reach a value held elsewhere is a new field, which does not
compile there until it is reported or written `field: _` with its reason.

### 3.4 Why moving/compacting GC is off the table

Raw indices escape `TaggedValue` into places a relocator cannot see or would
have to be taught about:

1. `Heap::symbol_table` maps names to bare `HeapIndex` (`heap/mod.rs:255`).
2. Syntax provenance in `Heap` and diagnostic snapshots in `SourceMap` key
   locations by `tv.raw_bits()` (`heap/source.rs`, `source_map.rs`).
3. `eq?`/`eqv?`/hashing compare raw bits (`heap/mod.rs:1474,1507,1593,1719`).
4. `CallFrame.closure: Option<ObjectIndex>`, a bare object-arena index that
   keeps the generation stamp in check builds (`crates/patina-vm/src/types/mod.rs:52`).
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
    pub fn visit_object_index(&mut self, index: ObjectIndex);
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
/// method — the safe point never asks the collector anything. Crate-private
/// since #624, with every other way to run a collection (§7).
pub(crate) trait Collector {
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
| `Macro` | `CompiledMacro`: pattern/template literal `TaggedValue`s, and `definition_env` and each `foreign_expansions` environment (`visit_env`) — the latter traced since #623, though today the registry or an importer's `owners` also roots each (`trace_compiled_macro`) |
| `Continuation` | `CpsContinuation`: env (`visit_env`), `dynamic_winds` — each record's before/after thunks and the handler stack it captured at its `dynamic-wind` call (`visit_wind`, the one tracing point for a record wherever it sits: a stack, a continuation, a prompt frame, or a `DynamicWindSetup`/`Jump` cont value), `exception_handlers` (via `trace_exception_handler`), `prompt_stack` — each frame's handler and the continuation below it (`trace_prompt_frame`; the tag is a plain `Rc` struct), `captured_cont_env` (deduplicated worklist), `resume` (`trace_cont_value`), body literals (§4.4) |
| `EnvironmentSpecifier` | `visit_env(env)` |
| `VmContinuationRef`, `VmDelimitedContinuationRef` | **weak key** — marking one records its id; the payload in `VmState`'s side tables is traced only for recorded ids, via the `GcRoots::trace_weak_ids` fixpoint (driven by `run_mark_phase`) (§5.2, §9.5) |
| `Ephemeron` | **weak key** — neither field is traced on arrival; the pair is recorded, and its key *and* datum are traced only once the key is marked by some other path. Unretained pairs are broken (both fields cleared) before the sweep, so a dead key's datum stops being a root. Shares one fixpoint with the row above, in `run_mark_phase` — separate fixpoints lose a continuation payload whose ref only a late ephemeron retention marks (SRFI 124) |

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
   drop, so plain release builds skip the store entirely; check builds
   (debug, or release with `gc-check`) write a reserved poison value
   (`TaggedValue::GC_POISON`) instead.

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

**Use-after-free detectability by arena** (check builds: every debug build,
and release with `patina-core`'s `gc-check` feature; #621): every arena
accessor refuses a reference to a freed slot, and one to a slot freed and
reused since the reference was made, by a per-slot generation that the heap
stamps into each reference (`heap/check.rs`). Marking refuses the same two
cases: a free slot at sweep's pre-mark (§11 item 5), a reused one in
`GcVisitor::visit`. The tombstones are no longer what detects a use after
free — before #621, vector and string tombstones (empty) were legal values and
a reused slot read as its new tenant, so those cases went undetected. Check
builds also write the pair poison, so a stale pair that marking reaches traces
nothing.

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
| Suspended outer `StepResult`s in nested trampolines | `run_trampoline` (`eval/cps_eval/mod.rs`), entered by `apply_from_direct_with` and the `eval` primitive | **Not rooted** — handled by deferral (§7), not by tracing. Since 2026-09-01 only Rust-primitive callbacks and parameter converters run there; a continuation jump's wind thunks are ordinary steps (`ContValue::Jump`) and are traced like any other |
| `Parser.labels` | `crates/patina-frontend/src/parser/mod.rs:47` | Datum labels during parse; GC never runs mid-parse (deferral), listed for completeness |
| Macro-expansion `MatchEnv` / `Matcher` / `Expander` state | `crates/patina-core/src/pvref.rs:236` etc. | Live only during expansion; covered by deferral |

### 5.2 VM

Everything hangs off `VmState` (`crates/patina-vm/src/runtime/vm_state.rs`).
Its `GcRoots` implementation lives in the child module
`runtime/vm_state/gc_roots.rs`, the only production code outside the storage
implementation allowed to prune its private weak stores. The five dynamic
collections are owned by `ExecutionState` and traced through read-only slices;
the inventory below uses their component names (see `VM_RUNTIME.md` §2.2).

| Field | Root? | Notes |
|-------|-------|-------|
| `registers` | **yes** | Whole vector after completed expression temporaries are cleared using per-PC compiler maps (#423); local bindings remain conservative |
| `frames[*].closure` | **yes** | **Bare `Option<ObjectIndex>`, not a TaggedValue** (`types/mod.rs:52`) — use `visit_object_index` |
| `pending_escape` | **yes** | Value parked while crossing a Rust re-entry boundary; multiple values otherwise travel in ordinary registers as heap values |
| `scratch_args` | yes | Empty at safe points (`mem::take`n during primitive calls), but rooting it is free and future-proof |
| `prompt_stack`, `dynamic_winds`, `exception_handlers` | **yes** | `tag`/`handler`/`before`/`after` values, and a wind record's `handlers` — the stack of its own `dynamic-wind` call, which its thunks run under and which nothing else holds once the live stack has moved on (`types/continuation.rs`). An `ExceptionHandler` is one procedure now — it used to also carry the wind depth `raise` unwound to, which no raise path needs since Track L families 22/28 |
| `code_store[*].constants` | **yes** | Kept while a frame, a captured continuation or a live closure can run the code; a finished form's code is released with its constants (#338) |
| `globals` | **yes** | `visit_env` |
| `continuation_store` / `delimited_continuation_store` | **weak** (stage 5) | `VmContinuation` snapshots hold full `registers` copies, frames (each with a bare closure index), wind/prompt/handler stacks (`types/continuation.rs:59,:101`). Heap-side `VmContinuationRef(u64)` is opaque; only this impl reaches the payload — but only for ids whose ref object was marked (`trace_weak_ids` fixpoint), and entries whose ref died are pruned (`sweep_weak`). Tracing them strongly made every capture immortal (§9.5). |
| `tracer` | yes | `StepTracer.pre_regs`/`pre_all_regs` (`crates/patina-vm/src/tracer.rs:270-272`) |
| `library_registry` | yes | §5.3 |
| `primitive_registry`, `shadowed_primitives`, `fs` | no | No TaggedValues |

**Temporary retirement (#423):** the compiler records possible-root bitsets at
instruction boundaries (`docs/VM_COMPILER.md` §10.4). Before an outermost
collection, the VM replaces excluded slots with `UNSPECIFIED`, including
excess capacity in reused tail-call windows; a check build writes `DEAD_SLOT`
instead and panics where one is read (#625, §11 item 6). Full and delimited
continuation snapshots receive the same cleanup at capture, with the
delimited base offset accounted for. Tracing still visits complete vectors,
so snapshots and tracer register views never carry deliberately untraced
pointers to swept objects.
Runtime stubs without maps remain conservative. Local bindings are not
retired at their last textual use: `reference-barrier` still roots its
argument through the call, and a focused test pins that contract.

Rust-stack temporaries (continuation-capture register clones, the
`saved_globals` swap windows, primitive args in `VmApplyContext` callbacks):
**not rooted — handled by safe-point placement + deferral (§7).**

**Implementation note (stage 3):** every dispatch loop takes a
`GcDeferGuard`, so any nested `run_loop_until` — reached via `execute_nested`,
a re-entrant primitive, or `eval` — is deferred by construction. `with_globals`
also defers across each temporary environment substitution and restores the
original environment on every returned result, including transfer errors.

Library loading is the case that needed more. The predicate is **"does this
Rust frame hold heap values that must survive across an evaluation call?"** —
*not* which entry point it uses. `execute` versus `execute_nested` is a red
herring: `vm_evaluate_parsed_library` calls `execute_nested` and still needed
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
| `Heap.core_syntax_table` | `heap/mod.rs` | Syntactic-keyword markers (`begin`, `if`, `else`, …). Rooted on the same terms as `symbol_table` and marked beside it in `GcVisitor::new`: a marker *is* the identity of a form, so collecting one would let the next intern mint a different object for the same keyword. Leaves, so mark-only. Should join the immortal set with the symbol table (§9.2) |
| `CompiledMacro` literals and environments | `compiled_macro.rs` | Reached via the `Macro` heap-variant trace rule when the macro binding is live: pattern and template literals, `definition_env`, and each `foreign_expansions` environment (§4.3) |
| In-flight `ExceptionObject.irritants` | `crates/patina-core/src/error.rs:44` | Lives in a propagating `Err` on the Rust stack; covered by deferral (GC never runs during unwinding — safe points are at loop tops, not in error paths) |

### 5.4 A new field, variant or root provider (#623)

The tables above say what is traced; the trace code is where a new field goes
missing. A field that holds a value but has no trace rule is a premature free
that no dynamic check sees while another path still reaches the value: #38
added `CompiledMacro.definition_env` and `Environment.alias_bindings` without
one, and #47 traced `CpsContinuation.resume` after 1.6 days untraced, hidden
because every construction site also stored the value in the traced
`captured_cont_env`. So every change that adds a heap object type, a field to
a traced struct, or a root provider meets two rules:

1. **A full destructure.** The trace function takes the struct (or variant)
   apart by name, with no `..` and no catch-all arm (`_` or a lone binding
   before `=>`, guarded or not). A field or positional payload that is
   deliberately not traced is written `field: _` or `Variant(_)` (or bound
   to an unused `_name`) with a comment, on its line or the line above,
   saying what it holds instead of a value or which test pins the decision.
   A new field is then error E0027 until someone decides.
   `scripts/check_gc_trace_names.py` enforces both in CI's Clippy job, over
   the functions it lists: `trace_object_children` and the functions it calls
   (`trace_compiled_macro`, `trace_continuation_children`, `visit_env`,
   `visit_library`, the wind, prompt and handler traces, `trace_cont_value`)
   in `heap/gc.rs`; `Environment::for_each_gc_edge` and
   `Library::for_each_gc_edge`; every root provider — `VmState` with
   `ExecutionState` and the VM's frame, record, code and continuation traces,
   the tree-walker's `Evaluator`, `StepRoots` and pending escape, the library
   registry and the VM's step tracer. A new trace function joins that list,
   and an `impl GcRoots for` outside test code in a file the list does not
   name with `trace_roots` fails the script, so a new root provider cannot
   go unread.
2. **A sentinel test whose value is reachable only through the new edge**
   (#164's rule). The test builds the struct by a struct literal, so a new
   field breaks the test as well; puts a fresh value in every field that can
   hold one; roots the struct and nothing else; collects; and asks
   `heap::sentinels::Sentinels` whether each value survived, which names the
   field of one that did not, in any build. Deleting the trace line must
   fail the test: run that once. The destructure forces a decision but
   cannot judge it (`resume: _ // aliased` compiles); the sentinel is what
   judges it. The tests live beside the code: `heap/trace_sentinels.rs` for
   heap kinds, `CompiledMacro`, `CpsContinuation` and `ContValue`; the
   `gc_edge_tests` modules of `environment.rs` and `library.rs`;
   `vm_state/trace_sentinel_tests.rs` for the VM's records, on the VM;
   `cps_eval/gc_roots/sentinel_tests.rs` for the tree-walker's, on the
   tree-walker; and `library_registry.rs`'s `gc_root_tests`.

This is detection on today's collector. `PRD/GC_PRD.md` replaces it with
generated tracing: `declare_layouts!` for heap kinds (stage 5a) and a `Trace`
derive for the Rust structures that stay off-heap (§14, stage 2).

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
- Adaptive collection is **always on** since stage 4c (before it, Off was
  the default while the trigger still had a standing cost — see §6.1 for the
  measurements that justified the flip). The environment variables are
  **testing-lane hooks**, not supported user configuration; the grammar lives
  in `patina-core` (`GcMode::from_env`) because the variables are
  process-global and both backends must agree on them. Zeal wins over stress,
  and stress over `PATINA_GC=0`:
  | Mode | Selected by | Behavior |
  |------|-------------|----------|
  | On (default) | — | adaptive threshold above |
  | Off | `PATINA_GC=0` *(testing lanes only — the no-collection reference run the differential suite diffs against, §11)* | collect only when `(gc)` has been called |
  | Stress | `PATINA_GC_STRESS[=n]` | collect once `n` allocations (default 1) have happened, **bypassing the adaptive `2 × live` floor** |
  | Zeal | `PATINA_GC_ZEAL=entry` *(the zeal lane, §11 item 7)* | collect at **every** outermost safe point, allocation or not: the threshold is 0, so the collection that lowers the pending flag re-installs it and raises the flag again. Any other value panics; `entry` is GC_PRD §14's name, and the PRD's other zeal modes come with the redesign |

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
     `run_trampoline` (`cps_eval/mod.rs`, the one loop every run shares since
     2026-09-10). The entire machine state is `current_step`, which the
     safe point passes as a transient root along with the `expr` the
     trampoline was entered with (its literals stay live for the call).
2. **`gc_defer_depth` counter** on the shared heap, managed by the
   `GcDeferGuard` RAII type so early returns and `?` propagation cannot leak
   an increment.

   The tree-walker takes a guard **on every trampoline entry** (`run_trampoline`,
   whether entered for a form or for a primitive's callback) rather than
   instrumenting each re-entrant call site. This inverts the failure mode: a nested trampoline is deferred
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

**Asserted, not only argued (#624).** Until #624 the protocol above was held
by comments. Each part is now checked in check builds (`heap::GC_CHECK`:
debug, or release with `gc-check`). A plain release build compiles none of
these checks except the defer balance, which every build checks.

- *A collection runs under exactly one guard.* `GcController::safe_point`
  asserts that the defer depth is 1 when it collects: the collecting loop's
  own guard and no other. A loop reads `is_outermost` once, at entry, so
  without this a guard that a callee took and kept past its instruction would
  not stop the running loop from collecting under it.
- *A holder sees no collection.* A Rust scope or value that keeps heap values
  across a call that can evaluate takes `GcDeferGuard::holding` instead of
  `new`: `ParsedLibrary` (whose constructor now requires the heap, so it
  cannot be built without its guard), `Desugarer::desugar_with_imports` and
  `VmState::with_globals`. The guard records the heap's collection count, and
  its drop panics if the count moved. Every loop entered under a holder is
  nested today, so it cannot fire yet; once stage 4e lets nested loops
  collect, the first that does under a holder fails at the holder rather than
  freeing what it holds. It does not reach the values a primitive holds
  across `ApplyContext::apply_proc`, which takes no guard of its own.
- *The defer balance* (`Heap::exit_gc_defer`) is an `assert!` in **every**
  build: in release an underflow would wrap the depth and end collection with
  no report.
- *No-collection windows.* A stretch whose soundness rests on reaching no safe
  point, rather than on a root, is covered by an `AssertNoGc` scope: a depth
  counter on the heap. Every dispatch loop and trampoline checks at its poll
  that no scope is open (`NoGcScopes::assert_none_open`), before
  `maybe_collect` and on every iteration, nested or not. Inside `safe_point`
  the check would almost never run, since it returns at once unless a
  collection is pending and the loop is outermost. The VM's windows:
  - a continuation's capture to its weak-store entry
    (`alloc_vm_continuation`, `alloc_vm_delimited_continuation`), under the
    weak-table rule in `gc_roots.rs`, and on to the write of its handle
    where the capture site writes it or hands it to a jump
    (`CaptureComposable`, `abort_to_prompt`, `exit`). `call/cc` hands the
    handle to its procedure instead, and a higher-order primitive may poll
    in a nested loop before it stores it, so from the store entry on the
    handle is covered by the deferral rule (no *collecting* safe point), not
    by a window;
  - an invoke, from where its handle and value leave the machine's roots,
    or from before the value is built, to the copy-back of the snapshot, or
    to the write of its operands into the next step's stub
    (`step_wind_jump`, `invoke_delimited`). A full continuation's weak-store
    lookup is inside the window; a delimited one's comes just before it,
    while the handle is still where the caller found it. The window is
    handed by value to `push_wind_step` / `push_invoke_step`, which drop it
    once the stub holds the operands and before they call the thunk, which
    may run a nested loop; the identity continuation hands it back to the
    caller, which drops it once it has placed the value;
  - the composable invoke's freed stub window (#172, `ResumeComposableInvoke`);
  - `ResumeWindJump`'s retained target and value, from `finish_wind_step` to
    the next write (#156).
- *Collector entry points are crate-private.* `Collector`,
  `MarkSweepCollector`, `run_mark_phase`, `Heap::sweep` and
  `GcController::collect` cannot be named outside `patina-core`, so
  `safe_point` is the only way to collect. `heap::gc::collect_for_tests`
  (`#[doc(hidden)]`) serves unit tests that drive the collector against
  hand-built state; it is compiled only with `patina-core`'s `test-support`
  feature, which only `patina-vm`'s dev-dependencies enable, so no build that
  ships contains it.

The positive controls make each check panic on purpose:
`patina-core`'s `heap::gc::tests::protocol` (depth, holder, balance and poll)
and `crates/patina-tests/tests/gc_protocol.rs` (a poll inside a window, on each
backend). CI's release GC lane runs them on its `gc-check` build, and runs
the balance control once more in the plain release build, the one build
where only that check is compiled in. Two tests
show the deferral itself is load-bearing: `parameterize` over a
parameter-like procedure that calls `(gc)` while `%parameterize-swap!` holds
an old value in Rust (both backends, `gc_shared_tests!`), and
`collection_inside_higher_order_primitive` (`gc_tree_walker.rs`). With the
nested loops' deferral disabled, both fail.

**Review rule.** A comment that argues "no safe point here" comes with an
`AssertNoGc` at the same lines (AGENTS.md). An argued window with no scope is a
missing assertion.

**Every Rust re-entry carries its reason (#622).** A Rust call that reaches a
driver loop can collect, and nothing in its type says so. The workspace
`clippy.toml` lists each such entry under `disallowed-methods`:
`ApplyContext::apply_proc`, `eval_expr` and `load_scheme_library`, and
`run_synchronously`; the VM's `execute`, `execute_nested`, `run_loop_until`,
`run_loop_until_outcome` and `across_reentry`, and its library loading; the
tree-walker's `run_trampoline`, every function that starts one (`eval_cps`,
`CpsEvaluator::eval_in_env`, `apply_from_direct_with`, `eval_core`,
`Evaluator::apply` and their wrappers; the VM routes every nested run through
`across_reentry`, the tree-walker has no such boundary), its library loading,
and `eval`'s expansion (`expand_for_eval`, `eval_step`), which loads the
libraries a datum imports while `resumable_step` holds a primitive's state and
the step's stacks; `Backend::eval`, `eval_global` and `eval_with_source_map`;
`Interpreter::eval_*` and the deprecated `Pipeline` and `SimpleInterpreter`
adapters; and `Environment::with_parent`, since an environment
built with it is reachable from no root unless its caller makes it so (#620).
Only `with_parent` is listed, as #622 scoped it: an environment from
`Environment::new` or `with_heap` that is held across a re-entry is named in
that call's reason instead (the loaders' `lib_env`, the environment
primitives' `env`). Every call outside the exempt crates carries
`#[expect(clippy::disallowed_methods, reason = "…")]` on the narrowest `let`,
match arm or statement around it (a tail call bound in a `let`, which
`let_and_return` then leaves alone; the function only for a wrapper whose one
statement is the call), saying what the frame holds across the call and why
that is safe:

- it holds nothing it reads after the call: a wrapper, or a call whose
  arguments move into the run, which roots them;
- a guard on the data defers collection: `ParsedLibrary`,
  `desugar_with_imports` and `with_globals` hold `GcDeferGuard::holding`;
- the state is in the machine: a frame it pushed, `Step::Call` and
  `resume_stub`;
- the call runs on a nested loop that defers: every `apply_proc` a primitive
  makes from a machine's dispatch, `%parameterize-swap!`'s among them. The
  tree-walker's detached `ApplyContext for Evaluator`, which an embedder
  reaches through `Interpreter::evaluator()` with no loop above it, defers
  the same calls with a holder's guard in each of its methods, and so covers
  `run_synchronously`, which only it and an embedder's own context reach.

One site is none of these, and its reason says so: `Interpreter::run_forms`
holds the last form's value unrooted across the next form, returning it stale
whenever every later form fails, on any continue-on-error entry (#605's
shape). Two others were closed when this list was drawn up: the detached
context ran an outermost trampoline beneath a primitive's Rust frame, which
collected what the primitive held (`gc_tree_walker.rs` has the case), and the
tree-walker's `-extras.scm` step ran a Rust-defined library's extras file,
found on any search path, on an outermost trampoline while an environment no
root reached held its definitions; no such file shipped, and the VM never had
the step, which is gone.

`expect` rather than `allow`: it fails clippy once its lint stops firing, so
an entry that stops matching anything fails the build, and so does a
misspelled one, crate name included, which clippy otherwise skips without a
word. `crates/patina-interpreter/src/reentry_lint_control.rs` matches every
entry that crate can name, and each private entry is matched by its own call
sites. `patina-tests` and `patina-repl` allow the lint through `[lints]` in
their `Cargo.toml` (the REPL's standard-input driver, a second
`Interpreter::run_forms`, keeps an `expect` of its own, which overrides the
crate's allow), and the test and example targets of other crates through
a crate-level `allow`, `patina-core`'s unit tests (`cfg(test)`) among them;
the core's own code is linted like any other crate's.

The reasons are the holder inventory the redesign walks: stage 2 opens loading
points A, B and D once every loading site is rooted, guarded on its data or
held in the machine, and stage 4e lets nested loops collect once every
`apply_proc` and `across_reentry` site stays deferred or keeps its values in
the machine (`PRD/GC_PRD.md` §11.3, §19).

`std::thread_local` is under `disallowed-macros` (C9, `PRD/GC_PRD.md` §18.6).
Clippy takes that lint only at a crate root: an `expect` on the invocation is
an unused attribute, and one on the enclosing module, inner or outer, is
unfulfilled while the lint still fires (measured with the pinned 1.97.1). So
each crate that has one lists its statics, with what they hold, in a
crate-root `expect`, a departure from #622, which asked for one on each
invocation's module. That `expect` is fulfilled by every later invocation in
the crate too, so the lint alone would pass a new one; a test in
`crates/patina-interpreter/src/reentry_lint_control.rs` reads each crate's
sources and fails when the statics they declare differ from the ones its
crate root names.

These checks detect violations on today's collector. The redesign replaces
them with `NoGcScope` and the collect capability at stage 3, where a nested
entry cannot collect by type, and stage 4e deletes the weak continuation
tables and with them the first two VM windows (`PRD/GC_PRD.md`).

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

The heap's `cond-expand` library-availability service is metadata outside the
Scheme arenas. Its runtime implementation holds **weak** references to the
library and loader registries, never library environments or `TaggedValue`s.
It therefore adds no GC roots and cannot form a strong heap → registry → heap
cycle. `availability_handles_do_not_retain_registries` pins that ownership boundary;
the backends remain the registry owners.

---

## 9. Known Hazards and Policies

### 9.1 Syntax provenance and source-map snapshots

Program parsers allocate a distinct, empty-scoped `Identifier` for each written
identifier. Its `written` flag preserves ordinary symbol semantics in macro
scope edits, template compilation and ellipsis recognition; an expansion's
identifiers remain distinct from these annotated source names. Ordinary datum
parsers and Scheme `read` still intern symbols and allocate no source metadata.
Their parser frames also omit source fields. The heap's `syntax_sources` table
records node spans and pair/vector child spans (including immediate values). Desugaring uses
this table, never an interned symbol as an occurrence key. Scope edits preserve
provenance; stripping syntax identifiers into quoted data preserves graph
sharing and cycles, using the same iterative graph copier. A memo shared by all
quotes in one form preserves sharing across separate insertions of a macro
argument; it is cleared when desugaring that form ends, before GC can reuse slots.

This table is **not a root**. Before reclaiming slots, sweep retains only entries
whose values are marked, then shrinks excess table capacity. This visits annotated
syntax rather than doing a lookup for every freed datum, and removes entries
before slots can be reused. There is no drain shared between nested loaders,
and a later parse cannot see stale provenance. Spans retain an `Arc` to a source
document containing text and a line-start index, but no Scheme values. Expansion
chains travel with each expanded span, so separate uses of a template do not
accumulate history at its definition. Within an invocation, nodes with the same
history prefix share its extended name array; copying every name for every node
makes large library imports consume gigabytes while collection is deferred.
Syntax copies likewise share location and child-span payloads, detaching them
only when their provenance changes. The table's entries still own no Scheme values.
IR, bytecode and errors retain a document
independently of the parsed syntax's lifetime. `SourceLocation` remains `Send + Sync`.

`SourceMap` keeps a compatibility snapshot of parsed node locations for callers
that inspect it, plus the primary document used by streaming input and parse
error formatting. These snapshots are pruned at form boundaries using the
existing capped `Heap::take_gc_freed_bits` buffer; overflow clears the snapshot.
The compiler does not consult those raw-bit snapshots. Streaming documents
forget old lines under the existing text budget, preserving the unfinished
datum; old compiled locations still report their file/line/column but cannot
quote text that was deliberately discarded.

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

**Dedup does not bound the Rust call stack.** Larceny family 6 exposed this
on 2026-09-12: `trace_cont_value(Local)` called `trace_cont_env`, which called
`trace_cont_value` for the next local continuation. Each node was visited
only once, but a deep chain still overflowed during collection. The existing
iterative walk through boxed wrappers did not cover this edge.
`trace_cont_env` now deduplicates on enqueue and queues an O(1) `ContEnv`
snapshot. `GcVisitor::drain` processes these alongside heap values and
captured continuations until all three worklists are empty. The snapshot
keeps queued entries alive, and every local continuation still traces its
environment, expression literals and captured continuation environment.
`collection_at_deep_call_depth_preserves_suspended_values` in
`crates/patina-tests/tests/gc_tree_walker.rs` collects with 50,000 suspended
calls and then reads the distinct heap pair retained by each call; the
depth-30 timing guard beside it continues to check shared-tail dedup.

### 9.5 Root sets that grow without bound (measured)

Two roots scaled with *everything ever created* rather than with live data, so
the pause grew monotonically in a long-running process. They were recorded
here because they are invisible until a session runs long; the first no longer
does.

**`code_store` constants.** Code objects were never evicted, so every compiled
top-level form added roots permanently. Since #338 a form's code is released,
constants and all, once no frame, captured continuation or live closure can run
it — a closure names its code by id, so the VM counts the closures naming each
code object — and the store follows the code still in use. What follows is the
measurement from before that change; the flat-vector or immortal-bitmap idea
still applies to a program that keeps a great many closures alive. Instrumented over one 130 ms chibi run
(17 collections), `code_store` grew **356 → 4,660 code objects** and the scan
grew **8.8 µs → 107–151 µs per collection** — by the last collection, **57% of
the entire root-tracing phase** and ~19% of the pause. The cost is the hash-map
walk and chasing scattered `Rc<CodeObject>`s, not the marking (only ~1.9
constants each). Fix: append constants to one flat `Vec<TaggedValue>` at load
time and `visit_slice` it once, or mark them into a persistent immortal bitmap
seeded into `MarkBits::for_heap`. The same argument applies to the symbol
table, which `GcVisitor::new` re-marks every collection; one immortal set
covers both.

**Continuation side tables — FIXED (stage 5, 2026-08-05).** Nothing ever
removed entries from `continuation_store` / `delimited_continuation_store`.
With 20 000 `call/cc` captures whose continuations are immediately discarded,
a collection spent **1 456 µs of a 1.79 ms pause (81%) in root tracing** — and
every register and frame snapshot those dead continuations pinned stayed
unreclaimable. Worse than the pause: snapshots routinely contain *other*
continuation refs, so the strong tables pinned themselves transitively —
`ctak` grew to 4 GB RSS and died thrashing.

The same fixpoint now also drives SRFI 124 ephemerons, which are the second
weak-key kind in the heap. They share one loop rather than running in sequence:
retaining an ephemeron can mark a `VmContinuationRef`, and tracing a weak
payload can reach an ephemeron, so either order leaves one kind discovering
work after the other has stopped. Termination is unchanged in character — an
id enters the queue at most once, and each retaining round permanently removes
a pair from a finite pending set.

The stores are now weak tables keyed by their ref objects, resolved as an
ephemeron-style fixpoint: `trace_roots` skips the stores; marking records the
id of every `VmContinuationRef` / `VmDelimitedContinuationRef` it reaches;
`run_mark_phase` then alternates `GcRoots::trace_weak_ids` broadcasts (the VM traces
payloads for newly recorded ids — which may mark further refs) with worklist
drains until quiescent, and `GcRoots::sweep_weak` prunes entries whose id was
never recorded. A whole dead chain goes in one collection because dead
payloads are never traced at all. Ids are minted by the `Heap` from one
counter shared by both continuation kinds and every `VmState` on the heap, so
an id names at most one side-table entry ever — which is what makes the
single shared id namespace and the broadcast to every provider sound.
Soundness otherwise rests on the existing safe-point discipline: capture and
invocation each touch a store only within one instruction dispatch, and
nested loops defer, so at a collecting safe point an unmarked ref proves its
payload unreachable (this is the `is_outermost` care the plan called for).
Measured after: `ctak` runs CPU-bound at flat ~220 MB RSS, and 20 000 dead
captures leave single-digit net live objects.

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
| **4c. Always-on** ✅ *(2026-08-03)* | Adaptive threshold on unconditionally; the env vars remain only as testing-lane hooks (§6). The differential script gained a default-mode reclamation proof (200k churn crosses the floor → collections > 0, arena bounded) so the lanes verify the default itself | **Met:** all lanes byte-identical with the new default; full test suite and chibi green under GC-on; interleaved on-vs-off sanity: fib −1.05%, 10M-cons churn −1.07% — parity within the 3–7% spread |
| **5+. Future** *(tracked in `PRD/ARCHIVE/GC_STAGE5_PRD.md`, superseded 2026-10-01 by `PRD/GC_PRD.md`)* | Pause work first: ~~weak continuation side tables~~ ✅ *(2026-08-05 — `trace_weak_ids`/`sweep_weak` fixpoint, §9.5; ctak's 4 GB blowup fixed)* + immortal set for `code_store` constants and symbols (§9.5, measured); explicit rooting of re-entrancy boundaries so nested loops can collect (§7); then `Collector` upgrades — lazy sweep, non-moving generational (sticky mark bits + write barriers on `set-car!`/`set-cdr!`/`vector-set!`/`MutableCell` stores); weak symbol table | Each behind the trait, benchmarked interleaved; differential lanes stay byte-identical |

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
   S=(-A test-lib scheme_tests/chibi/r7rs-tests.scm)   # (chibi test) is supplied, not bundled
   ./target/release/patina --tree-walker "${S[@]}" > base.txt 2>&1
   PATINA_GC_STRESS=1 ./target/release/patina --tree-walker "${S[@]}" > stress.txt 2>&1
   grep -qE '^1226 out of 1226 .*tests passed' base.txt \
     || echo "suite did not run in full — the diff below proves nothing"
   diff base.txt stress.txt   # must be empty
   ```
   The `grep` is not decoration. This is an *equality* check, so it passes
   hardest when both sides fail identically: drop the `-A` and patina reports
   `Library (chibi test) not found`, keeps going, and exits 0 with the same
   4537 lines each time — an empty diff that tested no GC behaviour.
   The pattern is anchored and pins the count for the same reason
   `run_chibi_tests.sh` pins `EXPECTED_TOTAL`: an aborted run still prints an
   *indented* per-section tally, and `(chibi test)` honours `TEST_FILTER`, so
   "a tally exists" is satisfied by a two-assertion run.
   `scripts/run_gc_differential.sh` asserts the same thing, the same way, and
   `2>&1` matches what it actually compares.
   Run the stress lane in a **debug build too**, not just release. Release
   tolerates a use-after-free silently (the swept slot reads back as a
   `Free`/poisoned value and surfaces later as a confusing type error at an
   unrelated call site); the debug build panics at the exact accessor with the
   slot number, and the backtrace names the code that lost the root. Stage 3's
   missed library-loading guard was diagnosed this way in one run.
2. **Poison mode (debug):** tombstoned slots hold sentinels; accessors assert.
   Any missed root becomes a deterministic panic under stress, not a
   heisenbug. Superseded by #621's stale-reference checks (§4.5), which also
   see vectors, strings and reused slots, and run in release `gc-check` builds.
3. **Reclamation proofs:** cycle tests (`set-cdr!` self-loop, closure
   capturing its own env, `call/cc` captured and dropped); arena-length
   plateau test (allocate-and-drop in a loop; assert arena `len()` stabilizes).
4. **Pause/overhead:** interleaved A/B benchmark runs (main / branch / main)
   per the project's established methodology; record `GcStats.last_pause`
   distribution on allocation-heavy benchmarks.
5. **Paranoid pre-sweep assertion (debug):** after marking, assert no free-list
   slot is marked and no marked slot is on a free list. Implemented by #621 in
   every check build (`heap::GC_CHECK`: debug, or release with `gc-check`):
   `sweep_arena`'s pre-mark panics with `dangling reference: <arena> slot N is
   free, but marking reached it` when a free-list slot's bit is already set.
6. **Retired registers are `DEAD_SLOT` (#625, GC_PRD §11.1 invariant 3):** in
   every check build, register retirement (§5.2) writes
   `TaggedValue::DEAD_SLOT` rather than `UNSPECIFIED` into each register its
   frame's per-pc map calls dead, at a collection and at a continuation
   capture. Every read of a register checks for it: `VmState::reg_at`, which
   every instruction's operands go through (`read of a retired register as
   an instruction's operand`); the closure-call fast path's argument copy and
   its rest list, which read the caller's registers directly
   (`call_closure_from_regs`: `... as a call's argument`); and
   `store_args_in_window`, for arguments read out by any other path. The heap
   refuses to store one, as a second line behind those reads: its mutators
   (`set_car`, `set_cdr`, `vector_set`, a cell write and a closure's
   free-variable write) and the constructors the VM moves register values
   through (`alloc_pair`, and so a rest list; `alloc_vector`; `AllocCell`'s
   cell; `MakeClosure`'s captures) panic with `store of a retired register
   into a ...` (`check_storable` in `heap/mod.rs`). The VM's inline
   `vector-set!` writes through a raw slice the heap cannot check, a value it
   read through `reg_at`. So a map that calls a live register dead panics at
   the read, where with `UNSPECIFIED` the program went wrong at an unrelated
   instruction, or not at all. Readers that only display registers (the step
   tracer, its watchpoints, the datum writer, `debug_format`) render it
   `#<dead>`; `--dump` disassembles without running, so it shows no register
   values. A plain release build writes `UNSPECIFIED` and checks
   nothing. Controls: `crates/patina-tests/tests/retired_registers.rs`, which
   drops the highest live register from every map with patina-vm's
   test-only switch (`test_support::DropHighestLive`, under its
   `test-support` feature, which only patina-tests' dev-dependencies
   enable), and the store controls in `heap/check.rs`; the release GC lane
   runs both. The check sees a wrong map only at a pc where a collection or
   a capture happens, which is why item 7 exists.
7. **Zeal lane (#625):** `scripts/run_gc_zeal.sh` runs the files of
   `crates/patina-tests/tests/scheme/control/` except `tail-recursion.scm`
   under `PATINA_GC_ZEAL=entry` (§6), on both backends, in a check build,
   and requires each to be byte-identical to GC-off, to exit 0 and to have
   collected. Stress collects only after allocations, so it never checks the
   maps at a pc no allocation precedes; zeal collects at every outermost safe
   point. It costs about 7× stress 1 (the full chibi suite under zeal: 882–920
   s on the VM, measured 2026-10-01), so it runs on that subset, in CI's
   `gc-zeal.yml`, path-filtered and weekly (docs/TEST_ORGANIZATION.md, "GC
   lanes"). The
   VM learns that a collection ran from `maybe_collect`'s answer, not from the
   pending flag, which zeal raises again before the collection returns:
   `finished_forms_release_code.rs` under zeal fails without it, since no
   finished form's code would be let go (#338). The positive control for the
   mode itself is `crates/patina-repl/tests/gc_zeal.rs`: a loop that
   allocates nothing collects at least once an iteration under zeal and
   hardly at all at stress 1, on both backends. That test runs in `ci.yml`,
   not against the lane's binary, and the lane's check that each file
   collected cannot see a binary that ignores the variable, since every
   control file collects at least once under the default GC while it loads
   SRFI 64; so the script first runs the same loop on its binary, on both
   backends, and fails unless it collects at least 1000 times across its
   1000 iterations under zeal and fewer than 100 with no GC variable set.
   GC_PRD's zeal-`entry` replaces this mode at stage 3, collecting at every
   poll site, nested ones included.
8. **Every traced field named, and tested with a sentinel (#623, §5.4).**
   The differential and zeal lanes see a missed trace edge only when no
   other path reaches the value; #47's `resume` was also stored in the
   traced `captured_cont_env` and went unseen for 1.6 days. So every trace
   function destructures its struct by name (`scripts/check_gc_trace_names.py`
   in CI), and every traced struct has a sentinel test with a value reachable
   only through each field. Each sentinel test fails with its field's trace
   line deleted; that break-test was run for every traced field when the
   tests landed.

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
