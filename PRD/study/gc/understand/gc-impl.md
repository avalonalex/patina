# Patina GC implementation map (gc-impl)

Revision read: `main` at `28a94f8` (2026-09-30, clean tree). Machine for new measurements: macOS 27.2 arm64, 12 logical CPUs, release build of that revision built into a separate target dir. Tags: **[V]** = verified by reading source or by a measurement run for this report; **[D]** = number quoted from a repo doc or commit message (not re-run); **[I]** = inference.

Probes used for the new numbers live under `PRD/study/gc/probes/gc-impl/src/{main.rs,bin/*.rs}` and `PRD/study/gc/probes/gc-impl/scm/*.scm`.

---

## 1. Executive summary

1. **One non-moving, stop-the-world, non-incremental mark-sweep** (`crates/patina-core/src/heap/gc.rs`, 1,812 lines) serves both backends. Marking uses side bitmaps per typed arena; sweep walks every slot of every arena, pushes dead indices onto LIFO free lists and overwrites each dead slot with a tombstone so `Rc` payloads drop immediately. **[V]**
2. **Allocation never collects.** `note_alloc` only raises a `Rc<Cell<bool>>` pending flag. Collection happens at backend safe points (top of the VM dispatch loop, once per instruction; top of the tree-walker trampoline, once per step), and only in the outermost loop. Nested loops and Rust frames holding unrooted values are protected by a `GcDeferGuard` counter rather than by rooting. This is why ~430 `alloc_*` call sites and ~450 heap `borrow`s in primitives can hold raw `TaggedValue`s in Rust locals safely. It is the main invariant a redesign must keep or replace. **[V]**
3. **Pause time grows with the arena high-water mark, not with live data**, because sweep visits every slot and arenas never shrink. Measured: after importing about 25 R7RS-large libraries, each `(gc)` costs **15.4 ms on the VM and 22 ms on the tree-walker**, with 99.7% of 4.62M pair slots and 2.89M object slots free. **[V]**
4. **Library loading and macro expansion run entirely under deferral**: `ParsedLibrary` carries a guard, and so does `desugar_with_imports`. Loading those 25 libraries peaks at **662 MB RSS with GC on and 659 MB with GC off**, so collection does nothing during a load. A library body that churns 5M conses gets 1 collection and keeps a 5M-slot arena (116 MB); the same code at top level gets 76 collections (12.7 MB). **[V]**
5. **The trigger counts allocations, not bytes.** 500 × `(make-vector 100000)` causes **0 collections and 414 MB RSS**. **[V]**
6. **Several costs are paid in full on every collection, regardless of live data:** the symbol table, which is immortal (1M dead symbols kept 238 MB and cost 3.9 ms per GC); the core-syntax table; every live code object's constants; every environment binding (deduplicated through hash sets); and, on the tree-walker, every live procedure's CPS body, walked with a per-node hash-set dedup. **[V]/[I]**
7. **Representation costs.** Pairs are lean at 16 B/slot. Every other object is a 72-byte `HeapObjectData` enum slot, so flonums, `MutableCell` boxes and continuation refs each cost 72 B. Vectors, strings, closures' free variables, records, bytevectors and bignums add a separate `malloc`. Strings are UTF-32 (`Vec<char>`). **[V]**
8. **Values handed to embedders are unrooted.** A `TaggedValue` returned by `Interpreter::eval_str` silently changed from `(#(1 2) #(3 4) "str")` to `(i 0)` after a later `(gc)` plus some allocation (release build). **[V]**
9. **Existing infrastructure a new collector can reuse:** the `Collector`/`GcRoots` seam, `run_mark_phase`'s single weak fixpoint, per-PC register-root bitmaps (a stack-map precursor), the pending-flag trigger design, the debug poison, and the CI differential lanes (off / adaptive / stress, release plus debug-poison, both backends) with their reclamation proofs. **[V]**

---

## 2. Object model the collector sees

### 2.1 Value encoding — `crates/patina-core/src/tagged_value.rs`
- `TaggedValue(u64)` is a low-3-bit tagged word (`:57`, tags `:76-83`). Fixnum is `000`, a 61-bit integer. `001` holds specials, `010` chars. Heap tags: `011` pair, `100` vector, `101` string, `110` closure, `111` object. **[V]**
- A heap reference is a `HeapIndex = u32` arena index shifted left by 3 (`:28`, `:376-411`), not a pointer. Each arena therefore holds at most 4G slots. **[V]**
- **`TAG_CLOSURE` (`110`) is never minted.** No non-test code calls `TaggedValue::closure(`. Closures are `TAG_OBJECT` `VmClosure`/`Procedure` objects, so one primary tag is free for a redesign. `value_is_live` treats such a key as "no arena tracks it", which is conservative (`gc.rs:528-534`). **[V]**
- Reserved specials: `GC_POISON = 0xF8|001` is the debug pair tombstone (`:103`); `FORWARDED = 0xF0|001` marks an imported environment slot (`:110`). **[V]**

### 2.2 Heap layout — `crates/patina-core/src/heap/mod.rs`
`SharedHeap = Rc<RefCell<Heap>>` (`:51`). `Heap` (`:304-431`) holds:

