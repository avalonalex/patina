# Premature collection in Patina's git history: every case found

Repository: `avalonalex/patina`, branch `gc-prd` (main `28a94f8` plus one docs commit). Read only.
Mined 2026-10-01.

## Method

- `git log --all -i -E --grep` over full commit messages with: use-after-free, UAF, freed, reclaim, poison, rooting,
  unrooted, GcRoots, register_roots, dangling, slot reuse, tombstone, ephemeron, safe point, safepoint, GcDeferGuard,
  defer, PATINA_GC_STRESS, premature, collected, kept alive, swept, untraced, "not rooted", "missed root",
  "lost root", "rooted only", "leaf arm", misfiled, "Rust local", outermost, reused, "arena slot", stale, weak store,
  "only root", "reachable only", "reachable from nowhere", insurance. Also a broad `gc|collect|sweep|mark|root|weak|
  live|heap|arena` pass.
- Pickaxe (`-S`/`-G`) for `GcDeferGuard`, `gc_defer`, `trace_roots`, `trace_weak_ids`, `sweep_weak`,
  `retire_registers`, `register_roots`, `GC_POISON`, `is_outermost`, `pending_escape`, `saved_globals`,
  `foreign_expansions`, `pub resume`.
- Path history of every GC file: `heap/gc.rs`, both `gc_roots.rs`, `library_registry.rs`, `tracer.rs`,
  `GC_DESIGN.md`, the stage-5 PRD, the GC test files and `run_gc_differential.sh`.
- `gh`: all 406 merged PR bodies and all 204 issues scanned with the same vocabulary (dumps in
  `scratch/prs.json`, `scratch/issues.json`); PR #551/#282/#306/#567/#602 bodies and issues #423, #587, #604, #605
  read in full.
- Every candidate's message and its GC hunks were read (`git show --stat` and the `gc.rs` / root-provider / test
  hunks).

Before 2026-07-31 Patina had no collector (the heap never freed a slot), so no earlier commit can contain a
premature-collection bug. All cases fall in the two months from GC stage 1 (`b908f16`, 2026-07-31) to today.

## Summary

"On main" is how long the defect was reachable from `main`. "Pre-merge" means it was introduced and fixed inside one
PR's own branch.

### A. Live values actually freed, or a latent unrooted holder fixed

| # | Commit, date | PR / issue | Class | Detected by | On main | Regression test |
|---|---|---|---|---|---|---|
| 1 | `753a904` 2026-07-31 | #6 | Rust locals held across an outermost safe point (second library-load path) | stress lane + debug poison panic | pre-merge (hours) | none dedicated; chibi suite under stress lane |
| 2 | `0a59c3d` 2026-08-09 | #38 | new out-of-heap edges not traced (macro `definition_env`, environment alias targets) | /simplify code review | pre-merge | none |
| 3 | `f578cc6` 2026-08-11 | #47 (audit #44, item B1) | field not traced, rooted only by an aliasing accident (`CpsContinuation.resume`) | post-merge audit, by reading | ~1.6 days, latent | none (could not be made to fail) |
| 4 | `1d18c49` 2026-08-26 | #130 | weak-processing order: ephemeron fixpoint after weak-id fixpoint | PR review (repro reproduced) | pre-merge | `an_ephemeron_holding_a_continuation_keeps_its_payload` |
| 5 | `7e69689` 2026-09-02 | #156 | only root for a weak-store handle freed (stub register window) | /code-review | pre-merge, latent | none |
| 6 | issue #605 (open) | #605 | embedder-held value; `run_forms` holds previous result in a Rust local across the next form | GC-redesign study probe | ~2 months, live today | planned (acceptance in #605) |

### B. Near relatives (precision, weak tables, slot reuse, collector crash)

