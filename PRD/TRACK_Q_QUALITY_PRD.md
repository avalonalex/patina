# Track Q — Code and Project Quality PRD

**Created:** 2026-08-09
**Status:** Planning → ready to execute
**Scope decision:** **structural, not cosmetic.** This track fixes gaps in how the
project *proves* it is correct — test structure, CI coverage, API surface, and the
single-source-of-truth rule for status — rather than chasing lint counts or style.
Every item must either close a demonstrated defect or make a class of defect
impossible to reintroduce silently.
**Umbrella:** `PRD/SNOW_AND_PERF_ROADMAP.md` sequences Tracks P and L. Track Q runs
alongside both as a standing track with no milestone dependency on either.

---

## 1. Context & problem

The routine health signals are green, and were re-verified at `7a6a797` on
2026-08-09 rather than assumed:

| Signal | Measured |
|---|---|
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| Tests | 1,751 (`#[test]`: 554 in-crate unit, 1,197 integration) |
| R7RS compliance | 1226/1226, both backends |
| CI jobs | 6 — test matrix (ubuntu + macos), chibi suite, two GC differential lanes (release + debug/poison), fmt, clippy |
| `todo!()` / `unimplemented!()` | 0 |
| Non-test `panic!` sites | 16 |
| Non-test `.unwrap()` | 102 — the largest cluster (28, `patina-core/src/debug_format.rs`) is `write!` into a `String`, which is infallible |
| `unsafe` blocks | 6, all one documented raw-pointer `ApplyContext` bridge (`patina-vm/src/runtime/vm_state.rs:2739-2795`) |
| Workspace dependencies | 9 |

**So the problem is not test volume or hygiene — it is test *structure*.** The
project's central architectural claim is that two backends implement the same
language. Nothing in the repository asserts that claim directly, and the default
backend is the one CI exercises least. §1.2 is what that gap was already hiding.

### 1.1 Verified current-state evidence

| Observation | Evidence |
|---|---|
| The integration suite is backend-switched by a Cargo feature that **nothing enables** — not CI, not any script. | `vm-backend` feature declared in `crates/patina-tests/Cargo.toml`; consumed at `crates/patina-tests/tests/common/mod.rs:21-70`; CI runs `cargo test --all --lib --tests` with default features (`.github/workflows/ci.yml:36`); zero matches for `vm-backend` in `scripts/`. |
| The lane is **free green** — it passes today and is simply not gated. | Measured 2026-08-09: `cargo test --package patina-tests --features vm-backend --tests` → **48 binaries, 1,142 tests, 0 failures**. |
| In CI the VM therefore gets only the chibi suite, the GC lanes, and 6 VM-specific test files; the other ~1,100 integration assertions run tree-walker-only. | `crates/patina-tests/tests/vm_*.rs` (4 files) + `gc_vm.rs`; everything else routes through `common/mod.rs`'s default (tree-walker) helpers. |
| **No cross-backend differential test exists.** The chibi suite is run twice by two scripts and the two reports are compared by a human reading two files. | `scripts/run_chibi_tests_tree_walker.sh:7` re-execs `run_chibi_tests.sh --tree-walker`; the outputs land in `scheme_tests/reports/compatibility.md` and `compatibility_tree_walker.md` with no programmatic diff. |
| A primitive is **registered and exported with a body that always errors.** | `with_exception_handler` returns `InternalError("with-exception-handler: not yet implemented - requires CPS integration")` — `patina-primitives/src/primitives/exceptions.rs:285-287` — yet is registered at `:369-375` and exported as `("with-exception-handler", Arity::Exact(2))` in `patina-runtime/src/stdlib/internal_errors.rs:35`. Both backends special-case the name at the call site, so the stub is reachable only as a first-class value — see §1.2. |
| Two god objects. | `patina-core/src/heap/mod.rs` — 2,878 lines, **155 `pub fn`** (plus 51 in `heap/numeric.rs`, 2,090 lines): a ~200-method surface on the type every crate depends on. `patina-vm/src/runtime/vm_state.rs` — 3,381 lines, 56 functions. |
| **No property or fuzz testing.** | Zero matches for `proptest`/`quickcheck`/`arbitrary`/`fuzz` across all `Cargo.toml`; no `fuzz/` directory. |
| **No perf regression gate.** After ~15 PRs of Track P work, nothing in CI would catch a regression. | The scoreboard is a manual sweep against a Chibi checkout outside the repo (`~/Project/r7rs-benchmarks`); `crates/patina-tests/benches/` is Criterion-only and not run in CI. |
| Status numbers are duplicated and have drifted. | `PRD/MILESTONES.md:5` headlines geomean **0.93×**; `PRD/TRACK_P_PERFORMANCE_PRD.md` §1.11 records **0.79×**. `PRD/SNOW_AND_PERF_ROADMAP.md:51-54`'s own housekeeping list of stale links is itself still stale. `PRD/README.md` — the directory's index — still reports **1159/1159**, calls Phase 2 "Next / Planning" (the VM shipped and is the default backend), lists `phase2/VM_BACKEND_DESIGN.md` as "(to be created)", and does not mention Tracks P, L, or Q at all. 221 markdown files / 3.7 MB under `PRD/` vs 16 under `docs/`. |
| Generated artifacts are version-controlled, so every test run dirties the tree. | `scheme_tests/reports/{compatibility,compatibility_tree_walker}.md` and `results*.txt` — currently modified in the working tree with timestamp-and-timing churn only. |