| Arena | Rust type | Slot size [V] | Extra storage |
|---|---|---|---|
| `pairs` | `Vec<(TV,TV)>` | 16 B | none |
| `vectors` | `Vec<Vec<TV>>` | 24 B | malloc'd buffer of 8n B per vector |
| `strings` | `Vec<Vec<char>>` | 24 B | malloc'd buffer of 4n B (UTF-32) |
| `objects` | `Vec<HeapObjectData>` | **72 B** | Rc/Vec/String payloads per variant |

The heap also has four `Vec<HeapIndex>` free lists (`:366-376`), `symbol_table: HashMap<String,HeapIndex>` (`:319`) and `core_syntax_table` (`:332`). It carries GC bookkeeping fields: `allocs_since_gc`, `gc_threshold`, the `gc_pending` handle, `gc_freed_bits` (capped at 65,536 entries), `gc_freed_closure_code_ids`, `gc_defer_depth`, `gc_collections`, `gc_last_swept`, and `next_vm_continuation_id` (`:378-430`). It also holds `syntax_sources: HashMap<u64 raw bits, Rc<SyntaxSource>>` (`:305`), which is pruned at sweep. **[V]**

`HeapObjectData` (`:143-229`) has 28 variants: BigInt, Rational, Real(f64), Complex, Symbol(Rc<str>), Bytevector(Vec<u8>), Exception{kind, String, Vec<TV>}, Procedure(Rc), Port(Rc), Macro(Rc<CompiledMacro>), RecordType(Rc), Record{Rc<RTD>, Rc<RefCell<Vec<TV>>>}, Identifier{Rc<str>, ScopeSet, bool}, Continuation(Rc<CpsContinuation>), Parameter{Rc<RefCell<Vec<TV>>>, Option<TV>}, Promise(Rc<RefCell<PromiseState>>), Library(Rc), Values(Vec<TV>), EnvironmentSpecifier{Rc<Environment>}, PromptTag(Rc), LabelPlaceholder, MutableCell(RefCell<TV>), Ephemeron(RefCell<Option<(TV,TV)>>), VmClosure{code_id:u64, free_vars:Vec<TV>, globals:Rc<Environment>}, VmContinuationRef(u64), VmDelimitedContinuationRef(u64), CoreSyntax, Free. **[V]**

- The enum is 72 B because of the largest variants. `Exception` is 24+24+24; `Rational` is 2×`BigInt`; `Identifier` is 16 + `ScopeSet` (40) + bool. **[V]** sizes / **[I]** which variant dominates.
- A boxed flonum therefore uses a 72 B slot, about 9× its payload. Chez uses 16 B for a flonum. **[V]**
- Most identifiers in parsed programs are per-occurrence `Identifier` objects, not interned symbols. A minimal program shows `symbols = 13` after bootstrap, against ~10k objects of which ~9.4k are free after the first collection. **[V]**

### 2.3 Identity is not uniformly "the slot"
- `values_eq` (`heap/mod.rs:2118-2146`) compares raw bits first. If those differ, for two objects it compares `Procedure`, `RecordType` and `Record` by **`Rc::ptr_eq`** on the payload, so two distinct slots can be `eq?`. **[V]**
- `tagged_value_hash_identity` (`:2541-2561`) hashes the heap index, or the `Rc` address for those three types. Its doc says explicitly that this relies on the collector not moving objects. SRFI 69/125 and `(scheme hash-table)` `eq?` tables go through it (`lib/srfi/69/srfi-69-impl.scm:118`). **[V]**
- **JIT consequence [I]:** today `eq?` on objects cannot be compiled to a single compare. Moving collection would need address-independent identity hashes, for example a header hash in the style of Java or Chez's rehash-after-GC `eq` hashtables.

### 2.4 Values held outside the arenas (must be traced, or covered by deferral)
- **Environments** (`environment.rs:487-547`, 224 B). Slot-based `Bindings` (`SmallVec<[(Rc<str>,TV);3]>` plus an FxHashMap index once a frame has more than 8 bindings, `:208-219`). Also `ScopedTable`, `alias_bindings`, `rare.owners` (import owners), and the parent `Rc`. GC hooks: `for_each_local_value` (`:2191`), `for_each_shared_owner` (`:2211`), `for_each_alias_target` (`:2226`), `gc_identity` (address, `:2185`). **[V]**
- **VM** (`vm_state.rs`, `types/`):
  - `ExecutionState.registers: Vec<TV>`.
  - `CallFrame` (40 B) with `closure: Option<HeapIndex>`, a bare index, and `code: Rc<CodeObject>`.
  - `CodeObject.constants: Vec<TV>` (CodeObject is 160 B).
  - `continuation_store` / `delimited_continuation_store: RefCell<FxHashMap<u64, VmContinuation>>`. A snapshot (152/160 B) clones the whole register file and frame stack (`execution_state.rs:245`), and invoking one clones them back (`:256`).
  - `pending_escape`, `scratch_args`, and the wind/prompt/handler stacks. **[V]**
- **Tree-walker:** the `StepResult` local, `ContValue`/`ContEnv` (a persistent `Rc` list, `cont_value.rs:39-48`), `CpsContinuation` (208 B), `Procedure::CpsLambda` (120 B) bodies holding `CpsExpr` literals, and the `PENDING_ESCAPE` thread-local. **[V]**
- **Shared:** `LibraryRegistry` libraries (exports map plus env), `CompiledMacro` literals (224 B), `ParsedLibrary.body`, and the parser/expander state. **[V]**

---

## 3. The collector — `heap/gc.rs`

