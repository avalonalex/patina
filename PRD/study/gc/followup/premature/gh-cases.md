# Premature collection in Patina's history: GitHub cases

Mined 2026-10-01 from avalonalex/patina, all states, at `gc-prd` (main `28a94f8` plus the PRD commit).

## Method

- `gh issue list` and `gh pr list` with `--state all --limit 50`, across these keywords: use-after-free, freed,
  GC, garbage, root, rooting, poison, collected, safe point, ephemeron, weak, stress, differential, reclaimed,
  dangling, slot, tombstone, sweep, live value, premature, stale, generation, collector, mark, GcRoots.
- `gh search issues --include-prs` with exact phrases: "use-after-free", "reclaimed by the GC", "pair slot",
  "missing CodeObject", "expected a procedure, got object" (the release-build symptom of a swept slot),
  "PATINA_GC_STRESS", "debug-poison", "GcDeferGuard", "is_outermost", "lost root", "weak store".
- Read every GC stage PR (#4–#12, #19), and #423/#551, #605, #604, #606, #609, #614, #616, #587, #338/#352/#353/#354,
  #306, #282, #130, #47, #78, #150, #156, #172, #547 and #201.
- **Squash-merge bodies hide review rounds.** Where a PR body claims "no findings", the merged commit message often
  records a review-round fix. I grepped every commit message for use-after-free, swept slot, unrooted, missed root and
  poison, and read the hits. That is how the #130 use-after-free and the #156 near-miss turned up; neither PR body
  mentions them.
- CI history: `gh run list --status failure` (39 runs). Only two branches failed a GC differential job. Neither failure
  was a lost root (see E3, E4).
- Cross-checked against `PRD/ARCHIVE/AUDIT_2026_08_10_PRD.md`, `PRD/ARCHIVE/AUDIT_2026_08_17_PRD.md`,
  `docs/GC_DESIGN.md` §4.5/§11 and `PRD/study/gc/DIGEST.md` §1.11.

## Headline

**Three real cases of live values being collected have reached code.** Two were fixed before merge. One is open:

| # | Where the live value hid | How it was found | Merged broken? |
|---|---|---|---|
| A1, PR #6 | Rust locals of a **second** library-loading path that started a dispatch loop which considered itself outermost | stress lane + debug poison, on the first collection of bootstrap | no |
| A2, PR #130 (`1d18c49`) | a VM continuation payload in the **weak side table**, reached only through an ephemeron retained after the weak-id loop had stopped | **code review** (no lane exercises ephemerons holding continuations) | no |
| A3, #605 | a `TaggedValue` **returned to the embedder** (`eval_str`) or held in `run_forms`'s Rust local between forms | GC research probe (prim-embed §7.2); debug poison panic | **yes, open** |

Every real case has the same shape: a value reachable only from somewhere the root providers do not enumerate (Rust
locals, a weak table's late-discovered edge, the host). No case came from a Scheme-level reachability mistake in the
tracer's per-variant rules, although the audit's B1 (below) shows that class exists latently.

---

## A. Live values actually collected (confirmed use-after-free)

### A1. PR #6, GC stage 3: a second library-loading path left its forms unrooted

- **Symptom.** VM bootstrap died on its first collection under `PATINA_GC_STRESS=1`. In release:
  `type error: expected a procedure, got object` at an unrelated call site (a swept slot read back as a tombstone). In
  debug: `use-after-free: pair slot 313`, with a backtrace naming the desugar call that read it.
- **Root cause.** `backend.rs::evaluate_parsed_library` was a second library-loading path, separate from the guarded
  one in `vm_state.rs`. It called `execute` rather than `execute_nested`, starting a fresh dispatch loop that
  considered itself outermost. Meanwhile `parsed.body`, `parsed.imports`, `lib_env` and `saved_globals` were unrooted
  Rust locals. The guard lived at call sites, and one of three call sites missed it.
- **Detection.** The differential stress lane, in a debug build with poison assertions, diagnosed it in one run
  (commit `753a904`). This is why `docs/GC_DESIGN.md` §11 tells you to run the stress lane in debug as well as release.
- **Fix.** Same PR. The `/simplify` round moved the guard **onto the data**: `ParsedLibrary` carries a `GcDeferGuard`
  for as long as it holds unevaluated body forms (`crates/patina-runtime/src/library_loader.rs:162-216`). A fourth
  path is safe by construction, and so is a `ParsedLibrary` held beyond one call. The PR also added
  `LibraryRegistry::try_roots`: a busy registry aborts the collection rather than tracing a partial root set.
- **Regression test.** No dedicated test. Bootstrap under the stress lane (`scripts/run_gc_differential.sh`, release and
  debug-poison, both backends) is the standing check.
- **Lesson.** A deferral or rooting obligation attached to a call site gets forgotten by the Nth path. Attach it to the
  object that holds the values.
- **Side effect still visible today.** The same guard means a top-level `define-library` never reaches an outermost
  safe point (#614 part 2: 32,000 forms, 1.9 GiB, one collection).

### A2. PR #130 (commit `1d18c49`): weak continuation ids and ephemerons in sequence, not one fixpoint

- **Symptom.** One `(gc)` was enough:
  ```scheme
  (define e #f)
  (define (capture)
    (let ((secret (list 'a 'b 'c)))
      (let ((v (call/cc (lambda (c) (set! e (make-ephemeron kk c)) 0))))
        (if (= v 0) 'captured secret))))
  (capture) (gc) ((ephemeron-datum e) 1)
  ```
  The VM died with `expected a procedure, got object`; the tree-walker printed `(a b c)`.
- **Root cause.** The first draft of SRFI 124 ran the ephemeron fixpoint *after* the weak-id fixpoint (from #19) had
  gone quiescent. A `VmContinuationRef` marked only by a late ephemeron retention was queued into `new_weak_ids` and
  never broadcast. Its payload in `VmState`'s side table went untraced, while `sweep_weak` kept the store entry, so the
  continuation pointed at swept slots. Appending a second weak-id pass would not have been enough, because
  `trace_weak_ids` can itself reach an ephemeron after the ephemeron loop has stopped.
- **Detection.** **Code review** of #130 caught it before merge; the author's commit says "I introduced a
  use-after-free". The GC differential lane runs only the chibi suite, which has no ephemerons, so it could not have
  caught this. The same review found the termination argument wrong ("pending shrinks each round" is false; `drain` can
  grow it).
- **Fix.** Same PR (second commit). `run_mark_phase` (`crates/patina-core/src/heap/gc.rs:1022-1101`) now runs one loop
  over both weak kinds, ending only when a round neither broadcasts ids nor retains a pair. The function is public so
  alternative collectors compose *around* it and cannot mis-order its interior. Its doc records why sequencing was
  unsound.
- **Regression test.** `crates/patina-tests/tests/ephemerons.rs::an_ephemeron_holding_a_continuation_keeps_its_payload`
  (`:195`), on both backends.
- **Lesson.** Every weak kind (ephemerons, weak side tables, and later guardians or host payloads) must share one
  fixpoint. `PRD/GC_PRD.md` §9.5 keeps this rule, and #609's O(n²) fix must preserve it.

### A3. #605 (open): a value returned to the embedder is freed by a later collection

- **Symptom.** A host keeps `interp.eval_str("(list 'held (vector 1 2 3))")`, then runs an `eval_program` that
  collects. Debug panics with `use-after-free: pair slot 15992 was reclaimed by the GC` (`heap/mod.rs:721`); release
  prints `(45264 . 45264)`. Both backends. `eval_program_resilient` frees its own result the same way when a later form
  collects and then fails.
- **Root cause.** Every `eval_*` returns a bare `TaggedValue`, which is an arena index, and nothing roots it once it
  leaves the interpreter (`crates/patina-interpreter/src/lib.rs:278` and following). `run_forms` keeps the previous
  form's value in a Rust local (`lib.rs:491`, assigned at `:517`) while the next form runs, so a safe point in that form
  can collect it (re-checked at HEAD). The embedding API has no handle or root type; the only workaround is
  `global_env().define(name, v)`.
- **Detection.** The GC redesign research (prim-embed §7.2, DIGEST §1.11 defect 1), with a probe crate. Nothing in CI
  exercises the embedding API across a collection: the differential lane drives the CLI, and the CLI never reads a
  returned value after a later form has run.
- **Fix (planned).** Owned handles `{heap id, slot, generation}` registered in a per-heap handle table that the
  collector traces. `eval_*` gains handle-returning forms (bare forms deprecated), and `run_forms` holds its running
  value in a handle. `GC_PRD.md` §11.5, stage 2. To land after #604's teardown.
- **Regression test (planned).** Both programs as tests on both backends, debug and release, under
  `PATINA_GC_STRESS=1`. A handle used with another interpreter is refused; a handle dropped after its interpreter does
  not panic or leak in the debug-poison lane.

---

## B. Missed roots found before they fired (latent; fixed or guarded)

### B1. PR #47, audit B1 (HIGH): GC never traced `CpsContinuation.resume`

- **Symptom.** None observed: GC-stress repros survived.
- **Root cause.** `trace_continuation_children` visited `body`, `env`, `dynamic_winds` and `captured_cont_env`. The new
  `resume` field, which holds consumer procedures, `after` thunks, cached promises and exception payloads, was absent.
  It stayed rooted only because every construction site happened to alias the payload through the traced
  `captured_cont_env`. That invariant was undocumented and had no poison behind it.
- **Detection.** The post-merge audit (`PRD/ARCHIVE/AUDIT_2026_08_10_PRD.md` §B1), found by reading the code.
- **Fix.** PR #47 traces `resume` and documents the aliasing.
- **Regression test.** None specific. The GC differential lanes were rerun.
- **Lesson.** A field added to a traced struct without a trace rule is invisible to the compiler. The exhaustive
  `match` in `trace_object_children` catches a new *variant*, not a new *field*. `heap/mod.rs:137-141` states the
  related rule: "a value-bearing variant misfiled as a leaf is a use-after-free, not a compile error."

### B2. PR #150 review: a third hand-rolled copy of a trace rule

- **Finding.** `trace_continuation_children` hand-rolled the body of `trace_exception_handler` instead of calling it.
  The review's words: "a third copy to go stale, and a missed root is a use-after-free". Fixed in the review commit
  (`1447b90`).
- **Related.** #5 and #282 consolidated wind, promise and library trace rules into shared `GcVisitor` helpers for the
  same reason.

### B3. PR #156 review: a stub frame's register window was the only root for a weak-table continuation

- **Finding.** `/code-review` of `2235fa43` found that `ResumeWindJump` freed the stub frame's register window. That
  window was the only root for the jump's `target` and `value`, and `target` is a `VmContinuationRef` whose payload lives
  in the *weak* store, so freeing it left both reachable from Rust locals alone. "No safe point runs in that window
  today, so this is insurance, not a bug fix."
- **Fix.** The window is deliberately kept. The comment is at `crates/patina-vm/src/runtime/vm_state.rs:1865-1875`.
- **Tension.** #172's review then *freed* a composable invoke's stub window, because keeping it leaked 27 MB per 400k
  invokes. A leak fix and a rooting rule pulled in opposite directions, and the rule is held by a comment, not a test.
  #602 ("preserve the wind-jump stub's intentional retention of its register roots") had to carry it through a
  refactor by hand.

### B4. PR #78: VM `pending_escape` rooted when introduced

- `pending_escape` parks an escaping value while the `ContinuationEscape` sentinel propagates. "While set, the value is
  reachable from nowhere else." It was traced in the same PR that introduced it, mirroring the tree-walker's
  `trace_pending_escape`. The 2026-08-17 audit re-verified it ("no safe point inside its window").

### B5. PRs #353 and #354: code freed under a live closure (non-heap premature free)

- **Hazard.** #353 releases a top-level form's compiled code when no frame, continuation or closure can run it. Closures
  name code only by id, so each `CodeObject` counts live closures (`live_closures`), decremented when the sweep frees a
  closure. "One kept too high keeps code a collection longer; one too low would free code a closure can still run."
- **Detection and guards.** Break-tests: "with the closure count ignored, the four closure-held tests fail — the code is
  freed under them". Debug asserts that a closure's code is loaded at `MakeClosure` and counted at free. #354 added a
  generation in the high 32 bits of `CodeObjectId`, so a stale id fails loudly with "missing CodeObject" rather than
  running whatever reused the slot.
- **Regression tests.** `crates/patina-tests/tests/finished_forms_release_code.rs` and `crates/patina-vm/tests/integration.rs`
  (stale id while the slot is empty and after reuse).
- **Lesson.** Generation-checked handles turn a premature free into a deterministic error; this is the same idea as
  #605's planned handles.

### B6. PR #547: spliced imports evaluated during expansion

- Imports inside a top-level `begin`, or brought in by `include`, now run during expansion, which loads and evaluates
  libraries while expansion holds unfinished datums and IR in Rust. The PR made the import callbacks "defer collection
  while expansion holds unfinished datums and IR". 10/10 new regressions passed under `PATINA_GC_STRESS=1`.

### B7. PR #28: dropping staging Vecs relies on "allocation never collects"

- Rest lists and `list_from_iter` now cons directly. "Safe because collection only runs at the interpreter loops' safe
  points, never inside alloc_pair, so partial lists need no root." This is a load-bearing invariant: DIGEST fact 1
  counts about 230–890 Rust functions that hold unrooted values across allocation, plus offheap §5.2's 19
  fresh-unrooted-across-alloc and 51 value-used-after-alloc sites. Any change that lets allocation collect turns all of
  them into A-class bugs at once.

---

## C. Precision and leak fixes whose mechanism carries premature-collection risk

### C1. #423 → PR #551: stale registers kept a replaced value alive

- **Symptom (over-retention).** After `(set! keys (reverse (reverse (list-tail keys 5))))`, a collection in the same
  frame kept the old list. Larceny's `ephemeron` suite scored 5/6 on the VM. Found by #130, since ephemerons are the
  only way Scheme can observe collection.
- **Fix, #551.** The compiler records per-PC register-root bitsets. Before collection and before capturing a full or
  delimited continuation, the VM clears finished temporaries (including stale slots in reused tail-call windows).
  Locals and runtime stubs stay conservative.
- **Why it belongs here.** This is the first place Patina deliberately *stops* tracing something the frame still holds.
  A wrong liveness bit is a premature free. #551 added tests that guard the dangerous direction:
  `earlier_operands_stay_live_across_collection_in_a_later_operand`,
  `reference_barrier_keeps_its_argument_live_until_the_call`, `a_continuation_keeps_an_earlier_operand_for_reentry`,
  `a_delimited_snapshot_retires_dead_temps_and_keeps_pending_operands` (all in `ephemerons.rs`), and
  `wide_pending_operands_survive_collection` and `rebound_control_operator_keeps_operands_after_a_larger_expression`
  (`gc_vm.rs`). It also ran `PATINA_GC_STRESS=1` on `control_flow_matrix` and `ephemerons`, plus both
  differential lanes. The PR had no recorded review comments.

### C2. PR #19: weak VM continuation side tables (ctak 4 GB fix)

- **Change.** Side tables became weak, keyed by `VmContinuationRef` objects and resolved in a mark-phase fixpoint
  (`trace_weak_ids` / `sweep_weak`).
- **Soundness argument, in the PR.** Capture inserts the entry and allocates the ref within one instruction dispatch.
  Invocation copies the payload back within one dispatch. Nested loops defer collection. So at a collecting safe point,
  an unmarked ref proves its payload unreachable. Ids are monotonic, so pruning cannot alias a later capture.
- **Risk realised.** This weak table is what A2 broke and what B3 had to protect. Its soundness rests on "no safe point
  inside the window", which only comments assert.
- **Tests.** Six weak-store tests (now under the storage owner since #602) and `gc_vm.rs::continuation_in_heap_data_survives_collections_and_invokes`.

### C3. #614 (open): `owners` only grows, so a redefined library stays alive

- The planned fix counts links into each owner and drops it at zero. Its acceptance criteria explicitly guard the
  premature direction: "`(define old f)` and a procedure that expands the old macro, both taken before a redefinition,
  still run after the redefinition and a `(gc)` in the debug-poison build on both backends."

### C4. #604 (open): heap teardown on `Interpreter` drop

- Breaking the `Rc` cycle means clearing every arena slot when the interpreter is dropped. A `SharedHeap` or
  `Rc<Environment>` clone that outlives the interpreter then sees an empty heap, and "reading a value through it is a
  use-after-free, which the debug-poison build reports". The fix documents the contract that values do not outlive
  their interpreter. It is sequenced before #605 so a late handle drop meets a torn-down heap, not a leaked one.

---

## D. GC crashes adjacent to this class (not premature collection)

- **PR #306.** Rust stack overflow *during* GC tracing on Larceny's Unicode sweep (tree-walker):
  `trace_cont_value(Local)` recursed into each captured `ContEnv`. Fixed with an iterative worklist. Regression test:
  `gc_tree_walker.rs::collection_at_deep_call_depth_preserves_suspended_values` (50,000 suspended non-tail calls, each
  value verified after collection).
- **PR #5 `/simplify` round.** Exponential `ContEnv` tracing (6.8 s at depth 26) made the stress lane unusable on
  nested programs, until the `visit_once` dedup. Guarded by `deeply_nested_continuations_collect_promptly`.

## E. Failures of the detection machinery itself

- **E1. PR #5: a vacuous reclamation test.** `repeated_collection_keeps_arena_bounded` passed with zero collections.
  It now compares with and without collecting. `(gc-stats)` gained `collections` and `last-swept`, so tests can assert
  on collector activity directly.
- **E2. PR #5: `exit_gc_defer` saturated.** Saturating "would silently re-enable collection", which is exactly the
  failure the defer counter exists to prevent. It now uses `debug_assert`.
- **E3. PR #39: the lane could never pass, then was too slow.** Upstream `(chibi test)` prints wall-clock durations,
  now normalised away. At a stress interval of 1 the suite took 103 s, and the debug lane over 35 minutes. **The
  stress interval became 16** (`PATINA_GC_STRESS_INTERVAL`), so the lane collects about every 16 allocations, not at
  every safe point. A Rust-local root window shorter than 16 allocations can slip through. The script says to override
  to 1 "when hunting a specific lost root". #302 notes a collect-every-allocation run exceeded its 30 s limit.
- **E4. PR #52: a false failure.** CI failed both GC jobs with `STRESS RECLAMATION BROKEN` / "did not actually
  collect": 1,252 collections, but pairs at 15,387 against an absolute bound of 10,000. The proof was measuring
  bootstrap's live peak. It now asserts that the 20k-cons churn grows the arena by 0.
- **E5. PRs #200 and #201: a half-strength tally guard.** A truncated or filtered suite prints a well-formed
  `2 out of 2 tests passed`, diffs clean and goes green. The lane now pins `1226 out of 1226` (anchored).
  `GC_DESIGN.md` §11 explains that an equality check "passes hardest when both sides fail identically".
- **E6. Poison blind spot (by design, `GC_DESIGN.md` §4.5).** Swept vector and string slots become `Vec::new()`, which
  is a legal value, so a use-after-free in those two arenas goes undetected even in debug. Only object-arena (`Free`
  assert) and pair-arena (`GC_POISON` assert) UAFs panic. `REVIEW_DISPOSITIONS.md` RUST-2 notes that lazy sweep would
  remove eager poison altogether, so `GC_PRD.md` adds an eager poisoning sweep, a 2-collection hole quarantine and
  `VERIFY_ROOTS` to the debug and zeal lanes.
- **E7. #606 (open): changing the trigger can make the lanes stop collecting.** An 8 MiB byte floor exceeds the
  default-mode proof's churn of about 3.2 MB, so `run_gc_differential.sh:160-175` would stop collecting. The issue
  requires each proof to guard `bytes-reclaimed > 0`.
- **E8. Coverage gap.** The differential lanes run one program (the chibi suite via the CLI). A1 is the only real case
  they caught. A2 needed ephemerons holding continuations, and A3 needed the embedding API; neither is in that suite.
  #587 (open) plans callback and prompt histories under GC stress, comparing ordinary and stress-mode observations in
  the debug-poison lane.

## F. Searched and excluded (leaks, performance, or not GC)

#338, #352 (code-store retention, fixed by #353/#354) · #606, #615 (trigger blind to bytes or external memory) · #609
(O(n²) ephemeron fixpoint; its fix must keep A2's single loop) · #611, #612, #614 part 1, #616 (retention, sweep cost
or RSS) · #113 (stale multiple-values side buffer, a VM semantics bug; its GC root was removed with it) · #342, #473,
#435 (panics in control flow or expansion, not GC) · the 39 failed CI runs other than E3 and E4 (fmt, clippy, oracle and
unrelated test failures).