| # | Commit, date | PR / issue | Class | Detected by | On main | Regression test |
|---|---|---|---|---|---|---|
| 7 | `f44bb11` 2026-08-05 | #19 | continuation side tables made weak; soundness obligations plus heap-wide id minting | ctak 4 GB crash (over-retention) | n/a (new mechanism) | `gc_weak_continuations.rs` (7, now inside `vm_state/gc_roots.rs`), 2 in `gc_vm.rs` |
| 8 | `235526b` 2026-09-29 | #551 fixes #423 | register precision: per-PC retirement must not clear a live register | Larceny `ephemeron` 5/6 (over-retention) | over-retention ~2 months (seen 2026-08-26) | 10 liveness tests in `ephemerons.rs` / `gc_vm.rs` |
| 9 | `5b67c3d` 2026-09-03 | #172 | keep-alive rule for a weak-store handle copied where it leaks; window freed with a "no safe point in between" argument | review (leak measured) | pre-merge | leak measured; liveness covered by argument only |
| 10 | `951a82a`, `65792d6` 2026-09-16 | #353 (#338), #354 (#352) | second liveness system (code units): closure count only drops on a real sweep; generation-checked code ids on slot reuse | design + tests | n/a | `finished_forms_release_code.rs` (9), exhausted-slot test |
| 11 | `68fb1fd` 2026-09-12 | #306 (triage family 6) | collector crash: recursive trace overflowed the Rust stack | Larceny `char` suite crash | ~6 weeks (recorded 2026-08-24) | `collection_at_deep_call_depth_preserves_suspended_values` |
| 12 | `9844127` 2026-08-03, `56d2950` 2026-09-30 | #10, #567 | raw-bits-keyed side tables pruned before slot reuse (SourceMap, `syntax_sources`) | by design | n/a | prune unit tests, source-retention-after-GC test |

### C. Protections added when a holder was introduced (no bug shipped)

`bfff2e4` #5 (defer-counter underflow asserts; registry-busy abort), `77f6970` #78 (`pending_escape` root), `8da5efd`
#152 / `7e69689` #156 (wind records carry and trace their handler stack), `1447b90` #150 (handler stack on
`CpsContinuation`, trace deduplicated), `7ebfed1` #164 (delimited continuation's carried stacks traced, test made
non-vacuous), `0bdbc2c` #175 (prompt `ContValue`s traced; prompt-heavy stress run on debug), `2c175b4` #282 (one wind
traversal for both backends), `3614f8c` #155 (environment dedup identity made sound: `Environment` no longer `Clone`),
`f053037` #434 (import owner environments traced), `b237770` #481 (resumable-primitive state traced), `203313f` #547
(defer guard over expansion with imports), `28a94f8` #602 (defer guard bound to the globals swap).

### D. Detection-infrastructure fixes

`bfff2e4` #5 (vacuous arena test), `92dd8ec` #39 (stress interval 16 so the lane is affordable), `3a1ee89` #200 and
`8da0c5d` #201 (GC differential lane passed vacuously; now pinned to 1226), `7ebfed1` #164 (vacuous GC trace test).

---

## A. Detailed cases

### 1. Second library-loading path ran an "outermost" dispatch loop over unrooted Rust locals

