# Live values that got collected: the history, the cause, and how to stop it

**Note, 2026-10-01.** Filed after this study: A7 as #620; the do-now issues as #621–#626; comments on #605 and #609.

**The question.** "In the current implementation we have bugs where live values got collected. How do we prevent such
things from happening?"

**Inputs.** All of these are in this directory:
- `catalogue.md`: the incidents A1–A7, the near relatives B1–B6, and the classes R1–R8 and X.
- `prevention.md` and `do-now.md`: the two proposals.
- Two review rounds of those proposals. Appendix A lists every finding and what was done with it.

**Sources and measurements.**
- Source state is `gc-prd` at `3682302`, which is `main` at `28a94f8` plus PRD-only commits.
- Experiments ran on scratch copies under `scratch/`. The repository was not modified.
- Measurements were taken on 2026-10-01 on this machine (macOS, 12 cores). CI runners are slower, so treat timings as
  ratios.

**Labels.**
- **[V]** means read in source, or re-run, for this answer.
- **[M]** means measured in the scratch experiments.
- **[I]** marks an inference or an estimate.

---

## 0. The answer in brief

**What happened.**
- Seven times since collection began (stage 1, `b908f16`, 2026-07-31), a live value lost its last root.
- Each one sat somewhere no root provider looks, at a moment when a collection could run.
- Two of the seven are live today: A6 (#605) and A7 (not filed).

**What made it possible.** Every one was made possible by a rule that only a comment, a reviewer or a runtime counter
enforces:
- "values are bare `Copy` indices";
- "take a `GcDeferGuard` before you re-enter";
- "add a trace arm for each new field";
- "nothing between here and there reaches a safe point";
- "weak kinds share one fixpoint".

**How they were found.** The CI lanes caught one of the seven. Reviewers caught three. The three that reached `main`
were found by reading the code or by a research probe.

**How to prevent the next one.** There are three layers, and they need to land in this order:

1. **Make the next one loud, now.** Today a stale reference often reads as a legal value: `#()`, `""`, or whatever
   object reused the slot.
   - Make every stale reference panic in debug and `gc-check` builds.
   - Turn each comment-held rule into an assertion.
   - Put every Rust call that re-enters the evaluator under a clippy rule, so a new one cannot be added silently.
   - Give every detector a test that proves it fires, so a lane cannot go quietly blind. That has happened four times
     (#5, #164, #200, #201).
2. **Fix the live ones.** #605 has to cover every public path that hands the host a raw value or environment. Today it
   covers only the `eval_*` return values.
3. **Make the bug unwritable.** This is the redesign:
   - rooted `Owned` handles (stage 2);
   - a collect capability and a value brand, so the type system decides who may collect (stage 3);
   - continuations as heap objects (4e);
   - generated tracing (5a);
   - a `#[derive(Trace)]` for off-heap structures. The PRD lacks this, and it is where A2 and A3 actually happened.

What stays uncatchable is in §5. The largest part is a missing trace edge that is masked by aliasing (A3). Only
generated tracing, or a test that removes the alias, sees it.

---

## 1. The history

| Case | What lost its root | Class | Detected by | How late | Reached `main`? | Regression test |
|---|---|---|---|---|---|---|
| A1 (#6) | Library body, imports, environment and saved globals, held in Rust locals by a second loading path that called `execute` (a loop that believed it was outermost) | R2 Rust frame across a re-entry | stress lane + debug poison: `use-after-free: pair slot 313` at bootstrap's first collection | hours, same day, pre-merge | no | none dedicated; bootstrap under `run_gc_differential.sh` |
| A2 (#38) | Values reachable only through `CompiledMacro.definition_env` or an `Environment.alias_bindings` target; neither edge was traced | R4 edge with no trace rule | `/simplify` review, by reading | on the branch, pre-merge | no | none (`macro_definition_env.rs` never collects) |
| A3 (#47) | `CpsContinuation.resume`: consumers, `after` thunks, promises, exception payloads. Untraced, but nothing was ever freed, because every construction site aliased it through the traced `captured_cont_env` | R4 | post-merge audit, by reading the trace against the struct | about 1.6 days on `main` (2026-08-09 to 08-11), latent | yes, latent | none; no failing program exists |
| A4 (#130) | A VM continuation payload in the weak side table, retained by an ephemeron only after the weak-id fixpoint had stopped, so its payload was never traced | R5 weak-processing order | PR review; the author then reproduced it (`expected a procedure, got object`) | pre-merge | no | `ephemerons.rs::an_ephemeron_holding_a_continuation_keeps_its_payload` |
| A5 (#156) | `ResumeWindJump`'s target handle and value, left in Rust locals after their register window was freed | R3 value parked off-root across the next safe point | `/code-review` ("insurance, not a bug fix") | pre-merge; no safe point reaches the window today | no | none possible today |
| A6 (#605) | `eval_*` results held by the host; `run_forms`' previous value while the next form runs; `eval_program_resilient`'s result | R1 holder above the outermost loop | the GC study's probe | **about 2 months, live** (explicit `(gc)` since 2026-07-31, automatic since `7401cba`, 2026-08-03) | **yes, live** | planned in #605 |
| A7 (unfiled) | Bindings of a host-built `Environment::with_parent(global)` passed to `Backend::eval`. Masked on the VM only because `VmBackend::eval` ignores `env` | R1 | the same probe | **about 2 months, live** | **yes, live** | none |

**Who found them.**

| Channel | Incidents |
|---|---|
| Lanes | 1 (A1) |
| Review | 3 (A2, A4, A5) |
| Post-merge audit | 1 (A3) |
| Research probe | 2 (A6, A7) |

**Regression tests.** Five of the seven fixes or open incidents have no regression test.

**Around the incidents** (catalogue §2.2–§2.4):
- **Six near relatives.** In each, a change created or traded a liveness obligation and no bug shipped.
  - B1, #172: a freed window, argued safe in a comment.
  - B2, #150: a third hand-copied trace.
  - B3, #19: the weak tables created.
  - B4, #551: register retirement.
  - B5, #353, #354 and #483: code release.
  - B6, #10 and #567: tables keyed by raw bits.
- **Fourteen protections** were added with the holder they protect. The progression is the lesson:
  1. a guard placed at a call site was forgotten (A1);
  2. a guard owned by the data (`ParsedLibrary`), or bound to the operation (`with_globals`), cannot be forgotten;
  3. moving the value into the machine (`Step::Call`, `resume_stub`) removes the holder altogether.
- **Six detector failures**, fixed. Four of them were a test or lane that could not fail:
  - #5: a bound test passed with zero collections;
  - #164: a GC test passed with the trace deleted;
  - #200: the differential lane compared 4,537 identical import failures;
  - #201: a tally guard accepted any tally.

---

## 2. Why these bugs keep happening

### 2.1 The safety condition, and which part each class breaks

| Part of today's safety condition | Classes that break it | Incidents |
|---|---|---|
| Collection runs only at the top of an outermost driver loop, with a complete root set | R1 (the holder is above the loop), R2 (a loop wrongly believes it is outermost), R8 (partial or foreign root set) | A6, A7, A1 |
| At that moment, every live value is in a root | R3 (parked off-root), R6 (liveness precision dropped it) | A5 |
| The trace from the roots reaches it | R4 (an edge with no rule), R5 (weak resolution ran in the wrong order) | A2, A3, A4 |
| After sweep, nothing names a freed slot | R7 (raw bits or an index outlive the sweep) | none yet |

### 2.2 The design properties that make them possible

1. **A value is a bare `Copy` arena index.** It carries no lifetime, no brand and no generation, so a Rust variable
   holding one looks exactly like a root.
   - The embedding API returns these indices (A6), and accepts host-built environments (A7).
   - The invariant is stated about *where collection runs*, not about *who holds values*. So it says nothing about
     frames above the loop.
2. **"May this call collect?" is visible in no type.** It is a run-time counter, and each call site has to remember to
   raise it (A1).
   - Today's safety for nested loops is a policy ("they never collect").
   - Ten Rust holders, plus the `Rc` container holders, survive only because of that policy (catalogue R2 latent 1–10
     and 13). One example is the `TaggedValue`s that primitives hold across `ctx.apply_proc` in `parameters.rs`,
     `lazy.rs`, `values.rs`, `lists.rs`, `ports.rs` and `file.rs` [V].
3. **Trace rules are written by hand, across the arenas and `Rc` structures** (`Environment`, `CompiledMacro`,
   `CpsContinuation`, `ContValue`, wind, handler and prompt records).
   - The exhaustive `match` catches a new *variant*, not a new *field*.
   - A miss is usually hidden by a second path to the same value, which makes it invisible to every dynamic check
     (A3).
4. **Some soundness arguments rest on where safe points happen to be, and are held only by comments.** Examples:
   `gc_roots.rs:21-28`, B1's window at `vm_state.rs:1815-1832` [V], and A5's window. Adding a poll can silently break
   them.
5. **Weak payloads live outside the heap, in a provider's side table** (A4). Each weak kind arrived with its own loop.
6. **The detectors are blind exactly where these bugs land** (class X):
   - **Legal-looking leftovers.** A swept vector or string reads back as an empty value. A reused slot reads as its new
     occupant, and reuse is last-in, first-out, so it comes soon.
   - **Bypasses.** Nine object-arena accessors bypass the `Free` check (`heap/mod.rs:970, 1085, 1355, 1370, 1385, 1400,
     2875, 2890, 2903` [V]), and `frame_globals` silently falls back to the global environment.
   - **The collector absorbs dangling references.** Sweep pre-marks the free list and ignores whether a slot was
     already marked (`gc.rs:836-838` [V]). GC_DESIGN §11 item 5 planned that assertion; it was never written.
   - **Narrow stress.** Stress runs only the chibi suite, only through the CLI, and only at interval 16. It cannot
     reach the embedding API (R1) or paths the suite never runs.
   - **Hand-placed collections.** Every test places its `(gc)` by hand.

---

## 3. Prevention: construction, detection, process

Effect codes:
- **P**: prevents; the bug cannot be written.
- **F**: flags; the site needs an explicit, reviewable decision, but nothing makes the decision.
- **C**: catches; a test or lane fails on the bug.
- **C\***: catches, given a test of the right shape.

### 3.1 By construction (the redesign, plus its gaps)

| Measure | Effect on cases | PRD stage | In the PRD? |
|---|---|---|---|
| `Owned {heap_id, index, generation}` handles from `eval_*` and `run_forms`. The handle table is **traced**: each handle is visited and enqueued, not marked like the symbol table | **P** A6; R1-L3 (#604, use after teardown) | 2 (#605) | yes |
| **Every** public path that yields a value or environment returns a handle: `global_env().get` (R1-L1), host-built environments (A7), `backend()` and `evaluator()` restricted or documented as raw | **P** A7, R1-L1, R1-L2 | 2 | **gap**: the PRD puts a namespace handle at 4b (DESIGN E.3), and #605 names only `eval_*` |
| `CallFrame.closure` as a value | **P** stale closure indices in continuation snapshots (an A4-adjacent shape; R7) | 2 | yes |
| Collect capability: `&mut Heap` collects, `Cx<'gc>` cannot. `Value<'gc>` is branded. A nested entry under a `Cx` is `NoGcScope` by type. Trybuild cases pin the brand | **P** A1 and R2 outside the VM and tree-walker cores; R2-L11, L12; R8-L1 | 3 | yes |
| Move each Rust holder into the machine (`Step::Call` state, `RootScope` for loading). `%parameterize-swap!` stops calling back | **P** R2-L2 to L9 | 2 (loading), 4a, per site | partly |
| Continuations as heap objects. The weak side tables and `VmContinuationRef` are deleted | **P** A4's shape; **P** R3-L1; A5's damage shrinks to an ordinary lost value | 4e | yes |
| `return_into(frame)`: the only way to write into a suspended frame | one choke point for R3 | 4d | yes |
| One weak fixpoint as a collector contract, with a conformance suite on `MarkRegion`, `NullGc` and `TestModel` | **P** A4, R5-L2 | 5a (A4's fix already merged the loops) | yes |
| Traced code liveness | **P** R6-L4 | 4e | yes |
| `declare_layouts!` generates the trace of heap kinds | **P** misfiled leaf arms, R4-L2 | 5a | yes |
| **`#[derive(Trace)]`** for every host payload and every struct a root provider walks: `Environment`, `CompiledMacro`, `CpsContinuation`, `ContValue`, wind, handler and prompt records, `VmState`, `Library`. A field is skipped only with `#[trace(skip, reason)]` | **P** A2, A3, B2, R4-L1 (`foreign_expansions`) | — | **gap**. §14's `HostPayload::trace` is hand-written. Land it before 4f; the tree-walker never moves, so `move-all` never covers it |
| `PENDING_ESCAPE` leaves `thread_local!`; per-heap tables; `heap_id` checked at boundaries | **P** R8-L2 to L4 | 2–3 | yes |
| Allocation never collects | **P** R2-L14 (the 107-site census) | standing rule (K16) | yes |

### 3.2 Detection (all can land on today's collector)

| Measure | Effect on cases | When | PRD |
|---|---|---|---|
| Panic when marking reaches a free slot | **C** write-back of a stale value (shapes of A1, A6, A7) at the next collection, even if nothing reads it | now | GC_DESIGN §11.5, never written; verifier at 5a |
| Freed-slot bitset in every accessor, plus 16-bit generation stamps in `TaggedValue`, checked by the accessors and the marker | **C** all five probe shapes of A6 and A7, including a reused slot; A1; A4 at invoke; A2 if the swept value is read. **Not** A3, which never freed anything | now; replaced by the 5b poison sweep | **gap** until 5b |
| **Positive controls**: unconditional tests that each detector fires | **C** a detector switched off by a missing check, a cfg typo, or a CI binary built without the feature (class X) | now | **gap** |
| clippy `disallowed-methods` on every re-entry API, with a reasoned `allow` per site | **F** A1's class on every path, executed or not; the static holder list for stages 2 and 4e | now; the brand supersedes it in safe crates at 3 | only the `thread_local!` check (C9, stage 3) |
| Recursive sentinel tests: each traced struct built by literal, with a fresh value in every value-bearing field, rooted only through it; collect, then read every field | **C** A2, A3 and B2 shapes, because building by literal breaks the alias that hid A3 | now | **gap** |
| Field-exhaustive destructuring in trace code (no `..`) | **F** A2, A3, R4-L1: forces a decision, does not make it | now; derive later | gap |
| Deferral assertions: defer depth 1 at collection; holder guards that assert no collection happened during their extent; balance checked by `assert!` | **C** R2-L11, L12, holder misuse | now; `NoGcScope` at 3 | partial |
| `AssertNoGc` around the four comment-held windows, **asserted at the poll sites** | **C** A5 and B1 shapes the day a poll lands inside them | now; zeal-`entry` at 3 | **gap** |
| `DEAD_SLOT` for retired registers, checked at every read **and copy** | **C** R6-L1 to L3 at pcs where a collection runs | now | 4d/5b; earlier here |
| Liveness recomputation when a unit loads (a backward dataflow pass checks every map) | **C** R6-L1, L2 wherever a test program compiles, whether or not a collection lands there | after `DEAD_SLOT` | **gap** |
| Zeal: collect at every outermost safe point | R6 at pcs no allocation precedes; R3 if a poll lands in a window | now, on a subset | `entry` form at 3 |
| More programs under stress: `cargo test` targets, Larceny, an embedding test | **C** A6 and A7 (embedding); **C\*** the shapes those suites contain | now | partly (§16 scoreboards, not under stress) |
| Type-directed holder census (fixpoint over field types after alias expansion) | **F** R4-L3, hidden references | stage 0 inventory | census **gap** |
| Verifier, `VERIFY_ROOTS` | **C** dangling roots; A4's retained-but-untraced entry. **—** hidden references (it sees only reported words) | 5a | yes; §20 overstates it |
| `move-all` with `PROT_NONE` | **C** a missed slot in VM heaps even when aliased; R7-L2 | 8 | yes; VM heaps only |
| #587 program generator under zeal; embedding-API fuzzer | **C** A4, A6 and A7 shapes systematically | later | not scheduled |

### 3.3 Process

Put these in AGENTS.md's "When Adding Features" and GC_DESIGN §5. No new file is needed.

**Checklists.** Each is keyed to a class, and each names the test it requires:
- a new heap kind or traced field (R4): a full destructure and a sentinel test;
- a new off-heap holder (R2, R3, R4): choose one fate (traced, carried in the machine, or covered by an `AssertNoGc`
  window), add an inventory row, and add a test where the holder is the only reference;
- a new re-entry call (R2): a clippy `allow` with a reason, plus a guard on the data or a `Step::Call`;
- a new weak kind (R5): it joins the one fixpoint;
- a new precision optimization (R6): an assertion that turns a wrong answer into a panic, break-tests, and a kill
  switch;
- a new raw-bits table (R7): its pruning story;
- a new embedding API (R1): handles, plus a hold-across-collection test.

**Review rules.**
1. A comment that argues "no safe point here" comes with an `AssertNoGc` at the same lines.
2. A GC test states which edge it exercises, and why the value is reachable only through that edge (#164's rule).
3. A premature-collection fix lands with a test that fails without it. Where no failing program exists (A3, A5), it
   lands with an assertion that would fire.
4. A leak fix that frees a root (B1) needs the same evidence as a rooting fix.
5. A GC PR names its class (R1–R8) in its body.

**Issues first.** A7 is a live defect with no issue. Under the project's rule it is filed, or added to #605, before any
fix.

### 3.4 Sequencing against the PRD stages

Three stages move collection points. Each can turn a latent hazard into a bug:
- stage 1's byte trigger;
- stage 2's loading points A, B and D;
- stage 4e, which lets driver-level nested loops collect.

**Before stage 1.** Have the stale-reference checks, their positive controls, and the release `gc-check` lane in CI.

**Before stage 2.** Every loading-path entry in the clippy allow list has been converted to a `RootScope`, a guard on
its data, or machine state.

**Before stage 4e.** Every `ApplyContext::apply_proc` and `across_reentry` site must be on the side that stays deferred,
or hold its values in the machine.
- On the VM, `apply_proc` *is* `across_reentry` (`control.rs:2264-2277` [V]). That is the function 4e lets collect.
- The PRD (§11.3) keeps `apply_proc` fallbacks under `NoGcScope` for good. So 4e has to key the lift on where the entry
  came from, and the clippy list is how to check every site.
- A dynamic "ignore-defer" run validates that list, but cannot prove it complete. Only paths a lane executes can trip
  it. For example, `ParsedLibrary::new(heap: None)` has no caller that passes `None` (`library_support.rs:160` [V]).

**Proposed PRD amendments.** Each is one line:
- **§19.** No stage moves a collection point before the detectors that would see its failure are in CI.
- **§11.5 and stage 2.** Add the host-environment and global-read handles.
- **§14.** Add `derive(Trace)` for host payloads.
- **§20.** Drop `VERIFY_ROOTS` from the hidden-references mitigation.

---

## 4. The ranked do-now list

The ranking is by cases caught per unit of cost. The exception is #605: it is the only fix for the two live bugs, so it
is scheduled on its own track whatever its rank.

| # | Measure | Catches | Cost | Issue |
|---|---|---|---|---|
| 1 | **Panic when marking reaches a free slot** (`sweep_arena`, `gc.rs:836-838`), with a unit test that marks from a root holding a freed slot and expects the panic | the write-back shapes of A1, A6 and A7, at the next collection | 3 lines plus 1 test; one branch per free slot per collection; no false positive on chibi under stress on either backend [M] | new issue A (first commit) |
| 2 | **Stale-reference checks** in debug and `gc-check` builds. Details below the table | A6 and A7 (all five probe shapes, including a reused slot), A1, A4 at invoke, A2 if read; makes item 1's class panic at the read too | about 200 lines plus about 1 day of tests. Release `gc-check`: within noise at stress 16; **+13–24% at stress 1**. Debug as first written: +28% VM, +9% tree-walker, to be reduced [M] | new issue A |
| 3 | **clippy `disallowed-methods` on every re-entry API.** Details below the table | **F** every new A1-shaped call, executed or not; the static holder list that stages 2 and 4e need | about 1 day. On the pinned 1.97.1 [V] it flags trait calls through `&dyn`, generic and concrete receivers; a reasoned `allow` silences it; paths into crates a crate does not depend on are ignored, so one workspace `clippy.toml` works | new issue B |
| 4 | **#605, widened.** Details below the table | A6, A7, R1-L1 to L3 | moderate (days to a week [I]); gated on #604; PRD stage 2 | #605 (amend), or a sibling issue for the non-`eval_*` surface |
| 5 | **Name every traced field, and test each with a sentinel.** Details below the table | A2, A3 and B2 shapes (**C** via the sentinel; **F** via the destructure); decides R4-L1 (`foreign_expansions`, `exp4` [M]) | 1–2 days of destructuring plus 1–2 days of tests | new issue C |
| 6 | **Assert the deferral protocol.** Details below the table | R2-L11 and L12, holder-guard misuse, R8-L1, R2-L8. It does **not** certify stage 4e: L2–L7 have no guard of their own (no `GcDeferGuard` in `patina-primitives` [V]); item 3's list covers them | about 15 lines plus hours | new issue D |
| 7 | **`AssertNoGc` scopes** on the four windows held only by comments. Details below the table | A5 and B1 shapes the day a poll is added inside one | hours | issue D |
| 8 | **`DEAD_SLOT` for retired registers, plus a zeal lane.** Details below the table | R6-L1 to L3 at pcs where a collection runs | about 30 lines; zeal subset about 7.5 min locally (8.5–9 on CI [I]); the debug cost of the compare is not yet measured | new issue E |
| 9 | **The release GC lane runs a `gc-check` build at stress 1**, and runs item 2's positive controls in the same job, so the lane proves the feature is compiled in | item 2's classes at 14× the collection density (129,012 against 9,308 collections) [M] | +4.5 min locally; about +5–5.5 min on CI (calibrated from CI run 36819314497) | issue A |
| 10 | **More programs under stress.** Details below the table | the shapes chibi lacks: ephemerons, prompts, callbacks, the matrices | per PR about 45 s; Larceny nightly 1–2 days plus CI time | new issue F |

**Item 2, in detail.**
- A freed-slot bitset checked by every arena accessor. That includes vectors and strings, and all nine bypassing
  accessors, the promise write at `:1085` among them.
- 16-bit generation stamps in `TaggedValue` bits 40–55, checked by the accessors and by `GcVisitor::visit`.
- In check builds, a generation stored beside `CallFrame.closure` and checked in the `*_vm_closure_*` accessors. The
  closure index in a continuation snapshot otherwise reads a reused slot silently. Stage 2's "closure as a value"
  replaces this.
- Unconditional positive controls, using `#[should_panic]`:
  - every accessor family, read after a collection with an empty root set;
  - a stale read after the slot is reused;
  - item 1's panic.

**Item 3, in detail.** The APIs to list:
- `ApplyContext::apply_proc`, with its 11 sites [V];
- `run_synchronously`, `across_reentry`, `execute` and `execute_nested`;
- `run_loop_until` and `run_loop_until_outcome`;
- `run_trampoline`, `Backend::eval` and `Interpreter::eval_*`;
- `Environment::with_parent` outside `patina-core`.

Add `disallowed-macros` for `thread_local!`. That makes about 20 existing sites, each with `#[allow(..., reason)]`.

**Item 4, in detail.**
- The handle table **visits** its handles, so they are traced. It must not mark them only, the way the symbol table is
  marked (`gc.rs:449-465` [V]).
- Add A7.
- Add the write-back shape.
- Cover `global_env().get`, `backend()`, `evaluator()` and `with_parent`.
- Add a table-driven test over all **10** public `eval_*` methods, which fails when a new one appears.
- Add a list whose tail is reachable only through a handle.
- Run the acceptance tests in the check build.

**Item 5, in detail.**
- No `..` in trace code.
- A literal-built sentinel test for every traced struct, recursively. That covers `HeapObjectData` variants,
  `CpsContinuation`, `ContValue`, `CompiledMacro`, `Environment`'s edges, and the `VmState`/`ExecutionState` records.
- A CI grep that requires a test name next to each `field: _`.

**Item 6, in detail.**
- `debug_assert!(defer_depth == 1)` at collection (in `exp4`, no false positive [M]).
- A `GcDeferGuard::holding` constructor, which asserts on drop that no collection happened. It applies to the holder
  guards only: `ParsedLibrary`, `desugar_with_imports` and `with_globals`.
- The balance check becomes `assert!`.
- An observable-deferral test (`parameterize` with the old value only in `olds`), and a repaired version of the drifted
  `collection_inside_higher_order_primitive`.
- The collector entry points become crate-private.
- `ParsedLibrary::new` takes a heap that is not an `Option`.

**Item 7, in detail.**
- The four windows: capture to insert (`vm_state.rs:631-656`), invoke to copy-back, B1's freed window
  (`:1815-1832` [V]), and A5's retained target.
- Assert at the poll sites: the top of `run_loop_until_outcome`'s loop, before `maybe_collect`, and the tree-walker's
  trampoline loop. Not in `safe_point`, because `maybe_collect` returns before reaching it unless a collection is
  pending and the loop is outermost (`vm_state.rs:1287-1290` [V]).

**Item 8, in detail.**
- `retire_registers` writes a `DEAD_SLOT` sentinel under `GC_CHECK`.
- It is asserted in `reg_at`, in `call_closure_from_regs`' argument copies and rest-list iterator (`control.rs:185-205`
  [V]), in `store_args_in_window`, and in the heap write paths.
- The mutation test is extended to a variadic call's argument register.
- `GcMode::Zeal` runs on `tests/scheme/control/` without `tail-recursion.scm`.
- Load-time liveness recomputation is the follow-up (about 1 week [I]). It is the only check on the register maps that
  does not depend on where collections happen to land.

**Item 10, in detail.**
- **Per PR:** 13 GC- and control-relevant `cargo test` targets under stress 16 in the debug check build (about 45 s
  [M]). First make the five tests that react to the process-wide variable choose their GC mode per interpreter.
- **Nightly:** both Larceny lanes, at per-suite intervals. This needs work CI does not have today:
  - a `schedule:` trigger, since `ci.yml` has push and pull_request only [V];
  - a pinned fetch step, since the suites are LGPL and not vendored;
  - a harness that compares per-suite tallies against a pinned baseline, since the script exits 1 whenever a suite is
    not fully clean, and 8 of 33 are not [V];
  - leaving out `ephemeron` until #609 lands.
- The tree-walker and `--r6rs` lanes have not been measured under stress, so cost them before quoting a budget.

**Issues to file, in order.**
- **A: "GC: make every stale reference panic in debug and `gc-check` builds"** (items 1, 2 and 9, plus the positive
  controls).
- **#605**: widen the acceptance as in item 4, and file A7.
- **B: "GC: list every Rust re-entry into the evaluator under clippy"** (item 3). Its allow list feeds the stage 2 and
  4e holder conversions.
- **C: "GC: name every traced field, and test each with a sentinel"** (item 5). Decide `foreign_expansions` there or
  under #614.
- **D: "GC: assert the deferral protocol and the no-collection windows"** (items 6 and 7).
- **E: "VM: `DEAD_SLOT` for retired registers, and a zeal lane"** (item 8). The recomputation is a second phase.
- **F: "CI: run more programs under GC stress"** (item 10).
- **#609**: add a naive-fixpoint mark oracle to its acceptance. That rewrite is the one planned change that can
  reintroduce A4.

A, B, C and D are independent of one another, and each is about a day to two. Item 9 follows A; item 10 follows A.

**Evaluated and kept for later:**
- `derive(Trace)`: about 1 week; **P** for R4. Do it before stage 4f.
- The load-time liveness recomputation.
- The type-directed holder census: a syn or rustdoc-JSON script, seeded on `alias_bindings` and `resume`, which the
  grep pattern in `prevention.md` misses.
- Mutation testing of the trace code (it needs approval as a tool).
- The #587 generator, and an embedding-API fuzzer.

**Rejected:**
- **Quarantine of freed slots.** Any FIFO deep enough to matter fails the reclamation proofs in
  `run_gc_differential.sh`, which require `pairs < 150000` and `grown < 256` [V]. At K=2 it already failed with 186,711
  pairs [M].
  - Never reusing a slot under zeal cost 36% on `callability.scm` (18.6/18.8 s against 25.4/25.4 s, two interleaved
    runs, re-measured [V]).
  - It cost 88% on `prompts.scm`, and `tail-recursion.scm` was killed unfinished at 1200 s, more than 4.3× [M].
  - Generation stamps catch reuse at any distance.
- **A "verify" re-trace with the same rules.** It is blind to the same missing edges.
- **Collection epochs stamped into values.** They check the wrong predicate.
- **An environment-variable poison switch in release.** It taxes the shipped binary.
- **A Dylint MIR lint.** It would take 2–4 weeks and a nightly toolchain, it is noisy, and stage 3 makes it obsolete.

---

## 5. What remains uncatchable, and why

1. **A missed trace edge masked by aliasing (A3's shape).** Every dynamic check, the verifier and `VERIFY_ROOTS` walk
   the same edges, and the value is still reachable another way.
   - Only generated tracing prevents it.
   - Only a test that removes the alias catches it.
   - Destructuring forces a decision, but `resume: _ // aliased` compiles, and `exp4` showed exactly that.
   - Until `derive(Trace)` lands, a wrong decision carrying a plausible comment passes until the alias breaks.
2. **Host code outside the workspace (R1).** Clippy does not lint it, and no CLI lane drives it. For R1 the only defense
   is the handle types and the removal of the bare-value forms.
3. **Unexecuted paths.** The dynamic detectors see only what some lane runs.
   - Item 3 turns R2 sites into compile-time decisions, but cannot judge whether a given `allow` is right.
   - Stage 3's brand removes the class only outside the VM and tree-walker cores.
4. **A new parked window inside the VM or tree-walker cores (R3).** Creating one still depends on review. Only its
   *activation* (a poll landing inside it) is caught, by `AssertNoGc` if annotated, or by zeal-`entry` at stage 3.
5. **Register-map errors at pcs where no collection runs (R6),** until the load-time recomputation lands. A precision
   analysis of a new kind (#614's owner count) needs its own assertion.
6. **Stale references the stamps cannot see:**
   - a bare index without a generation, until stage 2 makes `CallFrame.closure` a value;
   - a slot freed exactly 65,536 times between a value's creation and its read, where the 16-bit stamp wraps [I];
   - a value from another interpreter's heap. Generations start at 0 in every heap [V], so only #605's `heap_id`
     refuses it.
7. **A stale reference that is never read and never reached by marking.** It is harmless by definition, but
   unobservable.
8. **Production builds.** The checks exist only in debug and `gc-check` builds. A premature free in the shipped binary
   still reads whatever reused the slot. That is why the lanes have to run the check build.
9. **The detectors themselves.** A positive control proves that a detector fires on the shapes it tests, not on all
   shapes.

---

## Appendix A. The review findings: what was verified and what changed

The two review rounds made 17 findings. Two pairs were duplicates (A3's credit, and `DEAD_SLOT`'s copy paths), and one
finding (the census and `VERIFY_ROOTS`) is split into two rows here, which gives 16 rows. All 16 held up; one count in them was off by one (there are 10 public `eval_*` methods, not 11).

| Finding | Verified | What changed in this answer |
|---|---|---|
| A3 is credited to the stale-reference checks and to destructuring "P", but nothing was ever freed | yes: `gc.rs:807-813` calls the trace "redundant"; the catalogue says no failing program exists [V] | A3 is credited only to the sentinel tests (**C**) and the derive (**P**). Destructuring is scored **F** (§3, §4 item 5, §5 item 1) |
| The detectors have no positive control; every acceptance test is an embedding test that would be ignored | yes, by reading `do-now.md` §5 | unconditional positive controls added to items 1, 2 and 9 |
| #605 covers only the `eval_*` return values | yes: #605's fix direction and acceptance name `eval_*`, `run_forms` and `eval_program_resilient`; `global_env` `lib.rs:532`, `backend` `:562`, `evaluator` `:622`, `Environment::get` `:953`, `with_parent` `:571` [V]. There are **10** public `eval_*` methods, not 11 [V] | item 4 widened; §3.1 gap row |
| Ignore-defer cannot prove the holder list complete; no `GcDeferGuard` in `patina-primitives`; L2–L7 hold values across `apply_proc` | yes: grep finds no `GcDeferGuard` in `patina-primitives`; 11 `apply_proc` sites in 8 files [V] | static list via item 3; ignore-defer is validation only; item 6 rescoped (§3.4) |
| Nothing static in do-now; `prevention.md`'s clippy list misses `apply_proc` | yes. I also tested the pinned clippy 1.97.1: it flags `apply_proc` through `&dyn`, generic and concrete receivers, and a reasoned `allow` silences it [V]. `call_any_sync` and `run_thunk` do not exist in today's source [V], so the list uses current names | item 3 added at rank 3; item 7 added |
| The census pattern misses `alias_bindings` (`environment.rs:138, 513`) and `resume` (`continuation.rs:136`) | yes [V] | census made type-directed; listed as later work |
| `VERIFY_ROOTS` cannot see hidden references | yes, by its definition in PRD §16; PRD §20 nonetheless lists it [V] | §3.2 row; PRD amendment proposed |
| "Frames are roots" is false for continuation snapshots; `CallFrame.closure` has no generation | yes: `VmContinuation.frames` (`types/continuation.rs:161`), `closure: Option<HeapIndex>` (`types/mod.rs:50`) [V] | the snapshot generation is in item 2; stage 2 replaces it |
| Generation stamps do not refuse another heap's values | yes: per-slot `Vec<u16>`, starting at 0 [V] | the claim is dropped (§5 item 6) |
| `DEAD_SLOT` is checked only in `reg_at`; argument copies and rest lists skip it | yes: `control.rs:185-205` copies registers raw [V] | item 8 asserts at the copies and heap writes; the load-time recomputation is named |
| The #605 note "mark exactly as `symbol_table`" would recreate A6 | yes: those tables are mark-only, with no worklist push (`gc.rs:449-465`) [V] | item 4 says the table visits its handles, and adds a tail-reachability test |
| `prevention.md` still recommends quarantine | yes: the reclamation bounds are at `run_gc_differential.sh:145,169` [V]; never-reuse cost re-measured on `callability.scm`, +36% against the review's +39% [V] | rejected (§4) |
| Cost wording and noise | yes: `exp2` and `exp3` differ in no tree-walker file, yet the tree-walker ran 150.6 s against 162.0 s (7.6%) [V] | stress-1 overhead stated as 13–24%; the debug `DEAD_SLOT` cost is marked unmeasured |
| `AssertNoGc` inside `safe_point` is unreachable on the VM's usual path | yes: `maybe_collect` returns early (`vm_state.rs:1287-1290`) [V] | item 7 asserts at the poll sites |
| The Larceny nightly needs infrastructure | yes: no `schedule:` in `ci.yml`; the script exits 1 on any unclean suite [V] | item 10 lists the three pieces |
| The tree-walker zeal run is reported as "stopped" | it completed: 1226 of 1226 in 1120 s (`exp2`, whose tree-walker code is identical to `exp3`'s) [V] | corrected wherever zeal cost is cited |

## Appendix B. Evidence

All paths are under `scratch/`:
- `exp*.diff`: the experiments against `base`.
- `outs/` and `timing-batch.txt`: the chibi lanes.
- `review/`: the reviewers' runs.
- `final-verify/q*/`: the never-reuse re-timing.
- `final-verify/clippydyn/`: the clippy trait-method test.
- `probe*/`: the embedding probes for A6, A7 and write-back.
