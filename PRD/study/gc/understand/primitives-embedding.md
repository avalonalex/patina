# Primitives, front end, macros and embedding: how the rest of Patina touches the heap

Scope: `patina-primitives`, `patina-runtime` (stdlib internals, library loading), `patina-frontend`, `patina-macros`, `patina-interpreter`, `patina-repl`, `patina-compat`, and `PRD/FFI_DESIGN.md`. Repo state: `main` at `28a94f8` (after #601 and #602). The VM, tree-walker and core heap internals are covered by other reports; numbers for them appear here only for comparison.

Labels: **[V]** is verified in source or by running code. **[I]** is inference or a recommendation.

---

## 0. Summary

1. **[V] The whole system outside the collector runs under one implicit contract: allocation never collects, and collection happens only at the top of a backend's driver loop, outside any deferral scope.** (`docs/GC_DESIGN.md` §7; `Heap::note_alloc` only raises a flag; `(gc)` only requests one, `crates/patina-primitives/src/primitives/gc.rs:1-6,37-40`.) That contract is why about 1,300 non-test heap call sites outside core never root anything, and why primitives can hold `TaggedValue`s in Rust locals, `Vec`s and raw-bit hash sets. The code relies on it in writing. For example, `Desugarer::quoted` says "No GC runs while desugaring; clear on exit so raw indices never outlive a form" (`crates/patina-frontend/src/desugarer/mod.rs:370-372`).
2. **[V] Chez Scheme follows the same contract while running a moving, generational collector.** Its allocation slow path (`S_get_more_room` → `find_room`) gets more space and only *requests* a collection (`S_maybe_fire_collector` → `S_fire_collector` sets `$collect-request-pending`). The collection then runs at the next event check (`~/Project/reference/ChezScheme/c/alloc.c:213-220,500-555`, `c/schsig.c:574-596`). So Patina can keep its migration cost low and still get a moving collector. Moving does not force OCaml-style `CAMLparam` rooting into roughly 300 primitives.
3. **[V] The public embedding API has no rooting at all, and it is a live use-after-free.** A probe (`PRD/study/gc/probes/primitives-embedding`, §7.2) holds the result of `VmInterpreter::eval_str` across a later `eval_program` that collects. A debug build panics with `use-after-free: pair slot 16002 was reclaimed by the GC`. A release build silently prints `(45294)` instead of `(host held value #(1 2 3) "str")`. Dropping a `VmInterpreter` also leaks its heap, because of an `Rc` cycle between heap objects, `Environment.heap` and `CompiledMacro.heap` (probe: 37 strong references remain).
4. **[V] Primitives are already insulated from the *index* encoding.** Outside core, `heap_index()` is used only by the VM (6 sites), and primitives use `.raw()` only for transient identity sets (9 sites). A change to pointer-based `TaggedValue` costs primitives almost nothing. What *does* couple them to the representation: `Vec<char>` strings, `num-bigint` payloads, `Rc` payload objects (records, parameters, promises, ports, procedures), `eq?`/identity-hash defined via heap index or `Rc` address, and one `HeapObjectData` match in the datum writer.
5. **[V] The re-entry surface is small, and the resumable `Step` protocol already shows the right pattern.** Of 304 primitive registrations, 280 are heap-only, 19 higher-order and 5 resumable. Most of the 19 higher-order ones need only `fs`. Live Rust→Scheme re-entries are `load_scheme_library` (`environment`, `null-environment`, `scheme-report-environment`) and `%parameterize-swap!`'s `apply_proc` on parameter-like procedures. The other `apply_proc` users are shadowed fallbacks.
6. **[I] Recommended API: keep `TaggedValue` as an opaque `Copy` word.** Keep the accessor *names* on a context type. Change only how the context is acquired: `&SharedHeap` + `RefCell` becomes `&mut Cx`. Add a LIFO root stack plus a handle table, used *only* at re-entry boundaries and by hosts. Do not adopt "allocation may GC". Do not brand values with lifetimes inside the runtime.

---

## 1. Method and caveats

All counts come from grep or small Python scanners over `crates/*/src`. Code after the first `#[cfg(test)]` in a file is excluded unless stated. Scripts are in `PRD/study/gc/probes/primitives-embedding/tools/` (`count_heap.py`, `fn_classify.py`, `tv_structs.py`, `handlers.py`).

- "Heap method calls" counts `.name(` for all 239 public `Heap` methods (from `heap/mod.rs`, `numeric.rs`, `source.rs`). Generic names (`new`, `stats`, `features`, …) are excluded, which slightly undercounts. Calls made through a named guard (`let h = heap.borrow(); h.car(x)`) are counted. Calls through helper functions are counted once, at the helper.
- "Borrow sites" counts lines that contain both `heap` and `.borrow()` or `.borrow_mut()`.
- These are estimates to roughly ±10%, which is enough for sizing.

Measured with `std::mem::size_of` (`PRD/study/gc/probes/primitives-embedding/src/main.rs`): `TaggedValue` = 8 bytes, `HeapObjectData` = **72** bytes (every object-arena slot, including each boxed flonum from `alloc_real`), `Heap` = 528 bytes.

---

## 2. The contract everything outside core relies on

**[V]**

- Safe points exist only at the top of `run_loop_until_outcome` (VM, `crates/patina-vm/src/runtime/vm_state.rs:1143-1200`) and the tree-walker trampoline (`crates/patina-tree-walker/src/eval/cps_eval/mod.rs:219`). They collect only when `GcDeferGuard::is_outermost()` (`crates/patina-core/src/heap/gc.rs:235-268`).
- Rust frames that keep values across an evaluation call do not root them. They *defer*:
  - every nested dispatch loop;
  - `VmState::with_globals` (`vm_state.rs:337-347`);
  - `ParsedLibrary` holds a guard for its whole lifetime (`crates/patina-runtime/src/library_loader.rs:173-215`);
  - `Desugarer::desugar_with_imports`, because imports can run Scheme (`crates/patina-frontend/src/desugarer/mod.rs:1831-1841`).
- Consequence: a heap-only primitive, the reader, the desugarer and the macro expander are **GC-atomic**. Nothing can move or free a value they hold until they return. The ~1,300 non-test heap call sites below rely on this; none of them roots anything.
- Known cost, already accepted in `GC_DESIGN.md` §7: nested execution (library bodies, callbacks) never collects. A library body is evaluated with collection off for its whole duration.

The redesign keeps this contract or replaces it. The migration estimate (§9) differs by an order of magnitude between the two.

---

## 3. `patina-primitives` (47 files, 15,387 lines)

### 3.1 Shape

**[V]**

- `crates/patina-primitives/src/registry.rs:14` `TaggedHandler = fn(&SharedHeap, &[TaggedValue]) -> Result<TaggedValue, EvalError>`. The doc comment says heap handlers never re-enter, so args may point into the VM register file.
- `:20` `HOTaggedHandler = fn(&dyn ApplyContext, Vec<TaggedValue>)`.
- `:33-56` `Step::{Done, Call{callee,args,state}, Eval{expr,env,state}}`, plus `ResumableStart` and `ResumableResume`.
- `:99-121` `run_synchronously` is the Rust-loop fallback for callers with no machine.

Registrations: **280 `new_heap` (22 files), 19 `new_higher_order` (6 files), 5 `new_resumable` (2 files)**, 304 in total.

`ApplyContext` (`apply_context.rs:16-48`) provides `heap()`, `fs()`, `apply_proc`, `eval_expr`, `load_scheme_library` and `interaction_environment`. The VM implements it with a raw `*mut VmState` (`crates/patina-vm/src/runtime/control.rs:2123-2132`). Every re-entry goes through `across_reentry`.

### 3.2 Volume

**[V]**

| Metric (non-test src) | Count |
|---|---|
| Files mentioning `SharedHeap` | 37 |
| `SharedHeap` mentions / `&SharedHeap` params | 321 / 283 |
| Heap borrow sites | 406 (258 `borrow()`, 148 `borrow_mut()`) |
| Heap method calls | 466 in 39 files: 124 alloc, 22 mutate, 54 return interior references, 266 plain reads |
| Functions touching the heap | 291 |
| …with both `borrow()` and `borrow_mut()` (borrow-then-allocate) | 78 |
| …that read then allocate | 65 |
| Explicit `drop(guard)` before re-borrowing | 43 |
| Named guards (`let h = heap.borrow();`) | 186 |
| Functions that call back into the machine | 12 |

Most-used calls: `type_name` 28, `try_pair` 21, `alloc_string` 17, `get_string_contents` 14, `alloc_port` 13, `get_bigint` 11, `car`/`cdr` 11 each, `alloc_string_chars` 11, `list_from_iter` 10, `alloc_vector` 10. By heap-mention density the heaviest files are `strings.rs` (150), `lists.rs` (124), `characters.rs` (102), `vectors.rs` (95), `bytevectors.rs` (85), `datum_writer.rs` (82), `ports.rs` (79) and `records.rs` (73).

Of the 221 heap handlers the scanner could resolve, about half (111) may allocate, transitively, counting numeric ops that can box a flonum or bignum. The other ~110 only read. **[I]** Under the contract in §2 this split does not matter for GC safety.

### 3.3 Access patterns, and what each needs under a new design

**(a) Borrow-then-allocate.** **[V]** Example: `record_type_of` borrows, clones the `Rc<RecordTypeDescriptor>` out, calls `drop(heap_ref)`, then `borrow_mut().alloc_record_type` (`records.rs:110-123`). The same dance appears 78 times, with 43 explicit drops. It is the runtime-checked form of what `&mut` gives at compile time. AGENTS.md has a standing "RefCell borrow discipline" rule because of it. **[I]** With a `&mut Cx` receiver the dance disappears. Holding an interior `&[char]` or `&BigInt` across an allocation becomes a borrowck error, not a `BorrowMutError` panic. Mechanical.

**(b) Values held across allocations.** **[V]** These are everywhere and harmless only because of §2:
- arithmetic folds keep a possibly boxed accumulator across `numeric_add` (`arithmetic/basic.rs:21-34`);
- list builders collect a `Vec<TaggedValue>` and then call `list_from_iter`;
- `make_list` threads `result` through repeated `alloc_pair` (`lists.rs:602-640`);
- `load` parses a whole file into `Vec<TaggedValue>` before building its state vector (`eval.rs:681-701`).

**[I]** Keep "allocation never collects" and these need nothing, even with a moving nursery. Drop it (OCaml/SpiderMonkey style) and every one of the 65+ read-then-allocate functions, every accumulator loop, and the reader's `Frame { elements: Vec<TaggedValue> }` stacks (`parser/datum.rs:54-63`) needs rooted handles.

**(c) Interior references into heap storage.** **[V]**

| Accessor | Uses |
|---|---|
| `vector_slice` | 9 |
| `get_string_chars` | 5 |
| `get_string_chars_mut` (returns `&mut Vec<char>`, `heap/mod.rs:858`) | 2, both use only slice operations (`strings.rs:677,761`) |
| `get_bigint` / `get_rational` | 11 / 7 |
| `get_exception` | 8 |
| `get_symbol_name` / `get_symbol_or_identifier_name` | 4 / 4 |

**[I]** These are the calls that change if strings stop being `Vec<char>`, or bignums stop being boxed `num_bigint::BigInt`. Change `get_string_chars_mut` to return `&mut [char]` now; nobody needs `Vec` semantics.

**(d) `Rc` payloads cloned out of the heap and mutated outside it.** **[V]**
- `get_parameter` returns an owned `(Rc<RefCell<Vec<TaggedValue>>>, Option<TaggedValue>)`. `install` writes through it with no heap borrow (`parameters.rs:158-171,267`).
- Record fields are `Rc<RefCell<Vec<TaggedValue>>>`, written in place at `records.rs:262`.
- `force` writes `*cell.borrow_mut() = PromiseState::Forced(result)` (`lazy.rs:134`).
- Ports: `get_port(..).cloned()`, and current ports are thread-local `Rc<Port>`s (`io/ports.rs:41-45`). Each `(current-output-port)` allocates a fresh `Port` heap wrapper (13 `alloc_port` sites).

**[I]** These are **unbarriered stores of `TaggedValue`s into GC-traced memory**. A generational or incremental collector would miss them. They must either go through a barrier-aware API (`cx.record_set(rec, i, v)`) or become plain heap object fields. A moving collector also needs these slots to be *writable* by the tracer. Today the tracer sees them through `&` references and `RefCell::borrow()` (`heap/gc.rs:755-767`).

**(e) Identity defined by `Rc` address or heap index.** **[V]**
- `Heap::values_eq` (`heap/mod.rs:2118-2145`) is *not* a word compare for objects. It calls `get_object` and compares `Rc::ptr_eq` for `Procedure`, `RecordType` and `Record`, because primitives allocate several heap wrappers for one Rust entity (`%record-type-of` allocates a new `RecordType` wrapper on every call, `records.rs:123`).
- `tagged_value_hash_identity` (`heap/mod.rs:2541-2563`), exposed as `identity-hash` (`equality.rs:76`), hashes the heap index or the `Rc` address. It says the index is sound "because the collector does not move objects".
- SRFI 69, SRFI 125 and R6RS hashtables are implemented in Scheme and store these hashes in Scheme vectors (`lib/srfi/69/srfi-69-impl.scm:118`, `lib/srfi/125/hash.scm:5`). **A moving collector silently breaks every `eq?` hash table** unless identity hashes are stable: stored in a header and preserved on move, as HotSpot does with header hashes, or rehashed after GC, as Chez does (§8).
- **[I]** For a JIT, `eq?` must be one compare. That means canonical heap identity: one heap object per record type, procedure and port. About 9 wrapper-allocating sites need it (`alloc_record_type` ×2, `alloc_port` ×13, `alloc_procedure` sites).

**(f) Transient raw-bit identity sets.**
- **[V]** `check_import_datum` (`eval.rs:26-49`) and the datum writer's cycle labels (`io/datum_writer.rs:552,574,603,776,798`) key `HashSet`/`HashMap` by `.raw()` for the length of one primitive call.
- **[I]** These are safe under a moving collector *as long as no GC can run during the call*. They also stay correct with pointer encoding.

**(g) Layout coupling.** **[V]** Outside core, primitives call `get_object` once: the datum writer matches about 27 `HeapObjectData` variants (`io/datum_writer.rs:331-360`). 29 `HeapObjectData::` mentions in total, all in that function or in doc comments. **[I]** Replace it with a `cx.object_kind(v)` view enum or a `write_object` hook in core.

**(h) Instance metadata reached through the heap.** **[V]** `features` (`system.rs:52`), `command-line` (`process_context.rs:72`) and `gc-stats` read non-GC metadata stored on `Heap`. `Heap` doc comment: the heap is "the only per-instance thing all three already hold a `&SharedHeap` of" (`heap/mod.rs:334-349`). **[I]** A new primitive context should carry an `Instance` (features, command line, fs, library availability) next to the mutator. Otherwise this pressure keeps pushing non-GC state into the GC heap.

### 3.4 Re-entry: which callbacks are live

**[V]** Of the 19 higher-order registrations:

- **`fs` only, no re-entry:** `open-*-file` (×4), `file-exists?`, `delete-file`, and the directory primitives registered in a loop (`io/mod.rs:408-523`).
- **`interaction-environment`** needs only an accessor.
- **`load_scheme_library` re-entry, which runs library bodies (Scheme code):** `environment` (`eval.rs:54-80`), `null-environment` and `scheme-report-environment` (`eval.rs:485-551`).
- **`apply_proc` from Rust, live:** `%parameterize-swap!` on a parameter-*like* procedure (`parameters.rs:195-275`). It keeps an `olds: Vec<TaggedValue>` across the callback.
- **`apply_proc` from Rust, shadowed:**
  - `member` and `assoc` are imported as `%member`/`%assoc`; the comparator path is Scheme (`lib/scheme/base.sld:12-16`, `lib/scheme/base/higher_order.scm:186-205`).
  - `call-with-port` is excluded (`base.sld:23-24`).
  - `call-with-input-file` and `call-with-output-file` are Scheme (`lib/scheme/file/call-with-file.scm`).
  - `call-with-values` and `force` are fallbacks behind backend intercepts (`values.rs:15-19`; AGENTS.md on `force`).

Resumable (5): `eval`, `load` (`eval.rs:771,799`), `make-parameter`, `%parameter-convert` and `%parameter-set!` (`parameters.rs:283-301`). `Step::Call`/`Step::Eval` hand the call to the machine, and the primitive's state is a `TaggedValue` "the machine keeps where the collector sees it" (`registry.rs:40-44`). `load` keeps its remaining forms in a heap vector (`eval.rs:595-611,690-735`).

**[I]**
- This protocol is exactly what a moving GC and a JIT both want: no Rust frame holds Scheme values across a Scheme call, so nothing outside the machine needs rooting or a stack map.
- Finishing the job would leave **zero Rust→Scheme re-entries in primitives**, and GC deferral would be confined to machine-internal paths:
  - add `Step::LoadLibrary { name, state }` and make the three environment constructors resumable;
  - make `%parameterize-swap!` resumable, or reject non-parameter procedures as chibi does;
  - delete the shadowed fallbacks.
- Also give the context `fs()` and `interaction_environment()` so that "higher-order" stops meaning "needs filesystem".

---

## 4. `patina-runtime` (33 files, 3,890 lines)

**[V]**

- **The 22 `stdlib/internal_*.rs` files (1,171 lines) never touch the heap directly.** They list names and arities and call `env.define_primitive` (e.g. `stdlib/internal_lists.rs:11-80`). The single direct allocation is `seed_core_syntax` → `heap.core_syntax(form)` (`stdlib/internal_syntax.rs:67`). One more is in `rust_library_loader.rs:128`. Migration cost: about 0, apart from the `Environment` API.
- **`ParsedLibrary`** (`library_loader.rs:173-215`) keeps `body: Vec<TaggedValue>` outside any root set, plus `heap: Option<SharedHeap>`. It defers GC for as long as it exists.
- **`LibraryRegistry` implements `GcRoots`** (`library_registry.rs:569-575`). Each `Library` holds `exports: HashMap<String, TaggedValue>` and `env: Rc<Environment>` (`crates/patina-core/src/library.rs:22`).
- Totals: 6 borrow sites and 2 heap method calls in non-test src; 35 `Rc<Environment>` mentions.

**[I]** The runtime's real exposure is not call sites. It is that **`Library.exports` and `Environment` bindings are long-lived Rust containers of `TaggedValue`s**:
- they are roots on every collection;
- a moving collector must update them in place;
- a generational collector must write-barrier `define`/`set`/`set_slot_value`, or rescan them on every minor GC.

That decision, whether environments become heap objects, is the single biggest structural item for this layer.

---

## 5. `patina-frontend` (25 files, 17,080 lines)

**[V]** Volume: 14 files mention `SharedHeap` (134 mentions, 93 `&SharedHeap` params); 199 borrow sites; 210 heap method calls in 13 files (387 including tests); 37 `heap.clone()`; 40 `new_shared_heap()` (mostly tests).

Hot files by heap mentions: `desugarer/mod.rs` 468, `library_parser.rs` 367, `parser/mod.rs` 222, `cond_expand.rs` 93, `desugarer/utils.rs` 76. Most-used calls: `get_symbol_or_identifier_name` 22, `get_symbol_name` 14, `car` 11, `get_pair` 11, `type_name` 10, `intern_symbol` 10.

What allocates:

- **Reader.** `Parser` owns a `SharedHeap` and a `labels: HashMap<usize, TaggedValue>`, cleared per datum (`parser/mod.rs:66-104,462`). It builds lists from `Vec<TaggedValue>` frames. Each datum's source location goes into `Heap::syntax_sources`, keyed by `raw_bits()` (`heap/source.rs:22-58`), and into `SourceMap.locations: HashMap<u64, SourceLocation>` (`crates/patina-core/src/source_map.rs:61,178-185`). Stale keys are pruned by the sweep's freed-bits list (`prune_freed_locations`, called before each form in `crates/patina-interpreter/src/lib.rs:509-512` and `patina-repl/src/program_stream.rs:147`).
- **Desugarer.**
  - `quoted: Rc<RefCell<HashMap<u64, TaggedValue>>>` is a raw-bits memo, cleared per form (`desugarer/mod.rs:370-372,1821`).
  - `strip_identifiers_tagged` copies graphs.
  - `Aliases` holds `TaggedValue` identifiers (`:178-189`).
  - Quasiquote lowering caches three constructor procedures per compilation unit in `Cell<Option<TaggedValue>>` (`quasiquote_lower.rs:75-83`).
  - The output `CoreExpr` carries `Literal`/`Quote(TaggedValue)` and `Import { import_sets: Vec<TaggedValue> }` (`crates/patina-core/src/core_expr.rs:143`). These become VM code constants or CPS literals.
- **Re-entry.** Only `desugar_with_imports`: an import can load and run a library while the unfinished input and partial IR live on the Rust stack, so it defers (`desugarer/mod.rs:1838-1841`).
- **Metadata through the heap.** `cond-expand` reads `features` and `library_availability` from `Heap` (`cond_expand.rs:20`, `library_parser.rs:458`, `desugarer/mod.rs:3206`).

**[I]**
- Under the §2 contract, everything except the persistent identity maps is mechanical.
- **`SourceMap` and `syntax_sources` are the front end's real problem.** They are persistent, keyed by raw bits, and kept correct today by "freed bits" notifications. A moving collector would need "moved bits" too. Better options:
  - Chez-style post-GC rehash (§8);
  - a stable per-object id;
  - for syntax-case, keep source locations *in* the syntax objects. `PRD/macro/SYNTAX_CASE_DESIGN.md` "Syntax Objects" already plans a `SyntaxObject { datum, context, source }`.
- **syntax-case will end the front end's GC-atomicity.** Procedural transformers run Scheme during expansion. Everything the desugarer holds (the `quoted` memo, `Aliases`, open forms, the partial `CoreExpr`) will then sit across safe points, and must be rooted or kept under deferral for whole top-level forms.

---

## 6. `patina-macros` (29 files, 8,164 lines)

**[V]**

- Volume: 8 files mention `SharedHeap` (54 mentions; 15 `&SharedHeap` params, plus **32 `&Heap` params**, so it already partly passes a context); 61 borrow sites; 62 heap method calls (87 with tests); 3 explicit drops.
- **Expander.** Builds output as `Vec<TaggedValue>` → `list_from_iter` (`macro_expander/expander/mod.rs:199-216`). It holds `shared_heap: Option<SharedHeap>` (`:64`), as do the matcher (`matcher/mod.rs:51`) and compiler (`compiler/mod.rs:115`). `MatchEnv`/`MatchValue::Leaf(TaggedValue)` (`crates/patina-core/src/pvref.rs:124,236`) live only during one expansion.
- **`CompiledMacro`** keeps `Pattern::Literal(TaggedValue)` and `Template::Literal(TaggedValue)` (`compiled_macro.rs:31,260`) for its lifetime, plus **`pub heap: SharedHeap`** (`compiled_macro.rs:483`). The tracer reaches the literals through the `Macro` arm (`heap/gc.rs:746-754`) via `for_each_literal` (read-only).
- Identifiers are heap objects with Rust payloads: `Identifier { name: Rc<str>, scopes: ScopeSet (SmallVec<[ScopeId;3]>), written }` (`heap/mod.rs:166-171`, `scope.rs:110-114`).

**[I]**
- Mechanical apart from three items:
  - `CompiledMacro` literals need to be updatable (`Cell<TaggedValue>`, or stored in a heap vector the macro object owns);
  - the `heap` field must go, since it causes the leak in §7.3;
  - identifier payloads need a GC-friendly representation if objects become raw heap layouts, for example interned scope-set ids.

---

## 7. Embedding: `patina-interpreter`, `patina-repl`, `patina-compat`

### 7.1 API surface after #601

**[V]**

- `Interpreter<B: Backend>` (`crates/patina-interpreter/src/lib.rs:202-565`) has about 15 `eval_*` methods that return bare `TaggedValue` (e.g. `eval_str`, `:278`), plus `display_tagged`, `global_env() -> Rc<Environment>` and `backend()`.
- `Backend::eval(&self, TaggedValue, &Rc<Environment>) -> Result<TaggedValue, _>` (`crates/patina-runtime/src/backend.rs:33-93`) is public, and hosts can implement it (`tests/support/feature_consumer.rs:9-18`).
- There is **no handle or root API, no way to call a Scheme procedure from Rust with arguments, and no host primitive registration API** on `VmBackend` (`crates/patina-vm/src/backend.rs` public functions: `new`, `with_fs`, `set_tracer`, `add_library_search_path`, `load_library`, …).
- The only accidental root a host has is `global_env().define(name, v)`.
- Call sites: `eval_str`/`eval_program*` are called 62 times in `patina-interpreter` (mostly tests), 129 in `patina-tests`, 5 in `patina-repl` and 3 in `patina-vm`; `display_tagged` 23 times.
- REPL: one file of real heap use; it prints each result immediately (`patina-repl/src/main.rs:210-215,563-605`).
- `patina-compat` uses the reader with a private `new_shared_heap()` as a data-file parser (`exclusions.rs:109-134`), 12 `new_shared_heap` sites. The redesign must keep "reader without a running VM" working.

### 7.2 Host-held values are unrooted (measured)

**[V]** Probe (`PRD/study/gc/probes/primitives-embedding/src/main.rs`):
1. `held = eval_str("(list 'host 'held 'value (vector 1 2 3) \"str\")")`.
2. `eval_program` that churns 200k pairs and calls `(gc)`. Collections went 0 → 4.
3. `display_tagged(held)`.

Results:
- Debug build: `panicked at crates/patina-core/src/heap/mod.rs:721:9: use-after-free: pair slot 16002 was reclaimed by the GC`.
- Release build: prints `after:  (45294)`. The slot was reused, so the result is silent corruption.

The same hazard exists inside the API. `run_forms` keeps the previous form's `value` in a Rust local across the next form's `eval_with_source_map` (`lib.rs:491,517`). In `eval_program_resilient`, if trailing forms fail after allocating, it can return a stale value.

### 7.3 Interpreter drop leaks the heap (measured)

**[V]** Probe `src/bin/leak.rs`: take a `Weak` of the heap and drop the `VmInterpreter`. `weak.upgrade().is_some() == true`, `strong_count == 37`.

The cause is an `Rc` cycle: heap object → `Rc<Environment>` (`VmClosure.globals`, `EnvironmentSpecifier`, CPS procedures) → `Environment.heap: SharedHeap` (`crates/patina-core/src/environment.rs:489`). `CompiledMacro.heap` (`compiled_macro.rs:483`) does the same. `Heap` has no `Drop` that clears its arenas. **[I]** A context-passing design where environments and macros do not own the heap removes this for free.

---

## 8. `PRD/FFI_DESIGN.md` against a new GC

**[V]** The doc is dated 2026-03-20 and still says "When GC is added" (`:435-442`).

- Its plugin prelude re-exports `HeapIndex` and `SharedHeap` (`:72`).
- Its handler example takes `Vec<TaggedValue>`; that is now `&[TaggedValue]`.
- It proposes `HeapObjectData::Foreign { data: Box<dyn Any>, finalizer }` and `ForeignPointer(*mut ())` (`:176-222`).
- It proposes bytevector → `void*` "pinned for call duration" (`:262`) and `ffi-callback` turning a Scheme closure into a C function pointer (`:367`).

**[I]** Constraints for the GC design:
1. **Never export `HeapIndex`** or any encoding detail to plugins. The plugin prelude should be the same `Cx` plus `TaggedValue` that internal primitives use.
2. Foreign objects are **finalizable** objects. A moving or generational collector needs a cheap registry of objects with Rust `Drop` payloads, scanned after marking. Today 13 `HeapObjectData` variants already carry `Rc` payloads and depend on sweep-time tombstoning to drop them (`GC_DESIGN.md` §8).
3. Pinning is needed for the duration of a call:
   - Chez `Slock_object` immobilizes by bumping the segment's `must_mark` (`c/gcwrapper.c:297-340`);
   - G1 counts pins per region and promotes pinned young regions instead of evacuating them ([JEP 423](https://openjdk.org/jeps/423), JDK 22);
   - Java FFM's `Linker.Option.critical(true)` exposes heap memory only to short functions that cannot upcall ([Linker.Option](https://docs.oracle.com/en/java/javase/22/docs/api/java.base/java/lang/foreign/Linker.Option.html)).

   The cheapest design for Patina is a large-object or non-moving space for bytevectors, or a per-call pin count.
4. Callbacks need **global handles** that the collector updates: the JNI global-reference model.

---

## 9. Prior art on host and primitive rooting discipline

| System | When can GC happen relative to native code? | How native code holds values | Source |
|---|---|---|---|
| Chez Scheme | Allocation never collects. It sets a collect request handled at the next event check, while the collector itself is generational and copying. | `Slock_object`/`Sunlock_object`: root plus immobilize (mark-in-place segment). eq-hashtables are rehashed after GC through `tlc` cells. | `c/alloc.c:213-220,500-555`; `c/schsig.c:574-596`; `c/gcwrapper.c:297-340`; `c/gc.c:1734-1773` |
| gc-arena (Rust) | Only between `Arena::mutate` calls: "Either `Arena::mutate` is executing or `Arena::collect` is executing, but never both" | `Gc<'gc,T>` branded lifetimes; `DynamicRootSet::stash` → `DynamicRoot` held outside, `fetch` inside | [README](https://github.com/kyren/gc-arena), [DynamicRootSet docs](https://docs.rs/gc-arena/latest/gc_arena/dynamic_roots/struct.DynamicRootSet.html) |
| OCaml | Any allocation (`Alloc_small` → minor GC) | `CAMLparam`/`CAMLlocal` shadow-stack blocks; `caml_register_generational_global_root` | `~/Project/reference/ocaml/runtime/caml/memory.h:221-248,296,407,566-591` |
| SpiderMonkey | "at almost any time" (allocation, `ToNumber`, …) | `Rooted<T>`/`Handle<T>`/`MutableHandle<T>`, enforced partly by types and partly by a static hazard analysis | [RootingAPI.h](https://searchfox.org/firefox-main/source/js/public/RootingAPI.h) |
| V8 | Any allocation | `HandleScope`/`Local` (indirect slots), `Global` handles; recent "direct handles" put raw pointers in `Local` only when conservative stack scanning is enabled | [v8-local-handle.h](https://chromium.googlesource.com/v8/v8.git/+/main/include/v8-local-handle.h) |
| JNI | Any time; objects move | Local refs: "valid for the duration of a native method call, and are automatically freed after the native method returns". Global refs: valid "until they are explicitly freed" | [JNI design](https://docs.oracle.com/en/java/javase/21/docs/specs/jni/design.html) |
| Wasmtime | Within a store; host code holds handles | `Rooted<T>`: LIFO scopes (`RootScope`), cheap ("LIFO array"). `OwnedRooted<T>`: arbitrary lifetime, "medium" cost | [Rooted docs](https://docs.wasmtime.dev/api/wasmtime/struct.Rooted.html) |
| Gambit | C-allocated objects are "still" (never moved, mark-sweep) | Reference-counted still objects, `___release_scmobj`; movable objects live in a compacting area | `~/Project/reference/gambit/lib/mem.c:60-100,1712-1740` |
| Whippet (Guile) | Cooperative safepoints | Embedder implements `gc_trace_mutator_roots` / `gc_trace_heap_roots`; inline `gc_allocate_fast` and `gc_write_barrier` fast paths "parameterized by collector-specific attributes" for JITs; `gc_pin_object` | [manual](https://github.com/wingo/whippet/blob/main/doc/manual.md), [on safepoints](https://wingolog.org/archives/2023/10/16/on-safepoints) |

**[I]** Takeaways for Patina:
- Patina's existing discipline is the Chez/gc-arena discipline. Keep it.
- Use Wasmtime-style LIFO roots plus owned handles at the few boundaries that cross a safe point.
- Use Chez-style locks or pins for FFI.
- Avoid the OCaml/SpiderMonkey/V8 model. Its cost is per-call-site rooting everywhere, and SpiderMonkey needed a static analyzer to keep it correct.

---

## 10. Migration surface estimate

Non-test source unless noted. "Mechanical" means a codemod plus compile-error-driven fixes with no design decision.

| Crate | Files touching heap | Borrow sites | Heap method calls | Heap-typed params | Mechanical | Needs thought |
|---|---|---|---|---|---|---|
| primitives | 39 of 47 | 406 | 466 | 283 `&SharedHeap`, 304 registrations | ~95%: signatures, borrows, 43 drops | ~25 functions: re-entry (6), `Rc` payload stores (records, parameters, promises, ports, about 10), identity (`values_eq`, `identity-hash`, wrapper allocation, about 5), datum-writer layout match, string and bignum representation |
| frontend | 13 of 25 | 199 | 210 (387 with tests) | 93 | ~90% | Persistent raw-bits maps (`SourceMap`, `syntax_sources`); `desugar_with_imports` re-entry; Parser/Expander *owning* a `SharedHeap` (37 clones); syntax-case readiness |
| macros | 14 of 29 | 61 | 62 (87) | 15 `&SharedHeap` + 32 `&Heap` | ~95% | `CompiledMacro.heap` and its literals; identifier payloads |
| runtime | 3 of 33 | 6 | 2 | 1 | ~100% | `ParsedLibrary` deferral; `Library.exports`; environments (shared with core) |
| interpreter | 4 of 10 | 1 | 6 | 0 | — | About 15 public signatures; `run_forms` value; handle types; heap leak |
| repl, compat | 1 + 6 | 5 | 4 | ~4 | ~100% | Standalone reader heap must still work |
| tests | ~15 files | 7 | 17 (+ ~240 in core tests, 177 in frontend tests) | — | Mostly | About 200 `eval_*` call sites if return types change |
| *for comparison:* core | 8 | 46 | 498 (740) | — | — | — |
| *for comparison:* vm / tree-walker | 8 / 11 | 58 / 30 | 86 / 42 | — | — | — |

**[I]** How the numbers change with the choice of target:

- **A. Context-passing API, GC only at safe points, moving and generational allowed.** Roughly 1,300 non-test call sites outside core change *receiver acquisition* only (`heap.borrow().car(x)` → `cx.car(x)`), plus about 400 handler and param signatures. Most of this can be a sed codemod; the 78 borrow-then-allocate functions get *simpler*. The real work is in the needs-thought column, about 40–50 sites and structs, plus core and the backends.
- **B. Allocation may collect (OCaml/V8 model).** Adds rooting to every function that holds a value across an allocation: at least the 65 read-then-allocate primitives, every accumulator loop, the reader's frame stack, expander output vectors, desugarer graph copies, and library parsing. That plausibly touches most of the ~1,300 sites, and correctness depends on a hazard analysis that does not exist. Not recommended.
- **C. Pointer-based `TaggedValue`, on top of A.** Primitives: almost zero extra, since they use no `heap_index()` and only transient `.raw()`. Front end: the 13 `raw()`/`raw_bits` uses are transient except `SourceMap`/`syntax_sources`. Core and VM carry the cost (`heap_index` 44 and 6 sites; `raw_bits` 26 and 2).

---

## 11. Suggested API shapes, chosen to minimize churn

**[I]**

```rust
// patina-core: keep the name, privatize the encoding.
#[repr(transparent)] #[derive(Copy, Clone, PartialEq, Eq)]
pub struct TaggedValue(u64);          // immediate predicates unchanged (is_pair, as_fixnum…)
// remove from the public surface: heap_index(), from_raw(); keep raw() as
// `identity_bits()` documented "valid until the next safe point".

/// Proof of being between safe points. Accessors keep today's Heap names so
/// call sites change only in how the receiver is obtained.
pub struct Mutator { /* #[repr(C)] alloc_ptr/alloc_limit/card_base for JIT,
                       pending flag, heap internals */ }
impl Mutator {
    pub fn car(&self, p: TaggedValue) -> TaggedValue;              // unchanged name
    pub fn string_chars(&self, s: TaggedValue) -> &[char];         // borrow tied to &self
    pub fn set_car(&mut self, p: TaggedValue, v: TaggedValue);     // barrier inside
    pub fn alloc_pair(&mut self, a: TaggedValue, d: TaggedValue) -> TaggedValue; // never collects
    pub fn identity_hash(&mut self, v: TaggedValue) -> u64;        // stable across moves
}

/// What every primitive gets: the mutator plus per-instance metadata that today
/// rides on Heap (features, command line, fs, library availability).
pub struct Cx<'a> { pub heap: &'a mut Mutator, pub inst: &'a Instance }

pub type HeapHandler = fn(&mut Cx<'_>, &[TaggedValue]) -> Result<TaggedValue, EvalError>;
pub type ResumableStart  = fn(&mut Cx<'_>, &[TaggedValue]) -> Result<Step, EvalError>;
pub type ResumableResume = fn(&mut Cx<'_>, TaggedValue, TaggedValue) -> Result<Step, EvalError>;
// Step gains LoadLibrary { name, state }; HigherOrder disappears (fs/interaction env move onto Cx).
```

- **Codemod.** `heap.borrow().X(` and `heap.borrow_mut().X(` → `cx.heap.X(`; delete `drop(heap_ref)`; `let h = heap.borrow();` → `let h = &*cx.heap;`. Borrowck errors then mark exactly the 78 borrow-then-allocate functions, each a scoping fix.
- **Cold code** (reader, desugarer, library parser, REPL, compat) can keep a `SharedHeap`-like owner and borrow a `Mutator` per entry point. The constraint is that `Parser`, `Expander`, `Environment` and `CompiledMacro` should stop *owning* the heap: pass `&mut Mutator` per call. That removes the leak in §7.3 and the "heap as instance context" pressure.
- **Rooting at boundaries only.** Use a LIFO `RootStack` on the mutator (`let r = cx.root(v); …re-enter…; let v = cx.get(r)`), truncated by an RAII scope. This replaces `GcDeferGuard` at about 10 sites:
  - `across_reentry`;
  - `desugar_with_imports`;
  - `ParsedLibrary`;
  - `with_globals`;
  - `%parameterize-swap!`, until it is made resumable.

  Then nested loops can collect too, removing the §2 limitation. The collector traces and updates the stack like any other root.
- **Persistent Rust containers of values** (about 25 struct types found: `Library.exports`, `Environment` bindings, `CompiledMacro` literals, `CoreExpr`/`CpsExpr` literals, `ParsedLibrary.body`, `ContValue`, `WindRecord`, `ExceptionObject.irritants`, …) need one `Trace` trait that yields *writable* slots (`&Cell<TaggedValue>` or `&mut TaggedValue`). Where they are mutable after creation, writes must go through a barrier-aware method, or the type must become a heap object.
- **Host API.** `eval_*` returns an owned handle: `Global`, an index plus generation into a handle table the collector updates; `Drop` frees the slot. Wasmtime's `OwnedRooted` and JNI global references are the models. Add a scoped closure for bulk work, gc-arena style: `interp.with(|cx| { let v = cx.eval("…")?; cx.car(v) })`. Values never escape it un-rooted. Accept `impl AsValue` in `display_tagged` so most of the ~200 test call sites compile unchanged.
- **JIT ABI for primitives.** Under contract A, a JIT frame calling a heap-only primitive needs **no stack map and no spilling of GC references**, because the primitive cannot trigger a GC. Only calls that can reach a safe point need maps: closure calls, `Step` returns to the machine, raises. One option for an `extern "C"` shim is `fn(cx: *mut Cx, args: *const TaggedValue, n: usize) -> TaggedValue`, with errors signalled by a reserved tag and stored on `Cx`, similar to OCaml's exception-result encoding.

---

## 12. Constraints and lessons for the redesign

1. Keep "allocation never collects; GC only at safe points". It is what keeps primitives, the reader, the desugarer and the expander GC-oblivious, and Chez shows it works with a moving generational collector. The slow path must grow the nursery or allocation buffer and raise the pending flag.
2. Finish removing Rust→Scheme re-entry from primitives (`Step::LoadLibrary`, `%parameterize-swap!`, delete the dead fallbacks). Replace deferral with an explicit root stack at the remaining machine-internal boundaries.
3. Every Rust-side `TaggedValue` slot that survives a safe point must be enumerable, updatable (for moving) and write-barriered (for generational). Unbarriered stores exist today at `records.rs:262`, `parameters.rs:168,267` and `lazy.rs:134`, plus environment `define`/`set`.
4. Object identity must be the address. `eq?` must be one compare. Identity hashes must survive moves (header hash or post-GC rehash). Canonicalize wrapper objects.
5. Persistent raw-bits-keyed maps (`SourceMap`, `Heap::syntax_sources`) need a move-safe identity. Transient ones are fine under item 1.
6. Separate instance metadata from the GC heap.
7. Ship a handle API with the new collector. The current API is already unsound (probe), and the external surface is small now.
8. Break the `Rc` cycle by making environments and macros not own the heap.
9. Update the FFI plan: no `HeapIndex` in the plugin prelude, foreign finalization, pinning or a non-moving space for buffers, global handles for callbacks.
10. Plan for syntax-case: macro expansion will cross safe points.

## 13. Open questions

- Should heap-only primitives get `&mut Mutator` straight from the VM, which needs split borrows between the register file and the mutator? Or should the dispatch loop borrow once from a `RefCell` owner and leave cold code on `SharedHeap`?
- Is unbounded allocation without GC inside one primitive acceptable (e.g. `read-string` of a huge file, `make-vector 1e8`)? Chez says yes. It needs a nursery that can overflow into large-object space.
- Strings: keep `Vec<char>` (4 bytes per char, a Rust-allocated payload, finalizable), or inline them in the heap? This decides whether `strings.rs` and `characters.rs` (252 heap mentions) change beyond receivers. Bignums raise the same question for `num-bigint`.
- Host API style: handles only, scoped closures only, or both? Does the embedding API need "call procedure with arguments" and "register primitive" in the same release?
- Must nested execution be able to collect (library bodies, deep callbacks)? If yes, the root stack in §11 is mandatory rather than optional.
- Threads: the FFI doc's open question 3 and multiple mutators would change `Rc` to `Arc` and make the mutator per thread. Should the API be shaped for that now?
- Does the tree-walker survive the redesign? It shares `ApplyContext` and primitives, and doubles the backend-side migration.
