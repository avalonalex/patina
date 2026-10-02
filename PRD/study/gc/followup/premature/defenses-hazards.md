# Premature collection today: defenses and remaining hazards

Repository state: `gc-prd` at `f82e8e8`. That is `main` at `28a94f8` plus one PRD-only commit, so the source is
`28a94f8`. All file:line references are to that tree.

Evidence labels:
- **[V]** means I read it in source at the cited lines, or reproduced it with the probe below.
- **[I]** means inference.

The probe is a scratch crate that depends on the repository crates by path, built in its own target directory in both
debug and release, and run from the repository root:
`scratch/probe/src/main.rs` (absolute path:
`<scratch>/probe`).
The repository was not modified.

---

## 1. Summary

1. **One invariant carries all of today's safety:** a collection runs only at the top of an *outermost* driver loop,
   where every live value is in a registered root. Three things enforce it [V]:
   - Allocation never collects. `note_alloc` only raises a flag (`heap/mod.rs:581-584`, `:570-574`), and so does `(gc)`
     (`:609-611`).
   - Exactly two production sites collect: the VM's `maybe_collect` (`vm_state.rs:1287-1310`, called at `:1204-1208`)
     and the tree-walker's `maybe_collect` (`cps_eval/mod.rs:114-133`, called at `:230`). No other non-test code calls
     `collect`, `run_mark_phase` or `sweep`.
   - Every nested loop defers through one shared counter (`GcDeferGuard`, `gc.rs:232-268`).

   Inside the runtime, this makes every handle-hazard site counted in offheap.md §5 safe by construction (§5 below).