### 3.1 Seams
- `GcRoots` (`:177-197`): `trace_roots(&self, &mut GcVisitor)`; `trace_weak_ids(&self, &[u64], ..)`, a default no-op, for weak side tables; and `sweep_weak(&self, &GcVisitor)`, which prunes side tables before the heap sweep. Implementors in production code: `VmState` (`vm_state/gc_roots.rs:70`), `StepTracer` (`tracer.rs:283`), `LibraryRegistry` (`library_registry.rs:569`), and the tree-walker's `Evaluator`, `EscapeRoots` and `StepRoots` (`cps_eval/gc_roots.rs:29,42,58`). **[V]**
- `Collector::collect(&mut self, &mut Heap, &[&dyn GcRoots]) -> GcStats` (`:208-213`). The doc makes **non-moving** a contract. **[V]**
- `run_mark_phase(heap, roots) -> MarkBits` (`:1022-1101`) is public. Alternative collectors are meant to compose around it, as mark phase → `Heap::sweep`. **[V]**

### 3.2 Marking
- `MarkBits` is four `BitSet`s sized to the arena lengths and allocated fresh for each collection (`:85-136`). Nothing persists on `Heap`. **[V]**
- `GcVisitor::new` (`:449-479`) pre-marks every `symbol_table` and `core_syntax_table` index on every collection, without a worklist round-trip. **[V]**
- `visit` (`:485-501`) dispatches on the tag. Pairs, vectors and objects are pushed on `worklist: Vec<TV>`; strings are mark-only leaves. `visit_object_index` handles bare `CallFrame.closure` indices. **[V]**
- Deduplication of `Rc` graphs uses **four `FxHashSet<usize>`** keyed by address: `seen_envs`, `seen_conts`, `seen_exprs` and `seen_shared` (`visit_once`, `:580`). `visit_env` (`:539-559`) walks the parent chain and recurses into alias and shared-owner targets. Each live closure costs one hash-set probe for its `globals` env. **[V]**
- `drain` (`:663-677`) has three worklists: heap values, `Rc<CpsContinuation>`s, and `ContEnv` snapshots. The last makes deep continuation chains iterative (#306). Recursion remains in `visit_env`'s alias-target edge (`:554-556`) and in `visit_expr_literals` (bounded by program size; doc `:636-646`). **[V]**
- Trace rules (`:695-794`):

  | Class | Variants |
  |---|---|
  | Leaves | BigInt, Rational, Real, Symbol, Bytevector, Port, RecordType, Identifier, PromptTag, LabelPlaceholder, CoreSyntax, Free |
  | Traced | Complex, Exception irritants, Procedure (CPS body literals + env), Macro (literals + `definition_env`), Record fields, Parameter values + converter, Promise, Library, Values, EnvironmentSpecifier, MutableCell, VmClosure (`free_vars` + `visit_env(globals)`), Continuation |
  | Weak | Ephemeron, VmContinuationRef, VmDelimitedContinuationRef |

  The enum doc (`heap/mod.rs:137-141`) warns that misfiling a variant as a leaf is a use-after-free rather than a compile error. **[V]**
- **Worklist growth is O(n) on lists with heap cars.** `trace_children` visits car then cdr, and the LIFO pop takes the cdr next, so every car stays queued. Measured on a 2M-element list of 2-vectors: **peak RSS 72 → 127 MB during one collection**. A fixnum-car list shows no growth. End to end, a Scheme program building such a list peaks at **147 MB with GC on against 87 MB with GC off**. **[V]**

### 3.3 Weak references: one fixpoint for two kinds (`:1050-1098`)
- **VM continuation side tables** (#19, 2026-08-05). Marking a ref object records its id (`live_weak_ids`, `new_weak_ids`). The loop broadcasts newly recorded ids to every provider's `trace_weak_ids`, which traces the payload, then drains again. Ids are minted per heap and never reused (`next_vm_continuation_id`), so broadcasting to every provider is sound. `VmState::sweep_weak` → `prune_store` retains live ids and calls `shrink_to_fit` when `len*8 < capacity` (`gc_roots.rs:130-149`). **[V]**
- **SRFI 124 ephemerons** (#130). Neither field is traced on arrival; the pair goes on `pending_ephemerons`. In each round, a pair whose key is already marked has key and datum traced. Pairs still pending at quiescence are broken (`break_ephemeron` sets the `RefCell` to `None`, `heap/mod.rs:1237`). **[V]**
- The two must share one loop: a sequenced first version was a use-after-free (commit `1d18c49`, second part). Termination: each id is queued at most once, and each retaining round removes at least one pending pair. **[V]/[D]**
- **No weak pairs, weak boxes, weak/ephemeron hashtables, guardians or will executors.** A search for guardian, will and finaliz finds none. **[V]**

### 3.4 Sweep (`Heap::sweep`, `:891-961`)
1. `syntax_sources.retain(marked)`, then `shrink_to(64)` if capacity exceeds `max(4·len, 64)`. **[V]**
2. `sweep_arena` per arena (`:828-852`):
   - First **set the mark bit of every free-list index**, an O(free) pass that avoids double-pushing.
   - Then walk **every slot**. For each unmarked one, call `record_freed`, write the tombstone and push the index.
   - Tombstones: pairs get `(GC_POISON, GC_POISON)` in debug builds only (release skips the store); vectors and strings get `Vec::new()`, which frees the buffer; objects get `HeapObjectData::Free`, which drops all `Rc`/`Vec`/`String` payloads. **[V]**
3. `record_freed` captures raw bits for `SourceMap` pruning when enabled (cap 65,536, then `Overflowed`; `:857-873`). It also captures the `code_id` of each dead `VmClosure`, for code-unit release (#338/#353). **[V]**
4. Reset `allocs_since_gc`, lower `gc_pending`, increment `gc_collections`, and set `gc_last_swept`. **[V]**

**Eager drop is load-bearing.** Environments are `Rc`, outside the heap; env → heap edges are bare indices; heap → env edges are owning `Rc`s inside slots. Every closure ↔ environment cycle therefore passes through a heap slot, and tombstoning that slot breaks it (GC_DESIGN §8; test `tombstone_drops_rc_payload_breaking_env_cycle`, `gc.rs:1429`). Drop also has **finalizer-like side effects**: a swept `Port` drops its `BufWriter`, which flushes. #346 added a weak per-thread list of open file ports that `end_process` flushes at exit, because `process::exit` runs no destructors. **[V]/[D]**

### 3.5 `MarkSweepCollector` (`:974-1119`)
- `auto_threshold = max(min_threshold = 65,536, 2 × live_after_last)`. `live` is the total marked **slot count** across arenas. **[V]**
- `GcStats{collections, last_marked, last_swept, last_pause_micros}` is computed but **nothing outside gc.rs reads `last_pause_micros`**: it is not exported to Scheme, logs or benchmarks. **[V]**

---

## 4. Trigger policy

- `note_alloc` (`heap/mod.rs:581`) does `allocs_since_gc += 1`. If the count is at least `gc_threshold`, it sets `gc_pending = true`. Every `alloc_*` calls it: `alloc_pair :703`, `alloc_vector :770`, `alloc_string_chars :831`, `alloc_object :1469`. **[V]**
- `request_gc` (`:609`) is what `(gc)` calls. It raises the flag in any mode. **[V]**
- `GcMode::from_env` (`gc.rs:301`) defines three modes:
  - **On**, the default.
  - **Off**, selected by `PATINA_GC=0`; collects only on `(gc)`.
  - **Stress(n)**, selected by `PATINA_GC_STRESS[=n]`; collects every n allocations and ignores the adaptive floor.

  `GcController::current_threshold` maps the mode to a threshold, and `GcController::collect` reinstalls it after each collection (`:342-357`). The VM installs the threshold at `VmState::new` (`vm_state.rs:222-236`). The tree-walker caches its own handle (`eval/mod.rs:102`); each backend owns a separate `GcController`. **[V]**
- **The trigger is blind to object size.** Measured with zero collections in each case:
  - 500 × `(make-vector 100000)`: 414 MB peak RSS.
  - 500 × `(make-string 100000)`: 216 MB (UTF-32).

  Rust-side memory (environments, `ContEnv`, `CpsContinuation`, continuation snapshots, `CodeObject`s) is not counted at all. **[V]**

---

## 5. Safe points and deferral

### 5.1 Protocol
`GcController::safe_point(gc, heap, pending, is_outermost, with_roots)` (`gc.rs:381-409`):
- Fast path: `!is_outermost || !pending.get()`, one load and branch.
- Cold path: `#[inline(never)]`. It takes `heap.borrow_mut()` for the whole collection, and the backend closure can abort without collecting when a root (the library registry) is mutably borrowed. The flag stays raised so the next safe point retries. **[V]**

### 5.2 VM
- `run_loop_until_outcome` (`vm_state.rs:1142`) takes a `GcDeferGuard` on every loop (`:1151`) and hoists `is_outermost`.
- Each iteration reads `pending`, then calls `maybe_collect` (`:1205`, defined `:1287-1310`). That function:
  - calls `execution.retire_registers()`, which uses the per-PC `register_roots` bitsets to overwrite dead temporaries with `UNSPECIFIED` across **all** frames (`gc_roots.rs:45-68`);
  - roots `[VmState, LibraryRegistry]`.
- Afterwards, if `pending && !gc_pending`, it calls `after_collection` (`:542-571`). That drains freed closure code ids, decrements `CodeObject.live_closures`, and releases code units nothing can run.
- Safe points are **per instruction**; normal dispatch has no stack-map metadata cost. **[V]**

### 5.3 Tree-walker
`run_trampoline` (`cps_eval/mod.rs:213-230`) also guards every entry. `maybe_collect` (`:114-133`) roots `[Evaluator (global env), LibraryRegistry, EscapeRoots, StepRoots{step, entry expr}]`. **[V]**

### 5.4 `GcDeferGuard` sites (`gc.rs:232-268`, RAII on `Heap.gc_defer_depth`)

| Site | Location | What it covers |
|---|---|---|
| Every VM dispatch loop | `vm_state.rs:1151` | nested `run_loop_until` from `execute_nested`, `run_apply_proc` (`control.rs:2331`), library bodies |
| `VmState::with_globals` | `vm_state.rs:337-347` | globals-swap windows (`eval`, library load) |
| `Desugarer::desugar_with_imports` | `patina-frontend/src/desugarer/mod.rs:1841` | expansion, including imports that run library bodies |
| `ParsedLibrary::new` | `patina-runtime/src/library_loader.rs:195-208` | for the whole lifetime of a parsed library, i.e. its entire body evaluation |
| Every tree-walker trampoline | `cps_eval/mod.rs:219` | callbacks and nested runs |

**[V]**

`ApplyContext::apply_proc` callers still exist in Rust: `values.rs:35,48`, `lists.rs:440,587` (comparators), `parameters.rs:250,273`, `ports.rs:666`, `file.rs:216,263` and `lazy.rs:125`. AGENTS.md says the program-facing versions of `member`/`assoc`/`call-with-port`/`force` now run in Scheme or as stub frames; whether every route has stopped reaching these Rust paths is **[I]** unverified. Any that remain run nested and therefore cannot collect.

### 5.5 Measured consequences of deferral

| Workload | Collections | Arena / heap | Peak RSS | Pause |
|---|---|---|---|---|
| 5M conses churned in a library body | 1 | 5.0M-pair arena | 116 MB | — |
| Same churn at top level | 76 | 65k-slot arena | 12.7 MB | — |
| Load of ~25 R7RS-large libraries | — | 4.6M pairs, 2.9M objects; ~16k pairs and ~5k objects live afterwards | 662 MB (GC off: 659 MB) | ~15 ms per later `(gc)` |

All rows **[V]**. This also shows that expansion-time garbage dominates allocation volume during bootstrap. **[I]**

---

## 6. Root inventory and per-collection fixed cost

| Root | Provider | Cost characteristic |
|---|---|---|
| Symbol + core-syntax tables | `GcVisitor::new` | O(#symbols) per GC; symbols immortal. 1M symbols: (gc) 3.9 ms, 238 MB **[V]** |
| VM register file (all frames, after retirement) | `VmState::trace_roots` | O(stack); locals conservative, temps retired via per-PC bitsets (#423) **[V]** |
| `CallFrame.closure` (bare `HeapIndex`) | `trace_frames` | O(frames) **[V]** |
| `code_store[*].constants` | `trace_roots` loop | O(live code). Bounded since #353 by "can still run", but all library code is reachable through library closures. Pre-#353 measurement: 8.8 → 107–151 µs/GC, 57% of root tracing, ~19% of pause **[D]** |
| `globals` env + every library env + exports | `visit_env`, `visit_library` | O(total bindings) + hash-set dedup per env **[V]** |
| Wind/prompt/handler stacks, `pending_escape`, `scratch_args`, tracer snapshots | VM | small **[V]** |
| Continuation side tables | weak (`trace_weak_ids`) | O(live captures × snapshot size) **[V]** |
| `StepResult` + `ContEnv` chain + entry-expr literals | tree-walker `StepRoots` | O(continuation depth) with dedup **[V]** |
| CPS body literals of every live `Procedure` | `trace_object_children` | **O(code size of all live procedures) per GC**, walked through a per-node `FxHashSet`. **[V]** code / **[I]** this is why the tree-walker's floor exceeds the VM's (22 vs 15 ms in §5.5) |

Arena sizes at bootstrap and the resulting `(gc)` pause floor ("TW" = tree-walker throughout):
- After bootstrap of a minimal program (`(scheme base)`, `(scheme write)`, `(scheme time)`, `(patina debug)`): 16,891 pair slots and 10,243 object slots. Pause floor **72–114 µs on the VM and 97–105 µs on the TW** (first collection 782 / 600 µs). **[V]**

---

## 7. Debug, poison and instrumentation

- **Poison:**
  - Pairs: `GC_POISON`, debug only. `get_pair`, `set_car` and `set_cdr` assert against it (`heap/mod.rs:718-762`).
  - Objects: the `Free` tombstone, asserted in `get_object` (`:1484-1494`) in debug builds.
  - Vectors and strings: the tombstone is an empty `Vec`, which is legal, so a use-after-free there is undetectable.

  **[V]**
- **Not implemented:** GC_DESIGN §11.5's "paranoid pre-sweep assertion" (no free-list slot marked). Sweep *pre-marks* free-list slots, so a root that points at a freed slot is silently absorbed. A dangling pair holding poison is an immediate-tagged special and is simply skipped. **[V]**
- **Introspection:**
  - `(gc)` and `(gc-stats)` in `(patina debug)` (`patina-primitives/src/primitives/gc.rs`, `patina-runtime/src/stdlib/patina_debug.rs`).
  - `(gc-stats)` returns an alist of arena lengths, free-list lengths, symbols, allocs-since-gc, collections and last-swept (`HeapStats`, `heap/mod.rs:3120-3159`). It reports no bytes, no pause, no RSS and no histogram.
  - The benchmark harness snapshots these counters (`docs/VM_TESTING.md:166-174`, which says "No GC pause latency ... claim can be made").

  **[V]**
- **Differential lane** `scripts/run_gc_differential.sh`:
  - Runs the chibi R7RS suite with `-k -A test-lib` on both backends in three modes: off (`PATINA_GC=0`), default, and stress (`PATINA_GC_STRESS=16`; `1` is 0.15 s → 103 s). Output is normalised for ANSI codes and timing only.
  - Asserts the tally is exactly `EXPECTED_TOTAL=1226` so that a vacuous pass is impossible, and diffs default and stress against off.
  - Two reclamation proofs: stress mode needs more than 1000 collections and arena growth under 256 pairs across 20k churned conses; default mode needs at least one collection and a pair arena under 150k after 200k churn.
  - CI runs it in release and debug-poison jobs (`.github/workflows/ci.yml:92-120`).

  **[V]**
- **Tests:**

  | Location | Tests |
  |---|---|
  | `gc.rs` unit tests | 22 |
  | `weak_continuation_tests.rs` | 7 |
  | `gc_vm.rs` | 9 |
  | `gc_tree_walker.rs` | 4 (including a 50,000-deep suspended-call collection and a depth-30 dedup timing guard) |
  | `common/mod.rs` `gc_shared_tests!` | 13 cases × 2 backends |
  | `ephemerons.rs` | 15 (register retirement, `reference-barrier`, continuation interplay) |
  | `finished_forms_release_code.rs` | 9 (code-unit release) |

  Plus `interpreter_api.rs`, and `Keep` root tests in `source_map.rs:393` and `heap/source.rs:214`. **[V]**
- **External signal:** Larceny's `ephemeron` suite is 6/6 on both backends. Its `force-gc` allocates 100M pairs, which took 142 s on the TW (`scheme_tests/reports/larceny_triage.md:540`). **[D]**

---

## 8. Recorded measurements (from docs and commits) **[D]**

| What | Number | Source |
|---|---|---|
| Stage-3 safe point, GC off | −2.5 to −3.5% (0.24–0.34 ns/instr) | GC_DESIGN §6.1 |
| Stage-3 safe point, GC on, 0 collections | −13.7% (≈1.75 ns/instr: non-inlined call + 2 RefCell borrows) | §6.1 |
| After 4a (flag in `Rc<Cell<bool>>`) | off: −0.4% dispatch, −1.3% alloc-heavy vs main; +1.1–1.4% vs no-safe-point control; on vs off −0.2% | §6.1, commit `3d7f13f` |
| 4c on vs off | fib −1.05%, 10M-cons churn −1.07% (spread 3–7%) | `7401cba` |
| `ContEnv` un-memoized trace | 6.8 s per GC at depth 26, ~1.9×/level → 0.00 s after `visit_once` | §9.4, `bfff2e4` |
| Stress reclamation | 20k-cons: ~4,039 pairs held vs 24,047; 20,004 collections | stage 2/3 commits |
| Strong continuation tables | 20k dead captures: 1,456 µs of a 1.79 ms pause (81%) in roots; ctak 4 GB RSS crash | §9.5 |
| After weak tables | ctak(32,16,8) 72.6 s at 227 MB peak RSS; 20k dead captures → single-digit live | `f44bb11` |
| `code_store` growth (pre-#353) | 356 → 4,660 code objects in a 130 ms chibi run (17 GCs); scan 8.8 → 107–151 µs | §9.5 |
| #353 code release | 200k `(display 1)` forms: 143 MB → 29 MB; 800k streamed: 479 MB → 26 MB; residual ~8 B/form index (#352); ~1% closure-creation cost | `951a82a` |
| #423/#551 register retirement | fib(35) +0.9%, 20M tail sum +1.1%, 10M churn −0.1% | GC_STAGE5 P2b |
| Profiles (Track P) | `alloc_*` + GC sweep/visit 2.6% deriv, 3.1% nboyer; malloc/free 6.5% deriv; `read/write_mutable_cell` 2.1% nboyer | `PRD/TRACK_P_PERFORMANCE_PRD.md:229,315` |
| Bench "Allocation + GC: list 256" | VM 14.9 µs, TW 238 µs per op; 3 automatic GCs | `benchmark_reports/performance.md:22` |

## 9. New measurements for this report **[V]**

**Collector microbenchmarks.** Direct `MarkSweepCollector` calls, with no `RefCell`:

| Case | Result |
|---|---|
| n live pairs (a list) + n dead pairs, n = 100k / 1M / 4M | 0.47 / 4.73 / 19.6 ms ≈ 2.4 ns per arena slot |
| Mark only | 2.8–3.0 ns per live pair |
| Re-collect with the arena half free | slower than the first collect (5.5 vs 4.7 ms at 1M) because of the free-list pre-mark pass |
| 1M dead `VmClosure`s (2 free vars + `Rc<Env>`) | swept at 8.4 ns each |
| 1M already-free object slots | walked at 2.1 ns/slot |
| 1M live closures | 8.1 ns each (`visit_env` hash probe per closure) |
| 1M live + 1M dead flonums, plus 1M pairs | 23.6 ms |

**Allocation, without the `RefCell` borrow the VM pays:**
- pair 1.46–1.58 ns
- 4-element vector 11.2 ns (malloc)
- flonum 8.2 ns
- Arena `Vec` doubling: worst single allocation ≤ 93 µs over 16M pairs. macOS realloc remaps, so growth stalls are not a problem here. Growth does move the base address. **[I]** glibc mremap behaves similarly.

**`(gc)` pauses measured with `current-jiffy` (µs resolution), VM / TW:**

| Heap state | VM | TW |
|---|---|---|
| Minimal program after bootstrap | 72–114 µs | 97–105 µs |
| 1M live pairs | 2.9–3.1 ms | 4.2–4.6 ms |
| + 100k 8-vectors + 200k flonums | 6.8 ms | 8.5 ms |
| After all of that is dropped | 2.2 ms, then steady 3.3 ms | 2.7 / 3.7 ms |

The last row shows a pause sustained by the high-water mark.

**Whole runs, GC on vs off (time; peak RSS):**

| Workload | GC on | GC off | Collections (on) |
|---|---|---|---|
| nboyer(2) | 0.91 s, 57 MB | 0.89 s, 86 MB | 15 |
| deriv ×200k | 0.54 s, 14.7 MB | 0.53 s, 159 MB | 207 |
| ctak(18,12,6) | 0.07 s, 103 MB | 0.08 s, 184 MB | 2 |
| 10M-cons churn | 0.59 s, 13 MB | 0.57 s, 174 MB | 152 |

At today's interpretive speed, GC throughput cost is in the low single-digit percent. **[I]** It will grow sharply once a JIT removes dispatch overhead.

**Type sizes:** TaggedValue 8, pair slot 16, Vec slot 24, object slot 72, Environment 224, CpsContinuation 208, Procedure 120, CallFrame 40, VmContinuation 152, VmDelimitedContinuation 160, CodeObject 160, CompiledMacro 224, Library 152, ScopeSet 40, Rc<str> 16.

---

## 10. Limitations, stated precisely

1. **Sweep cost is proportional to arena size, and arenas never shrink.** `sweep_arena` iterates `arena.iter_mut()` over every slot and first pre-marks the whole free list (`gc.rs:836-850`). Nothing truncates a `Vec` or returns pages. Pause and RSS keep the peak forever (§5.5, §9). **[V]**
2. **Nested and deferred scopes cannot collect.** Library bodies, expansion with imports, `with_globals` windows, and Rust `apply_proc` callbacks never collect. The fix proposed in GC_STAGE5 Priority 2 (root the re-entry boundary) is not implemented. **[V]**
3. **The trigger counts slots, not bytes**, and ignores Rust-side memory (§4). **[V]**
4. **Symbols and core-syntax markers are immortal and re-marked on every GC.** There is no weak intern table (§6). The "immortal set" proposed in GC_STAGE5 P1.2 was never built; #353 bounded `code_store` instead. **[V]**
5. **Many root sets are rescanned in full on every GC** (§6): all environments, all live code constants, all live CPS bodies on the TW, and the symbol table. There is no generational or remembered-set structure, so every collection is a full-heap mark. **[V]**
6. **The mark stack is unbounded:** O(list length) for heap-car lists, with measured +55 MB (§3.2). Recursion remains on alias-environment edges. **[V]**
7. **Per-object malloc and fat slots.**
   - Vectors, strings and bytevectors have separate buffers; closures' `free_vars` are a `Vec`; records use `Rc<RefCell<Vec>>` plus `Rc<RTD>`; bignums are `Vec`-backed.
   - 72 B covers a flonum, a `MutableCell` or a continuation ref. Strings are UTF-32.
   - Closure variable access is three dependent loads: `RefCell` borrow → `objects[idx]` enum match → `free_vars[slot]` (`LoadClosure`, `vm_state.rs:1451-1467`).
   - `MakeClosure` costs a `Vec` malloc, an `Rc` env clone and a code-object counter bump (`:1538-1551`).

   **[V]**
8. **Drops at sweep are eager and unbounded.** A `HeapObjectData` drop cascades through `Rc` graphs (environments, `CompiledMacro`, `CpsContinuation`). The work happens inside the pause, and side effects such as port flush run inside sweep. Lazy or concurrent sweep would change *when* ports flush. **[V]/[I]**
9. **Free-list allocation reuses slots in reverse index order** (LIFO `Vec<u32>`, 4 B per free slot). Locality degrades after the first GC. Each type has its own high-water mark, so space freed in one arena cannot serve another (e.g. a pair burst cannot be reused by objects). **[V]/[I]** locality
10. **Raw indices and raw bits escape**, which blocks moving collection as built:
    - `symbol_table`, `syntax_sources` and `SourceMap` are keyed by raw bits.
    - Identity hashing uses the index (§2.3).
    - `CallFrame.closure` is a bare `HeapIndex`.
    - `eq?`/`eqv?` take a raw-bits fast path.
    - Transient raw-bits `HashSet`s are used in the writer, parser and cycle detection (GC_DESIGN §3.4, §9.3).

    The count of these sites is modest: `.raw_bits()`/`from_raw` appear in about 12 files outside tests, 15 of the uses in `heap/source.rs`. **[V]**
11. **Locals stay conservative and capture copies the whole stack.** Only expression temporaries are retired. Continuation capture and invocation copy the entire register file and frame vector (`execution_state.rs:245,256`), so capture costs O(stack) and snapshots pin everything they copied. **[V]**
12. **Embedding values are unrooted.** `Interpreter::eval_str` returns a raw `TaggedValue` (`patina-interpreter/src/lib.rs:278`) with no handle or root API. Verified dangling after a later `(gc)` (§1.8). The same gap applies to any future JIT ↔ runtime boundary. **[V]**
13. **Single-threaded `Rc<RefCell<Heap>>`.** Every VM allocation does `state.heap.borrow_mut()` (32 heap borrows in `patina-vm/src/runtime`, about 450 `borrow`/`borrow_mut` calls in `patina-primitives`). There is no thread-local allocation buffer and no raw heap pointer a JIT could cache. Arena `Vec` growth also invalidates base pointers. **[V]**
14. **Docs have drifted** (sources were not changed):
    - GC_DESIGN's header still says "Approved design, not yet implemented". §3.1 says 26 variants, against 28 now. Its line numbers are dated 2026-07-31.
    - `docs/VM_RUNTIME.md` §6 still lists `value_buffer` and says "Phase 2A uses `Rc` — cycles may leak".
    - GC_STAGE5's status omits #353 (P1.2 partially addressed) and #423 (P2b done) in its header.
    - The stage-2/3 commits claim 14 tests per backend file; the files now hold 9 and 4, with the rest in `gc_shared_tests!`.

    **[V]**

---

## 11. What a new collector can reuse

- **`GcRoots` providers**, including the weak-table hooks. They are the complete, mechanically enumerable root inventory (`grep 'impl GcRoots'`).
- **`run_mark_phase`'s combined weak fixpoint**, which ephemerons and continuation tables need whatever algorithm runs.
- **The pending-flag trigger**: decision at allocation, one load at the safe point. This maps directly onto a JIT safepoint poll or on a "nursery exhausted" bump-limit check.
- **Per-PC `register_roots` bitsets.** Compiler-produced liveness at instruction granularity, `Vec<Vec<u64>>` per code object. A precursor to JIT stack maps; it would need compressing to call and poll sites.
- **`GcDeferGuard`/`is_outermost`.** A proven pattern for unrooted Rust windows. A moving or generational design must either keep "allocation never collects" or replace these windows with handle scopes.
- **Differential CI lanes and reclamation proofs**, the debug-poison lane, Larceny's ephemeron suite, and the `finished_forms_release_code` and `weak_continuation_tests` fixtures. Lane stress interval and `EXPECTED_TOTAL` pinning are established practice.
- **Interleaved A/B benchmarking methodology** (GC_STAGE5 preamble; `docs/VM_TESTING.md`).

---

## 12. Constraints and lessons for the redesign

1. **Keep "collection only at safe points, never inside allocation" as the default contract**, or budget for replacing hundreds of Rust sites that hold `TaggedValue`s across `alloc_*`. This one invariant made a precise GC possible without a shadow stack. With a JIT, a nursery-full slow path should request a collection at the next poll (and overflow into a slow-allocation space), not collect in place. Otherwise JIT frames need full stack maps at every allocation site. **[I]**
2. **Keep the VM register file as the canonical frame storage that JIT code reads and writes**, spilling at polls and calls. Root scanning stays a slice scan with the existing per-PC liveness, and continuation capture keeps working on interpreter-shaped data. A base-pointer reload is needed after any call that can grow or realloc the register `Vec`. Otherwise use a reserved, non-moving stack region. **[I]**
3. **Make pause proportional to live data:** lazy or bitmap-driven sweep, page or segment-based arenas that can be decommitted, and free regions instead of per-slot free lists. The current design cannot meet this however roots are tuned. **[V]/[I]**
4. **Allow GC during library loading and expansion**, because that is where allocation volume is (§5.5). This means rooting `ParsedLibrary.body`, the expander's partial IR and `saved_globals` explicitly instead of deferring. **[V]/[I]**
5. **Change the trigger to bytes**, with a heap-size target (for example ~2× live, OCaml-style `space_overhead`), and count large and malloc'd payloads. **[V]/[I]**
6. **Unify identity.** One object, one address or handle. Give `Procedure`, `RecordType` and `Record` a stable identity, and provide an identity hash that survives moving (a header hash, or rehash-on-GC tables). This is a prerequisite for a JIT-compiled single-compare `eq?` and for any moving or compacting generation. **[V]/[I]**
7. **Write-barrier inventory, if generational:**
   - Heap mutators: `set_car`/`set_cdr` (`heap/mod.rs:743-762`), `vector_set`/`vector_slice_mut` (`:804-818`), `write_mutable_cell` (`:1273`), `set_vm_closure_free_var` (`:1394`).
   - Payload mutations through `RefCell`: record field set (`primitives/records.rs:253`), parameter values (`parameters.rs:168,267`), promise forcing (`lazy.rs:134`, VM `control.rs`), and `break_ephemeron`.
   - **Writes to environment bindings**, which live outside the heap. A young generation would have to treat all mutable environment slots as roots, or environments must become heap objects behind barriers.

   Strings and bytevectors are leaves and need no barrier. **[V]**
8. **Shrink the object model before tuning the algorithm** [I]:
   - unboxed or 16 B flonums (the free `110` tag, or NaN-boxing);
   - inline closure free variables and record fields in one object with a header;
   - 16 B cells;
   - UTF-8/Latin-1 strings;
   - environments and continuations as heap objects. Their `Rc` graphs hold the cycle-breaking logic and the dedup hash sets that cost about 8 ns per live closure.
9. **Do not lose the semantics the current GC provides:** SRFI 124 ephemerons, weak continuation tables, prompt port flush on drop together with the exit flush, the `SourceMap`/`syntax_sources` pruning hooks, and the #338 closure → code-unit release accounting. Code liveness then needs a GC-visible edge, for example code objects as heap objects or a code pointer in the closure header.
10. **Rooted embedding handles** (a handle scope or persistent roots) are required for the public `Interpreter` API, and will be for JIT-to-runtime helper calls. **[V]**

---

## 13. Prior-art pointers for the comparison work
Local paths were checked to exist; their content was not analysed in this report.
- Chez: `~/Project/reference/ChezScheme/c/gc.c` (dirty-card sweeping, `sweep_dirty_*`, guardians, ephemerons), `c/segment.c`, `c/alloc.c`.
- OCaml: `~/Project/reference/ocaml/runtime/{minor_gc.c, major_gc.c, shared_heap.c}`.
- Racket CS: `~/Project/reference/racket/racket/src/ChezScheme`.
- Gambit: `~/Project/reference/gambit/lib/mem.c`.
