# Off-heap value holders, the Rc graph, and handle hazards

Scope: every place outside the four heap arenas that holds a `TaggedValue` or a bare `HeapIndex`, how the collector reaches it, whether it could become a GC-managed object, and a survey of Rust code that keeps heap references in locals or collections across allocation or re-entry. Repo state: `main` at `28a94f8`. All line numbers refer to that commit.

Evidence labels: **[V]** means I verified it by reading source or by running a measurement. **[I]** means inference. The measurement tools are retained under `PRD/study/gc/probes/offheap/`. The scanners are `PRD/study/gc/probes/offheap/tools/scan.py` and `scan2.py`, and the probe crate is `PRD/study/gc/probes/offheap` (a release build that depends on the repo crates by path and runs from the repo root without changing anything).

---

## 1. Summary

1. **Runtime state is split across two memory systems.** The four typed arenas hold Scheme data. A large `Rc` graph of Rust structs holds TaggedValues and points back into the heap. The arenas are `pairs`, `vectors`, `strings` and `objects` (`crates/patina-core/src/heap/mod.rs:303-315`). The Rust graph includes `Environment`, `Library`, `CompiledMacro`, `Procedure::CpsLambda`, `CpsContinuation`/`ContValue`/`ContEnv`, `CodeObject`, `VmContinuation`, record field vectors, parameter value vectors and promise cells. At least 19 of the 28 `HeapObjectData` variants own an `Rc`, `Vec`, `String` or `BigInt` payload that must be dropped [V, `heap/mod.rs:143-229`]. The collector reaches the Rust side only through per-type visitors (`visit_env`, `visit_continuation`, `visit_expr_literals`, `visit_library`) that deduplicate by pointer address (`heap/gc.rs:422-662`). All of them take `&self` and pass values by copy, so none of them can update a reference.
2. **Rc cycles through the heap mean an `Interpreter` leaks its whole heap on drop** [V, measured]. Each `Environment` holds `heap: SharedHeap` (`environment.rs:489`). Heap objects hold `Rc<Environment>` (`VmClosure.globals` at `heap/mod.rs:212`, `EnvironmentSpecifier.env` at `:182`, `CpsLambda.env` at `procedure.rs:67`) and `Rc<CompiledMacro>`, and `CompiledMacro` holds `heap: SharedHeap` (`compiled_macro.rs:483`). After dropping a VM or tree-walker interpreter that has loaded `(scheme base)`, a `Weak` to its heap still upgrades, with `strong_count == 37`. A bare `Environment::new()` frees correctly. Sweep tombstoning (GC_DESIGN §8) only breaks cycles that pass through *unreachable* slots. Nothing breaks the cycles rooted at teardown.
3. **Global variables are an off-heap structure on the VM's hottest path.** `LoadGlobal` (`vm_state.rs:1492-1506`) does the following on every read:
   - borrows the heap and clones an `Rc<Environment>` out of the current closure (`frame_globals`, `vm_state.rs:1357-1364`);
   - probes a per-site cache keyed by `env_id`;
   - borrows the bindings `RefCell`;
   - checks the `FORWARDED` marker and, for an import, follows an `Owner = (Rc<Environment>, u32)` link (`environment.rs:341`, `:605-667`).

   A JIT cannot inline this. Every imported binding is an `Owner` link, and the collector follows these links as extra cross-environment edges (`for_each_shared_owner`, `environment.rs:2211`).
4. **Collection never happens inside allocation, and the code depends on it.** `alloc_pair` documents that callers may hold partial structures across allocation (`heap/mod.rs:697-701`). So does `list_from_iter_with_tail` (`:2929-2931`). `note_alloc` only raises a flag (`:581-584`). Collection happens only at two driver-loop safe points: VM `maybe_collect` (`vm_state.rs:1287`) and tree-walker `maybe_collect` (`cps_eval/mod.rs:114`). Five `GcDeferGuard` sites suppress it during nested work.
5. **Size of the handle-hazard surface** (floor counts from a direct-call scan of 2,365 non-test functions):
   - 365 allocation call sites in 230 functions;
   - 19 functions keep a freshly allocated, otherwise unreachable value in a local across another allocation;
   - 51 functions use a TaggedValue that was obtained before an allocation after that allocation's statement;
   - 25 functions allocate in a loop;
   - 9 functions keep a Rust collection of TaggedValues across allocating loops;
   - 3 functions both allocate and re-enter evaluation.

   A name-based transitive closure gives an upper bound of about 889 functions (2,315 sites). The recursive syntax builders are invisible to the direct scan but are the most hazardous sites: the parser frame stack, macro template instantiation, `map_syntax_identifiers_memo` and quasiquote lowering. The Chez model fits this codebase: allocation requests a collection, and the collection happens at the next poll. Collection inside allocation would force handle discipline on all of these sites.
6. **Identity is not a machine word today.**
   - `eq?` on procedures, record types and records compares the `Rc` behind the heap slot (`values_eq`, `heap/mod.rs:2118-2146`).
   - `hash-by-identity` hashes the arena index (`heap/mod.rs:2541-2561`). Scheme-level SRFI 69 tables store that hash (`lib/srfi/69/srfi-69-impl.scm:118`).
   - Provenance (`Heap.syntax_sources`, `heap/mod.rs:305`) and `SourceMap.locations` (`source_map.rs:61`) are keyed by `raw_bits()`.

   Each of these breaks under a moving collector, and the first one blocks a one-instruction `eq?` in JIT code.