### 1.2 The defect cluster this structure is hiding

R7RS §6.10 makes `call/cc`, `dynamic-wind`, `values`, and
`with-exception-handler` **ordinary procedures**. Both backends instead resolve
them by name at the call site, and the registry binding behind the name is
missing or a stub. The holes in the two backends are *complementary* — the
signature of no differential testing. Same source file, both release binaries at
`7a6a797`, 2026-08-09:

| Expression | VM | Tree-walker |
|---|---|---|
| `(define f call/cc)` then `(f (lambda (k) 1))` | `1` | ❌ `Undefined variable: patina.internal.control/call-with-current-continuation` |
| `(map (lambda (f) (f (lambda (k) 6))) (list call/cc))` | `(6)` | ❌ same |
| `(apply call/cc (list (lambda (k) 1)))` | ❌ same | ❌ same |
| `(apply dynamic-wind (list b t a))` | ❌ `Undefined variable: patina.internal.control/dynamic-wind` | `2` |
| `(apply values (list 7))` | ❌ `Wrong number of arguments: expected 1, got 2` | `7` |
| `(apply with-exception-handler (list h t))` | ❌ the §1.1 stub | `43` |

Every direct call of these forms works on both backends, which is why 1226/1226
does not catch any of it — the chibi suite never takes one as a value.

**Update 2026-08-10 — the table above was re-measured at `2d4ce29` and two rows
have moved.** `(apply values (list 7))` now returns `7` on *both* backends; the
VM arity failure recorded at `7a6a797` is gone. `(apply call/cc ...)` still fails
on both, which makes it a shared conformance gap rather than a divergence — the
differential harness cannot see it, so Q2 must not read backend *agreement* as
correctness. The four genuine divergences that remain (`define`-bound `call/cc`,
`call/cc` through a higher-order procedure, `apply dynamic-wind`, `apply
with-exception-handler`) are now committed as executable quarantine tests in
`crates/patina-tests/tests/backend_divergence.rs`, which supersedes this table as
the live inventory. Each pins both backends' current behaviour and is written to
**fail when the bug is fixed**, forcing collapse into a plain both-backends
assertion. Prefer that file over this section when starting Q2.

**Shared root cause with an open Track L defect.** `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md`
§6 records that Rust registry primitives ignore the import set at top level.
That is the same disagreement seen from the other side: the compiler's
name-special-casing and the primitive registry do not share one answer to "what
is this identifier bound to." Q2 and that Track L item should be designed
together even if they land as separate PRs.

## 2. Goals

- **Both backends are gated equally in CI**, and any behavioural divergence
  between them fails a test rather than being discovered by hand.
- **Every identifier a program can name is a real, first-class value** on both
  backends — no binding whose only working form is the call position.
