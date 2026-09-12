# Track H — Hygiene Assurance PRD

**Created:** 2026-08-31
**Updated:** 2026-09-12 — H1's bounded generator, historical capture check and
binding-preserving shrinking measured against `c18f1edf` and `1393c8f7`.
**Status:** H1's initial harness is implemented for the matrix's 28 shapes.
H2's harness is provided by [#288](https://github.com/avalonalex/patina/pull/288),
with three write-path defects tracked separately. H3 is not started;
H4 is unevaluated and optional; H5 remains deferred.
Written when triage families 36 and 38 closed with the matrix at 28 of 28:
hand-enumerated regressions still leave discovery to chance. This track exists
so the next family is found by generated tests.
**Scope decision:** test binding-aware properties over a specified subset,
with metamorphic and property-based tests before considering verification.
Uniform spelling substitution alone does not test capture avoidance. Full
mechanization is deferred to H5; a separate model risks drifting from Rust.
**Dependencies:** start with H2, then H1; they can be developed independently.
H3 builds on H1's generator and shrinker and needs a repeatable Chibi/Racket
runner. H4 is an optional evaluation after H2. Work item IDs remain stable.

---

## 1. Context & problem

Six macro-hygiene defects (triage families 33–38) shipped while the chibi
suite read **1226/1226 on both backends** throughout. Each was found by
accident — a corpus package tripping over one, a review noticing another —
and two fix attempts (PRs #133 and #138) were closed for fixing one direction
of a shape while silently breaking the opposite direction, because nothing
enumerated the opposite direction.

The structural reason confidence stays low after each round of patches:
Patina's hygiene is not one algorithm. It is distributed across the
desugarer's scope stamping, two backends' resolution (`Environment` at
runtime, `alpha_rename` at compile time), the relinker, and the by-name
fallbacks — and the recurring failure mode is an interaction between two of
those. Worse, **both backends share the desugarer**, so the VM/tree-walker
differential that catches backend divergence is structurally blind to
frontend defects: family 36's internal-define shape read identically wrong
on both backends.

What exists today, to build on rather than duplicate:

| Instrument | What it gives | Its ceiling |
|---|---|---|
| `crates/patina-tests/tests/hygiene_matrix.rs` | 28 shapes (7 binders × site × read/write), scored against chibi 0.12 **and** Racket 9.3, pinned in both directions | Hand-enumerated; its own header lists additional axes it does not cover |
| `PATINA_SCOPE_TRACE` | What scope set a binding actually carries, and how a reference resolved, per phase | Explains a failure; finds nothing on its own |
| Ambiguity as an error (Flatt's rule, PR #134–#137) | Read resolution refuses a reference with no candidate containing all other eligible candidates | Catches under-determination, not capture |
| `scripts/run_suite_oracles.sh` | Repeatable Chibi/Gauche comparisons with classified divergences | Fixed suite inputs; not H3's generated Chibi/Racket lane |
| Corpus + Larceny lanes | Real-world programs | Discovery by accident — the lesson of families 33–39 |

The initial gap was generated assurance: H1 now exercises binding-aware
transformations over the matrix's subset; H2's kernel/environment properties
are in #288. Known expected failures remain outside the 28 matrix shapes,
including introduced definitions (#269), cross-expansion globals (triage
family 40), and H2's environment write-path defects (#289–#291). Green CI
includes quarantines; it is not a claim that all hygiene behavior is correct.

## 2. The property

Hygiene preserves binding relationships through expansion. The cited
α-equivalence results require a definition of binding, not a substitution
of one spelling everywhere. Herman & Wand use explicit binding
specifications; Adams explains why arbitrary unexpanded macro syntax does
not itself reveal its binding structure.

For example, suppose a macro's reference to global `x` is incorrectly
captured by a local `x`. Replacing every `x` with `fresh-x` preserves the
collision: the buggy expander can give the same wrong answer twice. Renaming
only the local binder and the references belonging to it breaks the accidental
capture while preserving the intended program. This is the transformation H1
must exercise.

The first implementation therefore uses a small generated language with
explicit binder identities and reference edges. Those relationships come
from the generator's specification, independently of Patina's resolver.
Serialize an original program and binding-preserving variants, then compare
observable results on each backend separately. VM/tree-walker agreement is
additional evidence, not an oracle for a shared frontend defect.

Uniform whole-program spelling permutation may be a supplementary check over
this restricted language, but is neither sufficient capture coverage nor a
replacement for the historical-failure acceptance test. Imported names,
keywords, quoted data, syntax-rules literals, and constructed symbols cannot
be treated as ordinary variable occurrences.

## 3. Goals

- Hygiene coverage that is **generated, not enumerated** — the matrix's own
  "not covered yet" axes (macro-generated macros, cross-library macros,
  expansion nested more than one level, ellipsis depth > 1,
  `define-record-type` and derived binding forms, pattern literals) reached
  by a generator rather than by hand-written rows.
- The invariants that actually regressed, stated as **properties over random
  inputs** rather than as six repros: read/write symmetry (family 38),
  chain-totality (family 39), fallback-respects-rejection (family 36 step 1).
- A **counterexample pipeline**: anything a generator finds is shrunk and
  lands as a matrix row and a triage family, entering the existing process.

## 4. Non-goals (deferred)

- **Full mechanized verification of the expander.** Surveyed 2026-08-31:
  hygiene correctness proofs exist on paper (Herman & Wand; Adams; Clinger &
  Rees 1991 for their algorithm), Flatt's set-of-scopes model (POPL 2016 —
  Patina's model) has no mechanization, and no verified `syntax-rules`
  expander exists to adopt. A hand-built Coq/Lean model of *this* codebase
  would drift from the Rust it models. Revisit at Phase 3 (`syntax-case`):
  re-founding expansion on one specified algorithm is the moment a model is
  the spec rather than a shadow — see H5.
- Replacing the matrix. It stays the human-readable scoreboard and the
  landing place for shrunken counterexamples.

## 5. Work items

| Item | Tracking | Status |
|---|---|---|
| H2 | [#284](https://github.com/avalonalex/patina/issues/284) | Harness in #288; behavior fixes tracked in #289–#291 |
| H1 | [#285](https://github.com/avalonalex/patina/issues/285) | Initial 28-shape harness implemented; historical check and shrinker demonstrated |
| H3 | [#286](https://github.com/avalonalex/patina/issues/286) | Implementation not started |
| H4 | [#287](https://github.com/avalonalex/patina/issues/287) | Optional evaluation; not started |
| H5 | No issue until the syntax-case boundary | Deferred |

### H1 — binding-aware metamorphic harness *(oracle-free, after H2 in priority)*

Implement a bounded generator in `patina-tests` whose syntax carries stable
binder identities and references to those identities, plus explicit
macro-definition and use-site contexts. Start with the matrix's seven binder
forms, inside/outside sites, and read/write directions. The generator owns
the binding specification; do not ask Patina to determine the edges being
tested, and do not rewrite arbitrary source with textual replacement.

For each supported seed:

1. Emit an original program with intentional spelling collisions.
2. Rename one binder and exactly its bound references to a fresh spelling,
   preserving macro definition-site references to other bindings. Exercise
   both read and assignment occurrences.
3. Emit controlled poison-shadow variants: add a colliding use-site binder
   for a macro's definition-site reference only when the generator establishes
   that it changes no argument binding or other source-level reference.
   Wrapping arbitrary `(m x)` in a new `x` binding is not invariant.
4. Run each variant on both backends. Within each backend, compare termination,
   values and relevant ordered effects against the original. Compare backend
   answers separately; two implementations sharing a frontend can agree wrong.

Use the 28 matrix programs as the first seed templates. Extend to targeted
repros in `tests/scheme/expansion/{hygiene,let-syntax,ellipsis,template-references}.scm`
only after their binding forms are represented. Arbitrary suite/corpus source
rewriting is deferred, not an implied first milestone. Preserve quoted data;
exclude reflective symbol construction, `eval`, unsupported binding forms,
and unspecified-order effects from the initial language. Report exclusions.
Compare intentional errors by class, not name-bearing messages, in a separate
negative-test set; two rejected positive programs are not a successful case.
Timeouts and crashes are distinct failures, never equivalent outputs.

**Acceptance:** normal `cargo test` gate with deterministic seeds, a fixed case
budget and time bounds; recorded eligible/executed counts and nonzero coverage
of each advertised axis. A failing report contains source, transformation,
seed and backend. A shrinker preserves binding relationships and the failure;
its result can become a matrix row. Quarantines must name a specific defect
and fail on unexpected success; do not skip a whole axis to obtain green.

**Non-vacuity:** first demonstrate the binding-specific rename detects the
silent-capture case on historical commit `c18f1edf`, then passes on current
main. Record the exact seed and result. Demonstrate on that same case why
uniform spelling replacement alone misses the defect. This is the first
experiment, before building a larger harness. Current known defects found
by the harness become explicit regressions and separate fixes, not changed
metamorphic expectations.

#### H1 implementation and evidence — 2026-09-12

The normal test gate runs
[`hygiene_metamorphic.rs`](../crates/patina-tests/tests/hygiene_metamorphic.rs),
with its binding model in
[`hygiene/generator.rs`](../crates/patina-tests/tests/hygiene/generator.rs)
and process isolation in
[`hygiene/runner.rs`](../crates/patina-tests/tests/hygiene/runner.rs).
It uses the existing public interpreter helpers; no resolver, expander,
backend or dependency change is needed.

The generated AST assigns stable identities to the global, local, macro and
auxiliary bindings. Read and assignment nodes refer to identities; only the
serializer assigns spellings. The macro template's target is the global for
an outside definition and the local for an inside definition. Renaming either
binding changes its name-table entry, preserving the other binding's template
references. Keywords, builtins and quoted data are separate AST nodes.

Each case emits five programs: original, local-binder rename, global-binder
rename, poison shadow, and a supplementary uniform permutation of the colliding
variable spelling. Poison insertion wraps exactly the **nullary macro call**,
which has no argument references to capture. It never wraps the observations
or the macro definition. All calls in this initial grammar are eligible.

The bounded language follows the matrix's seven binder forms and both site
and action directions. Each of its 28 shapes has one canonical sample and
three generated samples: **112 cases**, with **560 program evaluations per
backend**. The canonical samples use numeric global/local values 1 and 5,
assignment 99, and the same binding bodies as the matrix, including the empty
`let` that supplies a definition context for an inside macro in `do`.
Generated samples vary global values 1–4, local values 5–8, assignments 99–102,
poison values 199–202, three colliding spellings, and 0–2 extra lexical bindings.
Their ordered `before`/`after` effects are returned as data alongside the
result, global value and literal symbol `x`. `do` exits immediately and the
named-let body does not recurse. This is bounded sampling, not a universal
claim about those binding forms.

The seed base is **285**. Canonical samples use seed 285; other samples use
`285 + 4 * shape_index + sample_index`, where samples are 1–3 and shapes follow
`BINDERS`, outside/inside, read/write order. The generator's explicit integer
stream makes those seeds replayable without process-global randomness.
Counts assert four executed cases for every shape on each backend, nonzero
coverage at each padding depth, and 28 cases without / 84 with effect logging.
The first sweep on main's runtime **`1393c8f7`** completed all **112/112** eligible
cases for every transformation, with zero rejected, timed-out or crashed
programs and no new quarantine. The focused gate took approximately 7 seconds
on the development machine; this is not a CI performance threshold.

Each case/backend batch runs in a child process with a **5-second deadline**;
each generated test has a **120-second total budget**, including shrinking.
A timeout kills and reaps the child. Rejection, process failure, missing output
and timeout are distinct failures, even if another variant fails the same way.
A harness test exercises rejection, a nonzero child exit and a nonterminating
program. Intentional negative Scheme programs are not part of the generated
language; syntax-rules arguments/literals/ellipsis, generated macros, libraries,
other binding forms, reflective symbols/`eval`, and unspecified-order effects
remain excluded. H3 owns expansion beyond this subset.

The historical experiment ran first, before building the larger harness.
The completed three-file harness was then ported **unchanged** to an isolated
worktree at **`c18f1edf19fd9785bba06616e2332e2bccab0074`**. No runtime code,
dependency manifest, lockfile or toolchain adaptation was needed. Replay:

```bash
cargo test -p patina-tests --test hygiene_metamorphic -- --nocapture --skip generated_matrix_programs_preserve_bindings_and_ordered_effects
```

For `Let/Outside/Read`, seed **285**, the recorded observations were:

| Runtime | Original | Rename only local | Uniform permutation |
|---|---|---|---|
| `c18f1edf`, VM | `(1 1 x)` | `(1 1 x)` | `(1 1 x)` |
| `c18f1edf`, tree-walker | `(5 1 x)` | `(1 1 x)` | `(5 1 x)` |
| `1393c8f7`, both backends | `(1 1 x)` | `(1 1 x)` | `(1 1 x)` |

The old tree-walker silently captures the macro's global reference. A uniform
permutation preserves that collision and misses the defect. The local rename
breaks it while retaining the literal `x`. The historical test exits **101**;
the same test passes on the current runtime.

`generated_capture_case_retains_its_failure_while_shrinking` starts from seed
285 with global/local values 2/6, assignment/poison values 100/202, two extra
bindings, effect logging, and spelling `h1-value`. On the historical tree-walker
it reduces in **8 attempts** to values 1/5/99/199, no extra bindings or logging,
and spelling `x`. The reducer re-emits paired programs from the binding model,
keeps the form/site/action and the identities of surviving references, and
retains only the same backend, transformation and failure class. It has a
**32-attempt cap**; it does not claim a globally minimal Scheme program.
The retained pair is ready to become a matrix regression:

```scheme
(define x 1)
(define-syntax h1-m (syntax-rules () ((h1-m) x)))
(define h1-result (let ((x 5)) (h1-m)))
(list h1-result x 'x)
;; Variant: change only the let binder to h1-local-285.
```

For the normal gate and coverage report, run
`cargo test -p patina-tests --test hygiene_metamorphic -- --nocapture`.
Metamorphic failures report the initial case, seed, backend, transformation,
outcome class, shrink budget and both reduced sources. Backend agreement is
checked separately after each backend's metamorphic checks pass.

Local validation: `cargo test --all --lib --tests` passed **1,158 tests** with
three existing ignored tests; the release build and both Chibi backend scripts
passed **1,226/1,226** each. Clippy with all targets/features and warnings denied,
the format check, PRD relative links, and `git diff --check` also passed.

### H2 — property tests on the resolution kernel *(first priority; pure Rust)*

Add `proptest` suites in `patina-core` over bounded scope sets, ordered
candidate tables and environment chains. Include empty tables, invisible
bindings, identical-scope ties, incomparable eligible candidates, and distinct
bindings holding equal values. State the supported domain explicitly; random
tests sample it and do not constitute a universal proof.

| Property | Precise outcome and regression family |
|---|---|
| Chain resolution | Eligible candidates ordered by subset cannot be ambiguous. Return no binding when none is eligible; otherwise select the greatest eligible scope set, breaking identical-set ties by recency. Family 39. |
| Read/write symmetry | For a uniquely resolvable reference, reading and assignment select the same binding identity, not merely equal values. Check with a fresh sentinel and inspect all bindings. Ambiguous references must be rejected without mutation; unbound references must not create or change a binding. Family 38. |
| Fallback respects rejection | A by-name fallback cannot recover a scoped binding rejected for this reference. Distinguish that binding from a legitimately visible plain binding of the same spelling. Family 36 step 1. |
| Determinism and ties | Fixed inputs and candidate order produce the same result. Identical-scope ties select the most recent candidate and remain reportable, not ambiguous. Recency is part of the input; candidate permutation is not generally invariant. Family 39. |

**Acceptance:** runs in normal `cargo test`; every property names its family
and input domain. Use reproducible seeds, bounded cases, and shrinking;
record seed, minimized table/environment and outcome. Historical non-vacuity:
the pre-#137 exact-match write rule fails a valid subset read/write case
within the fixed budget. Record the historical revision and case, then verify
the corrected behavior on main. Any controlled mutation is supplemental and
must be documented, not substituted silently for the historical check.

H2 may discover current failures: `set_with_scopes` still resolves writes
separately from reads and does not enforce the same ambiguity rule. Land the
minimal reproducer before a separate behavior-changing fix, with a narrowly
identified expected failure if necessary. Unresolved cases stay visible; a
property's target is not weakened to make current code pass.

These properties guard Track Q Q7.1's later write-path consolidation. Any
behavior correction discovered here must be separated from that refactor;
Q7.1 starts only after its applicable properties pass without quarantine.

### H3 — differential generation and shrinking *(manual or scheduled lane)*

Extend H1's binding-aware grammar to macro-generated macros, library imports,
expansion depth, ellipsis depth, derived binding forms, pattern literals and
introduced globals. Reuse its controlled poison-shadow transformations and
shrinker rather than build a second transformation engine.

Script one command to run identical generated programs on patina-VM,
patina-tree-walker, Chibi and Racket. Record the tested versions (the existing
matrix used Chibi 0.12 and Racket 9.3). The current Chibi/Gauche suite oracle
script is supporting infrastructure, not completion of this deliverable.

Generate positive programs in the agreed language subset. Sequence observable
effects explicitly, avoid unspecified evaluation order and R7RS "is an error"
territory. Record rejected, unsupported, timed-out and crashed runs separately;
none counts as an agreeing successful program. Preserve and classify such
cases rather than silently discard unexpected oracle failures. An intentional
negative-program lane is separate. Missing required oracles fail the lane.

Every disagreement is minimized while preserving binding structure and its
classification, then investigated: it may be a Patina defect, oracle defect,
language latitude or generator error. Only confirmed Patina defects become
triage entries and regression rows asserting the justified answer. Do not
adopt an oracle's answer by majority vote or retire an existing pin blindly.

**Acceptance:** a documented manual/scheduled command, fixed replayable seeds,
case/time budgets, per-axis generated/accepted/compared/excluded counts and
retained minimized failures. Reproduce a historical hygiene failure, then run
a recorded first sweep that yields either classified defects or a clean
baseline with a nonzero compared count for each claimed axis. Demonstrate the
shrinker on a failing seed; preserve its seed, versions and reduced source.
The larger sweep is not required on every PR.

### H4 — bounded verification of the kernel *(optional; evaluate before committing)*

The resolution kernel (`patina_core::scope_resolve` plus the environment's
get/set pair) is small enough to evaluate two different approaches:

- **Kani or Creusot on the real code**: bounded proof that for all tables up
  to size N, resolution is deterministic, follows H2's chain rule, and the fallback
  respects rejection. No model-implementation gap. Evaluate first whether
  the `Rc<RefCell<…>>` environment plumbing needs the kernel extracted
  further (it is already mostly a pure module).
- **An independent reference expander** over a documented mini-core,
  compared against the desugarer on H3's generated programs. This adds an
  independent implementation, not a proof: it needs its own reviewed binding
  specification and validation, and can itself be wrong. Do not assume a line
  count or correctness by inspection. It addresses the shared-frontend blind
  spot of comparing only Patina's two backends.

**Acceptance:** a written evaluation (which route, what N or what mini-core
subset, assumptions, exclusions, cost, and exactly what evidence it provides)
even if the outcome is "not worth it" — the evaluation is the deliverable;
commitment to more is a separate decision.

### H5 — deferred: mechanization at the syntax-case boundary

If Phase 3 re-founds expansion on one specified algorithm (see
`PRD/macro/SYNTAX_CASE_DESIGN.md`), that specification is the moment to
consider a mechanized model — Lean 4 is the natural host (its own expander
is the hygiene design built for a theorem-proving language: Ullrich & de
Moura, IJCAR 2020). Until then a model would shadow a moving implementation.
Parked deliberately; nothing in H1–H4 depends on it.

## 6. Sequencing

Priority is H2 → H1 → H3. H1 and H2 have no implementation dependency on
each other; both become bounded per-PR gates. H3 reuses H1's binding-aware
generator and requires external-runner scripting. H4 is opt-in after H2,
with H3 needed only if evaluating the reference-expander route. H5 stays
parked; no implementation issue is needed until the syntax-case boundary.

## 7. Verification (track-wide)

Every harness must detect a recorded historical defect before it counts as
landed. Use an isolated worktree at the historical revision, port only the
harness and required test plumbing, and document compatibility adaptations.
Do not transplant the runtime fix along with the harness. Record revision,
seed, case budget, command and failing semantic observation, then the result
on main. The anchor for H1 is `c18f1edf`, the last main on which the matrix
read 10 cells wrong. H2 records the applicable pre-#137 revision explicitly.

H3 must also demonstrate shrinking. H4's deliverable is an evaluation, not a
mandatory green harness. Existing expected failures remain named and visible;
new failures get minimized cases and separately reviewed fixes. Distinguish a
clean tested subset from unexplored axes, skipped cases and quarantined bugs.

Documentation-only updates need path/link checks and `git diff --check`;
implementation follows AGENTS.md's affected-test and backend verification
requirements. Preserve the existing matrix, suite oracle classifications and
runtime boundaries while adding these instruments.

## 8. References

- E. Kohlbecker, D. P. Friedman, M. Felleisen, B. Duba. *Hygienic Macro
  Expansion.* LFP 1986.
- W. Clinger, J. Rees. *Macros That Work.* POPL 1991.
- R. K. Dybvig, R. Hieb, C. Bruggeman. *Syntactic Abstraction in Scheme.*
  LSC 1992 (syntax-case).
- D. Herman, M. Wand. *A Theory of Hygienic Macros*.
  ESOP 2008 — α-equivalence preservation with explicit binding specifications.
- M. D. Adams. [*Towards the Essence of Hygiene*](https://michaeldadams.org/papers/hygiene/hygiene-2015-popl-authors-copy.pdf).
  POPL 2015 — algorithm-independent criteria and the binding-structure problem
  for unexpanded macro syntax.
- M. Flatt. *Binding as Sets of Scopes.* POPL 2016 — Patina's model; §4's
  use-site scopes are the distinction family 39 reached by a smaller change.
- S. Ullrich, L. de Moura. *Beyond Notations: Hygienic Macro Expansion for
  Theorem Proving Languages.* IJCAR 2020 — Lean 4's expander, designed to be
  reasoned about.
- W. Clinger, M. Wand. *Hygienic Macro Technology.* HOPL IV, 2020 — the
  survey.
- W. M. McKeeman. *Differential Testing for Software.* DTJ 1998; X. Yang et
  al. *Finding and Understanding Bugs in C Compilers* (CSmith). PLDI 2011 —
  the differential-fuzzing practice H3 follows.