7. **The sweep is also the finalization and weak-table engine.** Sweep tombstoning:
   - drops `Rc` payloads, which flushes file output ports through `BufWriter`'s `Drop` (`port.rs:171-235`);
   - prunes `syntax_sources` (`gc.rs:891-900`);
   - reports freed raw bits for `SourceMap` pruning (`gc_freed_bits`, `heap/mod.rs:400-410`);
   - reports dead closures' code ids so the VM can evict code (`gc_freed_closure_code_ids`, `heap/mod.rs:412-418`, #338).

   A copying or evacuating nursery never visits dead objects. Every one of these side effects needs another mechanism, such as finalizer lists like OCaml's minor custom table, weak tables processed by the collector, or allocating Drop-carrying objects outside the nursery.

---

## 2. The ownership graph (as built)

```
 VmBackend / Evaluator ──Rc──► Environment (global) ──parent──► ...
        │                         │ heap: SharedHeap ───────────────┐
        │                         │ bindings: SmallVec<(Rc<str>,TV)>│ TV = bare index, no ownership
        │                         │ scoped_bindings / alias_bindings│
        │                         │ rare.links: Vec<Option<(Rc<Env>,slot)>>  (imports, #406)
        │                         │ rare.owners: Vec<Rc<Env>>       │
        ▼                         ▼                                 ▼
 LibraryRegistry ──► Library{exports: HashMap<String,TV>, env: Rc<Env>}     Heap (RefCell)
                                                                 objects[i] = HeapObjectData (72 B)
 VmState{globals: Rc<Env>, code_store: Vec<Rc<CodeObject{constants: Vec<TV>}>>,
         registers, frames{closure: Option<HeapIndex>}, continuation_store (weak, Rc<VmContinuation>)}
                                                                 ├─ VmClosure{free_vars: Vec<TV>, globals: Rc<Env>} ──► Env
                                                                 ├─ Procedure(Rc<CpsLambda{body: Rc<CpsExpr>, env: Rc<Env>}>)
                                                                 ├─ Macro(Rc<CompiledMacro{literals, definition_env, foreign_expansions, heap: SharedHeap}>)
                                                                 ├─ Continuation(Rc<CpsContinuation{env, ContEnv, winds...}>)
                                                                 ├─ Record{rtd: Rc, fields: Rc<RefCell<Vec<TV>>>}
                                                                 ├─ Parameter{values: Rc<RefCell<Vec<TV>>>}, Promise(Rc<RefCell<..>>)
                                                                 └─ Port(Rc<Port>)  ◄── thread_local CURRENT_*_PORT
```

The edges from environments to the heap are non-owning indices. The edges from the heap to environments are owning `Rc`s. This asymmetry is why the GC_DESIGN §8 argument works for unreachable cycles. It also explains the teardown leak in finding 2, because the roots themselves sit inside the cycle (`Environment` → `SharedHeap` → `objects` → `Rc<Environment>`).

---

## 3. Inventory of off-heap holders

Columns: what the holder contains, who owns it, how the GC reaches it today, and whether it could become a heap object.

### 3.1 Environments and namespaces

| Holder | Holds | Owner | GC reach today | Could be heap-managed? |
|---|---|---|---|---|
| `Environment.bindings` (`environment.rs:208-217`) | `SmallVec<[(Rc<str>, TV);3]>`, plus a hash index above 8 entries | `Rc<Environment>` held by backends, closures, libraries, the desugarer | `visit_env` → `for_each_local_value` (`environment.rs:2191`), deduped by struct address (`gc_identity`, `:2185`) | **Yes, as heap binding cells.** Top-level and library bindings become heap "variable" objects; the name table stays a compile-time Rust structure. |
| `scoped_bindings` (`:50-53`, values at `:14-23`) | TV per (name, ScopeSet) | same | `for_each_local_value` | Values yes (cells). The ScopeSet keys are expander metadata and can stay in Rust. |
| `alias_bindings` (`:90-94`, `:137`) | `Option<Rc<Environment>>` + name + scopes; no TV | same | `for_each_alias_target` (`:2226`) → `visit_env` as a separate root | Becomes "alias → cell" once bindings are cells. The Rc edge disappears. |
| `RareTables.links/owners` (`:384-434`) | `(Rc<Environment>, slot)` per imported slot; slot holds `TaggedValue::FORWARDED` | same | `for_each_shared_owner` (`:2211`) | **Deleted by cells.** An import installs the exporter's cell, giving R7RS §5.2 shared locations by construction (as in Chez library-globals and Racket CS `variable`, §8). |
| `RareTables.introduced_global_names`, `import_aliases` | names only | same | n/a | Stays in Rust (compile-time). |
| `Environment.parent` | `Rc<Environment>` | child | followed by `visit_env` | Tree-walker frames could become heap vectors (§3.4). |
| Tree-walker per-call frames | 5 `Environment`s (224 B each) per call [V, measured: 50,003 for `(f 10000)`, 50,003 for tail `(g 10000 0)`, 40,011 for a 10,000-step named let]. The VM mints 0 per call. | `StepResult`, `ContValue::Local.env`, `CpsLambda.env` | `trace_step` → `visit_env` | Yes (frame vectors) if the tree-walker is kept on the same heap. Otherwise these frames are an unbounded off-heap root set that a generational GC must rescan at every minor collection. |