- Defect classes the current suite structurally cannot reach (arbitrary input,
  numeric promotion boundaries, reader/writer round-trips) are covered by
  generated tests rather than enumerated cases.
- A performance regression is **caught by CI**, not by the next manual sweep.
- Every status number lives in exactly one place, so no two documents can
  disagree about it.

## 3. Non-goals

- **Lint-count or style churn.** Clippy is already clean at `-D warnings`; this
  track adds no new lint gates and does no cosmetic refactoring.
- **Removing the tree-walker.** Two backends is a deliberate educational choice;
  the cost is maintenance, and the answer is a differential harness (Q1), not
  consolidation.
- **Rewriting the `unsafe` `ApplyContext` bridge.** Six blocks, documented,
  single-threaded, no observed miscompile. Revisit only if Q3's fuzzing or a
  Miri run produces evidence — noted in §6 as a watch item, not a work item.
- **Full API-surface redesign of `Heap`.** Q4 is bounded to reducing visibility
  and grouping facets; it does not change the NaN-boxed representation or the
  `Copy` invariant.

---

## 4. Work items

### Q0 — Gate the VM integration lane in CI  *(small; do first)*  ✅ **done 2026-08-10 — superseded in approach**

Original plan: add a second CI job running `cargo test --package patina-tests
--features vm-backend --tests`, keeping it separate so both lanes stay gated.

**What shipped instead: the test suite is parametrized, and the feature flag is
gone.** The helpers in `crates/patina-tests/tests/common/mod.rs` are now generic
over `B: Backend` and run *every* program on both backends, holding both to the
same expectation. This is strictly better than two lanes and was a smaller diff:

- **No new CI job, and one build instead of two.** The existing
  `cargo test --all --lib --tests` now covers both backends, so the second
  feature-variant compile the original plan required never happens.
- **No test call-site edits.** Only `common/mod.rs` changed; the ~690 helper
  call sites became ~1,380 assertions untouched.
- **It subsumes most of Q1 for free** — see that item.

The two lanes would have given *coverage* on both backends but not *agreement*:
each lane checks its own hardcoded expectations, so a divergence still only
surfaces where someone happened to write a test for it.

- **Acceptance (met):** 48 binaries / 1,142 tests / 0 failures with both backends
  exercised per test; `SKIP_CHIBI_TESTS=1` unchanged; both chibi lanes 1226/1226;
  clippy clean.

### Q1 — Cross-backend differential harness  *(the centrepiece)*  🟡 **largely delivered by Q0's parametrization**

One corpus, both backends, assert identical outcomes — value *and* error class,
not just exit status. This is what makes "two backends implement the same
language" a checked claim instead of a stated one.

**Status 2026-08-10.** Q0's parametrized helpers deliver the core of this: the
corpus is the *existing* ~1,142-test suite rather than a hand-written third
list, which is exactly what §6's "becomes a second place to enumerate cases"
risk warned against. Error class is covered too — `assert_eval_error` now
requires *both* backends to reject, so one backend erroring where the other
succeeds is a failure. Verified by mutation: injecting the §1.2 `call/cc`
divergence fails with `[tree-walker] failed to evaluate … Undefined variable`.
The §1.2 seed lives in `crates/patina-tests/tests/backend_divergence.rs`.

Remaining under this item:
- **The 22 test files that construct interpreters directly** (`hygiene.rs`,
  `scheme_base.rs`, `numeric_operations.rs`, `record_types.rs`, the SRFI
  suites, …) still run tree-walker-only. Some are legitimately backend-specific
  (`interpreter_api.rs` uses `evaluator()`, which is not on the `Backend`
  trait), but most are plain R7RS coverage that should be parametrized.
- **The chibi report diff** below is still unwritten.

- A table-driven test in `crates/patina-tests/` taking a list of program
  snippets and asserting `tree_walker(p) == vm(p)` for each, including the error
  variant when both are expected to fail.
- Seed it with the §1.2 matrix, the control-op edge cases from
  `PRD/phase2/INSTRUCTION_LEVEL_CONTROL_OPS.md`, and the existing `vm_*.rs`
  cases run on both sides.
