# Premature collection in Patina: merged catalogue and root-cause taxonomy

Inputs, all in this directory: `git-cases.md` (git history), `gh-cases.md` (issues and PRs) and `defenses-hazards.md`
(today's defenses, the probe, and the hazards). Source state is `28a94f8` (`gc-prd` is `main` plus one PRD-only
commit). Every commit hash and date below was re-checked with `git log` on 2026-10-01. `2235fa43` is a squashed branch
sub-commit of #156 and is not in `main`'s history.

Terms:
- **Incident.** Code was written in which a live value lost its last root, whether or not a program observed it.
- **Near relative.** A change that created, discharged or traded off a liveness obligation, with no incident.
- **Protection.** A root or guard added by the same change that introduced the holder, so no bug shipped.
- **Latent.** A hazard in today's tree with no incident yet. These come from `defenses-hazards.md`, whose [V] (read in
  source or reproduced) and [I] (inference) labels are kept.

---

## 1. Headline

1. **Seven incidents** have occurred since GC stage 1 (`b908f16`, 2026-07-31). Before that date the heap never freed a
   slot, so no earlier history matters.
   - Four were fixed before merge: A1, A2, A4 and A5.
   - One (A3) was on `main`, latent, for about 1.6 days.
   - **Two are live today:** A6 (#605) and A7, a host-built environment the probe found, which is not filed.
2. **The seven incidents share one fact.** Each live value sat somewhere no root provider enumerates, at a moment when a
   collection could run. The classes differ only in where the value hid, and in why nothing forced it into a root.
3. **The lanes caught one of seven** (A1, by the stress lane plus debug poison).
   - Review caught three: A2, A4 and A5.
   - A post-merge audit caught one: A3.
   - The GC study's probe caught two: A6 and A7.
   - **The three that reached `main` were found by reading or by probing, never by a lane.**
4. **Each enabling property is a rule enforced by a comment, by review or by a runtime counter.** None is enforced by a
   type or a checker.
   - "Values are bare `Copy` indices."
   - "Take a `GcDeferGuard` before you re-enter."
   - "Add a trace arm for each new field."
   - "Nothing between here and there reaches a safe point."
   - "Weak kinds share one fixpoint."

The safety condition today has four parts, and the classes group by which part failed:

| Part of the safety condition | What must hold | Classes that break it |
|---|---|---|
| Where collection may run | only at the top of an outermost driver loop, with a complete and correct root set | R1 (the frame is above the loop), R2 (a loop wrongly believes it is outermost), R8 (the root set is partial or belongs to another heap) |
| A live value is in a root at that moment | registers, providers and traced side tables | R3 (parked off-root across the next safe point), R6 (precision dropped it) |
| The trace from the roots reaches it | per-type trace rules and the weak fixpoint | R4 (edge with no rule), R5 (weak resolution ran in the wrong order) |
| After sweep, nothing names a freed slot | keys and ids are pruned or generation-checked | R7 (raw bits or an index outlive the sweep) |

### Class table

| Class | Incidents | Near relatives / protections | Latent today | First detector of the incidents | Enabling design property |
|---|---|---|---|---|---|
| **R1** Holder above the outermost loop (embedder, host) | **2**: A6 #605, A7 child env (both live) | open #604; protections: none | 3 | GC study probe, ~2 months after automatic collection | `TaggedValue` is a bare `Copy` index; the embedding API returns it with no root or handle type; the lanes drive only the CLI |
| **R2** Rust frame holds values across a re-entry into the evaluator | **1**: A1 #6 | 6 protections | 14 | stress lane + debug poison, within hours | "may this loop collect?" is decided at run time by a counter that each call site must remember to raise; the Rust stack is not a root |
| **R3** Value parked off-root across the next safe point | **1**: A5 #156 (latent) | B1 #172; 2 protections | 2 | `/code-review`, pre-merge | removing a root and handing its value on are separate steps; soundness is argued from where safe points are and held by comments |
| **R4** Edge or field with no trace rule | **2**: A2 #38, A3 #47 | B2 #150; 7 protections | 3 | review (`/simplify`) and post-merge audit, by reading | trace rules are hand-written per type, across arena data and `Rc` structures; the exhaustive `match` checks variants, not fields; a redundant path hides the miss |
| **R5** Weak-processing order | **1**: A4 #130 | B3 #19 | 3 | PR review | weak payloads live in a provider's side table outside the heap; each weak kind brought its own fixpoint |
| **R6** Liveness precision too aggressive | 0 | B4 #551, B5 #353/#354/#483; open #614 | 5 | (tests written with each feature) | a second analysis decides what *not* to trace; its error writes a legal value, not poison |
| **R7** Raw bits or an index outlive a sweep | 0 | B5 (#354 generations), B6 #10/#567, #19 heap-wide ids | 3 | (by design at introduction) | a value is its arena index, reused LIFO without a generation |
| **R8** Collection with a partial or foreign root set | 0 | 2 protections | 4 | — | collector entry points are public; one process-wide `thread_local!`; providers registered dynamically |
| **X** Detector blindness (cross-cutting, not a root cause) | — | 6 fixed detector failures | 2 open, 1 drifted test, 6 poison and stress blind spots | — | an equality check passes when both sides fail identically; poison writes legal values; stress collects only where it is easy |

---

## 2. Merged catalogue

Each case appears once. The class column points to §3.

### 2.1 Incidents (a live value lost its last root)

| ID | What lost its root | Commit(s) | PR / issue | Class | Observed? | Detected by | Lived | Regression test |
|---|---|---|---|---|---|---|---|---|
| A1 | `parsed.body`, `parsed.imports`, `lib_env` and `saved_globals`, held as Rust locals by a **second** library-loading path (`backend.rs::evaluate_parsed_library`). It called `execute`, which started a loop that considered itself outermost | `753a904` | #6 | R2 | yes. Release: `expected a procedure, got object`. Debug: `use-after-free: pair slot 313`. Both on bootstrap's first collection | stress lane + debug poison | hours, inside PR #6 | none dedicated; bootstrap under `run_gc_differential.sh` |
| A2 | values reachable only through `CompiledMacro.definition_env` or through an `Environment.alias_bindings` target. `visit_env` walked only the parent chain, and the `Macro` arm traced only literals | `0a59c3d` | #38 | R4 | recorded as "swept and its arena slot reused"; no repro kept | `/simplify` review on the branch | pre-merge | none (`macro_definition_env.rs` never calls `(gc)`) |
| A3 | `CpsContinuation.resume`, which holds consumers, `after` thunks, cached promises and exception payloads. It was kept live only because every construction site aliased the payload through the traced `captured_cont_env` | introduced `92dd8ec` (#39, 2026-08-09); fixed `f578cc6` (2026-08-11) | #47; audit #44 (`fc3e71c`) item B1 | R4 | no. GC-stress repros survived, and no failing program could be built | post-merge audit, by reading the trace against the struct | ~1.6 days on `main`, latent | none |
| A4 | a VM continuation payload in the weak side table, reached only through an ephemeron retained *after* the weak-id fixpoint had stopped. The ref was marked, so `sweep_weak` kept the entry, but its payload was never traced | `1d18c49` (second sub-commit) | #130 | R5 | yes. VM: `expected a procedure, got object` after one `(gc)`. The tree-walker was correct | PR review; the author reproduced it | pre-merge | `ephemerons.rs::an_ephemeron_holding_a_continuation_keeps_its_payload` |
| A5 | `ResumeWindJump`'s `target` (a weak-store `VmContinuationRef`) and `value`. The stub register window that was their only root was freed, leaving them in Rust locals until the next step wrote them back | branch `2235fa43`, fixed in `7e69689` | #156 | R3 | no: no safe point runs in that window today | `/code-review` ("insurance, not a bug fix") | pre-merge | none (no safe point can reach the window) |
| A6 | `eval_*` results held by the host. Also `run_forms`' `value` (`patina-interpreter/src/lib.rs:491`, assigned `:517`) while the next form runs, and `eval_program_resilient`'s return | — (open) | #605 | R1 | yes, on both backends. Debug: `use-after-free: pair slot 15992`. Release: `(45264 . 45264)`. A held vector prints `#()` and a string `""`, with no panic | GC study probe (primitives-embedding §7.2; gc-impl §1.8) | ~2 months, live. Explicit `(gc)` since stages 2–3 (2026-07-31); automatic since `7401cba` (2026-08-03) | planned in #605 |
| A7 | the bindings of a host-built `Environment::with_parent(global)` passed to `Backend::eval`. Nothing roots an environment that only the host holds | — (unfiled) | — (a candidate for #605's acceptance) | R1 | yes on the tree-walker. Debug: `pair slot 16004`. Release: `(45294 . 45294)`. The VM is masked only because `VmBackend::eval` ignores `env` (`backend.rs:675-693`) | probe (`defenses-hazards.md` §2) | ~2 months, live | none |

### 2.2 Near relatives (an obligation created or traded; no incident)

| ID | Change | Commit(s) | PR / issue | Class | Obligation | How it is held |
|---|---|---|---|---|---|---|
| B1 | A composable invoke's stub window is freed on the argument that "nothing between here and the next write of `cont` reaches a GC safe point" | `5b67c3d` | #172 | R3 | Keeping the window (A5's rule) leaked 27 MB against 10 MB at 400k invokes. Freeing it reopens A5's window, argued safe | comment at `vm_state.rs:~1825-1832`; no test |
| B2 | `trace_continuation_children` hand-rolled a third copy of `trace_exception_handler`, and the root inventory doc missed the new root | `1447b90` | #150 | R4 | "a third copy to go stale, and a missed root is a use-after-free" | review; the shared helper is now called and the doc updated |
| B3 | VM continuation side tables made weak, fixing ctak's 4 GB crash | `f44bb11` | #19 | R5 (it also sets R3's and R7's rules) | Capture and invoke each finish within one dispatch; nested loops defer; ids are minted heap-wide, so one id names at most one entry ever | 7 weak-store tests and 2 in `gc_vm.rs`; `run_mark_phase` made public as one unit |
| B4 | Per-pc register retirement before collection and capture | `235526b` | #551, fixes #423 | R6 | Clearing a live register is a premature free | 11 tests that check both directions; stress on `control_flow_matrix` and `ephemerons` |
| B5 | Code units released by a live-closure count; `{slot, generation}` code ids; `retire_vm_closure` | `951a82a`, `65792d6`, `9545d38` | #353 (#338), #354 (#352), #483 (#477) | R6; generations R7 | A count one too low frees code a closure can still run | break-tests in `finished_forms_release_code.rs` (9); stale-id test; debug asserts |
| B6 | Side tables keyed by raw bits are pruned before slot reuse: `SourceMap` and `syntax_sources` | `9844127`, `56d2950` | #10, #567 | R7 | A stale key would misattribute a new object | prune unit tests; source-retention test |

### 2.3 Protections added with the holder (no bug shipped)

| Commit | PR | Class | What was rooted or guarded at introduction |
|---|---|---|---|
| `b908f16` | #4 | R4, X | Symbol table rooted inside `GcVisitor::new`; the `HeapObjectData` doc warns that a misfiled leaf arm is a use-after-free; debug tombstones and `GC_POISON` |
| `bfff2e4` | #5 | R2, R8 | Every trampoline takes a `GcDeferGuard` and only `is_outermost()` collects; `exit_gc_defer` asserts instead of saturating; the safe point aborts if the library registry is mutably borrowed; `PENDING_ESCAPE` is a provider |
| `753a904` | #6 | R2, R8 | The guard moved onto `ParsedLibrary` ("a fourth path is now safe by construction"); `LibraryRegistry::try_roots` |
| `87da821` | #28 | R2 | States the load-bearing invariant "collection ... never inside `alloc_pair`", so a partial list needs no root |
| `77f6970` | #78 | R3 | `pending_escape` traced from its first commit ("reachable from nowhere else") |
| `8da5efd`, `7e69689` | #152, #156 | R4 | Wind records carry and trace the handler stack of their `dynamic-wind` call |
| `3614f8c` | #155 | R4 | The trace dedup key was re-derived when bindings moved inline. It is now the struct's address, sound because `Environment` is not `Clone` |
| `7ebfed1` | #164 | R4, X | A delimited continuation's carried prompts and handlers are traced; the vacuous test was rewritten so the value is reachable only through the edge under test |
| `0bdbc2c` | #175 | R4 | Prompt `ContValue`s are traced; a prompt-heavy program was run under stress on a debug-poison build, since the lane has no prompts |
| `2c175b4` | #282 | R4 | One wind traversal for both backends, so the rule is not restated |
| `f053037` | #434 | R4 | Import owner environments are walked by `visit_env` |
| `b237770` | #481 (#478) | R2 | Resumable-primitive state lives in a `resume_stub` register (VM) or a traced `ContValue::ResumePrimitive` (tree-walker), not on the Rust stack |
| `203313f` | #547 | R2 | `GcDeferGuard` over `desugar_with_imports`, because loading an import can run Scheme while expansion holds datums and IR in Rust |
| `28a94f8` | #602 | R2, R3, R8 | `with_globals` takes the guard together with the globals swap; `finish_wind_step` keeps A5's retention as "the one deliberate exception to paired removal"; debug heap-identity assert |

### 2.4 Detector failures, fixed and open (class X)

| Commit / issue | What was wrong |
|---|---|
| `bfff2e4` #5 | `repeated_collection_keeps_arena_bounded` passed with zero collections; it now compares with and without collecting |
| `92dd8ec` #39 | The stress interval was raised to 16 so the lane is affordable (103 s at 1). A root window shorter than 16 allocations can slip through |
| `101300b` #52 | False failure: the reclamation proof's absolute bound measured bootstrap's peak; it now asserts the churn delta |
| `7ebfed1` #164 | The delimited-continuation GC test passed with the trace deleted |
| `3a1ee89` #200 | The differential lane passed vacuously: both runs printed the same 4,537 import failures |
| `8da0c5d` #201 | The tally guard accepted any tally; it is now pinned to `1226 out of 1226` |
| #587 (open) | No generator for callback and prompt histories under stress |
| #606 (open) | A byte floor of 8 MiB would make the default-mode proof stop collecting; each proof must guard `bytes-reclaimed > 0` |
| drift | `gc_tree_walker.rs:38-48` `collection_inside_higher_order_primitive` no longer exercises deferral, because `map` is Scheme since #479 |

### 2.5 Excluded or adjacent (not premature collection)

- **Collector crashes.**
  - `68fb1fd` #306: a recursive `ContEnv` trace overflowed the Rust stack. Test:
    `collection_at_deep_call_depth_preserves_suspended_values`.
  - #5's `/simplify` round: exponential `ContEnv` tracing, fixed by `visit_once`.
- **Retention, cost or RSS.** #338, #352, #606, #609, #611, #612, #614 part 1, #615, #616. #609's fix must keep A4's
  single fixpoint, and #614's fix carries R6 risk; both are noted in their classes.
- **Not GC.** #113 (a stale multiple-values buffer), and #342, #473 and #435 (panics in control flow or expansion).

### 2.6 Timeline of the incidents

| ID | Introduced | Detected | Latency | Channel | Reached `main`? |
|---|---|---|---|---|---|
| A1 | 2026-07-31 (PR #6 branch) | same day | hours | stress lane + debug poison | no |
| A2 | 2026-08-09 (PR #38 branch) | on the branch | pre-merge | `/simplify` review | no |
| A3 | 2026-08-09 `92dd8ec` | 2026-08-10 audit `fc3e71c` | ~1.6 days to the fix | audit, by reading | yes, latent |
| A4 | 2026-08-26 (PR #130 branch) | PR review | pre-merge | review | no |
| A5 | 2026-09-02 (`2235fa43`) | `/code-review` | pre-merge | review | no |
| A6 | 2026-07-31 (explicit `(gc)`); 2026-08-03 (automatic) | 2026-10-01 | ~2 months | research probe | **yes, live** |
| A7 | same as A6 | 2026-10-01 | ~2 months | research probe | **yes, live, unfiled** |

---

## 3. The taxonomy, class by class

### R1. A holder above the outermost loop: the embedder or host

**Cases.** A6 (#605) and A7 (child environment). Both are live today, and both reproduce on a debug build.

**Mechanism.** The rule "collect only at the top of an outermost driver loop, where every live value is in a root"
covers frames *below* that loop. The embedder's frame sits *above* it. `eval_*` returns a bare `TaggedValue`, which is
an arena index. After it returns, nothing roots it. The next `eval_*` that reaches a safe point frees it, and the
LIFO free list soon hands its slot to a new object.
- A6: `run_forms` repeats the same mistake internally. It keeps form *k*'s value in a Rust local while form *k+1* runs.
- A7: the same thing happens to an environment the host built with `Environment::with_parent` and passed to
  `Backend::eval`.

**Detection and lateness.** Found by the GC study's probe crate, about two months after collection became automatic.
- **No lane could find it.** The differential lanes drive the CLI, which never reads a result after a later form has
  run, and no test holds an `Interpreter` result across a collecting call.
- Debug poison catches it only for pairs and objects, and only before the slot is reused. The probe's held vector
  printed `#()`, its string `""`, and a held pair printed as part of a live list in a *debug* build.
- A7 is masked on the VM by an unrelated divergence: `VmBackend::eval` ignores `env`.

**What made it possible.**
- `TaggedValue` is `Copy` and carries no lifetime, brand or generation. Holding one is indistinguishable from holding a
  root.
- The embedding API has no root or handle type. The only workaround is `global_env().define(name, v)`.
- The invariant is stated about *where collection runs*, not about *who holds values*. So it is silent about every frame
  above the loop.
- The test lanes enter through the CLI only.

**Latent entries** (`defenses-hazards.md` §4.1):
1. A value a host reads from a rooted place becomes unrooted once that place changes [I]. Examples:
   `global_env().get(name)` followed by a later `(set! name …)`, or a host-built datum kept for re-evaluation.
2. Once `VmBackend::eval` honours `env` (`patina-vm/src/backend.rs:675-693`), A7 appears on the VM too [I].
3. #604 (open), teardown: after the interpreter drops and its arenas clear, a `SharedHeap` or `Rc<Environment>` clone
   that outlives it reads a torn-down heap. The planned contract is that values do not outlive their interpreter.

**What closes it.**
- `Owned` handles `{heap_id, index, generation}` returned by `eval_*` and held by `run_forms` (GC_PRD §11.5, stage 2,
  #605).
- `interp.with(|cx| …)` branded, with a trybuild test that a value cannot escape it (§11.3).
- Add A7 to #605's acceptance.
- An embedding-API test under `PATINA_GC_STRESS=1` on a debug build, since the CLI lanes cannot reach this class.

### R2. A Rust frame holds heap values across a re-entry into the evaluator

**Cases.** A1 (#6).

**Mechanism.** A Rust function holds `TaggedValue`s in locals, a `Vec` or an `Rc` container, then calls something that
runs a driver loop. If that loop believes it is outermost, it collects, and the Rust-held values die.
- In A1 a new, second library-loading path called `execute` rather than `execute_nested`. The PR's own diagnosis says
  the real predicate is "does this Rust frame hold heap values across an evaluation call?", not which entry point it
  used.
- The defense is deferral. A `GcDeferGuard` raises a heap-wide counter, and a loop collects only if the counter was 0
  when it started.

**Detection and lateness.** Caught within hours, before merge, by the stress lane on the debug-poison build, which
localized it in one run. It was easy because the path ran during *bootstrap*, so every program hit it at the first
collection. A rarer path of this shape would get the same weak coverage as R1.

**What made it possible.**
- The Rust stack is not a root.
- "May collect" is not visible in a function's type. Any function that can reach the evaluator is a collection point,
  and nothing says so.
- Deferral is a dynamic counter, and each caller had to remember to raise it. The obligation lived at call sites, so the
  N-th call site forgot.
- Today's safety for nested loops is "they never collect". That is a policy, not a property of the code that holds the
  values.

**Protections.** `bfff2e4` #5, `87da821` #28, `753a904` #6 (guard on the data), `b237770` #481 (state moved into the
machine), `203313f` #547 and `28a94f8` #602 (guard bound to the swap). The progression is the lesson. A guard at a call
site was missed. A guard on the holding object (`ParsedLibrary`) or bound to the operation (`with_globals`) cannot be.
Moving the value into the machine (`Step::Call`, `resume_stub`) removes the holder.

**Latent entries** (`defenses-hazards.md` §3.2, §4.2, §4.5, §5). Each is safe today **only because nested loops never
collect**, or because allocation never collects:
1. `with_globals` (`vm_state.rs:337-347`). The real global environment is unrooted while it is swapped out. Guarded
   since #602.
2. `%parameterize-swap!` (`parameters.rs:195-236`). `olds` is the old values' only holder while a parameter-like
   procedure runs.
3. `run_synchronously` (`registry.rs:99-120`). A resumable primitive's `state`.
4. The `force` fallback (`lazy.rs:115-135`). The promise, re-read and updated after the thunk.
5. The `call-with-values` fallback (`values.rs:30-48`). `consumer`, across the producer call.
6. Rust `member`/`assoc` with a comparator (`lists.rs:440,587`). Reachable only through `(patina internal lists)`.
7. The Rust `call-with-port` and `call-with-*-file` (`ports.rs:666`, `file.rs:216,263`). The port's `TaggedValue`.
8. `ParsedLibrary.body` (`library_loader.rs:173-215`). `ParsedLibrary::new(heap: None)` installs no guard (`:208`). The
   only caller passes `Some`.
9. `desugar_with_imports` (`desugarer/mod.rs:1838-1841`). Partial IR.
10. Nested tree-walker trampolines (`cps_eval/mod.rs:213-221`). The suspended outer `StepResult`.
11. `is_outermost` is hoisted at loop entry. A guard that a callee creates and keeps past the instruction would not stop
    the running loop [V/I].
12. Defer balance is checked only by a `debug_assert` (`heap/mod.rs:674-680`).
13. `Rc` container holders (§4.5): an `Rc` clone of a record's `fields` (`records.rs:250-253`), a parameter's `values`
    (`heap/mod.rs:1820-1828`), a closure's `globals`. The container survives sweep but its values do not, so these are
    safe only within one primitive or a guarded window.
14. The "allocation never collects" census (offheap §5.2): 19 fresh values unrooted across an allocation, 51 values used
    after an allocation, 25 allocations in loops, 9 Rust collections held across an allocating loop, and 3 allocations
    plus re-entry.
    - Any change that lets allocation collect (excluded by GC_PRD §14) turns them all into this class.
    - Any change that lets a nested loop collect (stage 4e) turns items 1–10 into incidents.

**What closes it.**
- The `&mut Heap` vs `Cx<'gc>` capability: no method on `Cx` collects, and a nested entry beneath a `Cx` gets
  `NoGcScope` by type. trybuild tests pin "a value used after a may-collect call" (GC_PRD §11.3).
- Until then: move each latent holder into the machine before stage 4e (`Step::Call` state or a rooted scope).
- Add a test that makes deferral observable: a parameter-like procedure that calls `(gc)` inside `parameterize`, with
  its old value reachable only from `olds`.

### R3. A value parked off-root across the next safe point of the running loop

**Cases.** A5 (#156), latent and pre-merge. Near relative B1 (#172).

**Mechanism.** An instruction ends with a value held only in a Rust local, or in a register window it has just freed,
and expects the *next* step to write the value back into a root. The top of the next step is a safe point. If that safe
point collects, the value dies.
- A weak-store handle makes it worse. Losing the `VmContinuationRef`'s only root makes `sweep_weak` prune the payload,
  so the damage is not one stale value but a continuation whose snapshot points at swept slots.
- `pending_escape` (#78) is the same shape, done right: it was rooted from its first commit.

**Detection and lateness.** `/code-review`, before merge. No test can construct it, because no safe point reaches the
window today. The rule is now written at `control.rs:31-33`: "Carry the heap continuation *handle* in a rooted register
across thunk execution".

**What made it possible.**
- Removing a root and handing on its value are separate operations, and soundness is argued from *where safe points
  happen to be*.
- The weak table's soundness, "every store touch is confined to one instruction dispatch and nested loops defer"
  (`gc_roots.rs:21-24`), is a comment.
- A leak fix and a rooting rule pull in opposite directions. #156 kept the window. #172 freed a similar one because
  keeping it leaked 27 MB, and that decision rests on reasoning alone. #602 had to carry `finish_wind_step`'s
  intentional retention through a refactor by hand.

**Latent entries.**
1. Nothing asserts that a weak-store touch stays within one dispatch, or that no safe point lies between capture and
   insert, or between invoke and copy-back (`gc_roots.rs:21-24`; capture at `vm_state.rs:631-656`) [V].
2. B1's freed window (`vm_state.rs:~1825-1832`) and A5's kept window (`vm_state.rs:1865-1875`) are both held by comments
   only. A safe point added inside either one (for example a poll on a new path) is an incident [I].

**What closes it.**
- Stage 4e deletes the weak continuation tables, which makes handles ordinary heap objects.
- `return_into(frame)` as the only way to write into or activate a suspended frame (GC_PRD §11.1).
- Frame invariant 1, "window initialization".
- Zeal `entry`, which services a collection at every poll site, so a window that spans a safe point is exercised rather
  than argued about.

### R4. An edge or field added without a trace rule

**Cases.** A2 (#38) and A3 (#47). Near relative B2 (#150). Seven protections at introduction (§2.3).

**Mechanism.** The object graph spans the arenas and Rust-owned structures: `Rc<Environment>`, `CpsContinuation`,
`ContValue`, `CompiledMacro`, wind records, prompts and handler stacks. Each edge needs a hand-written visit. A new
field or a new `Rc` edge without one is a missed root.
- A2: two new `Rc<Environment>` edges had no rule.
- A3: a new `ContValue` field had no rule.
- B2: a third hand-copied traversal, which was due to go stale.
- In practice the miss is usually **latent by aliasing**: the value is also reachable through a traced path, so no test
  fails. A3 "could not be made to fail", and `foreign_expansions` is covered today because the registry roots library
  environments.

**Detection and lateness.**
- Every detection was by *reading the trace against the struct*: `/simplify` (A2), a post-merge audit (A3, about 1.6
  days on `main`) and review (B2).
- No lane found any of them. The #164 review showed why. Written "the obvious way", with the payload also in the
  registers, the GC test "passed with them deleted".

**What made it possible.**
- Trace rules are written by hand rather than derived from the layout.
- The exhaustive `match` in `trace_object_children` catches a new *variant* but not a new *field*. A `{ .. }` pattern
  or a misfiled leaf arm "is a use-after-free, not a compile error" (`heap/mod.rs:137-141`).
- The root inventory (GC_DESIGN §5) is a manual checklist.
- Redundant reachability hides the miss from every dynamic check.

**Latent entries.**
1. `CompiledMacro.foreign_expansions: Vec<(ScopeId, Rc<Environment>)>` (`compiled_macro.rs:540`, added `9bb8d2a`
   #462) is not traced. The `Macro` arm visits literals and `definition_env` only (`gc.rs:746-753`).
   - It is sound while the generating library stays registered; the probe found 0 entries.
   - It is the same shape as A2 recurring.
   - It becomes reachable as a bug if #614 lets a redefined library's owner drop [I].
2. A new `TaggedValue` or `Rc` field inside a variant matched as `{ .. }`, or inside one of the leaf arms
   (`Identifier`, `Procedure::Primitive`, `PromptTag`, `Port`) [V].
3. A new off-heap holder that nobody adds to a provider [V].

**What closes it.**
- Derive tracing from the layout (`ObjectModel` / `declare_layouts!`, GC_PRD §14), and report off-heap structures as
  host payloads traced inside the one fixpoint.
- `PATINA_GC_VERIFY_ROOTS`, together with the pre-sweep "no free-list slot is marked" assertion (GC_DESIGN §11.5, not
  implemented). With them, an untraced edge whose referent was freed panics at the next collection even if nothing
  reads it.
- The #164 rule for tests: the value under test is reachable *only* through the edge under test.

### R5. Weak-processing order and fixpoint composition

**Cases.** A4 (#130). Near relative B3 (#19), which created the weak tables.

**Mechanism.** Two weak mechanisms each had their own fixpoint: weak continuation ids, whose payloads sit in `VmState`'s
side tables and are traced by a provider callback (`trace_weak_ids`), and ephemerons. The ephemeron loop ran after the
weak-id loop had stopped.
- A ref marked only by a late ephemeron retention was queued but never broadcast, so its payload went untraced.
- `sweep_weak` kept the entry, because the ref was marked.
- A second weak-id pass would not have been enough either, because `trace_weak_ids` can itself reach an ephemeron. The
  termination argument was also wrong: "pending shrinks each round" is false, since `drain` can grow it.

**Detection and lateness.** Review of #130, before merge. No lane runs ephemerons that hold continuations, and the
chibi suite has no ephemerons at all.

**What made it possible.**
- Weak payloads live outside the heap, in a provider's side table, and are resolved by a callback.
- Each weak kind was added with its own loop. Nothing structural forced them into one.
- Liveness queries (`value_is_live`) depend on how far the worklist has drained. A provider calling one would skip a
  live trace, so the function was made private as part of the fix.

**Latent entries.**
1. #609's fix of the O(n²) ephemeron rescan must keep the single loop (GC_PRD §9.5).
2. Every future weak kind (guardians, host payloads, the weak symbol table at stage 5c) must join the same fixpoint.
3. `value_is_live` is protected only by visibility (`gc.rs:517-534`). The weak-store dispatch rule is shared with R3.

**What closes it.**
- One fixpoint owned by `run_mark_phase` (done in `1d18c49`).
- GC_PRD §14's weak contract and conformance suite, run on `MarkRegion`, `NullGc` and a `TestModel`.
- Stage 4e removes the side tables, which removes the second weak kind.

### R6. Liveness precision that is too aggressive

**Cases.** None shipped. Near relatives B4 (#551), B5 (#353, #354, #483). Open #614.

**Mechanism.** An optimization deliberately stops tracing something that is still held: a register the compiler says is
dead, code whose closure count reached zero, or an owner whose link count reached zero (#614, planned). If the second
analysis errs low, a live value or live code is dropped. **The failure writes a legal value.** A retired register reads
`UNSPECIFIED`, so no assertion fires.

**Detection and lateness.** No incident. Each feature landed with tests in the dangerous direction.
- #551 has 11 tests with `(gc)` placed mid-expression, plus stress runs.
- #353 has break-tests: "with the closure count ignored, the four closure-held tests fail".
- #354 has generation checks, which turn a stale code id into "missing CodeObject" rather than running whatever reused
  the slot.

**What made it possible.**
- The precision maps and counts are maintained apart from the code they describe.
- A wrong map is visible only if a collection happens at exactly that pc *and* the output changes. Stress collects only
  after an allocation, so it reaches few pcs.

**Latent entries** (§3.4, §4.3, §4.4):
1. A wrong per-pc map silently clears a live register to `UNSPECIFIED` [V].
2. A codegen change, a new opcode or a new runtime landing goes unchecked at run time [V]. `written_register` is
   exhaustive (`pass5_codegen.rs:294-303`), retirements are positional (`:679-683`), and a landing inherits the union
   of its predecessors.
3. VM primitive arguments at a mid-instruction pc (`vm_state.rs:1989-1994`, with `scratch_args` taken) are also in
   caller registers, but retirement at that pc is unproven [V].
4. Code liveness [V]. A unit stays while `live_closures > 0 || Rc::strong_count > 1` (`vm_state.rs:1003-1008`), and
   constants are traced only through `code_store` (`gc_roots.rs:97-99`). A release while a holder outside that
   accounting can still run the code would leave its literals untraced.
5. #614 (open): the owner link count. Its acceptance already guards the premature direction.

**What closes it.**
- The `DEAD_SLOT` sentinel in debug, plus an assertion in `reg_at` (GC_PRD §11.1 invariant 3). That turns a silent wrong
  value into a panic.
- The verifier's "a map at every frame pc" check.
- Zeal `entry`, so every pc that can collect does.

### R7. Raw bits or an index outlive a sweep

**Cases.** None shipped. Near relatives B6 (#10, #567), B5's generations (#354) and #19's heap-wide id minting.

**Mechanism.** A key that encodes an arena index survives the sweep. Reuse is LIFO, so a new object soon answers to the
old key: a source location misattributed, a code id running other code, or a side-table entry naming another
continuation. This is a use-after-free through a side table rather than through a root.

**Detection and lateness.** Prevented by design at introduction each time: prune before reuse, generation checks, ids
that are never reused.

**What made it possible.** A value *is* its arena index, with no generation, so any table keyed by bits is keyed by a
slot.

**Latent entries** (§4.6):
1. Inside one long form, `SourceMap` entries stay stale until the form ends (`lib.rs:509-512`). This affects
   diagnostics only.
2. `hash-by-identity`, the symbol table and `CallFrame.closure` are correct only because indices never move. They break
   under evacuation. This is the "root traced by value where a slot was needed" class, with zero cases only because
   nothing moves today.
3. The transient raw-bits sets (`desugarer/mod.rs:129`, `quasiquote.rs:119-123`, `macro_expander/mod.rs:254`,
   `primitive_calls.rs:43,215`) are safe only because no collection runs inside them.

**What closes it.**
- Generation-carrying handles (`Owned`, `Rooted`).
- The 2-collection quarantine of freed holes (GC_PRD §16).
- Slot-updating root visitors before evacuation (stage 8).

### R8. A collection with a partial or foreign root set

**Cases.** None. Two protections: `LibraryRegistry::try_roots`, which aborts the collection rather than trace a partial
root set while the registry is borrowed (`bfff2e4` #5, `753a904` #6), and `with_globals`' heap-identity assertion
(#602).

**Mechanism.** A collection runs while a provider cannot report, or is called with another heap's providers or with a
hand-built root set.

**What made it possible.**
- The collector's entry points are public API.
- One root lives in process-wide state.
- Providers are registered dynamically.

**Latent entries.**
1. `Collector`, `MarkSweepCollector`, `run_mark_phase` and `Heap::sweep` are re-exported from `patina-core`
   (`lib.rs:71-72`). Today only tests call them (`source_map.rs:421`, `heap/source.rs:229`,
   `weak_continuation_tests.rs:132`) [V].
2. `PENDING_ESCAPE` is a `thread_local!` (`cps_eval/types.rs:20-42`) while heaps are per interpreter. With two
   tree-walker interpreters nested on one thread, the wrong heap would trace it [I].
3. GC_DESIGN §9.6's `Rc::ptr_eq(macro.heap, heap)` assertion is absent [V].
4. `with_globals`' heap-identity check is debug-only (`vm_state.rs:342`) [V].

**What closes it.**
- Make `collect`, `run_mark_phase` and `sweep` crate-private.
- The `&mut Heap` capability (GC_PRD §11.3).
- `PENDING_ESCAPE` leaves `thread_local!`, with a CI check against new ones (§16, §18.6).

### X. Detector blindness (cross-cutting; why the classes above survive)

This is not a root cause. It is why A3, A6 and A7 reached `main`, and why A2, A4 and A5 needed a reviewer.

**Fixed detector failures.** Six are listed in §2.4. Four were tests or lanes that could not fail: #5, #164, #200 and
#201. Each fix made the value under test reachable only through the path under test, or pinned the exact tally.

**Blind spots that remain** (`defenses-hazards.md` §3.6–§3.9, §6):
1. Swept vector and string slots become `Vec::new()`, which is a legal value (`gc.rs:917-936`). A use-after-free there
   is invisible even in debug. The probe printed `#()` and `""`.
2. A freed slot that has already been reused is invisible. Reuse is LIFO, and the probe's held pair printed as part of a
   new list in debug.
3. Direct arena accesses bypass the `Free` assertion. `defenses-hazards.md` says "8" but cites nine sites:
   `heap/mod.rs:970, 1085, 1355, 1370, 1385, 1400, 2875, 2890, 2903`. On a freed closure, `frame_globals`
   (`vm_state.rs:1357-1363`) silently falls back to the global environment.
4. The collector absorbs dangling references.
   - Sweep pre-marks the free list and ignores the result (`gc.rs:836-838`), so GC_DESIGN §11.5's assertion was never
     implemented.
   - `trace_children` reads the arenas raw (`gc.rs:679-692`): a poisoned pair yields immediates, and a `Free` object is
     a leaf.
5. Stress mode collects at the first outermost safe point after N allocations.
   - It never collects under deferral or in a stretch without allocation.
   - CI runs it at N=16, on the chibi suite only, through the CLI.
   - No Larceny suite, matrix or `cargo test` lane runs under stress.
6. Every integration test places its `(gc)` by hand, so the tests cover only the shapes someone anticipated.

**Open:** #587 (a generator for callback and prompt histories under stress) and #606 (proofs must guard
`bytes-reclaimed > 0`). **Drifted:** `collection_inside_higher_order_primitive`.

**Which detector sees which class** (✓ sees it; ~ only partly or only if the output changes; ✗ blind):

| Class | Debug poison | Stress lane (debug) | Integration tests | Review / audit | Type system |
|---|---|---|---|---|---|
| R1 above the loop | ~ (pairs and objects, before reuse) | ✗ (CLI only) | ✗ | ✗ (no review found it; the study's probe did) | ✗ |
| R2 Rust frame across re-entry | ✓ if the path collects | ✓ for paths the chibi suite runs | ~ | ✓ | ✗ |
| R3 parked off-root | ~ | ✗ (no safe point in the window today) | ✗ | ✓ (A5) | ✗ |
| R4 untraced edge | ~ (only without an aliasing path) | ~ | ~ (vacuous unless the edge is the only path) | ✓ (A2, A3, B2) | ✓ variants, ✗ fields |
| R5 weak order | ~ | ✗ (no ephemerons in the suite) | ✓ after A4 | ✓ (A4) | ✗ |
| R6 precision | ✗ (`UNSPECIFIED` is legal) | ~ | ✓ for pinned shapes | — | ✗ |
| R7 stale key | ✗ | ✗ | ✓ for pinned tables | ✓ by design | ✗ |
| R8 partial root set | — | — | — | ✓ | ✗ |

---

## 4. What the taxonomy says about prevention

- **Reviewers found the bugs of R3, R4 and R5, and the lanes did not.** These classes need a structural fix, not more
  lanes:
  - R4: derived trace rules, and a verifier that panics on any reachable edge into a freed slot.
  - R5: one weak contract.
  - R3: no off-root windows at all.
- **The lanes cannot reach R1, and reach R2 only on common paths.** Two things close those classes:
  - a type that separates "may collect" from "holds values": `&mut Heap` versus `Cx<'gc>`, plus `Owned` and `RootScope`
    across collections;
  - a test that holds an embedder value across a collecting call.
- **R6 and R7 have produced no incident, but their failures are silent by construction.** The cheapest guards turn
  those silent failures into panics: `DEAD_SLOT` with a `reg_at` assertion, generations and quarantine.
- **Most of these guards can land on today's collector** (`defenses-hazards.md` §7), ahead of the redesign:
  - `Owned` handles (#605, plus A7);
  - a debug freed-bitset checked in every accessor, including vectors, strings and the bypassing accessors;
  - the GC_DESIGN §11.5 one-line assertion;
  - `DEAD_SLOT`;
  - zeal-every-safe-point;
  - stress on `cargo test` and the Larceny lanes in debug;
  - crate-private collection entry points.