Measured with the probe [V]. After VM bootstrap plus a small program, the global environment reaches 17 environments, 987 bound values and 30 owner edges, with 283 local names. After importing 14 libraries it reaches 25 environments, 2,427 values and 100 owner edges. A mark phase rooted only at the global environment takes 25–31 µs on the VM (393–520 objects marked) and 51 µs on the tree-walker. That is a fixed per-collection cost. Unless environment stores get a barrier, a generational design pays it at every minor GC.

### 3.2 Libraries and registry

| Holder | Holds | Owner | GC reach | Heap-manageable? |
|---|---|---|---|---|
| `Library.exports: HashMap<String, TV>` + `env` (`library.rs:22-35`) | TVs and `Rc<Environment>` | `LibraryRegistry.libraries: HashMap<Vec<String>, Library>` (`library_registry.rs:291-306`) | `impl GcRoots for LibraryRegistry` (`:569-575`) → `visit_library` (`gc.rs:629`) | Metadata stays in Rust. Exports become name → cell, so the separately stored export TV, a stale-able copy, goes away. |
| `Library` clones | `#[derive(Clone)]`; `load_library` returns `Library` by value (`vm/backend.rs:502`) | transient | not rooted (transient) | Becomes moot once exports are cells. |
| Registry borrow rule | `LibraryRegistry::try_roots` returns `Err` during a load, and the safe point then **skips collection** (`library_registry.rs:585-595`, `vm_state.rs:1300-1303`) | | | **[I]** Collection inside allocation would hit this during every library load. |
| `HeapObjectData::Library` | `Rc<Library>` | heap | `visit_library` | `alloc_library` has no non-test caller [V], so the variant looks dead. |

### 3.3 Macros and expander

| Holder | Holds | GC reach | Notes |
|---|---|---|---|
| `CompiledMacro` (`compiled_macro.rs:464-541`) | `Pattern::Literal`/`Template::Literal` TVs (51–67 literal TVs over 19–25 macros after bootstrap [V]), `definition_env`, `foreign_expansions: Vec<(ScopeId, Rc<Environment>)>`, `heap: SharedHeap` | `Macro` trace rule (`gc.rs:746-753`): literals plus `definition_env` | **Gap [V/I].** `foreign_expansions` envs are kept alive by `Rc` but their bindings are never traced. Low risk today: they are library envs, which the registry roots, and the probe found 0 entries. `heap` is a self-cycle. Could stay a Rust artifact, with its literals moved into one heap vector (one updatable edge) or allocated pinned or old. |
| Desugarer (`desugarer/mod.rs:330-381`) | `quoted: Rc<RefCell<HashMap<u64, TV>>>` (raw-bits keyed, cleared per form, `:1821`); `EarlyBinding.foreign` env map (`:280`); `declarations`; `open_forms` (raw bits, `walk.rs:32`) | not rooted; protected by `GcDeferGuard` in `desugar_with_imports` (`:1841`) | Comment: "No GC runs while desugaring" (`:370-371`). Moving or at-allocation GC would need the memo re-keyed. |
| `MatchEnv` (`pvref.rs:236-241`) | `MatchValue::Leaf(TV)`, pattern-variable bindings to input subforms | not rooted; deferral (GC_DESIGN §5.1) | Its subjects are themselves unrooted input syntax. |
| Macro and core-syntax bindings | `HeapObjectData::Macro`/`CoreSyntax` bound **in the runtime `Environment`** | through `visit_env` | **[V]** Runtime and syntactic environments are the same structure. Chez keeps them separate: compile-time ribs/labels versus runtime symbol value slots (§8). |

### 3.4 Tree-walker closures and continuations

| Holder | Holds | GC reach |
|---|---|---|
| `Procedure::CpsLambda` (`procedure.rs:57-64`), 120 B | `Rc<CpsExpr>` body (literal TVs), `Rc<Environment>` | `trace_object_children` → `visit_expr_literals` (deduped per node, `gc.rs:642`) + `visit_env` |
| `CpsContinuation` (`continuation.rs:22`), 208 B | body, env, wind/handler/prompt stacks, `ContEnv`, `resume` | `visit_continuation` + iterative worklist (`gc.rs:564`, `:796-820`) |
| `ContValue` (64 B) / `ContEnv` persistent list (`cont_value.rs:39`, `:221-396`) | TVs in 12 variants | `trace_cont_value`/`trace_cont_env` with `visit_once` dedup. GC_DESIGN §9.4 measured 6.8 s per collection before dedup. |
| `PENDING_ESCAPE` thread-local (`tree-walker cps_eval/types.rs:21`) | `(TV, Rc<CpsContinuation>)` | `EscapeRoots` provider (`gc_roots.rs:42`) |
| `StepResult` (trampoline local) | the whole machine state | `StepRoots` built per safe point (`gc_roots.rs:58-130`) |
| Suspended outer `StepResult`s in nested trampolines | everything | **not rooted**: deferral (`cps_eval/mod.rs:219`) |