- Extend `scripts/run_chibi_tests.sh` (or add a wrapper) to **diff the two
  generated reports programmatically** and fail on any per-section divergence,
  replacing the current read-two-files-by-eye step.
- A divergence must report *which* backend produced *what*, since the failure is
  usually in the one you were not thinking about.

- **Acceptance:** the §1.2 table exists as failing tests before Q2 lands and
  passing tests after; a deliberately introduced VM-only behaviour change fails
  the harness.

### Q2 — Make control operations genuine first-class procedures
Close the §1.2 cluster. `call/cc`, `dynamic-wind`, `values`,
`call-with-values`, `raise-continuable`, and `with-exception-handler` must
work when passed as values, applied via `apply`, and bound with `define` — on
both backends. The compiler's call-site special-casing stays as the fast path;
what changes is that the fallback binding is real.

Two sub-parts, both required:
1. **Every special-cased name gets a working registry binding**, so a value-position
   reference resolves. Where a true implementation needs backend cooperation
   (`call/cc` on the VM captures machine state; on the tree-walker it is a CPS
   operation), the binding is a backend-provided closure, not a shared stub.
2. **Delete or implement `with-exception-handler`'s stub.** A registered
   primitive whose body is `InternalError("not yet implemented")` is worse than
   an unregistered one: it turns a clean unbound-variable error into an internal
   error, and it advertises support in the library's export list.

Design alongside the Track L "primitives ignore the import set" defect — see
§1.2. A grep for other registered-but-stubbed primitives is part of this item.

- **Acceptance:** every row of the §1.2 table passes on both backends via Q1's
  harness; no registered primitive has a body that unconditionally errors;
  `./scripts/run_chibi_tests.sh` stays 1226/1226 on both backends.

### Q3 — Property and fuzz testing for the input-facing layers
Enumerated cases cannot cover a numeric tower with four representations or a
reader that accepts arbitrary bytes. Add `proptest` (dev-dependency) with a
small number of high-yield properties, ordered by expected yield:

1. **`write` → `read` round-trip.** For generated values across every heap type,
   `(read (open-input-string (write x)))` is `equal?` to `x`.
2. **Numeric promotion identities.** Generated fixnum/bignum/rational/real/complex
   operands: associativity/commutativity where they hold exactly, `exact→inexact→exact`
   behaviour at the boundaries, and agreement between the fast fixnum paths in
   `tagged_value.rs` and the general paths in `heap/numeric.rs`.
3. **The frontend never panics on arbitrary input.** Lexer and parser over
   arbitrary byte strings return `Err`, never unwind. This is a real property —
   a REPL is shipped — and it is the one that justifies the 16 non-test
   `panic!` sites being where they are.

Any property that finds a bug gets the shrunk case committed as a unit test
next to the fix. Run in CI with a fixed seed and a low case count so the job
stays fast; a nightly job may run a larger budget.

- **Acceptance:** the three properties above run in CI; each found defect lands
  with its shrunk regression case.

### Q4 — Reduce the `Heap` and `VmState` API surface
`Heap` exposes ~200 public methods, and the `RefCell` borrow rule documented in
`CLAUDE.md` exists because the API permits the mistake it warns about. This item
makes the footgun structural rather than documentary. Bounded scope:

- Audit `pub fn` on `Heap` for methods used only within `patina-core` and narrow
  them to `pub(crate)`.
- Group the remaining surface into facets (allocation / accessors / GC) as
  modules or extension traits, so a caller sees the ten methods it needs rather
  than two hundred.
- Same audit for `VmState`'s free functions in `vm_state.rs`; the file has clear
  section banners already, which is the natural split line.
- **Explicitly not** a representation change. No `TaggedValue` layout work, no
  behaviour change; the test suites must be untouched by this item.

- **Acceptance:** public method count on `Heap` measurably reduced with no test
  file edited; clippy and both chibi lanes unchanged.

### Q5 — Perf regression gate in CI
Track P's arc (2.2× → 0.79× vs Chibi) is measured entirely by manual sweeps
against an out-of-repo benchmark checkout. A cliff-shaped regression would reach
`main` unnoticed.