- **Commit:** `753a904` "GC stage 3: VM roots, dispatch-loop safe point, deferral" (#6), committed 2026-07-31 22:11
  -0700, merged 2026-08-01.
- **Symptom:** the VM bootstrap died on the very first collection under `PATINA_GC_STRESS=1`. Release build:
  `type error: expected a procedure, got object` at an unrelated call site (a swept slot read back as a tombstone).
  Debug build: `use-after-free: pair slot 313`, with a backtrace naming the desugar call that read it.
- **Root cause:** `backend.rs::evaluate_parsed_library` was a second library-loading path, distinct from the one in
  `vm_state.rs` that had been guarded. It called `execute` rather than `execute_nested`, which started a fresh dispatch
  loop that considered itself outermost. `parsed.body`, `parsed.imports`, `lib_env` and `saved_globals` were Rust
  locals no root provider could see. The PR records that `execute` vs `execute_nested` was a red herring: the predicate
  is "does this Rust frame hold heap values across an evaluation call?", since `run_loop_until` guards unconditionally
  and a nested call reached from outside any loop is equally outermost.
- **Detected:** the stress differential lane, localized in one run by the debug poison assertion added in stage 1.
- **Lived:** introduced and fixed inside PR #6, never on main.
- **Fix:** first a guard at both call sites. The /simplify round then moved the `GcDeferGuard` onto `ParsedLibrary`
  itself, held for as long as it holds unevaluated body forms ("a fourth path is now safe by construction"; also
  correct for a `ParsedLibrary` held beyond one call). `GcDeferGuard` was changed to own its heap handle to allow
  this. GC_DESIGN §11 now tells authors to run the stress lane in a debug build too.
- **Regression test:** none dedicated. The bootstrap itself, run by the differential lane, is the test.

### 2. Macro `definition_env` and environment alias targets not traced

- **Commit:** `0a59c3d` "Resolve a macro template's free identifiers where the macro was defined" (#38), 2026-08-09.
- **Symptom (as recorded):** "a value reachable only through a macro's definition environment or through an alias
  was swept and its arena slot reused." The record calls it "a soundness bug rather than a leak". No program-level repro
  is recorded.
- **Root cause:** the PR's own earlier commits added two `Rc<Environment>` edges that leave the heap's trace graph:
  `CompiledMacro.definition_env`, and `Environment.alias_bindings`, whose aliases point at a binding in another
  environment that is not on the parent chain. `visit_env` walked only the parent chain, and the `Macro` arm traced only
  pattern and template literals.
- **Detected:** the /simplify review on the same branch.
- **Lived:** pre-merge.
- **Fix:** `visit_env` collects alias targets and walks them as separate roots; the `Macro` arm calls
  `visit_env(definition_env)`.
- **Regression test:** none. `macro_definition_env.rs` (342 lines) never calls `(gc)`.
- **Recurrence:** the same shape of edge was added again by `9bb8d2a` (#462, 2026-09-23):
  `CompiledMacro.foreign_expansions: Vec<(ScopeId, Rc<Environment>)>`. The `Macro` arm still traces only literals and
  `definition_env` (`gc.rs:746-753`). The study (`understand/offheap.md:104`) rates it low risk today because those are
  library environments the registry roots, and the probe found 0 entries. It is an open latent gap of case 2's class.

### 3. `CpsContinuation.resume` never traced (latent)

- **Commit:** `f578cc6` "Close the continuation-refactor follow-ups from the audit" (#47), 2026-08-11 08:28 -0700.
- **Introduced:** `92dd8ec` (#39), 2026-08-09 18:17 -0700, which added `resume: Option<ContValue>` and moved
  `trace_cont_env` / `trace_cont_value` / `trace_exception_handler` into patina-core.
- **Symptom:** none observable. The field holds consumer procedures, `after` thunks, cached promises and exception
  payloads.
- **Root cause:** `trace_continuation_children` visited `body`, `env`, `dynamic_winds` and `captured_cont_env` but not
  `resume`. It stayed live only because every reify site stores the wrapper it read out of the already-traced
  `captured_cont_env`: "an undocumented invariant with no debug poison behind it". Any cont-env pruning, or a wrapper
  built outside the cont env, would have unrooted it.
- **Detected:** the post-merge audit (`PRD/AUDIT_2026_08_10_PRD.md`, item B1, rated HIGH, commit `fc3e71c`), by
  reading the trace against the struct. "Verified latent (GC-stress repros survive today)."
- **Lived:** about 1.6 days on main, latent.
- **Fix:** trace `resume` explicitly, with a comment stating the aliasing.
- **Regression test:** none; the audit could not construct a failing program.

### 4. Ephemeron fixpoint ran after the weak-continuation fixpoint (use-after-free)

- **Commit:** `1d18c49` "Implement SRFI 124 ephemerons in Rust as (scheme ephemeron)" (#130), 2026-08-26, second
  sub-commit "Address the review of #130: the two weak fixpoints have to be one".
- **Symptom:** after one `(gc)` the VM died with `expected a procedure, got object`; the tree-walker printed `(a b c)`.

  ```scheme
  (define e #f)
  (define (capture)
    (let ((secret (list 'a 'b 'c)))
      (let ((v (call/cc (lambda (c) (set! e (make-ephemeron kk c)) 0))))
        (if (= v 0) 'captured secret))))
  (capture) (gc) ((ephemeron-datum e) 1)
  ```

- **Root cause:** the ephemeron loop ran after the weak-id loop had gone quiescent. A `VmContinuationRef` marked only by
  a late ephemeron retention was queued into `new_weak_ids` and never broadcast, so `trace_weak_ids` never traced its
  payload in `VmState`'s side table, while `sweep_weak` kept the store entry, because its ref was marked. The result was
  a live continuation whose snapshot pointed at swept slots. A second weak-id pass would not have been enough either:
  `trace_weak_ids` can reach an ephemeron, which would then arrive after the ephemeron loop stopped.
- **Detected:** review of PR #130. The author reproduced it before fixing.
- **Lived:** pre-merge.
- **Fix:** one combined fixpoint in `run_mark_phase`. Each round broadcasts new ids and retains any pair whose key
  became live, ending only when a round does neither. The old termination argument was also wrong ("each round strictly
  shrinks pending"; `drain` can grow it) and was corrected: each retaining round removes a pair from a finite set, and
  each id is queued at most once. `value_is_live` was made private, because its answer depends on how far the worklist
  has drained and a root provider calling it would skip a live trace.
- **Regression test:** `crates/patina-tests/tests/ephemerons.rs::an_ephemeron_holding_a_continuation_keeps_its_payload`.
  The commit calls it "the case no test covered".

### 5. `ResumeWindJump` freed the only root of a weak-store continuation handle (latent)

- **Commit:** `7e69689` "Run wind thunks in their dynamic-wind call's environment on the VM" (#156), 2026-09-02, in the
  sub-commit "Fold in /code-review: GC rooting, ...". Introduced by the PR's own `2235fa43`.
- **Symptom:** none observable.
- **Root cause:** the new stub frame's register window was freed when `ResumeWindJump` ran. It was the only root for the
  jump's `target` (a `VmContinuationRef` whose payload is in the weak store) and `value`, leaving both reachable from
  Rust locals alone until the next step wrote them back. A collecting safe point in that window would have pruned the
  payload.
- **Detected:** `/code-review`. Recorded as "insurance, not a bug fix", because no safe point runs in that window today.
- **Lived:** pre-merge.
- **Fix:** keep the window. It is reclaimed when the travel ends, since arrival replaces the register file. The
  comment at `vm_state.rs:1866-1875` explains this. `control.rs:31-33` now states the rule: "Carry the heap
  continuation *handle* in a rooted register across thunk execution, not just an `Rc` payload." #602 (`28a94f8`)
  preserved it as `ExecutionState::finish_wind_step`, "the one deliberate exception to paired removal".
- **Regression test:** none (no safe point can reach the window).

### 6. Embedder-held value freed by a later collection (open)

- **Issue:** #605, filed 2026-10-01, open. Planned as GC_PRD stage 2.
- **Symptom:** `let held = interp.eval_str("(list 'held (vector 1 2 3))")`, then an `eval_program` that churns and
  `(gc)`s. Debug: `use-after-free: pair slot 15992 was reclaimed by the GC` (`heap/mod.rs:721`). Release: prints
  `(45264 . 45264)`. Same on both backends. `eval_program_resilient` frees its own result the same way when a later form
  collects and then fails.
- **Root cause:** every `eval_*` returns a bare `TaggedValue`, and the embedding API has no handle or root type.
  `run_forms` keeps the previous form's value in a Rust local (`patina-interpreter/src/lib.rs:491`, assigned `:517`)
  while the next form runs, and that form's safe points can collect it.
- **Detected:** the GC-redesign study's probe (`PRD/study/gc/understand/primitives-embedding.md` §7.2;
  `gc-impl.md` §1.8 saw `(#(1 2) #(3 4) "str")` become `(i 0)`).
- **Lived:** since collection became automatic (stage 4c `7401cba`, 2026-08-03; stage 2/3 for an explicit `(gc)`),
  about two months. Not fixed.
- **Why no lane caught it:** the differential lanes drive the CLI, and the CLI never holds a result across forms. No
  test holds an `Interpreter` result across a collecting call.
- **Planned fix:** owned handles (heap id, slot, generation) rooted through a per-heap handle table, and handles in
  `run_forms`.

---

## B. Near relatives

### 7. Weak continuation side tables (`f44bb11`, #19, 2026-08-05)

- **Problem fixed:** over-retention. Strong side tables made every `call/cc` capture immortal, transitively. ctak grew
  to 4 GB RSS and died, and 20,000 dead captures leaked 20,008 objects.
- **Liveness obligations it created**, written down as the soundness argument:
  - capture inserts the store entry and allocates the ref within one instruction dispatch;
  - invocation copies the payload back within one dispatch;
  - every nested loop defers collection.

  So at a collecting safe point, an unmarked ref proves its payload is unreachable. Cases 4, 5 and 9 are all
  violations, or near-violations, of these obligations.
- **Review hardening:** ids were minted per `VmState`. They are now minted by one counter on the `Heap`, shared by
  both kinds and every `VmState`, "so an id names at most one side-table entry ever", enforced where ids are born. The
  whole mark phase became the public `run_mark_phase()`, "so future Collector impls cannot mis-order its interior".
- **Tests:** 6 Rust tests in `patina-vm/tests/gc_weak_continuations.rs` (prune/survive, payload-chain fixpoint,
  dead-chain-in-one-collection, delimited and cross-store refs, churn plateau). Later 7, moved into the storage owner
  by #602. Plus 2 Scheme tests in `gc_vm.rs`.

### 8. Register precision without clearing a live register (`235526b`, #551, fixes #423, 2026-09-29)

- **Problem fixed:** over-retention. After `(set! keys (reverse (reverse (list-tail keys 5))))`, a collection in the
  same frame kept the old list alive through a stale temporary register. Larceny's `ephemeron` suite was 5/6 on the
  VM. First seen 2026-08-26 while building #130, filed as #423 on 2026-09-19.
- **Liveness risk it created:** clearing a register that is still live. The design guards against it as follows:
  - Compiler per-PC bitsets of possible roots. Parameters start live; joins take the union; fused predicates include
    their fast edges and deopt fallthrough.
  - Locals stay conservative. This tracks expression lifetimes, not last use.
  - Runtime-built stubs (`register_roots: None`) keep every slot.
  - Slots are **cleared, not skipped**: the VM overwrites dead slots with `UNSPECIFIED`, then traces whole vectors, so
    a continuation snapshot or tracer view can never carry an untraced pointer to a swept slot. The same retirement is
    applied to full and delimited snapshots at capture, with the delimited base offset.
  - `reference-barrier` keeps its argument live through the call.
- **Tests** (all `(gc)` mid-expression, both directions):
  - `earlier_operands_stay_live_across_collection_in_a_later_operand`
  - `reference_barrier_keeps_its_argument_live_until_the_call`
  - `a_tail_call_retires_the_previous_register_window`
  - `a_self_tail_call_retires_its_argument_copies`
  - `a_continuation_does_not_resurrect_finished_temporaries`
  - `a_continuation_keeps_an_earlier_operand_for_reentry`
  - `a_delimited_snapshot_retires_dead_temps_and_keeps_pending_operands`
  - `discarded_results_and_both_branch_tests_release_the_key`
  - `replacing_a_captured_local_releases_its_previous_value`
  - `wide_pending_operands_survive_collection`
  - `rebound_control_operator_keeps_operands_after_a_larger_expression`

  The PR was validated under `PATINA_GC_STRESS=1` for `control_flow_matrix` and `ephemerons`, and on both differential
  builds.

### 9. Composable-invoke stub window: retention vs liveness (`5b67c3d`, #172, 2026-09-03)

- The first cut copied case 5's rule ("do not free the window; it is the only root for the continuation handle").
  Review measured an unbounded leak: a composable invoke extends the register file rather than replacing it, giving
  27 MB against main's 10 at 400k invokes.
- The window is now freed, on the argument (comment at `vm_state.rs:~1825-1832`) that the four values are in Rust
  locals and "nothing between here and the next write of `cont` ... reaches a GC safe point".
- It shows the tension. The safety rule for weak-store handles is enforced by comment and reasoning about where safe
  points are, not by a mechanism, and the same author applied it in the wrong direction once.

### 10. Code-unit release: a second liveness system (`951a82a` #353, `65792d6` #354, 2026-09-16; `9545d38` #477)

- Code objects are released when no frame, continuation or live closure can run them. Frames and continuations hold an
  `Rc`. Closures name code by id, so each code object counts its live closures: up at `MakeClosure`, down only "for a
  closure the collector has actually freed". "One too low would free code a closure can still run, and the tests that
  hold code in each of those ways fail when the count is ignored." The tests are in `finished_forms_release_code.rs`:
  a closure keeps its code, a lambda made after its form finished, a captured continuation keeps its form's code, and a
  closure returned by `eval` keeps its code.
- #354 reuses released code slots and makes ids `{slot, generation}`. A lookup checks the generation, "so an id
  outliving its code finds nothing rather than running whatever took its slot". A slot that has used every generation
  is never handed out again, and a test pins that.
- #477 added `Heap::retire_vm_closure` for `eval`'s one-shot closure. It is safe because a frame running it holds the
  code `Rc`.

### 11. Collector stack overflow on deep continuations (`68fb1fd`, #306, 2026-09-12)

- Larceny `char`'s full Unicode sweep crashed the tree-walker: `trace_cont_value(Local)` → `trace_cont_env` →
  `trace_cont_value` recursed on the Rust stack. Pointer dedup bounded work but not depth.
- Recorded as triage family 6 on 2026-08-24 (#110), with the cause unknown until 2026-09-12. The recursion dated from
  stage 2 (2026-07-31).
- The fix queues `ContEnv` snapshots as a third worklist.
- Test: `gc_tree_walker.rs::collection_at_deep_call_depth_preserves_suspended_values`. It runs 50,000 suspended calls,
  each with a distinct heap pair used only after its recursive call returns, "so skipping the deep roots cannot make
  this test pass".

### 12. Side tables keyed by raw bits, pruned before slot reuse (`9844127` #10; `56d2950` #567)

- `SourceMap` (#10) is keyed by raw `TaggedValue` bits. Sweep records freed bits into a capped buffer, and each
  parse-eval loop drains it at the form boundary before the next parse can reuse a slot. Cap overflow clears all
  locations ("a missing location degrades a diagnostic, a stale one misattributes it").
- `Heap.syntax_sources` (#567) is "not a root". `sweep` retains only marked keys before any slot is reused.
- Both were done right at introduction, with no bug shipped. They are the slot-reuse analogue of the code-id generation
  check in case 10.

---

## C. Protections added at introduction (no bug shipped)

- **`b908f16` #4 (stage 1, 2026-07-31).**
  - Debug tombstones: `Free` asserted in `get_object`; `GC_POISON` pairs asserted in `get_pair` / `set_car` /
    `set_cdr`. Vectors and strings get an empty `Vec`, which is legal, so a use-after-free there is undetectable.
  - The symbol table is rooted inside `GcVisitor::new` as a heap invariant.
  - The `HeapObjectData` doc warns that a misfiled leaf arm "is a use-after-free, not a compile error".
- **`bfff2e4` #5 (stage 2).**
  - Every trampoline takes a `GcDeferGuard`, and only `is_outermost()` collects, "so a missed route costs a collection
    opportunity, not a use-after-free".
  - `exit_gc_defer` now `debug_assert`s instead of saturating ("saturating would silently re-enable collection").
  - The safe point aborts if `library_registry` is mutably borrowed ("a partial root set is a use-after-free"),
    formalized as `LibraryRegistry::try_roots` in #6.
  - `PENDING_ESCAPE` is a root provider.
- **`77f6970` #78 (2026-08-15).** `pending_escape` is traced from the commit that introduced it: "while set, the value
  is reachable from nowhere else."
- **`1447b90` #150 (2026-09-01).** The tree-walker continuation carries `exception_handlers`, traced. Review found the
  trace was a hand-rolled copy of `trace_exception_handler` ("a third copy to go stale, and a missed root is a
  use-after-free") and that the GC root inventory doc did not list the new root. Both were fixed.
- **`8da5efd` #152, `7e69689` #156 (2026-09-01/02).** Wind records carry the handler stack of their `dynamic-wind`
  call; `visit_wind` traces it ("a handler reachable only from a record is live for as long as the record can still run
  a thunk").
- **`7ebfed1` #164 (2026-09-02).** A delimited continuation carries its prompts and handlers, traced by
  `trace_delimited_continuation`. Review found the GC test vacuous: "written the obvious way — the same payload in the
  registers too — it passed with them deleted". It was rewritten to hold a prompt tag, prompt handler and exception
  handler reachable from nothing else (`a_carried_prompt_and_handler_are_traced`).
- **`0bdbc2c` #175 (2026-09-04).** New prompt `ContValue`s are traced. "That lane has no prompts in it", so a
  prompt-heavy program (2,000 abort/resume cycles, a composable continuation held across 20,000 allocations) was run
  under stress on a debug-poison build. The lane itself was not extended; #587 is the open follow-up.
- **`3614f8c` #155 (2026-09-02).** Environment dedup now keys on `Environment`'s own address instead of the
  bindings-map `Rc`. `Environment` is deliberately not `Clone`, "so one address means one environment, and a collection
  cannot free one while tracing it".
- **`2c175b4` #282 (2026-09-11).** One wind traversal for both backends (`visit_winds_with` plus a handler callback),
  so the rule is not restated, with a unit test proving thunks and all handler payloads stay rooted and are reclaimed
  once records drop.
- **`f053037` #434 (2026-09-19).** Imports share bindings through owner environments; `visit_env` walks
  `for_each_shared_owner`.
- **`b237770` #481 (2026-09-24).** Resumable-primitive `state` lives in a `resume_stub` register (VM) or a traced
  `ContValue::ResumePrimitive` (tree-walker).
- **`203313f` #547 (2026-09-28).** `GcDeferGuard` over `desugar_with_imports`: "Loading an import can execute Scheme.
  The unfinished input, include datums, and partially built IR live in this Rust stack".
- **`28a94f8` #602 (2026-09-30).** `VmState::with_globals` takes the defer guard together with the globals swap, so a
  swap can no longer be written without it. Library loading had previously relied on `ParsedLibrary`'s guard reaching
  `saved_globals`.
- **`87da821` #28 (2026-08-08).** Consing straight from registers is safe "because collection only runs at the
  interpreter loops' safe points, never inside `alloc_pair`". This is the load-bearing invariant later study counts
  as about 230–890 Rust functions depending on (DIGEST).

## D. Detection-infrastructure fixes

- **`bfff2e4` #5.** `repeated_collection_keeps_arena_bounded` passed with zero collections. It now compares with vs
  without collecting.
- **`92dd8ec` #39.** The stress lane went from 2.9 s to 103 s with upstream `(chibi test)`. The interval was set to 16
  (about 4,000x the adaptive rate), keeping more than 1,000 collections in the reclamation proof.
- **`101300b` #52.** The stress reclamation proof asserted an absolute arena bound that really measured bootstrap. It
  now asserts the churn delta (grows 0).
- **`3a1ee89` #200, 2026-09-06.** The GC differential lane passed vacuously. Without `-A test-lib`, both runs printed
  the same 4,537 import-failure lines and exited 0, so every diff was OK and CI went green "having exercised no GC
  behaviour at all". The hole was opened by #200 itself, and found in its review. The lane now requires the
  framework's own `N out of N` tally.
- **`8da0c5d` #201, 2026-09-06.** That guard accepted any tally, so a filtered or aborted two-assertion run passed. It
  is pinned to 1226 now. GC_DESIGN §11's manual recipe had the same hole and was fixed.

## Open or latent, with no fixing commit

- **#605:** the embedder use-after-free (case 6).
- **`CompiledMacro.foreign_expansions`:** traced by `Rc` only, never by `visit_env` (case 2's class, `9bb8d2a` #462;
  `offheap.md:104`).
- **Detection gaps** (`gc-impl.md` §7):
  - Vector and string tombstones are undetectable.
  - Sweep pre-marks free-list slots, so a root pointing at a freed slot is silently absorbed. GC_DESIGN §11.5's
    paranoid pre-sweep assertion is not implemented.
  - A dangling pair holding poison is an immediate and is skipped by tracing.
- **Hazard census of Rust code that holds values across allocation and is safe only because allocation never
  collects** (DIGEST, `offheap.md` §5.2): 19 fresh-unrooted-across-alloc, 51 value-used-after-alloc, 25 alloc-in-loop,
  9 Rust collections held across an allocating loop, 3 alloc plus re-entry.
- **#587 (open):** the generator for callback and prompt interactions under GC stress, because the differential lane
  (the chibi suite) has no prompts and few callbacks.

## Patterns across the cases

1. **Most premature-collection bugs were caught before merge** (cases 1, 2, 4, 5), by two mechanisms:
   - The stress lane run on a debug-poison build. It localized case 1 in one run.
   - Review that read the trace rules against the struct fields (cases 2, 3, 5) or against the weak protocol (case 4).
2. **The one that reached main through code (case 3) was latent by aliasing.** No test could fail it, and only an
   audit found it.
3. **The one live today (case 6) is at a boundary no lane drives**: the embedding API across forms. The lanes run the
   CLI, so the classes they cannot see are API boundaries, prompt and callback histories (#587), and holders added later
   whose edge happens to be covered elsewhere (`foreign_expansions`).
4. **Recurring classes:**
   - **(a) An out-of-heap edge (`Rc<Environment>`, a `ContValue` field, a side table) added without a trace arm:**
     cases 2 and 3, `foreign_expansions`, and the #150 inventory miss. Today's defence is comments, review and a doc
     table. Nothing is mechanical.
   - **(b) A heap value held only in a Rust local or an unrooted window across a point that could collect:** case 1,
     case 5, case 9's argument and case 6. The defences are deferral by construction (guards on `ParsedLibrary`, on
     every loop, on `with_globals`, on `desugar_with_imports`) and the comment rule in `control.rs` for weak-store
     handles.
   - **(c) Weak-processing order:** case 4. Fixed by one fixpoint owned by `run_mark_phase`, not by providers.
   - **(d) Slot or id reuse making stale keys alias new objects:** cases 10 and 12. Defended by generation checks and
     prune-before-reuse.
5. **Tests that cannot fail were found four times** (#5, #164, #200, #201). Each fix made the value under test
   reachable only through the path under test, or pinned the exact tally.
