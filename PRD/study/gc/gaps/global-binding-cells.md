# Global binding cells: a binding-model specification for Patina

Repo: `~/Project/patina`, `main` at `28a94f8`. All file:line references are to that commit.
Evidence tags: **[V]** means verified by reading source or by running something. **[I]** means inference.
Probes and their outputs are under `PRD/study/gc/probes/global-binding-cells/` (`probes/*.scm`, `*.ss`, and the census crate `src/main.rs`). The release binary used was `target/release/patina`, built Sep 30.

---

## 0. Summary

1. **Heap cells are feasible, and they simplify Patina's binding machinery rather than fight it.** Every mechanism layered on the `Environment` slot table turns into "which cell does this name denote":
   - the FORWARDED marker and `Owner` links (#406);
   - the `import_alias` hidden names (#438);
   - `BindingLocation` (#407);
   - the alias env edges;
   - the per-site `env_id` cache;
   - the shadow bitsets (#442).

   Each becomes cell identity or is deleted. [V for the mechanisms, I for the claim]
2. **Cell sharing preserves #406, but offheap.md's claim is incomplete.** The claim was "an import installs the exporter's cell, preserving #406". All 15 rows of `stdlib/library-bindings.scm` are about reading and writing one shared location, and a shared cell gives that by construction. It also closes #406's remaining copy exception: exports reached through an alias or a scoped definition (`library.rs:105-117`, `environment.rs:723-731`). [V/I] What the claim misses is that **putting the exporter's cell straight into the importer's name table changes when Patina binds a reference.** Today Patina follows the *name* on every access. Compiled code then sees a later `define` over an import, or a later re-import. Six Rust tests pin that behavior. [V]
3. **Measured against the oracles, Patina's current rebinding behavior is the outlier** (§2). For code that already ran before an import was redefined or re-imported, chibi 0.12, Gauche 0.9.15 and Chez all keep the old binding, and Patina follows the new one. The oracles also agree on two more shapes:
   - a `define` rebinds the name when it is *expanded*, so a define that never runs still leaves the name unbound;
   - a reference compiled before a mid-program import stays unbound.

   chibi and Chez bind every reference at compile time. Gauche memoizes the location at first execution (`GREF` patches its operand: `Gauche/src/vminsn.scm`, GREF). The cell model's natural semantics, binding at compile time, is chibi's and Chez's, and Racket documents the same rule. [V]
4. **Two variants are specified** (§3):
   - **C (recommended):** compile-time binding to cells. A JIT global access is one load plus an unbound check that can be dropped. Code needs no environment. Shadow bits become per-site cell-value guards.
   - **R:** a re-pointable per-importer slot over shared cells. It keeps today's semantics exactly and costs one extra dependent load (or code patching in a JIT).

   R is also the safe first migration step toward C.
5. **The tree-walker can read globals through cells while its per-call frames stay `Rc<Environment>`** (§5). For parity with the VM under C, global references must be bound once before evaluation. The shared desugarer is the place: it already resolves every reference once (`desugarer/mod.rs:985`, `:2660`).
6. **Cell volume is tiny.** One cell per *owned* top-level binding:
   - 377 after VM bootstrap;
   - 510 with 14 R7RS-small libraries plus SRFI 1;
   - 2,806 with the R7RS-large set (25 libraries).

   At 24 B a cell, that is 9 / 12 / 67 KB. Imports make up 61–86% of all slots and need no cell (§7). Cells are few, practically immortal, and never young. So a **non-moving, append-only cell space outside the `Heap` `RefCell`**, treated as a root region with a per-cell dirty bit, removes every Rust name table from the GC's concerns, under moving collection too (§6).

---

## 1. Today's binding kinds and access paths

### 1.1 Storage [V]

`Environment` (`environment.rs:486-547`, 224 B, capped by `environment_size_is_watched` at `:3382-3388`) has these fields:

- `bindings: RefCell<Bindings>`. An append-only `SmallVec<[(Rc<str>, TV); 3]>` with an FxHashMap index above 8 entries (`:207-219`). A slot never moves, and a redefinition overwrites it in place (`:191-197`). The VM's per-site cache rests on that.
- `scoped_bindings`: name to `SmallVec<ScopedBinding{scopes, tagged_value, visible_by_name}>` (`:14-23`, `:50-52`).
- `alias_bindings`: alias name to `AliasTarget{env: Option<Rc<Env>>, name, scopes: Option<ScopeSet>}`, re-resolved on every access (`:89-135`, `:502-513`).
- `rare: OnceCell<Box<RareTables>>` (`:383-434`), which holds:
  - `introduced_global_names` (VM: spelling, then ScopeSet, then renamed global);
  - `links: Vec<Option<Owner>>` with `Owner = (Rc<Environment>, u32)` (`:341`);
  - `import_aliases` (#438);
  - `owners` (for GC).
- `env_id` (process-unique, the cache key), `parent`, and `heap: SharedHeap`.

An imported slot holds `TaggedValue::FORWARDED` (`tagged_value.rs:110`, raw `0xF1`). Reads go `slot_value`, then `shared_value`, then `owner.slot_value` (`environment.rs:605-662`), which is one hop because `take_binding` resolves through an already-forwarded source (`:764-784`). `define` over a forwarded slot drops the owner and makes the slot the importer's own, in place (`:712-721`, test `:2824-2838`). Importing over a definition re-forwards in place (`:2841-2849`). An environment never forwards to itself, and copies the value instead (`:796-808`, test `:2948-2958`).

### 1.2 Binding kinds

| Kind | Where stored | Created by | Who reads | Who writes |
|---|---|---|---|---|
| Plain global (program, REPL) | root `bindings` slot | VM `Define` (`vm_state.rs:1529-1534`); TW `CpsExprKind::Define` → `def_env.define_*` (`step.rs:204-228`); Rust `define_primitive` (`environment.rs:906-916`), `install_primitives` (`vm_state.rs:390-402`), `seed_core_syntax` (`stdlib/internal_syntax.rs:34`) | VM `LoadGlobal` via cache then `slot_value` (`vm_state.rs:1492-1505`); TW `lookup_var_tagged` → `env.get` frame walk (`cps_eval/environment.rs:79-103`); desugarer `resolve_with_scopes`/`get`; primitives (`eval.rs:86-97` `is_definition_tagged`) | `StoreGlobal` (`vm_state.rs:1507-1527`), `Environment::set` (`:920-947`), TW `set_var_tagged` |
| Library-private | library root env slot | library body (VM `vm_state.rs:810`, `backend.rs:598` under `with_globals`) | code compiled against the library env (closures carry `globals: Rc<Env>`, `heap/mod.rs:205-213`); aliases from other envs | same |
| Imported | importer slot = FORWARDED, plus `links[slot]` and `owners` | `Library::import_into` → `share_binding` (`library.rs:105-117`), called by VM `import_export` (`vm_state.rs:2496-2506`), TW `eval/mod.rs:358,373,463,837`, `environment` / `scheme-report-environment` (`eval.rs:54-80`, `:221-232`, `:520-594`) | every by-name read follows the owner | writes land in the owner (`:620-632`) |
| Fallback copy | importer slot = value | `import_into` when `share_binding` fails: an export reached by alias or name-visible scoped definition, or a hand-built `Library` | as plain | as plain (a stale-able copy) |
| Alias | `alias_bindings` | relinker `define_alias` / `define_scoped_alias` / `define_alias_to_introduced` (`desugarer/mod.rs:1270-1304`); names minted from a process-wide counter (`:194-205`) | `get` after local slots (`:966-970`); `LoadGlobal` uncached fallback | `set` (`:928-930`) |
| Import alias (#438) | ordinary forwarded slot with a hidden name | `import_alias` (`:843-878`) via `early_bound` (`desugarer/mod.rs:1391-1453`) | as an import | as an import |
| Scoped (lexical) | child-frame `scoped_bindings` | TW `application.rs:118-135`; desugarer placeholders (`mod.rs:592`, `:3085`) | `get_with_scopes` / `resolve_with_scopes` (`:1668-1777`) | `set_with_scopes` (`:1472-1575`) |
| Scoped top-level (introduced) | **TW:** root `scoped_bindings`. **VM:** renamed global `"name #s1.s2"` (`alpha_rename.rs:232-247`) plus `introduced_global_names` | TW Define with scopes; VM `compile_pipeline` installs the identities (`compiler/mod.rs:95-106`) | TW scoped resolution; VM `alpha_rename::resolve` consults `for_each_introduced_global` (`:126-205`) | same |
| Keyword (macro / core syntax) | same slot table, value = `Macro` / `CoreSyntax` object | `define_syntax_binding` (`desugarer/mod.rs:2809-2834`); core-syntax seeding | desugarer; `eval`'s definition check | `define-syntax` |
| Environment specifier | `HeapObjectData::EnvironmentSpecifier{env, mutable}` (`heap/mod.rs:181-184`) | `environment` / `scheme-report-environment` / `null-environment` (`mutable=false`); interaction environment = global env | `eval`/`load` compile against `env` (VM `eval_closure` `vm_state.rs:973-986`) | an immutable env refuses definitions before desugaring (`eval.rs:86-114`). `set!` is allowed and writes the shared location |
| Desugar-time declaration views (#463) | throwaway `Environment`s whose markers are shared by `share_binding` (`declarations.rs:1-95`) | a `define` seen during expansion of a form | expander lookups within the form | n/a |

Further VM facts [V]:

- Every VM globals environment is parentless (`compiler/mod.rs:99-102`, `primitive_calls.rs:186-190`). Only alias-reached and name-visible-scoped names go uncached.
- `GlobalCacheEntry` (`code_object.rs:~196-240`) caches `(env_id, slot)` per pc.
- Code is 1:1 with the environment it was compiled against. `eval_closure` allocates the closure with that environment, and `MakeClosure` inherits `frame_globals`.

### 1.3 GC today [V]

`visit_env` walks each environment's slot values and scoped values, recursing into alias and owner edges (`heap/gc.rs:539-559`). `visit_library` also roots the stale `exports` copies (`gc.rs:629-634`). GC_STAGE5's planned generational barrier covers `set-car!`, `set-cdr!`, `vector-set!` and `MutableCell` only. Environments carry no barrier, so every minor GC would rescan them; offheap §3.1 measured 25–51 µs for the 2.4k-value case.

---

## 2. How the oracles treat rebinding (measured 2026-10-01)

Each probe was run under `timeout -s KILL 8` from a working directory outside the repository, with `-I lib` / `-A lib` pointing at a `(counter)` library that exports `count bump! get-count`. Chez was driven through its REPL on stdin with an R6RS `library` form.

| # | Shape | chibi 0.12 | Gauche 0.9.15 | Chez | Patina VM = TW |
|---|---|---|---|---|---|
| p1 | `f` uses `list-copy`, `g` uses `car`; both called; then the program defines both; call again | `((1 2) 1)`, keeps old | `((1 2) 1)`, keeps old | `((2 1) 1)`, keeps old (`reverse`, `car`) | `(mine mycar)`, follows |
| p1b | as p1, but `f` and `show` are **not called** before the redefinition | `((1 2) 0)`, old | `(mine 100)`, follows | `((2 1) 0)`, old | `(mine 100)`, follows |
| p4 | imported variable `count`; `show` compiled and run; program `(define count 100)`; library `bump!` | `(b 2 100 2)` | `(b 2 100 2)` | `(b 0 100)` | `(b 100 100 2)` |
| p4s | program `set!`s an imported variable | shared | shared | n/a (R6RS forbids) | shared |
| p7 | forward reference to a later `define` | works | works | works | works |
| p8 | reference compiled before a mid-program `(import (counter))` that supplies it | error: undefined | error | error: "variable … is not bound" | `0` (follows) |
| p9 | program defines `count`; `show` compiled; then `(import (counter))`; `(bump!)` | `(mine 1)` plus warning "importing already defined binding" | `(mine mine)` | n/a | `(1 1)` |
| q1 | `call-with-values` / `dynamic-wind` sites run, then the program redefines both | old | old (`dynamic-wind` result anomalous) | n/a | `mine mine` |
| q2, q3 | `(import (rename (only (scheme base) cdr) (cdr car)))` after `f`, with and without a prior call | `(1 (2))` | `(1 (2))` (car is inlined) | n/a | `((2) (2))` |
| d1 | `(begin (error "boom") (define list-copy 5))` evaluated, then `(list-copy '(1 2))` | error | error | error (`reverse`) | `(1 2)` |
| e2 | `(environment '(scheme base))`: `define` / `set! list-copy` | "immutable binding" / shared | allowed / shared | n/a | refused / shared |

Reading [V/I]:

- **chibi and Chez bind at compile time.** chibi's `analyze_var_ref` finds the cell, or creates one holding `SEXP_UNDEF` (`chibi-scheme/eval.c:790-805`). Its `define` creates the cell at analysis time (`eval.c:978`). An import shares the source cell and then pushes a fresh frame so that later defines make new cells (`eval.c:2640-2668`). Chez compiles a global to `($top-level-value 'label)`, a load of the symbol's value slot plus an unbound check (`ChezScheme/s/syntax.ss:595-610`, `s/cpprim.ss:3365-3384`). Library exports become `library-global (uid . label)` gensyms (`syntax.ss:930`, `:2768`).
- **Gauche** links a non-inlined global at first execution and inlines some primitives (car) at compile time.
- **Racket** documents compile-time linking (Reference §1.2.5, <https://docs.racket-lang.org/reference/syntax-model.html>): defining an imported identifier "shadows the syntax or import in future uses", and "changing the current namespace during evaluation does not change the variables to which executing expressions refer". Racket CS shares `variable` records across linked instances (`racket/src/cs/linklet.sls:985-1040`).
- **Patina pins its current behavior** in `vm_callprimitive.rs`:
  - `import_rebind_deoptimizes` (`:19-32`), shape q3: oracles say 1, Patina says `(2)`;
  - `define_after_use_deoptimizes` (`:67-74`), shape p1b;
  - `tail_deopt_returns_correct_result` and `tail_deopt_runs_deep_mutual_recursion` (`:35-64`), shape p1b;
  - `control_forms_define_after_use_deoptimize` (`:87-105`), shape q1: **every oracle disagrees**.

  It also pins it in `import_modifiers.rs` with `modifier_rebinding_invalidates_already_compiled_primitive_calls` (`:141-160`), shape q2: chibi and Gauche disagree. No `DIVERGENCES.tsv` row records any of these.

---

## 3. The cell model

### 3.1 Objects

- **Cell** [I]: a `#[repr(C)]` record `{ value: Cell<TaggedValue>, name: TaggedValue /*symbol*/, meta: u64 /*flags, home namespace id*/ }`, 24 B. The flags are `DIRTY` (in the remembered set) and `WATCHED` (compiled code speculates on the value).
  - An unbound cell holds a new special immediate `UNBOUND`. It replaces FORWARDED and is never handed to Scheme.
  - **Invariant: a cell that has been defined never becomes unbound again.** Under C, rebinding a name creates a new cell instead. So a JIT may drop the unbound check for any cell already bound when it compiles.
- **CellSpace** [I]: an append-only, chunked, non-moving arena of cells, owned by the runtime and *not* inside `Heap`'s `RefCell`. Reading a global then never borrows the heap, as `Environment::get` does not today. Any other choice risks RefCell panics across the desugarer, which reads bindings while holding heap borrows (`eval.rs:86-97` is one example).
  - `CellRef` is a stable address, or a `u32` index plus a base pointer in `VmCtx`.
  - The GC treats the space as a root region: major GC scans all cells, minor GC scans the dirty list.
- **Namespace**, the Rust top-level environment: the global env, library envs and environment specifiers. It holds:
  - `names: FxHashMap<Rc<str>, Binding{cell: CellRef, origin: Own | Imported(lib)}>`;
  - for the TW, `scoped_root: name → [(ScopeSet, CellRef)]`;
  - the alias table (alias to `CellRef`);
  - `introduced_global_names`;
  - the #463 declaration overlay.

  It holds **no values**.
- **Frame**: the tree-walker's lexical `Environment`, unchanged and parented on a Namespace.
- **Library**: `exports: name → CellRef`, plus the internal name for diagnostics. The `TaggedValue` copy goes away.
- **Keywords live in cells too.** The cell value is the `Macro` or `CoreSyntax` object: 46–115 cells in the census. This keeps import, rename and export uniform (TEST_ORGANIZATION "Syntax exports … exactly like variables").

### 3.2 Variant C rules (compile-time binding)

These are the chibi, Chez and Racket semantics.

- **C1 Resolve.** Each reference or `set!` in a unit compiled for namespace N resolves to a lexical variable or to a cell, in this order:
  1. the scoped root definition chosen by `scope_resolve::resolve_index` (TW), or the renamed introduced global (VM);
  2. otherwise the alias's cell;
  3. otherwise `names[name].cell`;
  4. otherwise a **placeholder**: a fresh cell owned by N, holding `UNBOUND`, inserted under the name.
- **C2 `define`.**
  - If `names[name]` is N's *own* cell, reuse it. This is R7RS §5.3.1, "acts like set!". Earlier code sees the new value, which keeps `vm_global_cache.rs` passing.
  - If it is an import, an alias, or absent, allocate a new cell and rebind the name.
  - The rebinding is **committed when the unit compiles successfully**, before it runs, in keeping with `compile_pipeline`'s "install last" rule (`compiler/mod.rs:88-94`). The `Define` instruction is then a pure cell store with no name-table mutation at run time. It reproduces d1 as the oracles do.
- **C3 `define-syntax`** follows the same rule at expansion time, which is already when it happens. The special case "a top-level `define-syntax` reports nothing to shadow marks, so CallPrimitive sites keep the procedure while plain sites raise" (`instruction.rs:~140-158`) disappears: every earlier site keeps its variable cell, which is chibi's answer.
- **C4 `set!`** stores into the resolved cell, and raises if the cell is `UNBOUND`. On an import that is the library's cell: #406 and the `library-bindings.scm` rows hold.
- **C5 Import** sets `names[name] = (L.exports[e], Imported)`. Earlier code keeps what it bound (p8, p9, q2 and q3 as chibi answers them). Optionally warn as chibi does.
- **C6 Export** sets `exports[ext]` to the cell `L` resolves for its internal name. That covers a plain own binding, a re-export (the same cell, so one hop by construction), an alias, or a scoped definition. The fallback copy is gone, and so is `distinct_globals`' value comparison (`literal.rs:97-133`).
- **C7 Alias** is resolved to a cell when it is installed. That is equivalent to today, because alias names are unique per process (`desugarer/mod.rs:194-205`) and so are never re-pointed in practice. Only the Rust-API test `a_re_pointed_alias_is_never_a_fast_path` (`vm_callprimitive.rs:133-156`) re-points one. The longer-term form is for the desugarer to emit a cell-carrying `Var`, which makes alias spellings, `alias_base` and the `car.17` collision concern disappear.
- **C8 Early binding (#438)** is subsumed. Every reference is bound when compiled, so `import_alias`, `import_aliases`, the hidden-name filtering in `visible_slots` (`:2124-2155`) and most of `settle_early_bindings` can be deleted. The project's "fast path keys on the binding" rule becomes structural: the binding *is* a `CellRef`.
- **C9 Identity.** `BindingLocation` becomes `CellRef` equality, which gives #407 for free.
- **C10 Environment specifiers.** `mutable=false` keeps refusing `define` and `define-syntax`. `set!` writes the shared cell, as in chibi, Gauche and Patina today (e2). `eval` and `load` compile against the specifier's namespace by C1–C5. An `environment` specifier creates **no** cells, only imports.

### 3.3 Variant R rules (keep "follow the name")

The importer's `names[name]` is a re-pointable binding record. Code links to (namespace, slot), as the VM does today, and reads `record.cell.value`.

- Define over an import allocates a new cell and re-points the record.
- Import re-points the record to the exporter's cell.
- Every current test passes unchanged.

For a JIT, the cost is a second dependent load per global, or V8's technique: embed the cell, record dependent code, and deoptimize on replacement. V8's `PropertyCell` carries `value`, `property_details` and `dependent_code`, and a reconfiguration may replace the cell and deoptimize dependents (`v8/src/objects/property-cell.h`; `PropertyCellType {kMutable, kUndefined, kConstant, kConstantType}` in `property-details.h:255-265`). R is also what a phase-1 migration ships before any semantic change (§8).

### 3.4 The model checked against each path

| Path | C | R |
|---|---|---|
| Reference compiled before its `define` (p7, `vm_global_cache.rs:59-63`) | placeholder cell, then define reuses it (own) | same |
| Define after reference, own name | same cell | same |
| Define over import, earlier code (p1, p4, q1) | keeps import (chibi/Gauche/Chez) | follows (today) |
| Define over import, later code | new cell | new cell |
| Import over import or define (q2, q3, p9) | earlier code keeps the old binding | follows |
| Import over placeholder (p8) | stays unbound (all three oracles) | follows (today) |
| `only`/`except`/`prefix`/`rename` (#592) | `resolve_bindings` unchanged; each pair installs the same `CellRef` (`import_set.rs:35-63`) | same |
| Renamed export `(export (rename count tally))` | `exports[tally]` = cell of `count` | same |
| Re-export chain | one cell | one cell |
| `share_binding(self, self)` (`:2948-2958`) | one cell, two names (fixes today's copy) | same |
| Alias to a library private | cell bound at install | same |
| Scoped alias (#408) | the scoped definition's cell, by identity | same |
| VM introduced global | cell under `"name #s…"`, or nameless | same |
| TW root scoped definition | cell in `scoped_root` | same |
| "Recorded but not yet bound" candidate (`:2028-2078`) | the cell exists and is `UNBOUND`; the rule becomes "cell defined" (see Q9) | unchanged |
| Hygiene forward `set!` (`DIVERGENCES.tsv:311`) | placeholder in the program namespace, so 20 (Patina/Gauche/Chez) | same |
| Unbound detection | `UNBOUND` check on load and on `set!`; message from `cell.name` | same |
| Immutable env | as today | as today |
| `interaction-environment` / REPL | the program namespace | same |
| `VmClosure.globals` | **deleted** (code holds `CellRef`s) | still needed (code links to slots), or a per-code link table |
| `GlobalCacheEntry`, `env_id`, `frame_globals` | deleted | kept, or replaced by link tables |

---

## 4. Primitive-shadow deoptimization as a cell check

How it works today [V]:

- `shadowed_primitives: Vec<u64>` and `shadowed_controls: u8` live on `VmState` (`vm_state.rs:174-181`).
- `mark_if_shadowing_primitive_value` (`:2464-2486`) sets them whenever *any* binding whose old value is primitive P gets a different value. Its callers are `StoreGlobal`, `Define` and `import_export` (`:2496-2520`).
- `CallPrimitive`, the inline opcodes and `JumpUnlessShadowed` (#442) test the bit (`:1377`, `:1574`, `:2185`; `control.rs:3467`).
- The bit is a VM-wide latch that is never cleared. It over-approximates: `(define my-car car) (set! my-car cdr)` deoptimizes every `car` site, because the check looks at the old *value* and not at which binding [V by reading `:2464-2486`].
- `resolve_primitive_calls` refuses names without a local slot, because an alias "can be re-pointed without a write" (`primitive_calls.rs:181-192`).

Under C [I]:

- `CallPrimitive{func_id, cell, expected: TV}` runs the fast path iff `cell.value == expected`.
- `JumpUnlessShadowed{form}` becomes `JumpUnlessCellHolds{cell, expected}`.
- Guards are per binding and precise. They recover once a program restores the procedure. `control_forms_set_after_use_deoptimize` checks answers only, and they stay right.
- The guard costs one dependent load against today's bit test.
- Aliases need no exclusion, since a site's cell never changes.
- `mark_if_shadowing_*`, `mark_if_import_rebound` and both bitsets are deleted.

For the JIT, embed the cell, emit no guard, and set `WATCHED` on the cell. The cell-store barrier's slow path, which a `DIRTY` or `WATCHED` flag test reaches, invalidates dependent code. This is V8's `kConstant` cell with `dependent_code`. Patina frames are shared with the interpreter, so deoptimization means "resume at the same pc in the interpreter" (jit-readiness §5 G/H).

Under R, the shadow bits can stay as they are, or become guards keyed on the binding record.

---

## 5. Tree-walker

Today every variable read walks the frame chain by name (`cps_eval/environment.rs:79-103`). That is about 5 `Environment`s per call (offheap §3.1). Globals pay the whole walk, and scoped references collect candidates over the whole chain.

- **Can globals go through cells while frames stay `Rc<Environment>`?** Yes [I]. The root namespace stores cells, and `local_value` at the root reads `cell.value`. Frames, CPS continuations and `CpsLambda.env` are unchanged. Under R that is the whole change, and the TW still follows the name.
- **Under C, the TW must bind at compile time** or the backends diverge on p1, p1b, q1–q3 and d1. A lazy per-node cache would give Gauche's semantics instead. The right place is the shared desugarer:
  - It already resolves each reference once (`resolve_with_scopes`, `mod.rs:985`, `:2660`).
  - It knows by `binding_location` whether the reference reaches the root or a lexical frame (`early_bound` does exactly that test, `:1424-1426`).
  - It would emit `CoreExpr::Var{name, scopes, global: Option<CellRef>}`, which the CPS transform carries.
  - The TW then reads the cell directly and skips the frame walk, which should be a TW speedup [I].
  - The VM's `alpha_rename` keeps lexical renaming. It should `debug_assert` that it finds no lexical binder for a cell-annotated `Var`, which gives a free differential check during migration.
- This is the first slice of SYNTAX_CASE_DESIGN's "Resolve Once, Before the Backends" (`PRD/macro/SYNTAX_CASE_DESIGN.md:89-131`). That doc names the obstacle directly (`:358-361`): "the desugarer's binding table *is* the runtime `Environment`". Cells are the split it asks for: compile-time Rust tables over runtime heap cells, the Chez separation of ribs from symbol value slots.
- TW frames remain off-heap roots for a generational GC. That is tree-walker.md's problem, and cells do not solve it.

---

## 6. Rust-side structures, roots and moving GC

What remains in Rust [I]:

- namespace name tables: 19,532 entries for the R7RS-large set;
- the TW scoped root tables;
- alias tables;
- `introduced_global_names`;
- desugar-time declaration views (#463), which C could reduce to an overlay of placeholder rebindings;
- `Library.exports`;
- `CellRef` operands in `CodeObject`s;
- macro `definition_env` handles.

**With cells in a non-moving, append-only CellSpace that is never reclaimed**, none of these needs tracing or updating:

- Cells are reachable by construction, because the space is a root region.
- Cells never move, so `CellRef`s in Rust tables, code and JIT immediates stay valid under nursery evacuation or old-generation defragmentation.
- Cells are old by construction, so minor GC sees only the dirty list.
- Major GC scans about 3k cells.

The barrier on `StoreGlobal`, `Define`, Rust `define` and import installs is `if young(v) && !DIRTY → set DIRTY and push`. Patina allocates only a few thousand cells, so immortality is fine. Growth comes from two sources:

- **Placeholders:** one per distinct unbound name referenced.
- **Introduced globals:** one per macro expansion. These already leak today (`environment.rs:1169-1174`).

The alternative is cells as ordinary old-space heap objects, which Chez does with relocations in code objects. Then name tables and code objects must be traced at every major GC and updated if cells move. For about 67 KB of cells that is not worth it.

Today's edges deleted by C [V that they exist]:

- `owners` and `links` (`for_each_shared_owner`, `:2211-2218`);
- alias env edges (`:2226-2234`);
- `VmClosure.globals: Rc<Environment>`. This edge is one link of the teardown-leak cycle in offheap §1.2, so deleting it helps break that cycle.
- `Library.exports` TV roots, which today also keep stale export values alive (`gc.rs:630-632`).

---

## 7. Measurements: binding census

The probe crate is `PRD/study/gc/probes/global-binding-cells`, run from the repo root. It walks every environment reachable from the global env through owner and alias edges and classifies each slot [V]:

| Stage | Backend | Root envs | Plain slots | Own (= cells) | Imports (FORWARDED) | Hidden #438 aliases | Alias edges | Owner edges |
|---|---|---|---|---|---|---|---|---|
| bootstrap plus `(scheme base)` | VM | 17 | 982 | 377 | 605 | 0 | 0 | 30 |
| plus 13 R7RS-small libs and SRFI 1 | VM | 25 | 2,426 | 510 | 1,916 | 6 | 0 | 100 |
| R7RS-large set (25 libs) | VM | 68 | 19,532 | 2,806 | 16,726 | 212 | 574 | 824 |
| same three stages | TW | 17 / 25 / 68 | 984 / 2,433 / 19,861 | 379 / 517 / 3,135 | 605 / 1,916 / 16,726 | 0 / 6 / 212 | 0 / 0 / 574 | 30 / 100 / 824 |

What the owned bindings hold:

- **Bootstrap (VM):** 314 primitives, 27 core syntax, 19 macros, 17 closures.
- **R7RS-large (VM):** 2,089 closures, 378 primitives, 88 macros, 27 core syntax, 72 records, 46 flonums, and the rest data.

Byte estimate [I]:

| Stage | Cells at 24 B | Link tables removed (≥ 16 B per import) | Name tables (stay, about 28 B per entry) |
|---|---|---|---|
| bootstrap | 9 KB | 9.7 KB | 27 KB |
| 14 libraries | 12 KB | 30.7 KB | 68 KB |
| R7RS-large | 67 KB | 268 KB | 547 KB |

So cells cost less than the link tables they replace. Context for scale: the R7RS-large VM heap holds 2.89M objects after loading, because loads never collect.

The access cost cells remove is reported elsewhere [V, cited]:

- globals are 14–15% of dispatches in fib/tak (jit-readiness §5 A5);
- the `frame_globals` `Rc` clone is 2.6% of samples (vm-runtime §5);
- today's minor-GC rescan of environments is 25–51 µs (offheap §3.1).

---

## 8. Migration plan

0. **Issue and decisions.** File an issue (project rule: issue first) and get answers to Q1–Q4. Add a suite file with rows p1, p1b, p4, p8, p9, q1–q3 and d1, with `DIVERGENCES.tsv` rows for whichever oracle disagrees under the chosen variant (under C, that is Gauche on p1b).
1. **Cells under slots (variant R, no semantic change).**
   - Add CellSpace. A root `Bindings` slot holds a `CellRef`.
   - An import stores the exporter's `CellRef` in the importer's slot.
   - Delete FORWARDED, `Owner`, `links`, `owners` and `for_each_shared_owner`.
   - Define over an import allocates a cell and re-points the slot in place, which keeps the per-site cache valid.
   - `Library.exports` becomes `CellRef`, and the fallback copies become shares.

   Adjust only the copy-pinning unit tests (`:2873-2895`, `:2948-2958`, `library.rs` `an_export_with_no_binding_behind_it_is_imported_by_value`) and `distinct_globals`. Gate on everything listed in §10.
2. **One barrier and the minor-GC root set.** All cell writes go through one function. Namespaces leave the root set, and CellSpace becomes a root region.
3. **Switch to C**, if approved.
   - Use `LoadGlobal{cell}`, `StoreGlobal{cell}` and `Define{cell}`.
   - Delete `GlobalCacheEntry`, `frame_globals`, `VmClosure.globals`, `env_id` caching, `import_alias`, `early_bound`'s minting, the shadow bitsets and `mark_if_*`.
   - Add `CallPrimitive` cell guards and desugarer-annotated globals for the TW.
   - Rewrite the six tests in §2 to the oracle answers.
4. **JIT ABI.** `#[repr(C)]` cell, the `UNBOUND` immediate, the `WATCHED` flag with invalidation, and immediate cell addresses in machine code.

---

## 9. Risks

1. **Semantic change** under C. Six tests flip. Gauche agrees with today's answer on p1b, and REPL users who redefine an imported procedure will no longer see earlier definitions change. This matches chibi, Chez and Racket.
2. **RefCell discipline** if cells live inside `Heap`. Mitigation: a separate CellSpace (§3.1).
3. **Misclassifying a lexical reference as global** in the desugarer silently reads the wrong variable. Mitigation: VM `alpha_rename` assertions, and a TW shadow mode that walks frames and compares against the annotated cell, run over every lane before switching.
4. **Hygiene interplay.** Triage families 36, 38 and 40, `collect_introduced_globals`' "recorded but not yet bound" rule, `hygiene_matrix.rs` (139 shapes), and #424's scoped macro bindings.
5. **Transactional commit.** A unit that fails to compile must not leave rebinding behind. `compile_pipeline` already installs last. The #463 declaration views must agree with the commit.
6. **Placeholder and introduced-cell growth** in long REPL sessions or code that runs `eval` in a loop.
7. **Mid-program imports.** Under C, p8 turns into an error, as in every oracle.
8. **Performance.** The `CallPrimitive` guard adds a load. Measure interleaved against `main` with the Criterion lanes, both backends.
9. **JIT invalidation and continuations.** Invalidated code can be captured in a continuation. Shared frames make resuming in the interpreter possible, but it must be tested against `control_flow_matrix.rs`.

---

## 10. Tests that would catch regressions

- **Unit:**
  - `crates/patina-core/src/environment.rs`: `shared_binding_tests`, `binding_location_tests`, `introduced_definition_tests`, `import_alias_tests`, `layout_tests`;
  - `library.rs` tests;
  - `hygiene_properties.rs`.
- **Integration (`crates/patina-tests/tests/`):**
  - `vm_global_cache.rs`, `vm_callprimitive.rs`, `vm_inline_opcodes.rs`;
  - `import_modifiers.rs` (190 runs across five contexts), `import_set_is_enforced.rs`;
  - `library_loading.rs`, `library_value.rs`, `spliced_imports.rs`, `internal_library_exports.rs`, `primitives_reachable_by_import.rs`, `r7rs_large_aliases.rs`;
  - `macro_definition_env.rs`, `core_syntax_bindings.rs`, `syntax_as_a_value.rs`;
  - `hygiene_matrix.rs`, `hygiene_metamorphic.rs`, `control_flow_matrix.rs`.
- **Suite files:**
  - `stdlib/library-bindings.scm` (15 rows on Gauche), `stdlib/eval.scm`;
  - `expansion/template-references.scm`, `imported-names-in-templates.scm` (#438), `hygiene.scm` (`DIVERGENCES.tsv:311`), `keyword-bindings.scm`, `introduced-definitions.scm`, `splicing-libraries.scm`, `import-modifiers.scm`, `let-syntax.scm`.
- **Lanes:**
  - both chibi scripts;
  - `run_suite_oracles.sh`;
  - both Larceny lanes;
  - `run_gc_differential.sh`, release and debug-poison, which catches a cell space that is mis-rooted;
  - `patina-compat check-smoke`.

---

## 11. Questions for the project owner

1. **Rebinding semantics.** Should Patina adopt compile-time binding (C: chibi, Chez, Racket), keep follow-the-name (R: today, which agrees with no oracle on p1, p4, q1–q3), or link at first execution (Gauche)? Six tests encode follow-the-name, and `control_forms_define_after_use_deoptimize` disagrees with all three oracles.
2. **When does a `define` rebind:** at compile or expansion time (every oracle; d1) or when it runs (today)?
3. **Import over a placeholder (p8).** Should earlier references stay unbound (every oracle), or should a forwarding placeholder that costs no fast-path time preserve today's answer?
4. **Import over the program's own definition (p9).** Should the import win for later code (chibi, which warns), the definition win everywhere (Gauche), or the slot be overwritten in place (today)? Should Patina adopt chibi's warnings ("importing already defined binding", "reference to undefined variable")?
5. **Are cells immortal?** That is, is placeholder and introduced-global growth acceptable, or do we want weak placeholders?
6. **Keywords in cells** (uniform imports), or a separate keyword table?
7. **Constancy.** Should cells stay universally assignable (the #406 policy rows) with JIT speculation through `WATCHED`, or should some cells become constant (Racket's `constance`, Chez's immutable library globals) so imports can be constant-folded?
8. **Tree-walker parity.** Is desugar-time global annotation acceptable as the first slice of resolve-once? Without it the TW cannot follow C.
9. **Introduced definitions.** Under C, should a compiled-but-not-yet-run introduced definition be a candidate in desugar-time resolution, as the oracles bind at expansion, or keep today's "not yet a binding" rule (`environment.rs:2028-2034`)?
10. **Primitive guards.** Is it acceptable for a `CallPrimitive` fast path to *recover* when a primitive is restored? Today's latch never recovers.

---

## 12. Verdict on the claim under review

offheap.md §3.1 and §7.3 say "an import installs the exporter's cell, preserving #406".

- **#406 sharing: true** [V/I]. Read, write and re-export semantics are a shared cell by construction, and the fallback-copy exception is closed.
- **Redefinition: incomplete** [V]. If the importer's name table holds the exporter's cell directly and code binds to cells, Patina's current "already-compiled code follows a later define or import" behavior changes. That change lines up with chibi, Chez and Racket, and against Patina's own tests. Keeping today's behavior exactly requires variant R's re-pointable importer slot, or dependent-code patching.
