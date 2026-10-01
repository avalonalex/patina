# Today's contract between the collector and the rest of Patina

Source: branch `gc-prd`, HEAD `f82e8e8`. That commit only adds `PRD/`, so every Rust line number below is also the
line at `main` `28a94f8`. Line numbers were checked against the tree on 2026-10-01. Issue titles and states come from
GitHub the same day; #603–#618 are all open.

This file describes the collector **as built**. `docs/GC_DESIGN.md` describes the same collector, but its header and
line references date from 2026-07-31. Where it and the code disagree, this file follows the code. For example,
GC_DESIGN §3.1 counts 26 object variants and the code has 28, and §5.2 still lists `value_buffer`. The target design is
`PRD/GC_PRD.md`. The last column of §1 and the "Redesign" line of each clause point to the PRD section that replaces
the clause.

Most of this contract is implicit: it sits in doc comments, in GC_DESIGN §5–§9, and in the fact that nothing has broken
it yet. Each clause therefore says what enforces it, if anything.

**Enforcement legend.**
- **T**: the type system or visibility. A violation does not compile.
- **R**: a runtime check in release builds. Examples are a `RefCell` borrow panic, a `try_borrow` that refuses to
  collect, and an `assert!`.
- **D**: a debug-build assertion or the debug poison. CI's `gc-differential-debug` lane runs these
  (`.github/workflows/ci.yml:109-120`).
- **L**: a named test, or a CI lane such as the off/default/stress differential (`scripts/run_gc_differential.sh`,
  `EXPECTED_TOTAL=1226`). A lane catches a violation only on paths the chibi suite exercises.
- **C**: convention only. A doc comment or design-doc rule that nothing checks.

Paths are crate-relative where unambiguous: `core:` is `crates/patina-core/src/`, `vm:` `crates/patina-vm/src/`, `tw:`
`crates/patina-tree-walker/src/`, `prim:` `crates/patina-primitives/src/`, `fe:` `crates/patina-frontend/src/`, `rt:`
`crates/patina-runtime/src/`, `interp:` `crates/patina-interpreter/src/`. `gc.rs` means `core:heap/gc.rs` and `mod.rs`
means `core:heap/mod.rs`.

---

## 1. Summary