All of these can stay Rust-side if the tree-walker is tolerated as non-moving-only. A moving collector would need `&mut` tracing into immutable `Rc<CpsExpr>` trees [I].

### 3.5 VM

| Holder | Holds | GC reach | Notes |
|---|---|---|---|
| `ExecutionState.registers` | `Vec<TV>` | `visit_slice` after `retire_registers` (`gc_roots.rs:71-75`) | Already a precise-ish stack. A JIT frame equivalent is a Cranelift user stack map (§8). |
| `CallFrame.closure: Option<HeapIndex>` (`types/mod.rs`), 40 B frame | **bare index** | `visit_object_index` (`gc_roots.rs:154-160`) | A moving collector must rewrite it. |
| `CodeObject.constants: Vec<TV>` inside `Rc<CodeObject>` (`code_object.rs`), 160 B | literal TVs | loop over `code_store` (`gc_roots.rs:91-93`) | Immutable inside `Rc`, so moving needs `Cell`/`UnsafeCell` or pinning. Instructions never embed heap refs, only fixnum immediates (`pass5_codegen.rs:436-440`). Keep this rule for JIT code: load constants from a GC-visible table, or emit relocation records. |
| `VmClosure{free_vars: Vec<TV>, globals: Rc<Environment>}` | malloc'd free-var buffer + Rc clone per closure (`vm_state.rs:1538-1550`) | trace rule `gc.rs:782-790` | Closure creation costs a malloc, an Rc increment and an arena push. A JIT wants an inline `[header, code, fv...]` object. |
| `continuation_store`/`delimited_continuation_store` (`vm_state.rs:196-199`) | `Rc<VmContinuation>` (152 B) with register snapshots | **weak-keyed** by the `VmContinuationRef(u64)` id fixpoint (`gc.rs:1022-1100`) | Natural heap objects in a JIT design (stack-segment copies). Today the weak-id protocol relies on "store touched only within one dispatch". |
| `scratch_args`, `pending_escape`, tracer `pre_regs` | TVs | rooted (`gc_roots.rs:76-88`, `:103-106`) | `scratch_args` is empty during a primitive (taken), so a primitive's arguments live only in a Rust `Vec` while it runs [V per comments]. |
| `GlobalCacheEntry{env_id, slot}` (`code_object.rs`) | env slot, not a heap ref | n/a | Obsolete once globals are cells. |
| Code eviction (#338) | `live_closures: Cell<u32>` counting, driven by sweep reporting freed closure code ids | sweep side effect | Needs a replacement under copying (§1 item 7). |

### 3.6 Rc payloads inside heap objects (records, parameters, promises, ports, rtds, identifiers)

- **Records**: `fields: Rc<RefCell<Vec<TV>>>` (`heap/mod.rs:163-166`). `record-set!` mutates through a clone of the `Rc` under a shared heap borrow (`records.rs:250-253`). Identity is the `Rc` (`values_eq`). These should be inline heap objects.
- **Parameters**: `values: Rc<RefCell<Vec<TV>>>`. `get_parameter` hands out the `Rc` clone (`heap/mod.rs:1820-1828`).
- **Promises**: `Rc<RefCell<PromiseState>>` (`:178`). Updated by `promise_update` (`:1078`).
- **MutableCell** and **Ephemeron**: `RefCell` inside the arena slot, written through `&Heap` (`write_mutable_cell(&self,…)`, `:1273`; `break_ephemeron(&self,…)`, `:1237`).
- **Ports**: `Rc<Port>`, a leaf with no TVs. They are shared with the `CURRENT_{INPUT,OUTPUT,ERROR}_PORT` thread-locals (`primitives/io/ports.rs:41-45`). `(current-output-port)` allocates a fresh heap wrapper on each call (`ports.rs:463`). Output flushing on drop relies on sweep dropping the last `Rc`, plus `OUTPUT_FILES` weak list at exit (`port.rs:171-235`, `flush_open_output_files` at `:211`). This is finalization by tombstone.
- **RecordTypeDescriptor** (48 B): `Rc`, a leaf. Ids come from a process-global counter (`record_type.rs`).
- **Identifier** (`name: Rc<str>, scopes: ScopeSet (40 B), written`): a leaf. It accounts for most of `HeapObjectData`'s 72-byte size. After VM bootstrap the arena holds **9,464 Identifier objects and 17,030 pairs** with 0 collections, and the symbol table holds 1 symbol [V]. Source syntax is mostly identifiers, not symbols.
- **Symbols**: `Symbol(Rc<str>)`. `symbol_table: HashMap<String, HeapIndex>` and `core_syntax_table` are immortal roots marked in `GcVisitor::new` (`gc.rs:449-465`). A moving GC needs these tables updated, as Chez's collector processes its oblist [I].

Drop-carrying variants (19/28, [V] by enum inspection): BigInt, Rational, Symbol, Bytevector, Exception, Procedure, Port, Macro, RecordType, Record, Identifier, Continuation, Parameter, Promise, Library, Values, EnvironmentSpecifier, PromptTag, VmClosure. The `vectors` and `strings` arenas also store a `Vec` per slot (24 B header, elements malloc'd elsewhere). Variants with no Drop: Real, Complex, LabelPlaceholder, MutableCell, Ephemeron, VmContinuationRef, VmDelimitedContinuationRef, CoreSyntax, Free. Flonums are 72-byte object slots, so every inexact arithmetic result allocates (`alloc_real`, `heap/mod.rs:896`).

### 3.7 Side tables keyed by raw bits (identity outside the object)

| Table | Lifetime | Maintained by |
|---|---|---|
| `Heap.syntax_sources: HashMap<u64, Rc<SyntaxSource>>` (`heap/mod.rs:305`, API in `heap/source.rs`) | long-lived | `sweep` retains entries whose key is marked and shrinks the table (`gc.rs:891-900`). Not a root. |
| `SourceMap.locations: HashMap<u64, SourceLocation>` (`source_map.rs:61`) | per program run | pruned at form boundaries from `take_gc_freed_bits`. Overflow clears it (`source_map.rs:234-260`, `interpreter lib.rs:510-512`). |
| `Desugarer.quoted`, `map_syntax_identifiers_memo` replacements/parents (`heap/source.rs:85-180`), `OpenNodes` (`walk.rs`), `eval.rs:32-40` active/complete sets, datum writer cycle sets, parser label maps, VM `prim_calls.by_value` (`primitive_calls.rs:216`) | transient (one call or form) | safe only because no collection runs inside them |
| `hash-by-identity` value stored in Scheme hash-table buckets (`equality.rs:76` → `tagged_value_hash_identity`) | **unbounded, in user data** | no maintenance. Correct only because indices never move. |

### 3.8 Thread-locals and statics

TV-bearing: only `PENDING_ESCAPE` (rooted). Rc-bearing with no TVs: `CURRENT_*_PORT`, `STDIN_*`, `OUTPUT_FILES` (weak). The rest hold no heap references: `ACTIVE_TRAMPOLINES` (ids), `SCOPE_ORIGINS` (unused in production: `fresh_with_origin` has no non-test caller), `MacroTracer`, `diagnostic::OUTPUT`, `EMPTY_REENTRY`/`EMPTY_HANDLERS`. Process-global counters (scope ids, env ids `environment.rs:442`, record type ids, prompt/wind ids, code labels) contain no heap refs. They are process-wide while heaps are per-interpreter, which is harmless.

### 3.9 Embedding API and errors

- `Interpreter::eval_*` returns a raw `TaggedValue` and offers no root or handle type [V: no `Rooted`/`Handle`/`protect` in the tree]. A value an embedder keeps across a later `eval` can be swept. `run_forms` (`interpreter lib.rs:483-528`) itself keeps `value` from form *k* while forms *k+1..n* run and collect. `eval_program_resilient` is documented to return "the last successfully evaluated result" (`:292-299`), which may have been swept if later forms failed after collecting. **[I] latent use-after-free, low impact.**
- `EvalError` serializes irritants to strings (`irritants_display`, `eval_error.rs:83-88`), so errors do not carry TVs [V]. `ErrorDetail`/`ExceptionObject.irritants: Vec<TV>` (`error.rs:50`, `:238`) are carried briefly and covered by "no GC during unwinding" (GC_DESIGN §5.3).
- Separate heaps: `Environment::new()` and `Library::new()` mint a **fresh heap** (`environment.rs:551-553`, `library.rs`). Only tests and `Desugarer::new()` use them. `with_globals` asserts heap identity in debug builds only (`vm_state.rs:342`). TVs from two heaps can be mixed silently [I].

---

## 4. Mutation channels the collector cannot see centrally (barrier inventory)

Relevant to any generational or incremental design. Approximate non-test call sites by grep:

- `&mut Heap` stores: `set_car` (10 sites), `set_cdr` (13), `vector_set` (11), `vector_slice_mut` (1), `promise_update` (3), `set_vm_closure_free_var` (1).
- `&Heap` stores through `RefCell` inside a slot: `write_mutable_cell`, `break_ephemeron`.
- Stores through cloned `Rc` handles with no heap borrow at all: record fields (`records.rs:253`), parameter values (`parameters.rs`, 3 `values.borrow_mut()` sites), promise state.
- **Environment stores** (off-heap): `define` (~72 call sites), `set`/`set_slot_value`/`set_with_scopes`/`define_with_scopes`. These include the VM's `Define`/`StoreGlobal` (`vm_state.rs:1508-1530`) and every tree-walker binding.

GC_STAGE5_PRD's planned barrier list covers set-car!/set-cdr!/vector-set!/MutableCell only (`PRD/ARCHIVE/GC_STAGE5_PRD.md`, "Non-moving generational"). It misses the record, parameter, promise and environment channels. **[I]** Either environment storage moves into heap cells, which get one barrier, or every environment is a minor-GC root (cost in §3.1).

---

## 5. Handle hazards

### 5.1 The current contract

- `Heap::alloc_*` never collects. `note_alloc` raises `gc_pending` past the threshold (`heap/mod.rs:581`).
- The only collection points are the two `maybe_collect` safe points at driver-loop tops. They are gated by `gc_pending` and by `GcDeferGuard::is_outermost`.
- Defer guards: VM `run_loop_until` (`vm_state.rs:1151`), `with_globals` (`:343`), tree-walker `run_trampoline` (`cps_eval/mod.rs:219`), `ParsedLibrary` for its whole lifetime (`library_loader.rs:208`), and `desugar_with_imports` (`desugarer/mod.rs:1841`).
- Consequence [V]: bootstrap and library loading never collect. After VM bootstrap, 17,030 pairs and 9,464 identifiers sit in the arenas with 0 collections. The first collection after loading 14 libraries swept 56,717 of 57,242 objects.

Under this contract every pattern below is **safe today**. Each one becomes unsafe if collection can occur inside allocation (pattern A, and C when the held values are fresh) or if objects move (all patterns).

### 5.2 Measured surface

`scan2.py` counts direct calls to Heap allocation methods (`alloc_*`, `intern_symbol`, `list_from_iter*`, `numeric_*` and similar). It scans non-test sources and excludes `patina-tests` and `patina-compat`. The numbers are floors.

| crate | fns | allocating fns | sites | A fresh-unrooted | B moving | alloc in loop | C collection held | D alloc+re-entry |
|---|---|---|---|---|---|---|---|---|
| patina-core | 720 | 51 | 113 | 6 | 18 | 2 | 1 | 0 |
| patina-frontend | 285 | 17 | 39 | 3 | 3 | 4 | 1 | 0 |
| patina-macros | 156 | 9 | 11 | 0 | 2 | 1 | 1 | 0 |
| patina-primitives | 448 | 129 | 166 | 10 | 21 | 16 | 5 | 3 |
| patina-runtime | 116 | 1 | 1 | 0 | 0 | 0 | 0 | 0 |
| patina-tree-walker | 132 | 9 | 12 | 0 | 3 | 0 | 0 | 0 |
| patina-vm | 346 | 14 | 23 | 0 | 4 | 2 | 1 | 0 |
| **total** | **2,365** | **230** | **365** | **19** | **51** | **25** | **9** | **3** |

Pattern definitions:
- **A**: `let x = <alloc>` and x is used after a later allocation.
- **B**: a TV obtained before an allocation statement (an argument, a TV parameter, or a local initialized from a heap read) is used after it.
- **C**: a Rust `Vec`/`HashMap`/raw-bits collection is live while allocation happens in a loop.
- **D**: the function allocates and re-enters evaluation.

The first `scan.py` pass closes over allocating functions by name. It flags 889 functions and 2,315 sites, an over-approximation because names like `read` collide. Spot checks of the strict B list confirmed real cases:

- `floor_div` (`arithmetic/division.rs:63-80`): `q` (possibly a fresh bignum) is held across the allocation of `r`, then `alloc_values(vec![q, r])`. This is A and B.
- `get_environment_variables` (`process_context.rs:150-171`): `key` is held across `alloc_string(v)`, then a `Vec` of fresh pairs is held across the `list_from_iter` allocation. This is A and C.
- `numeric_*` folds (`arithmetic/basic.rs:21-35`): the accumulator is fresh and passed as an argument to the next allocation. That is safe only if the allocator roots its own arguments.

### 5.3 By subsystem (examples the regex scan cannot see)

- **Reader/parser.** `read_with_frames` (`parser/datum.rs:124-460`) keeps an explicit `frames: Vec<Frame>` whose `elements: Vec<TV>` hold completed children while siblings allocate. List closing conses back to front over that `Vec` (`:240-262`). Datum labels keep TVs in `self.labels` across the whole parse (`:331-448`). The Scheme `read` primitive runs this inside a primitive.
- **Macro expander.** Template instantiation (`expander/list.rs:106-150`, `expand_element_into`) pushes each expanded subtemplate, a fresh allocation, into a `Vec<TV>` while expanding the next one recursively. It ends in `alloc_vector`/`list_from_iter_with_tail`. `MatchEnv` holds input subforms. `mark_substituted_in` (`hygiene.rs:87-155`) builds lists from raw-bits-keyed memo tables.
- **Desugarer.** `map_syntax_identifiers_memo` (`heap/source.rs:85-180`) holds `parents: HashMap<u64, Vec<TV>>`, `replacements: HashMap<u64, TV>`, `pending`, `changed` and `containers` across `alloc_pair`/`alloc_vector`. These are raw-bits keyed, so the code breaks under a move even without collection inside allocation. The per-form `quoted` memo (`desugarer/mod.rs:372`) and quasiquote lowering (`quasiquote.rs:128`) follow the same pattern. `desugar_with_imports` runs library loads, which means Scheme evaluation, in the middle of a form whose partial IR is on the Rust stack. It is protected only by the guard at `:1841`.
- **Library loader.** `ParsedLibrary.body: Vec<TV>` holds unevaluated forms across evaluation of earlier forms (`library_loader.rs:160-215`), protected by a lifetime-long defer guard.
- **VM runtime.** The interpreter loop is mostly register-to-register. Allocation sites read operands from registers and write results to registers (`Cons`, `vm_state.rs:2108-2117`; `MakeClosure` collects free vars into a `Vec` first, `:1538-1550`). Continuation capture builds a `VmContinuation` snapshot (a `Vec<TV>` copy of registers) and then allocates its ref (`vm_state.rs:636-643`). The snapshot is invisible to the collector until it is inserted into the store. `store_args_in_window` and `handle_control_primitive` (`control.rs:463`, `:530`) build rest lists from argument slices. Primitive arguments live in a taken `scratch_args` `Vec` during the call.
- **Tree-walker.** `apply_cps_step`, `invoke_continuation_step` and `apply_abort_current_continuation` hold step components across allocation. Nested trampolines (higher-order primitives, `eval`) hold whole suspended `StepResult`s.
- **Remaining re-entrant Rust callbacks.** `run_synchronously` in `registry.rs:100-120` drives `apply_proc`/`eval_expr` from Rust while holding `state`. Its callers are `call-with-values` and `force` (both intercepted by the VM), `%parameterize-swap!`, `call-with-port`/`call-with-*-file`, and `member`/`assoc` with a comparator (legacy Rust versions; AGENTS.md says the Scheme versions own these names).

### 5.4 Structural obstacle to "GC inside allocation" [I, from code shape]

Allocation happens under `heap.borrow_mut()`, often held across a whole loop (`let mut heap_mut = heap.borrow_mut()` in arithmetic folds and list builders). A collection triggered from inside `Heap::alloc_*` would run with `&mut Heap` but no access to the roots. Roots live in `VmState` (often `&mut`-borrowed by the caller), in `LibraryRegistry` (mutably borrowed during loads, so `try_roots` fails), and in `RefCell`ed environments, which may be borrowed by the caller. The existing safe points work because they are outside all of these borrows. Collection at allocation needs both a root-callback registry and the end of long-lived borrows, in addition to handles at every site above.

---

## 6. Prior art (verified sources)

- **Chez Scheme: allocation never collects.** `maybe_queue_fire_collector` sets `queued_fire` when generation-0 bytes cross `collect_trip_bytes`. `S_fire_collector` sets `$collect-request-pending` and `SOMETHINGPENDING` in each thread context (`ChezScheme/c/alloc.c:207-220`, `c/schsig.c:574-592`). The collection runs later at a Scheme event check. Chez's copying generational collector can therefore move objects without C code rooting locals across allocation. Only callbacks into Scheme and C globals (`S_protect`, `c/alloc.c:52-79`) need care.
- **Chez eq-hashtables are GC-aware.** The collector rehashes moved keys after collection (`tlcs_to_rehash`, `ChezScheme/c/gc.c:1734-1760`). This is the precedent for address-keyed side tables under a moving collector.
- **Chez library globals are heap locations.** Library variables are `library-global (uid . sym)` bindings resolved by `build-global-reference` to a symbol's value slot (`ChezScheme/s/syntax.ss:930`, `:3553-3566`). Importers reference the same location object, and the expander's environment is separate from runtime storage.
- **Racket CS variables.** "A potentially mutable import or definition is accessed through the indirection of a `variable`" (`racket/src/cs/linklet.sls:985-1001`). A `variable` is a record with `val`, `name` and `constance`. This is the cell model for imports.
- **Racket BC precise GC for C.** The xform tool rewrites C to register locals on `GC_variable_stack` (`racket/src/bc/gc2/xform-mod.rkt`, `gc2.h:342`). It shows the cost of retrofitting handles into an existing C runtime.
- **OCaml.** C stubs must use `CAMLparam`/`CAMLlocal`/`CAMLreturn` because allocation can trigger a moving minor GC (`ocaml/runtime/caml/memory.h:296`, `:407`, `:448-454`). Minor-heap custom blocks with finalizers are tracked in a per-domain `custom` table and finalized after minor GC (`runtime/minor_gc.c:786-795`, `:917-918`). This is the precedent for Drop-carrying objects in a copying nursery.
- **HotSpot.** "In order to preserve oops during garbage collection, they should be allocated and passed around via Handles within the VM", with `HandleMark` scopes (`openjdk/jdk src/hotspot/share/runtime/handles.hpp`).
- **SpiderMonkey.** A rooting hazard is "an unrooted variable holding a GC pointer live across a call that can GC". Its absence is enforced by static analysis with the sixgill GCC plugin (https://firefox-source-docs.mozilla.org/js/HazardAnalysis/index.html).
- **Wasmtime.** Host-held GC refs are `Rooted<T>`, indices into a root set valid in LIFO `RootScope`s, or `OwnedRooted<T>` for non-LIFO lifetimes. They are indices rather than pointers so they survive moves (https://docs.rs/wasmtime/latest/wasmtime/struct.Rooted.html). Cranelift "user stack maps" (2024-09-10) let the frontend mark values that need stack-map entries. Cranelift then spills them around safepoints and reloads them afterward, so objects may move (https://bytecodealliance.org/articles/new-stack-maps-for-wasmtime).
- **gc-arena.** "Mutation xor collection": `Gc` pointers are branded with a `'gc` lifetime and collection happens only between `Arena::mutate` calls, so holding them in Rust locals is safe (https://github.com/kyren/gc-arena). This is Patina's current model, made static.
- **Whippet.** A single embedder API with precise or conservative roots per collector. `mmc` supports both, with per-object pinning (https://github.com/wingo/whippet). Conservative stack scanning plus pinning is the escape hatch when handle discipline is too expensive.

---

## 7. Constraints and recommendations for the redesign

1. **Keep "allocation never collects; collection at polls" (Chez model).** Under this rule the patterns in §5 stay legal for both moving and non-moving collectors, provided Rust frames hold no live TVs at a poll. JIT code gets an inline bump fast path with a slow path that refills or grows and only *requests* a collection. Polls go at function entries and loop back-edges. Stack maps are needed only at polls and at calls that can reach a poll. Do not adopt allocation-triggered collection unless prepared to retrofit handles into about 230 to 890 functions and to break up the long-lived `RefCell` borrows (§5.4).
2. **Make the re-entrancy boundary the only place Rust needs roots.** Replace `GcDeferGuard` deferral with explicit, cheap LIFO root scopes, in the style of Wasmtime `RootScope` and HotSpot `HandleMark`. Use them for the few places Rust holds values across Scheme execution: `run_synchronously`, nested trampolines, `ParsedLibrary.body`, `desugar_with_imports`, and continuation-capture snapshots. This also resolves GC_STAGE5 Priority 2 (nested-loop collection) and lets library loads collect.
3. **Move global and library storage into heap cells.** One cell object per top-level binding. An import installs the exporter's cell, which preserves #406 semantics and removes `Owner`, `FORWARDED`, `links`/`owners` and the `env_id` global cache. `LoadGlobal` becomes "constant-pool cell → load". Keep name, scope and alias tables as compile-time Rust metadata whose payloads are cell or macro handles. Separate the syntactic environment from runtime storage as Chez and Racket do.
4. **Make identity a word.** Canonicalize so one heap object is one Scheme object: records inline, procedures as heap closures, one wrapper per port. Then `eq?` is `==`. Replace index-based `hash-by-identity` with a header hash field assigned lazily (survives moves; Java-style), or with GC-aware eq tables rehashed by the collector (Chez tlc).
5. **Move provenance off raw bits.** Store source info in syntax objects (Identifiers already exist, and pairs could get a wrapper), or make `syntax_sources` a collector-maintained weak table keyed by object identity. Do the same for `SourceMap`.
6. **Plan finalization explicitly.** 19 object variants and two arenas own Rust `Drop` payloads, and ports rely on `Drop` to flush. Give these objects either a no-nursery allocation path, OCaml-style finalizer tables for young objects, or Chez-style guardians. Replace `gc_freed_bits` and closure-code-id reporting, which only a sweep can produce. Code objects as heap objects would make code eviction ordinary reachability.
7. **Change the tracing interface to `&mut`/in-place.** `GcVisitor::visit(TaggedValue)` and `trace_roots(&self)` cannot relocate. Moving any object requires visitors over `&mut TaggedValue`, or `Cell<TaggedValue>`, in every off-heap holder. Immutable holders are `Rc<CodeObject>.constants`, `Rc<CpsExpr>` literals, `Rc<CompiledMacro>` literals and `Library.exports`. Each one must become updatable, be pinned or old-and-immortal, or be reached through one heap table.
8. **Break teardown cycles.** Remove `heap: SharedHeap` from `Environment` and `CompiledMacro` (pass the heap explicitly), or give `Interpreter` an explicit teardown that clears arenas. Today every dropped interpreter leaks its heap, which matters for embedding and for test binaries that create many interpreters.
9. **The tree-walker is the hardest client.** It creates 5 off-heap environments per call and Rc continuation chains. Either move its frames into heap vectors, or confine it to a non-moving mode with environment roots scanned at every minor GC. Decide this early, because both backends share one `SharedHeap`.
10. **Give the embedding API a handle type** (`Rooted`/`Persistent`) for values kept across `eval` calls. Fix `run_forms`/`eval_program_resilient`, which return a possibly swept value.

---

## 8. Open questions

- Is the tree-walker required to run on a moving or generational heap, or may it pin everything and pay full-root minor GCs? The cost per minor GC is about 25–50 µs from the global env alone, plus all live frames (§3.1).
- Bootstrap, library loading and `desugar_with_imports` never collect today. Is heap growth during a large library load (57k objects for 14 libraries) acceptable under the poll-only model, or must loads collect, which requires item 2?
- Do JIT frames call Rust primitives that may in turn reach a poll? Only callback-style primitives such as `run_synchronously` can. If they are all made resumable, JIT→Rust calls need no stack maps and only JIT polls do.
- Which identity-hash design: a header hash slot (needs a header word on every object) or GC-cooperative eq tables (requires native hash tables instead of the Scheme SRFI 69 implementation)?
- Can `foreign_expansions` envs (`compiled_macro.rs:540`) ever be non-library environments, such as an `eval` environment? If so, the trace rule at `gc.rs:746-753` must visit them.
- Should Identifiers keep a full `ScopeSet` inline (40 B), which sets `HeapObjectData` at 72 B? Or should scope sets be interned (an id or a heap object) so that syntax objects become small fixed-layout heap objects?
- How should code-object lifetime (#338) be expressed once closures reference code directly: as heap code objects, as in Chez and HotSpot nmethods, or as Rust-owned code with weak references from closures?