- Pick 3–4 short, in-repo benchmarks that exercise distinct mechanisms (call
  path, allocation, string/IO) and run them in CI against a committed baseline.
- **Threshold must be loose** — Track P §1.1 documents ±5–10% single-run drift
  on µs-scale benches, and CI runners are noisier than the dev machine. Fail at
  something like >25%, so the gate catches cliffs and never cries wolf. A gate
  that flaps will be disabled, which is worse than no gate.
- Record the baseline refresh procedure next to the baseline file, so updating
  it is a deliberate, reviewed act.

- **Acceptance:** an artificially introduced 2× slowdown fails CI; three
  consecutive no-op runs pass without flapping.

### Q6 — One number, one home
- **`docs/` is current truth; `PRD/` is an append-only engineering log.** A
  status number (geomean, pass rate, corpus score) lives in exactly one file;
  everything else links to it.
- Reconcile the known drift: `MILESTONES.md` 0.93× vs Track P §1.11 0.79×; the
  stale-link list in `SNOW_AND_PERF_ROADMAP.md:51-54` that is itself stale; and
  `PRD/README.md`, which is the directory's front door and is wrong about the
  test count, the current phase, and which tracks exist. Fixing the index is the
  highest-value single edit in this item — it is what a new reader hits first.
- Stop version-controlling generated artifacts, or regenerate them in one
  committed step: `scheme_tests/reports/*` currently dirty the working tree on
  every run with timestamp-only churn.

The PRDs themselves are an asset and stay — the recorded negative results (Track
P #30, the scratch-buffer regression, the globals-`Rc` invariant) are among the
most valuable documents in the repo. What changes is that they stop duplicating
live status.

- **Acceptance:** no two files state the same status number; a full test run
  leaves the working tree clean.

---

## 5. Sequencing

**Q0** (one CI job) → **Q1** (harness, seeded with the §1.2 failures) → **Q2**
(fix the cluster, watch the harness go green) → then **Q3**, **Q5**, **Q6** in
any order, with **Q4** last because it is the only item that touches code
everything depends on and it benefits from Q1 being in place first.

Q0 and Q1 are the ones that pay for the rest: Q0 is nearly free, and Q1 converts
every future backend divergence from a manual discovery into a test failure. Q2
is the proof that they were needed.

## 6. Risks & mitigations

- **A flapping perf gate gets disabled** → loose threshold, few benchmarks,
  documented baseline refresh (Q5).
- **The differential harness becomes a second place to enumerate cases**, drifting
  from the real suites → seed it from existing corpora (chibi report sections,
  `vm_*.rs`) rather than hand-writing a third list.
- **Q2 regresses the P2/P3 fast paths** by making the compiler less willing to
  special-case → the fast path is unchanged by construction; only the fallback
  binding is added. Re-run the Track P scoreboard subset before merging, and
  treat any movement as a blocker.
- **Q4 turns into a rewrite** → the acceptance criterion is deliberately "no test
  file edited." If a test needs changing, the change is out of scope.
- **Property tests that are slow or flaky** → fixed seed, low case count in CI,
  larger budget nightly only.
- **The `unsafe` bridge** (watch item, not a work item): `VmApplyContext::heap()`
  hands out a `&SharedHeap` tied to `&self` while `apply_proc` takes
  `&mut *self.state`. No miscompile has been observed and the argument in the
  doc comment holds for the current call pattern, but a Miri run over the
  higher-order primitive tests would cheaply confirm it. Fold into Q3 if
  convenient.

## 7. Verification (track-wide)

- Routine after every item: `cargo build --release && ./scripts/run_chibi_tests.sh`
  and `./scripts/run_chibi_tests_tree_walker.sh` — both must stay 1226/1226.
- `cargo test --all --lib --tests` **and** `cargo test --package patina-tests
  --features vm-backend --tests` must both pass (Q0 makes the second one CI's
  responsibility rather than a human's).
- Quality gate unchanged: `cargo clippy --all-targets --all-features -- -D warnings`
  and `cargo fmt`.
- Track-level metric: **the number of behaviours that differ between the two
  backends, as reported by Q1's harness.** It starts at the §1.2 table and is
  expected to reach — and stay at — zero.