2. **The premature-collection bugs that exist today all sit above the outermost loop, where the invariant says
   nothing.**
   - [#605]: values returned by `eval_*`, and `run_forms`' `value` (`patina-interpreter/src/lib.rs:491,517`).
   - A second shape #605 does not name **[V, probe]**: a host-created environment (`Environment::with_parent`) passed to
     `Backend::eval`.
     - On the tree-walker its bindings are freed. Debug panics with `use-after-free: pair slot 16004`; release prints
       `(45294 . 45294)`.
     - The VM survives only because `VmBackend::eval` ignores its `env` argument (`patina-vm/src/backend.rs:675-693`) and
       defines into the global environment. That is a separate backend divergence.
3. **Nested loops are safe only because they never collect.** Each of the following holds heap values in Rust across a
   nested loop and relies on that rule [V]:
   - the weak continuation tables (`gc_roots.rs:21-24`);
   - `with_globals`, which leaves the *entire global environment* unrooted for its window (`vm_state.rs:337-347`);
   - `%parameterize-swap!`, `run_synchronously` and the `force` fallback;
   - `ParsedLibrary` bodies and `desugar_with_imports`.

   Letting any nested loop collect (PRD stage 4e) turns these into use-after-frees unless their values move into the
   machine first.
4. **Debug poison catches much less than the lanes assume** [V, probe]:
   - Use-after-free of a vector or string is invisible. The probe printed `#()` and `""`.
   - Any use-after-free is invisible once the slot has been reused. Reuse is LIFO, so it comes quickly. The probe's held
     pair printed as part of a live list in a *debug* build.
   - Eight object accessors bypass the `Free` assertion. One of them, `frame_globals`, falls back silently to the
     global environment.
   - The collector itself silently absorbs a dangling reference. GC_DESIGN §11.5's pre-sweep assertion was never
     implemented.
5. **Register retirement (#423) adds a failure mode no assertion sees.** A wrong per-pc map clears a live register to
   `UNSPECIFIED`, which is a legal value. Only a changed output in a run that collected at that pc can reveal it [V].
6. **Stress mode collects at the first outermost safe point after N allocations.** It is not "every safe point", and it
   never collects in:
   - nested loops, library bodies, bootstrap or desugaring;
   - stretches without allocation.

   CI runs it only on the chibi suite, through the CLI, at N=16. It therefore cannot see embedding bugs, code under
   deferral, or rooting shapes the suite does not contain (#587) [V].
7. **No heap verifier, zeal mode, dead-slot sentinel or static check exists.**
   - Trace-rule completeness rests on an exhaustive `match` (a real compile-time defense) plus a doc comment.
   - Collection is public API: `Collector`, `MarkSweepCollector`, `run_mark_phase` and `Heap::sweep` are re-exported
     from `patina-core` (`lib.rs:71-72`) [V].

---

## 2. Probe results

| Case (both backends unless noted) | Debug build | Release build |
|---|---|---|
| Host holds `(vector 1 2 3)` from `eval_str`; a later `eval_program` churns 200k pairs and calls `(gc)` (collections 0→4) | prints `#()`, **no panic** | `#()` |
| Same for `(string #\a #\b #\c)` | prints `""`, **no panic** | `""` |
| Same for `(bytevector 1 2 3)` (object arena) | panics: `use-after-free: object slot 9813` (`heap/mod.rs:1487`) | prints `#<gc-freed-slot>` |
| Host holds `(list 'held 'pair)`; churn and `(gc)`; then 300k pairs kept live, so every freed slot is reused | prints `(49255 49254 …)`, part of the new list, **no panic** | (not run; same mechanism) |
| Host child env from `Environment::with_parent(global)`; `backend().eval('(define host-only (list …)), &child)`; churn and `(gc)`; read `host-only` back | **TW:** panics `pair slot 16004`. **VM:** fine, because the define went to the global env (`host-only` is visible there) | **TW:** `(45294 . 45294)`. **VM:** fine |

---

## 3. Defenses that exist today

### 3.1 Placement: the invariant itself [V]

| Element | Where | What it catches | What it cannot see |
|---|---|---|---|
| Allocation only requests a collection | `heap/mod.rs:570-584` (`note_alloc`, `refresh_gc_pending`), `:609-611` (`request_gc`); comment at `:698-702` | Makes every Rust local legal across `alloc_*`: offheap §5's patterns A, B, C | Rust frames that hold values *across a driver loop* |
| Collection only at the loop top, borrow-free | VM: `vm_state.rs:1204-1208`, `maybe_collect` `:1287-1310` (after `retire_registers`). TW: `cps_eval/mod.rs:230`, `:114-133`. Shared rule: `GcController::safe_point`, `gc.rs:381-409` (one `borrow_mut` spans the collection, `:405-407`) | Machine state is all in `VmState` or `current_step` at that point | Whatever sits *below* the outermost loop on the Rust stack, which is the embedder ([#605], §4.1) |
| Abort rather than collect with a partial root set | `LibraryRegistry::try_roots` (`library_registry.rs:578-592`), used at `vm_state.rs:1301-1305`; TW `try_borrow` at `cps_eval/mod.rs:125-127` | A registry mutably borrowed during a load. The pending flag stays set and the next safe point retries | Nothing unsafe; the cost is a delayed collection |
| A heap `RefCell` borrow excludes collection | `safe_point_cold` takes `heap.borrow_mut()` (`gc.rs:405`) | Code holding a heap borrow cannot be collected under. A collection reached there would panic, not corrupt | It is a runtime panic, not a type error, and fires only if the path is reached |

### 3.2 `GcDeferGuard`: five sites [V]

`GcDeferGuard` (`gc.rs:232-268`) holds an RAII depth on `Heap.gc_defer_depth`. A loop is outermost iff the depth was 0
when its guard was taken (`is_outermost`, `:259-261`). The value is hoisted once per loop: `vm_state.rs:1151-1154`, TW
`cps_eval/mod.rs:219-221`.

| Site | Location | Rust-held values it protects |
|---|---|---|
| Every VM dispatch loop | `vm_state.rs:1151` | Caller frames of nested loops: primitive callback arguments, `run_synchronously` state, `%parameterize-swap!`'s `olds`, capture temporaries, `across_reentry` |
| `VmState::with_globals` | `vm_state.rs:337-347` | `saved`, the *real* global environment, which is unrooted while `state.globals` is swapped (`trace_roots` visits only `self.globals`, `gc_roots.rs:101`). Used by library loads (`backend.rs:598`, `vm_state.rs:810`) and by the synchronous `eval_expr` fallback (`vm_state.rs:903`) |
| Every tree-walker trampoline | `cps_eval/mod.rs:219` | Suspended outer `StepResult`s |
| `ParsedLibrary::new`, for the object's lifetime | `library_loader.rs:192-215` | `body: Vec<TaggedValue>` of unevaluated forms. This is the fix for stage 3's "bootstrap died on the first collection" (GC_DESIGN §5.2) |
| `Desugarer::desugar_with_imports` | `desugarer/mod.rs:1838-1841` | Unfinished input, include datums and partial IR while an import runs library bodies |

**Can't see.**
- `ParsedLibrary::new` takes `heap: Option<SharedHeap>` and installs no guard on `None` (`library_loader.rs:208`). The
  only caller passes `Some` (`library_support.rs:160-167`), so this is latent [V].
- `is_outermost` is computed at loop entry. A guard that a callee creates and keeps alive past the instruction would not
  stop the running loop from collecting. No such guard exists today [V/I].
- Balance is checked only by a `debug_assert` (`heap/mod.rs:674-680`).

### 3.3 Root providers and trace rules [V]

| Provider | Location | Notes |
|---|---|---|
| `VmState` | `gc_roots.rs:70-115` | Registers, `scratch_args`, `pending_escape`, frames' bare `closure` index (`:154-160`), every `code_store` entry's constants (`:97-99`), `globals`, prompts, winds, handlers, tracer |
| `LibraryRegistry` | `library_registry.rs:569-575` | Exports and environment of every loaded library |
| Tree-walker `Evaluator`, `EscapeRoots`, `StepRoots` | `cps_eval/gc_roots.rs:29-118` | Global env, the `PENDING_ESCAPE` thread-local, `current_step` and the entry expression's literals |
| Symbols and core syntax | `gc.rs:449-465` | Immortal; mark-only |
| Trace rules | `gc.rs:695-793` | `HeapObjectData` is matched without a wildcard, so a new variant is a compile error; `trace_step` likewise (`cps_eval/gc_roots.rs:67-118`) |

I checked the `VmState` fields (`vm_state.rs:164-220`) and `ExecutionState` (`execution_state.rs:18-22`) against
`trace_roots`. Every field that holds a `TaggedValue` is traced or deliberately weak.

**Can't see.**
- A new `TaggedValue` field inside an existing variant matched as `{ .. }` or classified as a leaf (`Identifier`,
  `Procedure::Primitive`, `PromptTag`, `Port`). The enum's own comment says a misfiled leaf "is a use-after-free, not a
  compile error" (`heap/mod.rs:137-141`).
- A new off-heap holder that nobody adds to a provider. The root inventory (GC_DESIGN §5) is a manual checklist.

### 3.4 Register retirement, `register_roots` (#423) [V]

- The maps come from a forward "possibly written and not yet retired" analysis (`pass5_codegen.rs:223-286`). Its
  retirements are emitted after each non-tail expression (`:665-690`).
- `retire_registers` (`gc_roots.rs:47-68`) overwrites registers outside the map with `UNSPECIFIED`. It runs:
  - before an outermost collection (`vm_state.rs:1291`);
  - at capture, for both continuation kinds (`vm_state.rs:636-656`).
- Fallbacks are conservative: a frame with no maps is skipped (runtime stubs), a pc with no entry is skipped, and an
  unreached pc gets all-ones (`pass5_codegen.rs:275-279`).

This is a *precision* feature: it fixed retention, not loss. Its tests pin shapes that were risky when it landed (#551):
- `ephemerons.rs:94-195`: tail-call windows, self tail calls, continuations and delimited snapshots;
- `gc_vm.rs:120-185`: discarded results, more than 64 pending operands, a rebound control operator.

**Can't see** a wrong map. A live register cleared to `UNSPECIFIED` is a legal value, so no assertion fires. The error
surfaces only if a collection happens at exactly that pc *and* the output changes. It also only matters at collecting
pcs, so a mode that collects only after allocation (§3.6) exercises few of them.

### 3.5 Weak continuation tables and ephemerons [V]

- `run_mark_phase` (`gc.rs:1022-1100`) runs one fixpoint over weak ids and ephemerons. It replaced a sequential
  version that left a continuation pointing at swept slots (commit `1d18c49`, comment at `gc.rs:1030-1045`).
- `value_is_live` is private so that no provider can skip a trace on a not-yet-marked answer (`gc.rs:517-534`).
- **The soundness rule is stated at `gc_roots.rs:21-24`:** "every store touch (capture, invoke) is confined to one
  instruction dispatch and nested loops defer collection". Capture allocates the ref and inserts the entry back to back
  (`vm_state.rs:631-656`).
- Tests: `weak_continuation_tests.rs` (7), `ephemerons.rs` (15), Larceny's `ephemeron` suite (6/6).

**Can't see:** the rule is unchecked. Nothing asserts that a store is touched only within one dispatch, or that no
nested loop collects while a continuation id is held in Rust.

### 3.6 GC modes [V]

`GcMode::from_env` (`gc.rs:274-317`) is installed at backend construction (`vm_state.rs:232`,
`tree-walker eval/mod.rs:101`):

| Variable | Mode | Exact behaviour |
|---|---|---|
| `PATINA_GC=0` | `Off` | Threshold `usize::MAX`; `(gc)` still collects at the next outermost safe point |
| (default) | `On` | Threshold `max(65 536, 2 × live slots after the last collection)` (`gc.rs:1001-1003`) |
| `PATINA_GC_STRESS=n` (default 1, `0`/empty means unset) | `Stress(n)` | `note_alloc` raises `gc_pending` once `allocs_since_gc ≥ n` (`heap/mod.rs:570-584`). The collection then runs at the **next outermost safe point**; sweep resets the counter and flag (`gc.rs:956-958`) |

So stress is **neither** "every safe point" **nor** "inside the N-th allocation":
- With n=1 it collects at the top of the instruction (VM) or step (TW) that follows any allocating one, at the outermost
  level only.
- Allocations inside nested loops, library bodies, bootstrap and desugaring count toward n, but the collection waits
  until control returns to an outermost safe point.
- A stretch of non-allocating instructions never collects.
- `(gc)` likewise only requests a collection.

### 3.7 `scripts/run_gc_differential.sh` and CI [V]

The script runs the chibi suite with `-k -A test-lib` on both backends in three modes: off, default, and stress
(`PATINA_GC_STRESS_INTERVAL`, default **16**; 1 takes about 103 s against 0.15 s).
- It asserts `EXPECTED_TOTAL=1226` before diffing, so a vacuous pass is impossible.
- It normalises only ANSI codes and timings.
- Two reclamation proofs: stress needs more than 1000 collections and under 256 pairs of growth over 20k churned
  conses; default needs at least one collection and under 150k pairs after 200k churn.

CI runs it in two jobs (`.github/workflows/ci.yml:92-120`):
- release;
- a debug build with poison assertions, which the comment there calls "the strong one".

**Can't see:**
- Embedding misuse: everything runs through the CLI.
- Anything under deferral (library bodies, nested loops).
- 15 of every 16 allocation sites, since the interval is 16.
- Any program outside the chibi suite. The `cargo test` lane runs the default mode, where most tests never reach the
  65k floor. No Larceny or matrix suite runs under stress.
- A divergence that does not change printed output: a vector or string use-after-free, a reused slot, or a retired
  register whose value is unused.

### 3.8 Debug poison and other assertions [V]

| Check | Where | Catches | Blind spots |
|---|---|---|---|
| Pair poison | sweep writes `GC_POISON` in debug only (`gc.rs:906-913`); asserted in `get_pair`, `set_car` and `set_cdr` (`heap/mod.rs:718-762`); test `gc.rs:1781-1790` | A read of a freed pair **before the slot is reused** | After reuse (probe). Release skips the store entirely |
| Object `Free` tombstone | `get_object` (`heap/mod.rs:1484-1494`) | A freed object read through `get_object`/`get_object_type` and their roughly 90 callers | **8 direct arena accesses bypass it** (`heap/mod.rs:970, 1085, 1355, 1370, 1385, 1400, 2875, 2890, 2903`) and answer `None`/`false` on a `Free` slot. `get_vm_closure_globals` feeds `frame_globals` (`vm_state.rs:1357-1363`), which **silently falls back to `state.globals`**. `get_vm_closure_free_var` turns a freed running closure into "closure slot N out of range" (`:1451-1467`). Release prints `#<gc-freed-slot>` (`:1966`) |
| Vector and string tombstones | `Vec::new()` (`gc.rs:917-936`) | Nothing (documented at `gc.rs:885-886` and GC_DESIGN §4.5) | Every use-after-free in these two arenas (probe) |
| BitSet bounds | `gc.rs:66,76` | A wrong-heap or garbage index during marking | — |
| Defer balance | `heap/mod.rs:674-680` | An unbalanced guard | — |
| `with_globals` heap identity | `vm_state.rs:342` | Mixing heaps | Debug builds only |
| Freed closure counted | `vm_state.rs:552-555` | Code-count drift | — |

**Absent:**
- GC_DESIGN §11.5's "no free-list slot is marked" assertion. `sweep_arena` pre-marks the free list and ignores the
  `was_clear` result (`gc.rs:836-838`).
- Any check when *tracing* meets a dead slot. `trace_children` reads the arenas raw (`gc.rs:679-692`): a poisoned pair
  yields two immediates and a `Free` object is a leaf. So a dangling root or edge is absorbed silently at every
  collection.
- GC_DESIGN §9.6's `Rc::ptr_eq(macro.heap, heap)` assertion.
- Any verifier, zeal mode or `DEAD_SLOT`. There is no `PATINA_GC_VERIFY` and nothing named zeal in the tree.

### 3.9 Tests that target rooting [V]

| File | Tests | Target | How a collection is caused |
|---|---|---|---|
| `heap/gc.rs` unit | 22 | Reachability, trace rules (records, cells, procedure literals, wind handler stacks), tombstoning, trigger, `safe_point_collects_only_when_outermost_and_pending` (`:1749`) | Direct `collect` on synthetic roots |
| `common::gc_shared_tests!` | 13 × 2 backends | Survival of lists, deep structures, continuations, `dynamic-wind`, records, strings and vectors; reclamation | `(gc)` |
| `gc_vm.rs` | 9 | `CallFrame.closure`, boxed upvalues, side tables, multiple values, retirement shapes | `(gc)` |
| `gc_tree_walker.rs` | 4 | Closure env, 50,000-deep suspended calls, `ContEnv` dedup | `(gc)` |
| `ephemerons.rs` | 15 | Weak semantics and #423 retirement | `(gc)` |
| `weak_continuation_tests.rs` | 7 | Weak-id fixpoint and pruning | Direct `collect` |
| `finished_forms_release_code.rs` | 9 | Code released while something can still run it (#338) | `(gc)` |
| `escape_from_primitive.rs` | 12 | Control flow out of re-entrant primitives | **No collection is requested** (0 `(gc)`); GC-relevant only by accident |
| `interpreter_api.rs` | 4 `(gc)` sites | Source-map pruning, documents | `(gc)` |

**Drift.** `gc_tree_walker.rs:38-48` (`collection_inside_higher_order_primitive`) says `map` re-enters through a nested
trampoline. Since #479, `map` is Scheme (`lib/scheme/base.sld:12-24`), so the test now exercises an ordinary outermost
collection, not deferral [V].

Every integration test places its `(gc)` by hand. Together they cover the shapes someone anticipated.

---

## 4. Remaining hazards

### 4.1 Above the outermost loop: unsafe today [V]

1. **[#605]: values returned to the host are unrooted.**
   - `eval_*` returns a bare `TaggedValue`.
   - `run_forms` keeps form *k*'s value in a local while form *k+1* runs (`lib.rs:483-528`, `value` at `:491,517`).
   - `eval_program_resilient` returns that local (`:298`).
2. **Host-created environments are unrooted between calls** (probe, tree-walker). `Environment::with_parent` is public
   (`environment.rs:571`), `backend()` exposes `Backend::eval(expr, env)`, and nothing roots an environment that only
   the host holds.
3. **Values a host reads from a rooted place become unrooted once that place changes** [I]. Examples:
   `global_env().get(name)` followed by a later `(set! name …)`, or a host-built datum kept for re-evaluation. This is
   the same class as #605, and the same fix (an owned handle) covers it.
4. **Collection is public.** Any crate can call `MarkSweepCollector::collect`, `run_mark_phase` or `Heap::sweep` with a
   partial root set. Today only tests do (`source_map.rs:421`, `heap/source.rs:229`, `weak_continuation_tests.rs:132`).
5. **Backend divergence, not GC.** `VmBackend::eval` ignores `env` (`backend.rs:675-693`), so the same host code roots
   differently on the two backends.

### 4.2 Nested loops: safe today by deferral; unsafe the moment one collects [V]

Each item below holds heap values on the Rust stack across a nested driver loop:

| Holder | Location | Value that would die |
|---|---|---|
| `with_globals` | `vm_state.rs:343-347` | The real global environment (`saved`) and everything only it reaches |
| `%parameterize-swap!` | `parameters.rs:195-236` | `params`, `vals`, `olds`. The old values are swapped *out* of the parameters, so `olds` is their only holder while a parameter-like procedure runs (`read_parameter`/`install_parameter`, `:241-275`) |
| `run_synchronously` | `registry.rs:99-120` | The resumable primitive's `state` across `apply_proc`/`eval_expr` |
| `force` fallback | `lazy.rs:115-135` | The promise `obj`, re-read and updated after the thunk |
| `call-with-values` fallback | `values.rs:30-48` | `consumer` across the producer call |
| Rust `member`/`assoc` with a comparator | `lists.rs:440,587` | `obj` and the list. Reachable only through `(patina internal lists)`: `base.sld` imports the two-argument forms as `%member`/`%assoc` |
| `call-with-port`, `call-with-*-file` (Rust versions) | `ports.rs:666`, `file.rs:216,263` | The port's `TaggedValue`. The port itself is held as an `Rc` |
| `ParsedLibrary.body` | `library_loader.rs:173-215` | Unevaluated forms |
| `desugar_with_imports` | `desugarer/mod.rs:1838-1841` | Partial IR |
| VM primitive arguments | `vm_state.rs:1989-1994` (`scratch_args` taken) | Arguments. They are also in caller registers, but retirement at a mid-instruction pc is unproven |
| Tree-walker nested trampolines | `cps_eval/mod.rs:213-221` | The suspended outer `StepResult` |
| Weak continuation tables | `gc_roots.rs:21-24` | A payload whose ref is held only in Rust |

Today's cost is memory, not safety:
- a library body never collects (5M conses churned in a library body: 1 collection and 116 MB, against 12.7 MB at top
  level; gc-impl.md §5.5);
- a run of `define-library` forms never collects ([#614]).

### 4.3 Liveness maps (#423): silent [V]

See §3.4. The risk is a codegen change, a new opcode or a new runtime landing.
- `written_register` is exhaustive (`pass5_codegen.rs:294-303`).
- `retirements` are positional (`:679-683`).
- A runtime landing at a reached pc inherits the compiler's union of predecessor paths.

None of these is checked at run time.

### 4.4 Code liveness [V]

- Frames hold `Rc<CodeObject>`, but constants are traced only through `code_store` (`gc_roots.rs:97-99`).
- A unit stays in the store while `live_closures > 0 || Rc::strong_count > 1` (`vm_state.rs:1003-1008`), decided after
  each collection from sweep's freed-closure ids (`:542-571`).
- If that rule ever released code that a holder outside `code_store`'s accounting can still run, its literals would go
  untraced at the next collection.
- Tests: `finished_forms_release_code.rs` (9), with `(gc)`. Runtime stubs carry no constants (`control.rs:1425-1446`).

### 4.5 `Rc` payloads [V/I]

Sweep drops payloads eagerly (`gc.rs:942-950`). The following Rust holders keep the *container* alive but not the
values inside it:
- an `Rc` clone of a record's `fields` (`records.rs:250-253`, under a shared heap borrow);
- a parameter's `values` (`get_parameter`, `heap/mod.rs:1820-1828`);
- a closure's `globals` (`get_vm_closure_globals`).

This is safe only within one primitive or a guarded window. `with_globals` is exactly this shape.

### 4.6 Raw bits and indices [V]

- **Transient raw-bits sets are safe** because no collection runs inside them. They are the desugarer's `seen`
  (`desugarer/mod.rs:129`), the quasiquote `active` set (`quasiquote.rs:119-123`), the expander's `seen`
  (`macro_expander/mod.rs:254`) and the compiler's `prim_calls.by_value` (`primitive_calls.rs:43,215`).
- **Persistent ones are maintained, or rely on non-movement:**
  - `syntax_sources` is pruned before reuse at sweep (`gc.rs:894-900`).
  - `SourceMap` is pruned at form boundaries from the capped freed-bits buffer (`lib.rs:509-512`). Inside one long
    form, stale entries persist until it ends; for diagnostics only.
  - `hash-by-identity` and the symbol table, together with `CallFrame.closure`, are correct only because indices never
    move.
- None of these is a premature-collection risk today. All of them break under evacuation.

### 4.7 Process-wide state and multiple interpreters [I]

- `PENDING_ESCAPE` is a `thread_local!` (`cps_eval/types.rs:20-42`), but each interpreter has its own heap. With two
  tree-walker interpreters nested on one OS thread, the wrong heap would trace it.
- `CompiledMacro.foreign_expansions` environments are not traced (`compiled_macro.rs:540`; trace rule
  `gc.rs:746-753`). This is sound while the generating library stays registered.

---

## 5. offheap.md §5 re-verified

These are the patterns counted in offheap §5.2 (A 19, B 51, alloc in loop 25, C 9, D 3). I re-read a sample.

| Site | Pattern | Today | Breaks under |
|---|---|---|---|
| `floor_div` (`arithmetic/division.rs:63-80`): bignum `q` held across `r`'s allocation, then `alloc_values` | A, B | **Safe.** One `borrow_mut`, no driver loop reachable | Collection inside allocation (excluded by GC_PRD §14) |
| `get_environment_variables` (`process_context.rs:150-171`): `key` across `alloc_string(v)`; `Vec` of fresh pairs across `list_from_iter` | A, C | Safe, same reason | The same |
| `add` fold (`arithmetic/basic.rs:21-35`): fresh accumulator into the next allocation | A | Safe | The same |
| `%parameterize-swap!` (`parameters.rs:195-236`) | D | Safe by deferral only | **A nested loop that collects** |
| `force` fallback (`lazy.rs:115-135`) | D | Safe by deferral only | The same |
| Reader frames, expander template `Vec`s, `map_syntax_identifiers_memo` (offheap §5.3) | A, C, raw bits | Safe: no driver loop | Collection inside allocation; the raw-bits memos also break under moving if they span a collection |

**Verdict.**
- None of the A, B, C or alloc-in-loop sites is a bug today.
- Under GC_PRD's model (allocation never collects; no collection beneath a `Cx`) they stay safe even with evacuation,
  because no value is live across a poll.
- The D sites are the real future hazard, and §4.2 lists them all, not only the three the scan found.

---

## 6. Which defense sees which hazard

| Hazard | Debug poison | Stress lane (debug) | Integration tests | Compile time |
|---|---|---|---|---|
| Missed root in a VM or TW provider, outermost path, pair or object | yes, if read before reuse | yes, for chibi-suite shapes at N=16 | the anticipated shapes | — |
| The same, vector or string | **no** | only if the output changes | partly | — |
| Freed slot already reused | **no** | only if the output changes | — | — |
| Dangling reference that is never read | **no** (absorbed by the collector) | **no** | — | — |
| Accessors bypassing the `Free` check | **no** | only if the output changes | `gc_vm.rs:21` | — |
| Embedding, above the loop (#605, child env) | yes, if read before reuse | **no** (CLI only) | **none** | — |
| Nested-loop holders (§4.2) | n/a today | n/a (never collects there) | `gc.rs:1749` unit test only | — |
| Wrong retirement map | **no** (`UNSPECIFIED` is legal) | only if the output changes | pinned shapes | — |
| Trace rule missing a variant | — | partly | partly | **yes** (exhaustive match) |
| Trace rule missing a *field* | partly | partly | partly | **no** |
| Weak-table rule violated | partly | partly | 7 + 15 | — |

---

## 7. What follows for "how do we prevent it"

The cheapest additions, ranked by how many §6 holes each closes. Each can land on today's collector, ahead of the
redesign.

1. **Root the embedding boundary.** Add `Owned` handles for `eval_*` and `run_forms` ([#605]; GC_PRD §11.5, stage 2).
   - Add the child-environment shape to #605's acceptance.
   - Make `Collector::collect`, `run_mark_phase` and `Heap::sweep` crate-private or `#[doc(hidden)]`, so only the two
     safe points can collect.
2. **Make every stale reference fail loudly in debug.**
   - Keep one debug-only freed bitset per arena, set at sweep and cleared at allocation, and assert in every accessor.
     That includes vectors, strings and the eight bypassing object accessors, and it fixes `frame_globals`' silent
     fallback.
   - Add a quarantine of one or two collections before a freed slot is reused (GC_PRD §16), so a stale read cannot hit a
     reused slot.
3. **Implement GC_DESIGN §11.5 now:** `debug_assert!(marks.set(idx))` when sweep pre-marks the free list. Every
   reachable edge into a freed slot then panics at the *next* collection, even if nothing ever reads it. It is
   essentially the PRD verifier's "no reachable dead word" check, for one line.
4. **Write a `DEAD_SLOT` sentinel in debug from `retire_registers`, and assert in `reg_at`** (GC_PRD invariant 3).
   This turns a wrong liveness map from a silent wrong value into a panic.
5. **Widen what stress reaches.**
   - Add a zeal mode that collects at *every* outermost safe point, not only after allocation (GC_PRD's `entry`).
   - Run `cargo test -p patina-tests` and both Larceny lanes under stress in the debug build, nightly if not per PR.
   - Do #587's callback and prompt generator under stress.
6. **Before any nested loop may collect (stage 4e),** move each §4.2 holder into the machine: `Step::Call` state, or a
   rooted scope. Also add a test that makes deferral observable: a parameter-like procedure that calls `(gc)` inside
   `parameterize`, with its old value reachable only from `olds`.
7. **Make the rule a type** (GC_PRD §11.3: `&mut Heap` collects, `Cx<'gc>` cannot, trybuild tests). Steps 1–6 make
   today's invariant checked; this step makes it unbreakable.

[#605]: https://github.com/avalonalex/patina/issues/605
[#614]: https://github.com/avalonalex/patina/issues/614