| # | Clause | Party | Enforced by | Known violations and gaps | Redesign |
|---|---|---|---|---|---|
| P1 | Non-moving. A slot index is a value's identity for as long as the value is reachable | collector | C | none; it is what makes everything else work | §9.4 evacuation (stage 8), §9.8 identity hash |
| P2 | Collection runs only at an outermost backend safe point, never inside allocation. It is stop-the-world, whole-heap and synchronous | collector + drivers | T (structural), R, L | `(gc)` and the threshold wait while any deferral scope is open (#614) | §8, §12, §11.3 |
| P3 | Trigger: a pending flag, raised by a slot count crossing max(65,536, 2 × live slots) or by `(gc)`. A bare heap never triggers on its own | collector | L | object count, not bytes (#606, #615, #612, #607) | §15, stage 1 |
| P4 | Frees every slot unreachable from the roots plus the immortal symbol and core-syntax tables, and prunes the side tables keyed by those slots | collector | L | symbols immortal (#611); env side tables never pruned (#613, #614) | §9.1, §9.9, §17.2 |
| P5 | Tombstones dead slots, reuses them LIFO, and poisons in debug builds | collector | D, L | vector and string use-after-free is undetectable; release builds read reused slots silently (#605) | §9.3, §16 |
| P6 | Eager `Drop` at sweep is the only finalization. It breaks env cycles and flushes and closes ports, inside the pause | collector | L, C | no teardown (#604); no `EMFILE` retry (#607) | §9.6 (F1–F6), §6 |
| P7 | Weak semantics: SRFI 124 ephemerons and the VM's weak continuation ids, resolved in one fixpoint | collector | T (order), L | O(n²) rescans (#609) | §9.5, §9.9, §13 |
| P8 | Cost: pause ∝ arena high-water mark + full root sets; arenas never shrink; stats in slots | collector | — | #616 | §9.10, §7, §15, stage 5e |
| O1 | Safe-point owners place the safe point where every live value is in a provider and no heap borrow is open. They pass a complete root set, abort rather than pass a partial one, and ask their own guard whether they are outermost | VM, tree-walker drivers | R, L | root sets are closed arrays | §12, §11.3, §14 `RootSet` |
| O2 | Any Rust scope that holds values across an evaluation call holds a `GcDeferGuard` | everyone who re-enters | T (partly), R, D, C | deferred scopes never collect (#614) | §11.3 `NoGcScope`, §12, K16 |
| O3 | Root providers (`GcRoots`) are complete, use the typed visitor entries, dedup shared `Rc` graphs, do not recurse on data depth, and do not touch the heap | providers | T (shape), R, L, C | `foreign_expansions` untraced (latent) | §14 `RootProvider`/`SlotVisitor` |
| O4 | Weak-id rule: ids are heap-minted and unique, and the ref and its store entry are made in one dispatch. A handle is carried in a rooted register; `sweep_weak` touches only off-heap state | VM | T (privacy), L, C | capture bytes invisible (#606) | §13 design A (stage 4e) |
| O5 | VM registers: per-pc root maps, `retire_registers` before every collection and capture, windows filled before the next safe point, `CallFrame.closure` visited by index | VM compiler + runtime | T (exhaustive match), L, D | — | §11.1 frame invariants |
| O6 | VM code liveness through sweep's closure-death reports and `after_collection` | VM + collector | D, L | breaks under any collector that skips dead objects | §9.7 |
| O7 | Tree-walker: the whole machine state is the `StepResult`; every trampoline defers; `PENDING_ESCAPE` is a root | tree-walker | T (structural), L | `PENDING_ESCAPE` is per OS thread (latent) | §11.4 |
| O8 | Primitives: heap-only handlers are GC-atomic by signature; higher-order handlers re-enter deferred; resumable state is one value the machine roots; `(gc)` only requests; identity relies on P1 | `patina-primitives` | T, R, C | #607, #608, #618 | §11.3 `Prim`/`Step`, §11.2 helper classes, §9.8 |
| O9 | Frontend and macro expander are GC-atomic except across imports, which defer. Per-form memos may key on raw bits | `patina-frontend`, `patina-macros` | C, L | #611, #612, #613; #617 (Rust recursion) | §11.3 literal pool, decision 17, stage 4c |
| O10 | Raw-bits-keyed side tables are pruned through the freed-bits channel before slots are reused | frontend, `SourceMap`, heap | L, C | single-consumer channel (latent) | stage 4c, §17.2 |
| O11 | Library loading defers for the `ParsedLibrary`'s lifetime. The registry is a root and must be readable at the safe point | `patina-runtime` loader | T, R, L | #610, #614 | §11.3 points A–D, stage 2 |
| O12 | Environments: env → heap edges are bare values, heap → env edges are owning `Rc`s in slots, and every other env → env edge has its own trace hook | `Environment` authors | C, L | `heap` back-pointers (#604); strong `owners`/alias tables (#611, #613, #614); #615 | §11.6, §11.4, stage 4b |
| O13 | Embedding: nothing a host holds is rooted, and a dropped interpreter is never torn down | `patina-interpreter` and hosts | none | #605, #604, #618 | §11.5, decision 13, stages 2–3 |
| O14 | Adding a heap object type or heap tag: an exhaustive trace match, the right visitor entry, a decision on identity, and `Drop` at sweep | anyone extending `HeapObjectData` | T (variants), C (tags) | — | §6 `declare_layouts!` |
| O15 | A Rust struct storing `TaggedValue`s is reachable from a provider, or lives inside a GC-atomic call or a deferral scope, or is dead by the next outermost safe point | everyone | C, L | — | §11.3, §17.2 M2 |
| O16 | A `Collector` is non-moving, composes `run_mark_phase` → `Heap::sweep`, and is never asked anything at a safe point | collector implementers | T (partly), C | pluggability is nominal (one hard-coded collector) | §14, decision 24 |
| O17 | One heap per backend, one OS thread per heap (`!Send`), several heaps per thread, and thread-locals shared among those heaps | everyone | T, D | #618; `PENDING_ESCAPE` (latent) | §18, §12 `InterruptHandle` |

---

## 2. What the collector promises

### P1. Non-moving: a slot index is identity

- **Statement.** A live value never changes slot. `TaggedValue` is `index << 3 | tag`, and a reachable value keeps that
  word for its whole life. Everyone who keys on the word is relying on this: the intern table
  (`symbol_table: HashMap<String, HeapIndex>`, `mod.rs:319`), raw-bits maps (`syntax_sources`, `mod.rs:305`;
  `SourceMap.locations`), `eq?`'s raw compare (`values_eq`, `mod.rs:2118-2146`), and the identity hash, whose doc
  says "the heap index is a sound identity because the collector does not move objects" (`mod.rs:2532-2534`). SRFI 69
  stores that hash in Scheme vectors (`lib/srfi/69/srfi-69-impl.scm:118`), as do SRFI 125 and the R6RS tables.
- **Identity is not uniformly the slot.** `values_eq` compares `Procedure`, `RecordType` and `Record` by the `Rc`
  behind the slot (`mod.rs:2126-2142`), because primitives allocate several wrappers for one Rust entity, and
  `tagged_value_hash_identity` hashes that `Rc` address (`mod.rs:2548-2554`). `Port` has no such arm (#608).
- **Enforced by:** C. The `Collector` doc says "Implementations must be **non-moving**" (`gc.rs:199-207`).
  `GcVisitor::visit(tv: TaggedValue)` takes values by copy (`gc.rs:485`), so no provider could be updated even if a
  collector wanted to move.
- **Redesign:** evacuation behind a gate (§9.4, stage 8); a BFG identity hash (§9.8); raw addresses (§5). The
  tree-walker heap stays non-moving for good (§11.4, decision 1).

### P2. Collection happens only at an outermost safe point, never inside allocation

- **Statement.** The collector runs only when a driver calls `GcController::safe_point` with the pending flag raised
  and `is_outermost` true (`gc.rs:381-395`). It then takes one `heap.borrow_mut()` for the whole collection
  (`gc.rs:405-408`). There are two callers:
  - the VM, at the top of every dispatched instruction (`vm:runtime/vm_state.rs:1196-1209`, `maybe_collect`
    `:1287-1310`);
  - the tree-walker, at the top of every trampoline step (`tw:eval/cps_eval/mod.rs:227-230`, `maybe_collect`
    `:114-133`).

  It is stop-the-world, single-threaded, whole-heap (no generations) and synchronous: no incremental work, no
  background thread.
- **Allocation never collects.** Every `alloc_*` calls `note_alloc`, which only raises a flag (`mod.rs:576-584`).
  `alloc_pair` documents the consequence: callers may hold partial structures across `alloc_*` calls with no root
  (`mod.rs:698-701`). `list_from_iter_with_tail` relies on it explicitly (`mod.rs:2925-2931`). `(gc)` only requests
  a collection, because "a primitive runs mid-evaluation, where live values sit in Rust locals"
  (`prim:primitives/gc.rs:1-6,37-40`; `Heap::request_gc`, `mod.rs:603-611`).
- **Enforced by:**
  - T, structurally. `Heap` holds no reference to a `GcController` or to any root provider. `note_alloc` and every
    primitive (which receives only `&SharedHeap`) therefore *cannot* collect; only the owners of a controller (`VmState`,
    `Evaluator`) can.
  - R. The `borrow_mut` in `safe_point_cold` panics if any heap borrow is open at the safe point.
  - L. `safe_point_collects_only_when_outermost_and_pending` (`gc.rs:1749`), and the stress lane byte-identical to the
    off lane on both backends.
- **Gaps.**
  - A request made inside any deferral scope (O2) waits until control returns to the outermost loop. A run of
    `define-library` forms never reaches one (#614).
  - The VM's per-instruction poll costs about 1% against a control binary with no poll (GC_DESIGN §6.1).
- **Redesign:** keeps "allocation never collects" (§8; bet B2, §4; kill criterion K16). One poll word folded into the
  stack limit (§12). Collection reachable only from the driver's `&mut Heap` (§11.3). `(gc)` collects at its call
  (§9.5).

### P3. Trigger and pacing

- **Statement.**
  - `note_alloc` counts **one per object, whatever its size**, and raises `gc_pending` once
    `allocs_since_gc >= gc_threshold` (`mod.rs:570-584`).
  - The threshold is the mode made concrete (`GcController::current_threshold`, `gc.rs:342-348`): `usize::MAX` for
    `Off` (`PATINA_GC=0`, testing lanes only); `max(65,536, 2 × live slots after the last collection)` for `On`
    (`gc.rs:969`, `:1001-1003`); `n` for `Stress(n)` (`PATINA_GC_STRESS`), which bypasses the adaptive floor.
    `GcMode::from_env` reads the mode (`gc.rs:301-315`).
  - Pairing a heap with a controller installs the threshold (VM: `vm_state.rs:231-233`; tree-walker:
    `tw:eval/mod.rs:97-102`). `GcController::collect` re-installs it after each collection (`gc.rs:350-357`).
  - `request_gc` raises the flag in every mode. Sweep lowers it (`gc.rs:956-957`).
  - **A bare heap is inert.** Its threshold defaults to `usize::MAX` (`mod.rs:545`), so a heap with no controller
    (`patina-compat`'s reader, `new_shared_heap()`) never collects. Even `(gc)` there only raises a flag that no one
    reads.
  - The flag is an `Rc<Cell<bool>>` outside the heap's `RefCell` (`mod.rs:389-395`), so a safe point is one load with
    no borrow (`gc_pending_handle`, `:589-591`).
- **Enforced by:** L. `alloc_crossing_threshold_raises_pending_flag`, `request_gc_raises_pending_flag_in_any_mode`,
  `controller_collect_rearms_adaptive_threshold` (`gc.rs:1667,1687,1717`), and the two reclamation proofs in
  `scripts/run_gc_differential.sh:139-175`.
- **Violations.** The trigger sees neither payload bytes nor Rust-side memory:
  - #606: 500 × `(make-vector 100000)` gives 414 MB and 0 collections; continuation snapshots reach 3.2 GB.
  - #615: an `environment` call's ~44 KiB of Rust tables counts as one allocation.
  - #612: provenance chains in `syntax_sources` are not counted.
  - #607: descriptors are not counted.
- **Redesign:** bytes plus external bytes, with a whole-heap interval of `max(8 MiB, 2·L)` (§15; stage 1 brings the
  byte trigger to today's collector). M3 makes Rust bytes kept alive by heap objects count (§17.2).

### P4. Reachability, and what a collection frees

- **Roots** are exactly:
  - the providers passed to that collection (O1, O3);
  - **every interned symbol and every core-syntax marker**, pre-marked in `GcVisitor::new` (`gc.rs:449-464`).

  Symbols and syntactic keywords are therefore immortal. The comment calls this "a heap invariant, not policy": a
  dangling intern index would break any collector.
- **Trace rules** are per `HeapObjectData` variant (`gc.rs:695-794`):
  - leaves: BigInt, Rational, Real, Symbol, Bytevector, Port, RecordType, Identifier, PromptTag, LabelPlaceholder,
    CoreSyntax, Free (`:698-709`);
  - weak: Ephemeron, VmContinuationRef, VmDelimitedContinuationRef (`:717-728`, P7);
  - everything else traced, including `Rc<Environment>` payloads through `visit_env` (`VmClosure.globals`,
    `EnvironmentSpecifier`, CPS lambda env, `Macro.definition_env`) and CPS body literals through
    `visit_expr_literals` (`:739-789`).
- **What is freed.** Every unmarked slot in all four arenas (`Heap::sweep`, `gc.rs:891-961`). Before any slot is
  reused, the collector also:
  - prunes `syntax_sources` to marked keys (`gc.rs:892-900`);
  - lets `GcRoots::sweep_weak` drop dead continuation payloads (`gc.rs:1096-1098`);
  - records freed raw bits (when enabled; capped at 65,536, then `Overflowed`, `gc.rs:854-873`) and freed `VmClosure`
    code ids (`gc.rs:945-949`).

  Sweep completion is the "collection happened" boundary. It resets `allocs_since_gc`, lowers the flag and counts the
  collection (`gc.rs:956-960`).
- **Enforced by:** L. `interned_symbols_are_immortal` (`gc.rs:1498`), the cycle and reachability unit tests
  (`gc.rs:1321-1500`), and the reclamation proofs.
- **Violations.**
  - #611: each `guard` expansion leaks three immortal symbols plus alias entries.
  - Rust tables that hold heap values and are never pruned keep them reachable for ever: `owners` (#614), alias tables
    (#611), scoped tables (#613). See O12.
- **Redesign:** a weak symbol table (§9.5 key-liveness row, SD2, stage 5c); the fixed epilogue order (§9.9); M1/M2
  (§17.2).

### P5. Tombstones, slot reuse, and use-after-free detection

- **Statement.** `sweep_arena` first marks every free-list index, so a free slot is never pushed twice, then walks
  every slot (`gc.rs:828-852`). Each dead slot is pushed onto its arena's LIFO free list and tombstoned:
  - pairs: `(GC_POISON, GC_POISON)` in debug builds only; release skips the store (`gc.rs:907-916`);
  - vectors and strings: `Vec::new()`;
  - objects: `HeapObjectData::Free`.

  Allocation pops from the end of the free list (`alloc_pair`, `mod.rs:705`).
- **Detection.** In debug builds:
  - `get_pair`, `set_car` and `set_cdr` assert against the poison (`mod.rs:718-762`);
  - `get_object` asserts against `Free` (`mod.rs:1484-1494`);
  - a vector or string tombstone is a legal empty value, so a use-after-free there goes undetected (`gc.rs:884-886`).

  GC_DESIGN §11.5's "paranoid pre-sweep assertion" is not implemented. Because sweep pre-marks free-list slots, a root
  that points at a freed slot is silently absorbed (`understand/gc-impl.md` §7).
- **Enforced by:** D, plus the debug-poison CI lane; `swept_pair_access_panics_in_debug` (`gc.rs:1784`).
- **Violations.** #605: a host-held value reads back as `(45264 . 45264)` in release builds and panics in debug.
- **Redesign:** lazy metadata-only sweep that never reads a dead object (§9.3); an eager poisoning sweep, a quarantine
  and a `DEAD_SLOT` fill in the poison and zeal lanes (§16).

### P6. Eager `Drop` at sweep is the only finalization

- **Statement.** Writing the tombstone drops the old payload inside the pause. That is load-bearing (GC_DESIGN §8):
  - **cycle breaking.** Env → heap edges are bare indices and heap → env edges are owning `Rc`s inside slots, so an
    unreachable closure–environment cycle dies when its slot is tombstoned;
  - **port flush and close.** A dead file port's `BufWriter` flushes and its file closes on drop. A thread-local weak
    list of open output files (`OUTPUT_FILES`, `core:port.rs:170-185`) is flushed by `end_process`
    (`rt:exit_status.rs:62-63`), because `process::exit` runs no destructors.

  There are no Scheme-level finalizers, guardians or will executors. Drop order within a sweep is arena order and is
  not specified.
- **Enforced by:** L. `tombstone_drops_rc_payload_breaking_env_cycle` (`gc.rs:1429`). Otherwise C.
- **Violations.**
  - #604: nothing tears the heap down when an interpreter is dropped. An `Rc` cycle (`Environment.heap`,
    `CompiledMacro.heap`, O12) keeps the heap alive, so a still-reachable port is never flushed. The CLI escapes only
    through `end_process`.
  - #607: an open that fails with `EMFILE` does not collect and retry, although a collection would close the dead
    ports.
  - The cost lands inside the pause: the post-load 178 ms pause is mostly releasing 2.9 M `Drop` payloads (PRD §1
    item 1).
- **Redesign:** a `FinalRegistry` run after the pause and before Scheme resumes, with F1–F6 (§9.6, decision 12,
  decided); no `Drop` payload in any object (§6); teardown finalizes everything (F5, §11.5).

### P7. Weak semantics

- **Statement.**
  - **Ephemerons (SRFI 124).** Neither field is traced on arrival; the pair is queued (`gc.rs:711-717`). In the
    fixpoint, a pair whose key is already marked "by some other path" has its key and datum traced
    (`gc.rs:1062-1078`). Pairs still pending at quiescence are broken: both fields cleared, `ephemeron-broken?` ⇒ `#t`
    (`gc.rs:1090-1094`; `Heap::break_ephemeron`, `mod.rs:1231-1245`). An immediate key never breaks
    (`value_is_live`'s `unwrap_or(true)`, `gc.rs:528-534`). Symbol keys never break either, because symbols are
    immortal (P4).
  - **VM continuation ids.** Marking a `VmContinuationRef` or `VmDelimitedContinuationRef` records its id
    (`gc.rs:719-728`). `run_mark_phase` broadcasts each new batch to every provider's `trace_weak_ids`.
  - **Ordering.** Both kinds run in **one** loop (`gc.rs:1050-1088`), because each can feed the other. A sequenced
    version was a use-after-free (commit `1d18c49`). `sweep_weak` runs after the fixpoint and before the heap sweep.
  - There are no weak pairs, weak boxes, weak or ephemeron hash tables, or guardians.
- **Enforced by:**
  - T. `run_mark_phase` is the only public mark phase, so a collector cannot reorder its interior (`gc.rs:1012-1021`).
    `value_is_live` is private, so a provider cannot ask "is this live?" mid-trace, which would be a use-after-free
    (`gc.rs:518-527`).
  - L. `ephemerons.rs` (15 tests), `weak_continuation_tests.rs` (7), `dead_captures_are_pruned_from_side_table`
    (`crates/patina-tests/tests/gc_vm.rs:79`), and Larceny's ephemeron suite (6/6, both backends).
- **Violations.** #609: every round rescans every pending ephemeron (`pending.retain`, `gc.rs:1063`). A 16,000-link
  chain costs about 220 ms in one order and 1 ms in the other.
- **Redesign:** key-indexed resolution (§9.5); one fixpoint that later also covers host payloads and guardians (§9.9
  step 2). Design A deletes the weak continuation tables (§13, stage 4e).

### P8. Cost model and observability

- **Statement.**
  - Every collection allocates mark bit sets sized to each arena's length (`MarkBits::for_heap`, `gc.rs:117-124`) and
    sweeps every slot up to the high-water mark (`gc.rs:840`). Arenas and free lists are `Vec`s that nothing shrinks.
  - The symbol table, every loaded code object's constants, every environment and every live tree-walker procedure's
    CPS body are re-walked in full at every collection.
  - `GcStats` (with `last_pause_micros`) is returned by `collect`, but no caller reads it. `(gc-stats)` reports slot
    and free-list counts, `allocs-since-gc`, `collections` and `last-swept` (`prim:primitives/gc.rs:41-57`); it reports
    no bytes and no pause.
- **Enforced by:** nothing. This is a property, not a rule.
- **Violations.** #616: after a dropped peak, every later collection pays the peak's sweep (2,265 of 2,287 collector
  samples in `sweep_arena`), and the peak's memory stays resident.
- **Redesign:** a pause budget with no term in dead objects or heap size (§9.10); decommit with hysteresis (§7, 5e);
  byte keys and in-pause work counters in `(gc-stats)` (§15, §17.2).

---

## 3. What every other party must do

### O1. Safe-point owners (the two drivers)

- **Placement.** A safe point sits where every live Scheme value is reachable from the root providers about to be
  passed, and no heap `RefCell` borrow is open:
  - VM: "all live state is on `VmState`, capture temporaries are dead, buffers are restored, and no heap borrow is
    outstanding" (`vm_state.rs:1197-1203`);
  - tree-walker: "all live state is in `current_step` and `expr`" (`tw:eval/cps_eval/mod.rs:227-229`).
- **Root sets.** Each driver supplies a **closed** list:
  - VM: `[state, registry]` (`vm_state.rs:1305-1308`);
  - tree-walker: `[evaluator, registry, EscapeRoots, StepRoots{step, expr}]` (`tw:eval/cps_eval/mod.rs:129-130`).

  There is no registration API. A new holder of values must be wired into an existing provider.
- **Abort, never trace partially.** `with_roots` may return without calling `collect`. The flag stays raised and the
  next safe point retries (`gc.rs:366-372`). Both drivers abort when the library registry is mutably borrowed (VM
  `LibraryRegistry::try_roots`, `rt:library_registry.rs:577-594`; tree-walker `try_borrow`,
  `tw:eval/cps_eval/mod.rs:126`).
- **Outermost.** Each driver asks **its own guard** (`GcDeferGuard::is_outermost`, `gc.rs:252-261`) rather than
  comparing the depth with a literal. The VM also retires dead registers first (O5) and runs `after_collection` when
  the flag went from raised to lowered (O6).
- **Enforced by:**
  - R. An open heap borrow makes `safe_point_cold`'s `borrow_mut` panic; the registry check is a `try_borrow`.
  - L. Stress lanes, both backends, release and debug-poison.
  - C. That the root set is complete.
- **Gaps.** The tree-walker restates the registry rule instead of calling `try_roots`; that is harmless today.
- **Redesign:** open `RootSet::register` returning `RootToken` (§14); the poll slow path services events only with the
  `&mut Heap` capability (§11.3, §12).

### O2. Deferral: `GcDeferGuard` around every scope that holds values across an evaluation call

- **Statement.** The predicate is: "does this Rust frame hold heap values that must survive across an evaluation
  call?" (GC_DESIGN §5.2). If so, hold a `GcDeferGuard` (`gc.rs:219-268`). Any safe point opened inside it is not
  outermost, and so never collects.
- **Sites:**
  - every VM dispatch loop (`vm_state.rs:1151`), so every nested loop (`execute_nested`, `run_apply_proc`, library
    bodies, `Step::Eval` expansion) is deferred by construction;
  - `VmState::with_globals` (`vm_state.rs:343`);
  - `Desugarer::desugar_with_imports` (`fe:desugarer/mod.rs:1841`);
  - `ParsedLibrary` for its whole lifetime (`rt:library_loader.rs:195,208`);
  - every tree-walker trampoline (`tw:eval/cps_eval/mod.rs:219`).
- **Rules of use.**
  - The guard's constructor and its `Drop` each take `heap.borrow_mut()`, so a guard must not be created or dropped
    while a heap borrow is open (`gc.rs:224-225`).
  - It is RAII, so `?` cannot leak an increment.
  - An unbalanced exit trips `debug_assert!` (`mod.rs:674-680`).
  - VM `control.rs` states the rule for a future compiled driver: "A compiled driver must use the same
    guard/safe-point discipline and the existing `VmState` root provider, and publish all live Scheme values before
    servicing a safe point" (`vm:runtime/control.rs:23-29`).
- **Enforced by:**
  - T, partly. `ParsedLibrary`'s guard is a private field, so `ParsedLibrary::new` is the only constructor and cannot
    forget it (`rt:library_loader.rs:191-215`). Every driver loop takes a guard unconditionally.
  - R and D, as above.
  - C. That a *new* Rust scope holding values across re-entry takes a guard. Stage 3's missed library-loading guard is
    the precedent: bootstrap died at the first collection (GC_DESIGN §5.2).
- **Violations and cost.** Deferred scopes never collect:
  - a library body that churns 5 M conses gets 1 collection and keeps a 116 MB arena; the same code at top level gets
    76 collections and 12.7 MB (`understand/gc-impl.md` §5.5);
  - loading 25 R7RS-large libraries peaks at the same RSS with GC on and off;
  - #614: a run of `define-library` forms never reaches a safe point.
- **Redesign:** `NoGcScope` by type beneath a `Cx`, confined, counted and bounded by K16 (§11.3, §12 "Nested Rust
  loops"); loading collects at points A, B and D (§11.3). Nested VM loops may collect after stage 4e.

### O3. Root providers (`GcRoots`)

- **The trait:** `trace_roots(&self, &mut GcVisitor)`, plus `trace_weak_ids` and `sweep_weak`, both default no-ops
  (`gc.rs:177-197`).
- **Implementors:**
  - `VmState` (`vm:runtime/vm_state/gc_roots.rs:70`);
  - `StepTracer` (`vm:tracer.rs:283`);
  - `LibraryRegistry` (`rt:library_registry.rs:569`);
  - the tree-walker's `Evaluator`, `EscapeRoots` and `StepRoots` (`tw:eval/cps_eval/gc_roots.rs:29,42,58`).
- **Obligations.**
  1. **Complete.** A missing root is a use-after-free (GC_DESIGN §5: "Missing any 'yes' row is a use-after-free").
  2. **Use the entry that matches the holder:**
     - `visit` or `visit_slice` for `TaggedValue`s;
     - `visit_object_index` for a bare `HeapIndex` (`gc.rs:510-516`; `CallFrame.closure`);
     - `visit_env` for an `Rc<Environment>` (`gc.rs:539-559`);
     - `visit_continuation` and `trace_cont_env`/`trace_cont_value` for tree-walker continuation structures;
     - `visit_expr_literals` for a `CpsExpr` (`gc.rs:642-646`);
     - `visit_library` (`gc.rs:629-634`);
     - `visit_wind_with`/`visit_winds_with` with a handler callback (`gc.rs:600-626`).
  3. **Dedup `Rc`-shared structures** with `visit_once(identity)` (`gc.rs:570-582`). Without it, the tree-walker's
     `ContEnv` trace was exponential: 6.8 s for one collection at depth 26 (GC_DESIGN §9.4).
  4. **Do not recurse on data depth.** Dedup does not bound the Rust stack. `trace_cont_env` queues an O(1) snapshot
     on the visitor's worklist (`gc.rs:1130-1141`) after Larceny family 6 overflowed. `visit_env`'s alias and owner
     edges still recurse (`gc.rs:554-556`), and `visit_expr_literals` recurses to program depth (`gc.rs:636-641`).
  5. **Do not touch the heap.** `trace_roots` runs under the collection's `borrow_mut`, so a provider that borrows
     the `SharedHeap` panics. `sweep_weak` "must not touch the heap — it may only drop side-table payloads"
     (`gc.rs:191-196`). A provider's own `RefCell`s must be free at the safe point; the tracer relies on this
     (`vm:runtime/vm_state/gc_roots.rs:110-114`).
- **Enforced by:**
  - T for the shape: `&self`, and values passed by copy.
  - R for heap borrows.
  - L: stress and debug-poison lanes, `gc_vm.rs` (9 tests), `gc_tree_walker.rs` (4), `gc_shared_tests!` (13 × 2),
    and the depth-50,000 test `collection_at_deep_call_depth_preserves_suspended_values`
    (`crates/patina-tests/tests/gc_tree_walker.rs:50`).
  - C for completeness.
- **Gaps.**
  - `CompiledMacro.foreign_expansions: Vec<(ScopeId, Rc<Environment>)>` (`core:compiled_macro.rs:540`) is not visited
    by the `Macro` arm (`gc.rs:746-754`). It is latent: those environments are normally library environments the
    registry roots, but #614 shows a replaced library leaving the registry.
  - No provider can update a value, so P1 is load-bearing.
- **Redesign:** `RootProvider::trace(pass, &mut dyn SlotVisitor)` with writable `slot`s, `pinned` for tree-walker
  holders, and `host(id)` edges (§14, §11.4).

### O4. The weak-id rule (VM continuation side tables)

- **Statement.**
  - A continuation's payload lives off-heap in `continuation_store` or `delimited_continuation_store`; the heap holds
    only `VmContinuationRef(id)`.
  - **Ids are minted by the `Heap`** from one counter shared by both kinds and by every `VmState` on that heap, and are
    never reused (`mod.rs:425-430`, `:1408-1444`). An id therefore names at most one store entry, which is what makes
    broadcasting every batch to every provider sound (`gc.rs:180-188`).
  - The ref object and its store entry are created back-to-back within one instruction dispatch
    (`vm_state.rs:630-655`). Capture and invocation touch a store only within one dispatch, and nested loops defer, "so
    at a collecting safe point an unmarked ref proves its payload unreachable"
    (`vm:runtime/vm_state/gc_roots.rs:21-24`).
  - Code that runs thunks must carry "the heap continuation *handle* in a rooted register across thunk execution, not
    just an `Rc` payload" (`vm:runtime/control.rs:30-31`).
  - `trace_weak_ids` skips ids it does not own (`gc_roots.rs:117-130`). `sweep_weak` prunes entries whose id was never
    reached and shrinks a store under ⅛ full (`gc_roots.rs:132-152`).
- **Enforced by:** T for the privacy of `value_is_live` and the fixed order of `run_mark_phase`; L for
  `weak_continuation_tests.rs` and `gc_vm.rs:79`; C for one-dispatch atomicity and the handle-in-a-register rule.
- **Violations.** #606: a capture copies the whole register file and frame stack (about 160 KB at depth 1,000), but
  the trigger counts one slot; 80 K captures reach 5.7 GB.
- **Redesign:** continuations become one immutable, byte-accounted heap object traced word by word (design A, §13,
  stage 4e). That deletes both tables, `trace_weak_ids`/`sweep_weak` on the VM, and the reason nested VM loops must
  defer (§11.3).

### O5. VM register-file precision

- **Statement.**
  - **The compiler** emits per-pc may-root bitsets (`register_roots`, `vm:compiler/pass5_codegen.rs:143,156,1177,1188`;
    dataflow at `:223-286`). Each code object gets one bitset per instruction.
  - **Retirement.** Completed expression temporaries are retired at the pc after the expression (`:666-682`).
    Unreachable pcs are fully conservative (`:274-279`). Runtime stubs carry `register_roots: None`, so all their slots
    are conservative (`vm:runtime/control.rs:1442`, `vm_state.rs:252`).
  - **Before every collection**, `retire_registers` overwrites every non-root slot of every frame with `UNSPECIFIED`
    (`vm_state.rs:1291`; `vm:runtime/vm_state/gc_roots.rs:45-68`). It does the same to each continuation snapshot at
    capture (`vm_state.rs:637,650`). The whole register file is then traced as one slice (`gc_roots.rs:75`). Clearing
    rather than skipping means no snapshot or tracer view keeps an untraced pointer.
  - **Windows.** `push_frame` fills a new window with `NULL`, and the "caller fills operands before the next safe
    point" (`vm:runtime/execution_state.rs:52-74`).
  - **Locals stay conservative** for the whole frame. `reference-barrier` relies on that
    (`crates/patina-tests/tests/ephemerons.rs:78`).
  - **Other roots.** `CallFrame.closure` is a bare `Option<HeapIndex>` (`vm:types/mod.rs:50`), visited with
    `visit_object_index`. `scratch_args` is empty at safe points but still rooted. `pending_escape` is a root while
    set (`gc_roots.rs:76-87`).
- **Enforced by:**
  - T. `written_register` is an exhaustive match with no wildcard (`pass5_codegen.rs:297-354`), so a new instruction
    does not compile until it declares whether it writes a register.
  - L. `ephemerons.rs:94-162` (tail-call window retirement, continuation retirement, delimited snapshots) and
    `gc_vm.rs:120-173`.
  - D. The debug-poison lane.
  - C. That retirement ranges cover only completed temporaries. A wrong map overwrites a live value with
    `UNSPECIFIED`: wrong output, not a use-after-free.
- **Redesign:** the 40 B frame header, dense maps at every suspension point, clearing with `DEAD_SLOT`, and a verifier
  (§11.1, invariants 1–5). These are the JIT's tier contract (§11.2).

### O6. VM code liveness through the collector

- **Statement.**
  - A closure names its code by `CodeObjectId`, not by pointer, so a closure's death reaches the code store only
    through the collector.
  - The VM opts in with `enable_gc_freed_closure_tracking` (`vm_state.rs:234`; `mod.rs:628-633`). Sweep records the
    `code_id` of every freed `VmClosure` (`gc.rs:945-949`).
  - The driver detects that a collection happened from the flag transition, `pending && !gc_pending`
    (`vm_state.rs:1204-1207`), and runs `after_collection`. That decrements `live_closures` and releases every unit no
    frame, continuation or closure can run (`vm_state.rs:536-571`).
  - `eval`'s one-shot closures are retired eagerly with the `RETIRED_VM_CLOSURE_CODE` sentinel (`mod.rs:1340-1363`).
  - `code_store` constants are roots for as long as their unit is loaded (`gc_roots.rs:91-99`).
- **Enforced by:** D (`debug_assert!` that a freed closure's code counts a live closure, `vm_state.rs:550-556`) and
  L (`finished_forms_release_code.rs`, 9 tests).
- **Gap.** The protocol depends on the collector **visiting dead objects**. A copying nursery or a lazy sweep would
  never report them (`understand/vm-runtime.md` §0 item 7).
- **Redesign:** code units are released when none of their descriptors is marked; an `escaped` bit preserves the
  eager-release contract (§9.7).

### O7. Tree-walker

- **Statement.**
  - The entire machine state at a safe point is `current_step: StepResult` plus the trampoline's entry `expr`.
    `StepRoots` traces every variant (`tw:eval/cps_eval/gc_roots.rs:49-119`), and `Evaluator` roots the global
    environment (`:29-33`).
  - `EscapeRoots` roots the `PENDING_ESCAPE` thread-local, "a value and continuation in flight … reachable from nowhere
    else" (`:35-46`; `tw:eval/cps_eval/types.rs:20-40`).
  - Every trampoline takes a guard (O2), so only the outermost run collects (`tw:eval/cps_eval/mod.rs:213-230`).
  - `ContEnv` chains are queued, not recursed (O3.4). Every live procedure's CPS body literals are walked at every
    collection (`gc.rs:739-745`).
- **Enforced by:** T, structurally (the only driver loop takes a guard); L (`gc_tree_walker.rs:38,50`, the shared GC
  tests, both chibi and stress lanes).
- **Gap (latent, [I]).** `PENDING_ESCAPE` is per OS thread, not per heap. If one interpreter's escape were in flight
  while another on the same thread collected, the other heap would mark a foreign index. No safe point falls inside
  that window today.
- **Redesign:** the tree-walker is kept, may lag, and stays whole-heap and non-moving. Its Rust-side holders are
  reported through `SlotVisitor::pinned` (§11.4, decision 1).

### O8. Primitives (`patina-primitives`)

- **Heap-only handlers** are `fn(&SharedHeap, &[TaggedValue])` (`prim:registry.rs:10-14`). They receive no
  `ApplyContext`, so they **cannot re-enter Scheme and cannot reach a safe point**. That makes them GC-atomic by
  signature: they may hold values in locals, `Vec`s and raw-bits `HashSet`s, and their argument slice may point into
  the VM register file.

  Interior references (`vector_slice`, `get_string_chars`, `get_bigint`) are tied to a `heap.borrow()`. Allocating
  while one is held panics with `BorrowMutError` (AGENTS.md "RefCell borrow discipline").
- **Higher-order handlers** take an owned `Vec` because re-entry may reallocate the register file (`registry.rs:16-21`).
  Their `apply_proc`/`eval_expr`/`load_scheme_library` calls run in a nested, deferred loop, so values held across
  them survive. Live callers remain:
  - `%parameterize-swap!` on a parameter-like procedure (`prim:primitives/parameters.rs:250,273`);
  - the `environment` family through `load_scheme_library`.

  The VM side holds a raw `*mut VmState` (`vm:runtime/control.rs:2130-2132`).
- **Resumable primitives** hand calls to the machine (`Step::Call`/`Step::Eval`, `registry.rs:23-56`). Their only
  state across a call is **one `TaggedValue`**, and `ResumableResume` is a plain `fn` pointer that can capture nothing
  (`registry.rs:72-79`). The machine keeps the state where the collector sees it: the VM's `resume_stub` frame (whose
  registers are conservative), or the tree-walker's `ContValue::ResumePrimitive` (`gc.rs:1183-1189`).
- **`(gc)` requests; it never collects.** That follows from P2.
- **No write-barrier obligation.** Record fields, parameter stacks and promise cells are written in place through
  `Rc<RefCell<…>>`, outside any heap API (`prim:primitives/records.rs:262`, `parameters.rs:168,267`, `lazy.rs:134`).
  This is fine for a whole-heap collector. A generational collector would have to treat these writes as barriered
  stores.
- **Identity.** `eq?` and `hash-by-identity` follow P1. A primitive that allocates a fresh wrapper for an existing
  Rust entity breaks `eq?`, unless `values_eq` has an `Rc::ptr_eq` arm for that type.
- **Enforced by:** T for re-entry by handler kind and for resumable state; R for interior references; C for identity.
- **Violations.**
  - #608: `(current-output-port)` allocates a new `Port` wrapper per call (`prim:primitives/io/ports.rs:444,463,482`),
    and `values_eq` has no `Port` arm.
  - #618: the current ports are `thread_local!` statics (`ports.rs:41-45`), shared by every interpreter on the thread.
  - #607: the open primitives turn `EMFILE` straight into a `file-error` (`prim:primitives/io/file.rs:24,51,78,105`).
- **Redesign:**
  - `Prim = for<'gc> fn(&mut Cx<'gc>, &[Value<'gc>])`; no `Cx` method can collect; `Step` gains `Collect` and
    `CollectAndRetry` (§11.3);
  - `Leaf`/`Transfer` helper classes (§11.2);
  - the store funnel `Cx::store` (§10);
  - canonical ports (§9.6 F6);
  - the identity hash (§9.8).

### O9. Frontend and macro expander

- **GC-atomic.** Reading, desugaring and `syntax-rules` expansion run no Scheme, so no safe point is reachable while
  they hold values in Rust locals. That covers the parser's frame stacks and `labels`, the expander's `MatchEnv`, and
  the partial `CoreExpr`. The code says so: "No GC runs while desugaring; clear on exit so raw indices never outlive a
  form" (`fe:desugarer/mod.rs:370-372`).

  The one exception, an import that runs a library body mid-form, is covered by `desugar_with_imports`' guard
  (`fe:desugarer/mod.rs:1831-1841`).
- **Literals.** `CompiledMacro` pattern and template literals and `definition_env` are traced only through a live
  `Macro` heap object (`gc.rs:746-754`). `CompiledMacro.heap` must be the same heap: the field comment requires it
  (`core:compiled_macro.rs:478-483`), and GC_DESIGN §9.6's suggested `Rc::ptr_eq` assertion was never added. VM code
  literals live in `CodeObject.constants` (O6). Tree-walker literals live in `CpsExpr`, which `visit_expr_literals`
  reaches.
- **Enforced by:** C (atomicity is a consequence of P2, not a check); L (hygiene matrix, stress lanes).
- **Violations.**
  - #611: an alias and its interned symbols are minted per top-level expansion, and kept for ever (P4, O12).
  - #612: `stamp_expansion_source` rebuilds provenance chains in `syntax_sources` without bound, invisible to the
    trigger.
  - #613: macro-introduced definitions are scanned linearly, and their tables never lose entries.
  - #617 (not a GC defect, but the same layer): Rust recursion in the desugarer, scope resolution and compilers aborts
    at about 1,000 nesting levels. There is no native-stack contract.
- **Redesign:**
  - the `CoreExpr` literal pool, scoped to one compilation (§11.3, M2);
  - identifiers as ids and inline provenance (stage 4c, decision 16);
  - syntax-case after stage 5 with an `ExpansionContext` root provider, which ends the expander's GC-atomicity
    (decision 17);
  - a depth guard (§17.3, §19 "now" row).

### O10. Raw-bits-keyed side tables

- **Statement.**
  - **`syntax_sources`** is not a root. Sweep prunes it to marked keys before any slot is reused
    (`gc.rs:892-900`).
  - **`SourceMap.locations`** is pruned through the freed-bits channel. The parser enables recording
    (`fe:parser/mod.rs:209,230`), and the consumer drains it before each form (`prune_freed_locations`,
    `core:source_map.rs:255-261`; callers `interp:lib.rs:512`, `crates/patina-repl/src/program_stream.rs:147`).
    `Overflowed` (past 65,536 entries) means "treat the whole map as stale".
  - **Transient raw-bits sets** (datum-writer cycle labels, `check_import_datum`, the desugarer's `quoted` memo) are
    safe only inside a GC-atomic call (GC_DESIGN §9.3).
- **Enforced by:** L (`freed_bits_recorded_only_when_tracking_enabled`, `freed_bits_cap_degrades_to_overflow_not_growth`,
  `gc.rs:1611,1647`; the `Keep` tests in `source_map.rs:389-423` and `heap/source.rs:214-229`); C.
- **Gap (latent, [I]).** The freed-bits buffer is one channel per heap. Two consumers on one heap would each see only
  part of the freed set. Today each heap has one `SourceMap` consumer at a time.
- **Redesign:** provenance owned by documents and stored inline in syntax objects (stage 4c; §17.2 provenance row).

### O11. Library loading

- **Statement.**
  - **Deferral.** A `ParsedLibrary` holds its unevaluated `body: Vec<TaggedValue>` outside every root set, so it
    defers collection for its whole lifetime (`rt:library_loader.rs:159-215`). The guard is "a property of the data
    rather than of the loaders", so a fourth loading path is safe by construction.
  - **The registry is a root.** `impl GcRoots for LibraryRegistry` visits each library's exports and environment
    (`rt:library_registry.rs:565-575`; `visit_library`, `gc.rs:629-634`). The registry must be readable at the safe
    point, or the safe point aborts (O1).
  - A heap `Library` object is traced too. That is harmless redundancy (GC_DESIGN §4.3).
- **Enforced by:** T (the private guard field); R (`try_roots`); L (bootstrap under the stress lane).
- **Violations.**
  - #610: a top-level datum is recognized as `define-library` by its spelling (`fe:library_support.rs:61-71`, used at
    `vm:backend.rs:238` and `tw:backend.rs:114`), against AGENTS.md's binding rule.
  - #614: those forms run entirely under the `ParsedLibrary` guard and never reach a collecting safe point. A replaced
    library stays reachable through the importer's `owners` (`core:environment.rs:427-433`; `register_or_replace`,
    `rt:library_registry.rs:466`).
- **Redesign:** `import` and `define-library` recognized by binding; collection at points A, B and D (§11.3); a
  `define-library` collection point at stage 2; namespaces that own their records (§11.6, stage 4b).

### O12. Environments

- **Statement.**
  - Environments are `Rc` Rust structs outside the heap. **Env → heap** edges are bare `TaggedValue`s in binding
    tables. **Heap → env** edges are owning `Rc<Environment>`s inside slots (`VmClosure.globals`,
    `EnvironmentSpecifier`, CPS lambda env, `Macro.definition_env`).
  - The collector reaches an environment's values through three hooks (`core:environment.rs:2185-2234`):
    `for_each_local_value`; `for_each_alias_target` (macro aliases into the defining environment);
    `for_each_shared_owner` (import owners, #406). Each environment is deduplicated by `gc_identity`
    (`gc.rs:539-559`). GC_DESIGN §3.3: "Anything that gives an environment another way to reach a value held
    elsewhere needs a third."
  - The cycle argument (P6) requires every cycle to pass through a heap slot.
- **Enforced by:** C; L (`tombstone_drops_rc_payload_breaking_env_cycle`; `availability_handles_do_not_retain_registries`
  pins the weak link from the `cond-expand` service).
- **Violations.**
  - **Back-pointers close cycles that no sweep breaks.** `Environment.heap: SharedHeap` (`core:environment.rs:489`)
    and `CompiledMacro.heap` (`core:compiled_macro.rs:483`) point from the env side back to the heap. Dropping an
    interpreter leaves 36–37 strong references (#604).
  - **Strong env → env tables that only grow:**
    - `owners` ("Only ever added to", `environment.rs:427-433`; #614);
    - `alias_bindings` ("nothing removes a binding", `:528`; #611);
    - scoped bindings (#613).

    Through the hooks above, these keep whole environments and their values reachable.
  - #615: an environment specifier's tables (about 44 KiB) are invisible to the trigger.
  - #603: compiled code follows a later redefinition or re-import, which chibi and Gauche do not. This is a semantics
    record for the global-binding redesign, not a GC defect.
- **Redesign:** binding records and cells in the non-moving space, owned by namespaces, with weak placeholders,
  bounded aliases and namespaces charged as host payloads (§11.6, variant R at 4b; M1/M2 in §17.2). Tree-walker
  environments become host payloads (§11.4).

### O13. Embedding API (`patina-interpreter` and hosts)

- **Statement.** There is no rooting and no teardown:
  - every `eval_*` returns a bare `TaggedValue` (`interp:lib.rs:278,287,298,…`);
  - `display_tagged` takes one (`:541`);
  - `global_env().define(name, v)` is the only way a host can keep a value alive (#605);
  - `run_forms` keeps the previous form's value in a Rust local while the next form runs (`interp:lib.rs:491,517`);
  - no `Drop` on `Interpreter`, `VmBackend`, `TreeWalker` or `Heap` clears the arenas;
  - the public `Backend` trait (`rt:backend.rs:33`) offers no way to register roots;
  - `patina-compat` uses a bare heap that never collects (P3).

  The unstated host rule is: **do not hold a `TaggedValue` across any later evaluation, and do not expect output
  buffered in ports to reach disk after dropping the interpreter**.
- **Enforced by:** nothing. `TaggedValue` is `Copy` and `'static`.
- **Violations.**
  - #605: a held value is freed (a debug panic; `(45264 . 45264)` in release). `eval_program_resilient` returns such a
    value itself.
  - #604: the heap and its ports leak on drop.
  - #618: two interpreters on one thread share current ports.
- **Redesign:** `Owned` handles, a branded `interp.with`, `Interpreter::call`, `register_primitive` with a helper
  class, `interrupt_handle()`, mandatory teardown, `Heap::new_standalone()`, and `Backend` migrated one step per stage
  (§11.5, decision 13, stages 2–3).

### O14. Adding a heap object type (or a heap tag)

- **Statement.** Follow AGENTS.md "New heap object type" steps 1–4. The GC-specific obligations:
  1. **An arm in `trace_object_children`.** The match is exhaustive (`gc.rs:695-794`). A variant may go in the leaf arm
     only if it embeds no `TaggedValue`, `Rc<Environment>`, `Rc<CpsContinuation>` or `CpsExpr`. "A value-bearing
     variant misfiled as a leaf is a use-after-free, not a compile error" (`mod.rs:137-141`).
  2. **The visitor entry for each embedded holder** (O3.2), deduplicated if it is `Rc`-shared.
  3. **`Drop` behaviour.** The payload is dropped at sweep, inside the pause (P6). An `Rc` it shares with a live holder
     outlives the slot.
  4. **Identity.** Decide whether two slots can denote one entity. If so, `values_eq` and `tagged_value_hash_identity`
     need arms (P1; #608 is the failure when they are missing).
  5. **The datum writer's match** (`prim:primitives/io/datum_writer.rs:331-360`) and the debug formatter.
- **A new heap tag** is worse. `GcVisitor::visit` and `MarkBits::is_marked` are `if` chains over the four heap tags
  (`gc.rs:103-115`, `:485-501`), not exhaustive matches. A value under a fifth tag is treated as an immediate: never
  marked, so swept while live. `TAG_CLOSURE` (`0b110`) is defined but never minted; `value_is_live` treats it
  conservatively (`gc.rs:528-534`).
- **Enforced by:** T for variants; C for tags and leaf classification; L (stress lanes) once a program uses the type.
- **Redesign:** one declarative layout specification (`declare_layouts!`) generates size, trace, copy, verify and the
  JIT offsets; no `Drop` payloads (§6; §3 row "One declarative layout specification").

### O15. Any Rust struct that stores `TaggedValue`s

- **Statement.** Every off-heap holder must, at every outermost safe point, satisfy one of:
  - **(a) Reachable** from an existing `GcRoots` provider, through the right visitor entry (O3). Examples:
    `Library.exports`, `Environment` bindings, `CodeObject.constants`, `CompiledMacro` literals, `ContValue`,
    `WindRecord`, VM continuation snapshots.
  - **(b) Confined** to a GC-atomic call (O8, O9) or to a `GcDeferGuard` scope (O2). Examples: primitive locals, the
    desugarer's memos, `ParsedLibrary.body`, `saved_globals`.
  - **(c) Dead** before the next outermost safe point. Examples: capture-time register clones, `mem::take`n buffers.

  Further rules:
  - Bare `HeapIndex` fields need `visit_object_index`.
  - Persistent maps keyed by raw bits need the freed-bits protocol (O10).
  - `Rc`-shared graphs need `visit_once`.
  - An `Rc` clone taken out of a heap object (for example `get_parameter`'s `Rc<RefCell<Vec<TaggedValue>>>`) is safe
    only within (b). Its values are rooted only through the heap object.
  - Errors in flight (`ExceptionObject.irritants` inside an `Err`) are covered because safe points are at loop tops,
    never on error paths (GC_DESIGN §5.3). The VM converts an error into a heap exception inside the driver before its
    next safe point (`vm_state.rs:1249-1278`).
- **Enforced by:** C. Violations surface only in the stress and debug-poison lanes, and only when a collection lands
  in the window. About 25 struct types hold values off-heap (`understand/primitives-embedding.md` §11). Between 230
  and 890 functions hold unrooted values across allocation (PRD §8).
- **Redesign:** values may be alive only inside a `Cx` that cannot collect. Across a collection only roots,
  `RootScope`, `Owned` handles and machine-held resumable state hold values (§11.3). Every Rust table is bounded or
  pruned (M2, §17.2). DESIGN E.1 names each holder's fate.

### O16. Collector implementers (`Collector`)

- **Statement.**
  - `Collector::collect(&mut self, &mut Heap, &[&dyn GcRoots]) -> GcStats` (`gc.rs:199-213`) must not move anything
    (P1).
  - It composes the public `run_mark_phase` (`gc.rs:1012-1101`) and `Heap::sweep` (`gc.rs:891`). Sweep completion is
    what lowers the flag and counts the collection.
  - Automatic policy is a threshold installed into the heap. "The safe point never asks the collector anything" (the
    `Collector` doc, `gc.rs:204-207`).
- **Enforced by:**
  - T, partly. `MarkBits`' fields are `pub(crate)` and `for_heap` is private, so only a `GcVisitor` can produce marks
    for `sweep`.
  - C for non-moving.
- **Gaps.**
  - Nothing prevents calling `collect` outside a safe point; tests do (`core:source_map.rs:421`,
    `core:heap/source.rs:229`).
  - `GcController` hard-codes `MarkSweepCollector` (`gc.rs:322-325`), so the seam is nominal: one implementation, never
    swapped.
- **Redesign:** `Collector<M: ObjectModel>`, its obligations table (the epilogue order, safety and completeness), and
  a conformance suite run on `MarkRegion`, `NullGc` and `ActiveGc` (§14, decision 24).

### O17. Threading and instance isolation

- **Statement.**
  - `SharedHeap = Rc<RefCell<Heap>>` (`mod.rs:51`) is `!Send` and `!Sync`, so a heap and everything holding it stay on
    one OS thread.
  - Each backend constructs its own heap; no constructor shares one (`understand/tree-walker.md` §0).
    `with_globals` debug-asserts the same heap (`vm_state.rs:342`). GC_DESIGN §7's "backend coexistence" rule
    (whichever backend is outermost collects for both) therefore describes a configuration production never builds.
  - Many heaps may live on one thread: up to 318 in one `cargo test` process (#604).
  - Per-thread statics are shared by all of them:
    - `CURRENT_*_PORT` (`prim:primitives/io/ports.rs:41-45`);
    - `OUTPUT_FILES` (`core:port.rs:170-185`), which is the right scope for exit flushing;
    - `PENDING_ESCAPE` (O7);
    - immutable `EMPTY_*` sentinels, which are harmless.
  - The collector has no thread awareness: no handshake, no per-thread allocation buffer, no safe region.
- **Enforced by:** T (`!Send`); D (same-heap assertion); C for the thread-locals.
- **Violations.** #618 (current ports). `PENDING_ESCAPE` is latent.
- **Redesign:** M:1 green threads over a carrier/`Mutator` split with N-ready protocols (§18.1, decision 7); a
  per-heap `InterruptHandle` with no process-wide GC singleton (§12); a CI check against new `thread_local!` (§16,
  §18.6). The current ports belong to the interpreter (§11.5, F6).

---

## 4. Issues #604–#618, by clause

| Issue | Title (short) | Clauses it breaks or exposes | PRD fix and stage |
|---|---|---|---|
| #604 | Dropping an `Interpreter` leaks its heap; buffered port output is lost | P6, O12 (back-pointer cycle), O13 | teardown, F5 (§9.6, §11.5); stage 2 |
| #605 | A value returned by `eval_str` is freed by a later collection | O13, P5 (silent in release) | `Owned` handles (§11.5); stage 2 |
| #606 | Trigger counts objects, not bytes (large vectors, deep captures) | P3, O4 | byte trigger (§15); stage 1, then 4e (§13) |
| #607 | Opening files fails at the 1,021st unclosed port | P3 (descriptors unseen), P6, O8 | F3 collect-and-retry (§9.6); stages 1, 4a |
| #608 | `(eq? (current-output-port) (current-output-port))` is `#f` | P1 (identity), O8, O14 (identity arm) | canonical port, F6 (§9.6); stage 4a |
| #609 | One `(gc)` over 16,000 ephemerons: about 220 ms in one order, 1 ms in the other | P7 | key-indexed fixpoint (§9.5); stage 5 |
| #610 | A procedure named `define-library` is parsed as a library | O11 (recognition by spelling) | recognized by binding (§11.3); stage 2 |
| #611 | A macro with a private helper interns a new alias per expansion | P4 (immortal symbols), O9, O12 | aliases deduplicated by target ("now", §19); weak symbols (5c) |
| #612 | Re-evaluating a quoted `case` datum grows without bound | P3 (provenance unseen), O9, O10 | chains interned per document; stages 1, 4c |
| #613 | References to macro-introduced definitions scan every expansion | O9, O12 (tables never pruned) | indexed lookup (≤ 4b), reclaimed from 5c (§11.6) |
| #614 | A redefined library stays alive; `define-library` runs never collect | O2, O11, O12 (`owners`) | collection point (stage 2), namespaces (4b) |
| #615 | `(environment '(scheme base))` loop plateaus near 2.9 GiB | P3, O12 | namespace charged as external bytes (stage 1), dies with its specifier (4b) |
| #616 | After a dropped peak, every collection keeps the peak's sweep cost | P8 | lazy sweep, decommit (§7, §9.3); stage 5e |
| #617 | About 1,000 nesting levels aborts with a Rust stack overflow | O9 (no native-stack contract) | depth guard (§17.3); "now" row |
| #618 | Two interpreters on one thread share their current ports | O8, O13, O17 | ports belong to the interpreter (§11.5, F6); stage 4a |

---

## 5. What nothing enforces

These parts of today's contract rest on convention alone (**C**), and the stress lanes catch them only when a
collection happens to land in the bad window:

1. Completeness of every root provider (O3.1). This includes the latent `foreign_expansions` gap and the requirement
   that a new kind of env → env edge get its own hook (O12).
2. That a new Rust scope holding values across re-entry takes a `GcDeferGuard` (O2). Its one precedent was found by
   bootstrap crashing.
3. Leaf classification of a new `HeapObjectData` variant, and any new heap tag (O14).
4. The VM's one-dispatch atomicity for continuation refs and store entries, and its handle-in-a-rooted-register rule
   (O4).
5. Non-moving (P1), which every raw-bits map, identity hash and `eq?` fast path silently assumes.
6. Everything about embedding (O13). No host-facing rule is written down, and the API itself breaks the one rule a host
   would need (`run_forms`, `eval_program_resilient`; #605).
7. Thread-local state shared across heaps (O17).

The PRD converts most of these into type-level guarantees or checks:
- the `Cx<'gc>` brand with trybuild tests, and `NoGcScope` by type (§11.3);
- generated traces from `declare_layouts!` (§6);
- the heap verifier, `PATINA_GC_VERIFY_ROOTS` and zeal (§16);
- the conformance suite (§14);
- the CI check against new `thread_local!` (§16).
