# Track H — Hygiene Assurance PRD (archived)

**Created:** 2026-08-31
**Archived:** 2026-09-12 — H1–H3's initial bounded harnesses are merged in
[#292](https://github.com/avalonalex/patina/pull/292),
[#288](https://github.com/avalonalex/patina/pull/288) and
[#293](https://github.com/avalonalex/patina/pull/293). H4's evaluation is complete:
no additional verifier or reference expander is adopted for this architecture.
**Remaining work:** runtime defects #289–#291 and triage families 40/41 stay
open in the [defect queue](../../../scheme_tests/reports/larceny_triage.md). Closing
this assurance track does not claim that hygiene is fully correct or proved.
H5 is now owned by the
[syntax-case design](../../macro/SYNTAX_CASE_DESIGN.md#deferred-mechanization-at-the-syntax-case-boundary-h5).
Written when triage families 36 and 38 closed with the matrix at 28 of 28:
hand-enumerated regressions still leave discovery to chance. This track exists
so the next family is found by generated tests.
**Scope decision:** test binding-aware properties over a specified subset,
with metamorphic and property-based tests before considering verification.
Uniform spelling substitution alone does not test capture avoidance. Full
mechanization is deferred to the syntax-case design; a separate model risks
drifting from Rust.
**Dependencies:** start with H2, then H1; they can be developed independently.
H3 builds on H1's generator and shrinker and needs a repeatable Chibi/Racket
runner. H4 evaluated the next assurance options after H2/H3. Work item IDs
remain stable for historical links.

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
transformations over the matrix's subset; H2 samples the resolution kernel
and environment APIs; H3 compares generated programs against external oracles.
Known expected failures remain outside the 28 matrix shapes,
including introduced definitions (#269), cross-expansion globals (triage
family 40), pattern literals (family 41), and H2's environment write-path
defects (#289–#291). Green CI
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
  the spec rather than a shadow — see the transferred H5 decision in the
  [syntax-case design](../../macro/SYNTAX_CASE_DESIGN.md#deferred-mechanization-at-the-syntax-case-boundary-h5).
- Replacing the matrix. It stays the human-readable scoreboard and the
  landing place for shrunken counterexamples.

## 5. Work items

| Item | Tracking | Status |
|---|---|---|
| H2 | [#284](https://github.com/avalonalex/patina/issues/284) | Harness implemented; H2-A/B/C remain quarantined in #289–#291 |
| H1 | [#285](https://github.com/avalonalex/patina/issues/285) | Initial 28-shape harness implemented; historical check and shrinker demonstrated |
| H3 | [#286](https://github.com/avalonalex/patina/issues/286) | Initial bounded lane implemented; historical shrinking demonstrated; first sweep classified (families 40/41) |
| H4 | [#287](https://github.com/avalonalex/patina/issues/287) | Evaluation complete; no additional verifier or reference expander adopted |
| H5 | [Syntax-case design](../../macro/SYNTAX_CASE_DESIGN.md#deferred-mechanization-at-the-syntax-case-boundary-h5) | Transferred; deferred until that rewrite |

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
[`hygiene_metamorphic.rs`](../../../crates/patina-tests/tests/hygiene_metamorphic.rs),
with its binding model in
[`hygiene/generator.rs`](../../../crates/patina-tests/tests/hygiene/generator.rs)
and process isolation in
[`hygiene/runner.rs`](../../../crates/patina-tests/tests/hygiene/runner.rs).
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

#### H2 implementation and evidence — 2026-09-12

The normal `cargo test` gate runs
[`hygiene_properties.rs`](../../../crates/patina-core/src/hygiene_properties.rs)
as a test-only child of `environment`, so snapshots can inspect binding
identities without adding a runtime API. Its independent oracle uses bitmask
inclusion, with candidate position and `(frame, scope mask)` as identities.
It checks all generated cells after assignment and reads back a fresh sentinel
to distinguish cells that initially held equal values. A child process checks
that identical-scope ties still produce `TIE` diagnostics without an `AMBIG`.

Supported domain: six scope IDs; ordered tables of 0–12 candidates (including
empty scope sets); 1–4 environment frames with 0–6 insertions per frame,
nonempty scoped bindings, optional plain bindings, both name-visible and
scope-only entries, and initial fixnums 0–2. Environment references are
nonempty; aliases, empty-reference name lookup, heap values, GC, and
frontend/backend integration are excluded. Equal-scope redefinitions replace
the existing cell without moving its insertion position. Every generated
property uses seed **284**, **256 cases**, and at most **4096 shrink steps**
with locked `proptest` **1.11.0**. Failure messages retain the minimized input
and expected/actual outcome; no machine-local seed file is required.

Historical non-vacuity was measured in an isolated worktree at
**`5b93bf736be8f19b733cdc61bb1da001dc543227`**, the parent of #137
(`6a86e21`). The public-API probe is
[`hygiene_subset_property.rs`](../../../crates/patina-core/tests/hygiene_subset_property.rs),
test `family38_proper_subset_write_reaches_read_binding`. Porting required
only the `proptest` dev dependency/lockfile, `"x".to_string()` at the old
definition API, and removal of the read-result `.unwrap()` because the old
API returned `Option` directly. No runtime source or toolchain was changed.

Run in either checkout:

```bash
cargo test -p patina-core --test hygiene_subset_property family38_proper_subset_write_reaches_read_binding -- --exact --nocapture
```

With seed 284 and the budget above, the historical run exits **101**, shrinking
to `(mask=1, depth=1, value=0)`: one root binding `x` at `{S0}`, reference
`{S0,S5}`, successful read of `0`, but assignment of `999` returns `Err("x")`.
The same generated property passes on the current main runtime (`1393c8f`),
as does the explicit regression `family38_scoped_write_updates_a_proper_subset_binding`.
This is a run against historical Rust, not a controlled mutation.

The following minimized cases remain open. Masks use bit `n` for `Sn`;
frames are root first, insertions oldest first, and all initial values are 0.
They exercise the environment API; their reachability from Scheme is not
established by this harness.

| Quarantine | Minimal environment and reference | Required / observed outcome |
|---|---|---|
| H2-A ambiguous write | One frame, scoped `x` at masks 1 and 2; reference 3 | Reject with no mutation / writes 999 to mask 2 |
| H2-B binding identity | Root scoped `x` at mask 3, child at mask 1; reference 3 | Write root mask 3 / writes child mask 1 |
| H2-C plain fallback | Empty root, child plain `x`; reference 1 | Write child plain binding / returns `Err("x")` |

Each quarantine checks its named failure and rejects unexpected success.
The arbitrary-environment symmetry property classifies those same input
shapes independently of the observed result; all generated cases execute.
Other mismatches fail normally. Run `cargo test -p patina-core --lib
hygiene_properties -- --nocapture` to print the three minimized outcomes.
Correcting them is separate work and still blocks Q7.1.

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

#### H3 implementation and replay — 2026-09-12

Run the manual lane from the repository root:

```bash
./scripts/run_hygiene_differential.sh
./scripts/run_hygiene_differential.sh --seed 286 --case 36
./scripts/run_hygiene_differential.sh --historical
```

The script builds the release runtime, then selects one explicitly ignored
test in `hygiene_metamorphic.rs`. Normal `cargo test` still runs H1 and the
runner's self-checks; it does not require external oracles. `CHIBI`, `RACKET`
and `RACO` override executable paths. `H3_PATINA` can select an already-built
runtime; the report labels it as an override and records the binary checksum.
`--output NEW_DIR` selects an artifact directory without overwriting a prior
sweep. The default is a unique directory under `target/hygiene-h3/`.

Racket needs the [R7RS language package](https://github.com/lexi-lambda/racket-r7rs),
not just the Racket executable. An isolated local setup is:

```bash
export PLTUSERHOME="$(mktemp -d)"
raco pkg install --scope user --auto --batch r7rs
./scripts/run_hygiene_differential.sh
```

The first measurements used Chibi **0.12.0**, Racket **9.3 [cs]**, and
`r7rs-lib` revision **`dfe5a961eb6f305a84a086af788a6fd0f603e5da`**. The lane
records actual version output and the package checksum on every run. Required
executables, the R7RS language and the library adapter must pass preflight;
missing or nonfunctional oracles fail before any generated case counts.

[`hygiene/extended.rs`](../../../crates/patina-tests/tests/hygiene/extended.rs) adds
one axis at a time to H1's binding graph. Both lanes use the same
`Program::variants` transformation engine and
[`hygiene/shrink.rs`](../../../crates/patina-tests/tests/hygiene/shrink.rs) reducer.
Unused padding identities stay reserved when shrinking, so removing a wrapper
does not renumber the surviving H3 bindings. Library-private and
macro-introduced globals have identities distinct from the same-spelled
source global. Renaming the selected global changes those bindings and their
references together, including inside the generated library. The sole
poison-eligible source call remains nullary; helper calls with arguments and
macro definitions are separate nodes.

The bounded additions are:

| Axis | Generated subset |
|---|---|
| Generated macros | An installer generates a macro whose template reads or assigns a definition-site binding; both outside/inside sites |
| Imports | One generated R7RS library exporting a macro and a value observer; reads refer to private state and writes call a library-owned setter |
| Expansion depth | Chains of 2–4 nullary macro expansions |
| Ellipsis depth | Depths 1–3, widths 1–3, with nested numeric data whose reconstructed shape is checked before the target operation |
| Derived binders | `let-values`, `let*-values`, `letrec*`, `define-values`, both sites and read/write directions |
| Pattern literals | Bound literal matching and a same-spelled distinct local binding selecting the fallback rule |
| Introduced globals | One expansion introduces private state, a macro and an observer; the source global remains a separate binding |

The base seed is **286**, with case seed `base + index`. The fixed plan has
**92 candidates**: 28 baseline matrix cases, 16 derived-binder cases and eight
for each other new axis. Eight inside-site candidates for imports/introduced
globals are explicitly excluded because this grammar places those definitions
at top level. Every eligible case emits five variants. Arbitrary source
rewriting, arbitrary macro arguments, combinations of new axes, recursive
generated macros, record types, reflective symbols/`eval`, unspecified-order
effects and intentional negative programs remain outside this first sweep.

[`hygiene/oracles.rs`](../../../crates/patina-tests/tests/hygiene/oracles.rs) executes
the same `main.scm` on all four implementations and the same `generated.sld`
where a library is present. Racket's collection adapter is just a `#lang r7rs`
file that includes that `.sld`; it does not rewrite its contents. Values and
ordered effects are emitted as one marked datum. Rejects, unavailable commands,
signals/Rust panics, missing/malformed output and timeouts have separate
outcomes; none is successful agreement.

Budgets are **15 seconds per process**, **1,800 seconds per sweep including
shrinking**, and **32 reduction attempts per finding**. An earlier calibration
run used five seconds and encountered startup timeouts during heavy host load,
including a package-version query. It was interrupted and retained as
inconclusive; those timeouts are not semantic findings. The retry uses the
fixed budgets above.

Each case retains its initial sources and process stdout/stderr. Findings
retain every reduction attempt and the minimized sources, seed, versions and
outcomes. Shrinking preserves the outcome kind of every backend/variant pair
and the equality partition of successful values, so incidental numeric values
can shrink without changing which comparisons fail. Automatic reports say
`needs-investigation`; no result becomes a Patina defect by majority vote.
`summary.tsv` distinguishes generated, accepted, compared, excluded and
disagreeing cases per axis; accepted/compared require all 20 runs to return
values. `outcomes.txt` separately counts initial process outcomes, excluding
preflight and shrink attempts.

The historical port copied only the eight harness/script files to an isolated
worktree at **`c18f1edf19fd9785bba06616e2332e2bccab0074`**, without changing
runtime code, dependencies or toolchain. `--historical` uses H1's generated
seed **285**, two padding bindings and ordered effects. It failed as expected
and reduced in **eight attempts** to:

```scheme
(import (scheme base) (scheme write))
(define x 1)
(define-syntax h1-m (syntax-rules () ((h1-m) x)))
(define h1-result (let ((x 5)) (h1-m)))
(write (list h1-result x 'x))
;; Rename only the let binder to h1-local-285 for the paired variant.
```

On that old tree-walker, original/uniform both return `(5 1 x)`, local/global
renaming returns `(1 1 x)`, and poison shadow returns `(199 1 x)`. The old VM,
Chibi and Racket return `(1 1 x)` for all five. This is the confirmed historical
Patina capture defect, triage family 36, already pinned by H1 and the matrix;
it is not a new open defect. The same witness passes on current main's runtime
**`0c992ddb`**.

The first full sweep exposed a generator/oracle boundary at seeds **323** and
**327** (`Imports/Outside/Write`). The original library emitted `set!` directly
into the importing program:

```scheme
(define-library (h3 generated)
  (export h1-m h3-library-value)
  (import (scheme base))
  (begin
    (define x 11)
    (define-syntax h1-m (syntax-rules () ((h1-m) (set! x 99))))
    (define (h3-library-value) x)))
;; Importer:
;; (import (scheme base) (scheme write) (h3 generated))
;; (h1-m) (write (h3-library-value))
```

Both Patina backends and Chibi assigned 99; Racket rejected the expansion with
`set!: cannot mutate module-required identifier`. The sources and eight-attempt
reduction were retained. Classification: **outside the agreed positive subset
at the module mutation boundary**, not a confirmed Patina defect. R7RS
[§5.2](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-7.html) forbids
mutating imported bindings; this lane does not use the other implementations'
acceptance to settle how that restriction applies to a hidden binding exposed
by macro expansion. The positive generator now keeps `set!` inside a private
setter procedure in the library and makes the imported macro call it. Its
poison variant shadows that procedure's spelling, so referential transparency
remains observable. Direct cross-module mutation is explicitly excluded.

**Final first-sweep measurement.** With that subset correction, the finalized
generator on main's runtime **`0c992ddb16f582258517ab47cbdd97b139093612`**
completed in **400.77 seconds** at base seed **286**:

| Axis | Generated | Accepted | Compared | Excluded | Disagreements |
|---|---:|---:|---:|---:|---:|
| Baseline | 28 | 28 | 28 | 0 | 0 |
| Generated macros | 8 | 8 | 8 | 0 | 0 |
| Imports | 8 | 4 | 4 | 4 | 0 |
| Expansion depth | 8 | 8 | 8 | 0 | 0 |
| Ellipsis depth | 8 | 8 | 8 | 0 | 0 |
| Derived binders | 16 | 16 | 16 | 0 | 0 |
| Pattern literals | 8 | 8 | 8 | 0 | 4 |
| Introduced globals | 8 | 4 | 4 | 4 | 4 |
| **Total** | **92** | **84** | **84** | **8** | **8** |

All **1,680 initial process runs returned values**: zero rejected, unsupported,
timed-out, crashed or protocol-failed runs. Every advertised axis has compared
cases. All eight disagreements were reduced, in **5/7/6/5** attempts for the
literal seeds and **4/5/6/6** for the introduced-global seeds listed below.
The command deliberately exits **101** because those confirmed runtime bugs
remain. The final local artifacts are in `target/hygiene-h3/sweep.htM5Sg`;
`harness-files.txt` records checksums of all eight harness/script files.
The earlier boundary-finding sweep is retained in `sweep.D8qv25`. These local
directories are disposable build artifacts; the seeds, classification,
reduced programs and regression rows are the durable replay record.

The semantic findings belong to two triage families:

- **Family 41, both backends:** seeds **364/365/368/369** make a template bind
  an identifier of the same spelling as a surrounding helper's literal, then
  pass that distinct binding to the helper. Both Patina backends select the
  literal arm; Chibi and Racket select the fallback. Gauche independently
  confirmed the minimized read and write programs. R7RS
  [§4.3.2](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-6.html) requires
  matching by lexical binding, so this is a confirmed Patina defect. With the
  reduced values, the read should yield 5 rather than 799; the fallback write
  should change the local to 99 rather than leave it at 5. Two portable rows
  in `expansion/syntax-rules-literals.scm` assert those answers with Patina-only
  expected failures.
- **Family 40, VM only:** seeds **370/371/374/375** generate private state and
  a macro in the same expansion, alongside a source global with the same
  spelling. The generated macro must reach the private binding. The VM instead
  reads or writes the source global; renaming only the private binding and its
  references changes its answer. Tree-walker, Chibi, Racket and an independent
  Gauche probe agree with R7RS §4.3's hygienic binding rule. The reduced read
  observation `(result source 'x private)` is `(11 1 x 11)`, versus the VM's
  `(1 1 x 11)`; the write observation is `(5 1 x 99)`, versus `(5 99 x 11)`.
  Two positive rows in `expansion/hygiene.scm` now complement family 40's
  existing cross-expansion error rows, with VM-only expected failures.

Both families and reduced sources are recorded in the
[triage queue](../../../scheme_tests/reports/larceny_triage.md). The fixed suite's
expectations fail on unexpected success so a runtime fix must retire them.
The manual H3 lane retains and fails every disagreement, including these known
ones; the new findings are not suppressed to produce a green sweep. Existing
oracle-defect and language-latitude rows in `DIVERGENCES.tsv` remain unchanged.

Validation of the harness and regression additions:

- `cargo test -p patina-tests --test hygiene_metamorphic --test scheme_suite`:
  **17 passed, one manual test ignored**. This includes H1's 112 cases and
  five variants per backend, the process-outcome checks and all fixed Scheme
  suite files on both backends.
- `SUITE_ORACLES_REQUIRE_ALL=1 ./scripts/run_suite_oracles.sh expansion/`:
  **30 file/oracle pairs matched the divergence register**. Both Chibi and
  Gauche were present. The existing registered Chibi non-completion for
  `template-references.scm` remains classified; it is not a passing program.
  Both oracles pass all four new regression rows.
- `cargo build --release`, `./scripts/run_chibi_tests.sh` and
  `./scripts/run_chibi_tests_tree_walker.sh`: **1,226/1,226 on each backend**.
- `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo fmt --all -- --check`, shell syntax, relative links and
  `git diff --check`: passed.
- A deliberately absent `CHIBI` path fails preflight as `Unsupported` with
  **zero generated cases**. The historical replay fails with the documented
  capture and eight-attempt reduction; the same witness passes all 20 runs
  on current main. All eight ported harness files were byte-identical, and
  the historical worktree had no tracked runtime changes.

The full Rust workspace and GC differential lanes were not rerun for this
bounded test-harness change; no interpreter runtime or GC code changed.

### H4 — bounded verification evaluation *(complete, 2026-09-12)*

**Decision:** retain H1/H2 as normal gates and H3 as the manual external lane.
Do not add Kani, Creusot or a fifth expander now. Kani is the preferred option
to reconsider for a bounded check of the production selector after a specific
need emerges; whole-expander mechanization belongs to the syntax-case rewrite.
H4's required deliverable was this evaluation, including a decision against
adoption. It did not require a new proof harness.

#### What was evaluated

The source inspection and experiment use merged commit **`7892a7cf`** and
Rust **1.97.1**, on an Apple Silicon host. `cargo-kani` and `cargo-creusot`
are not installed; no symbolic or deductive verifier was run. Tool feasibility
below comes from current primary documentation and inspection of the code,
not from a successful build with either verifier.

The boundary is narrower than the original phrase "kernel plus get/set":

- [`resolve_index`](../../../crates/patina-core/src/scope_resolve.rs) selects an
  index from caller-ordered candidates and rejects ambiguity. It delegates
  subset checks to the real `ScopeSet`, whose `SmallVec` holds three scopes
  inline and spills at four. Empty sets, duplicates and caller ordering matter.
- Error construction and tie reporting reach allocation, formatting and the
  diagnostic `OnceLock`/mutex/file sink. The selection algorithm is mostly
  pure, but the callable function is not a dependency-free arithmetic kernel.
- [`Environment`](../../../crates/patina-core/src/environment.rs) has a separate
  per-frame write search, root fallback, name-visible views, aliases,
  `Rc`/`RefCell` tables and a heap. Proving the selector alone says nothing
  about which candidates those paths collect or which cell they mutate.
  H2-A/B/C are concrete counterexamples to the desired environment contract;
  a faithful verifier would reproduce them until the runtime is fixed.
- H3 already compares with two independent expanders and found a shared
  frontend literal defect. Its explicit limits and known failures remain
  visible. A new reference implementation would need evidence beyond simply
  agreeing with both Patina backends.

#### Measured bounded baseline

A disposable Rust program called the production `resolve_index` twice for
every reference and ordered table of **0–4 candidates over four scope IDs**
(`S0`–`S3`). The independent oracle uses bitmask inclusion: select the first
eligible set containing every eligible set, return unbound if none is
eligible, otherwise reject as ambiguous. Payloads are all `()`, so answers
must be compared by binding index rather than by equal values.

| Result | Cases |
|---|---:|
| Unbound | 374,176 |
| Selected binding | 619,884 |
| Ambiguous | 124,420 |
| **Total** | **1,118,480** |

All results and repeated calls agreed. **67,425** selected cases had a repeated
winning scope set, exercising the supplied recency order. The count is
`16 × (1 + 16 + 16² + 16³ + 16⁴)`; this includes the inline/spilled storage
boundary. The measured run made **2,236,960 selector calls in 159.68 ms**,
excluding its **4.95-second release build**. These are one host's timings,
not CI or model-checker performance estimates.

This exhausts that input encoding, using valid scope sets constructed through
the public API, a fixed name, logging disabled and normal allocation. It does
not establish a symbolic proof, memory-safety proof, arbitrary scope-ID or
table-size theorem, or environment/expander correctness. It is evaluation
evidence, not an additional maintained gate. The complete probe is retained
below so the measurement does not depend on a temporary worktree.

#### Options and cost

| Option | Concrete proposed domain and evidence | Integration and maintenance cost | Decision |
|---|---|---|---|
| Kani on production Rust | First pilot: the same four-scope, 0–4-candidate domain, checking returned index, ambiguity, ties and repeated-call determinism. Successful symbolic verification would cover all admitted inputs under the harness assumptions and supported semantics; larger domains need new measurements. | Pin Kani and its Rust toolchain separately; establish dependency support, unwind bounds and the diagnostic boundary. Initial planning cap: two engineering days for a pilot, plus recurring toolchain/CI upkeep. This is an estimate, not measured effort. | Best future fit for a bounded selector check; no adoption now. |
| Creusot on production Rust | A contract over sorted scope sequences and ordered candidates could target arbitrary finite lengths. Creusot is deductive verification, not merely another bounded enumerator. Correctness is relative to preconditions, loop invariants and dependency contracts. | Specify sorted-set representation, selection and ambiguity invariants, `SmallVec`/iterator behavior and diagnostics; environment ownership needs a separate design. Initial feasibility cap: five engineering days, followed by ongoing contract and prover maintenance. No compatibility or proof-cost measurement was made. | Too much integration for the present selector-only benefit. |
| Independent reference expander | A first mini-core could admit ≤24 AST nodes, ≤3 lexical binders, ≤2 nonrecursive single-argument transformers and expansion depth ≤3, with `lambda`, application, `quote`, `if`, `set!` and `let-syntax`. Compare resolved binding identities and ordered effects on matching H3 cases. | At least five distinct deliverables: binding specification, parser/IR, expander, canonical binding comparison, and independent validation plus shrinking. Initial planning cap: five engineering days to evaluate this limited core; full H3 parity is a separate project. | Declined. This subset excludes imports, literals, ellipses and generated definitions, so it misses the axes of the new family-40/41 cases. Extending it creates another substantial implementation to maintain. |

Kani supports this host platform, and its releases use their own recent Rust
nightly; compatibility with Patina's pinned compiler and dependencies would
need a real pilot. Its documentation requires adequate loop-unwind bounds:
an unwinding failure or resource exhaustion is not a proof. See
[installation](https://model-checking.github.io/kani/install-guide.html),
[toolchain model](https://model-checking.github.io/kani/), and
[loop bounds](https://model-checking.github.io/kani/tutorial-loop-unwinding.html).

The pilot must call the real selector and real subset operations. If diagnostics
need isolation, record the excluded behavior and any trusted stub explicitly;
do not replace selection or environment mutation with a model and call the
result a proof of Rust. Kani's
[stubbing](https://model-checking.github.io/kani/reference/experimental/stubbing.html)
is an unstable feature and brings its own maintenance cost. The proposed pilot
would cap each proof attempt at five minutes and retain incomplete results;
no successful symbolic bound or runtime is claimed here.

Creusot requires contracts and usually loop invariants, translates through
Coma/Why3, and brings a separate prover installation. Its trusted specifications
must be distinguished from verified implementations. These are documented
capabilities, not a claim that Patina's `SmallVec`, heap or interior-mutability
paths are already supported or verified. See the
[quick start](https://guide.creusot.rs/),
[loop invariants](https://guide.creusot.rs/basic_concepts/loop_invariants),
[trusted contracts](https://guide.creusot.rs/trusted.html) and
[installation](https://guide.creusot.rs/installation.html).

**Why stop here:** the measured small selector domain is cheap to enumerate,
while the known defects sit in environment integration and expansion. Spend
runtime work on #289–#291 and families 40/41, retaining their failing guards.
Reconsider Kani if a selector change needs assurance beyond H2's sampling, or
after read/write consolidation exposes one shared decision function. Fix the
known behaviors before Q7.1 consolidation; a kernel proof cannot discharge
those obligations. A reference expander or mechanized specification should
be reconsidered when the syntax-case design owns one resolution algorithm.
These are revisit conditions, not unfinished H4 implementation tasks.

#### Reproducing the evaluation probe

Create a disposable Cargo binary package outside the workspace with a path
dependency on `patina-core` at commit `7892a7cf` (adjust the checkout path):

```toml
[package]
name = "patina-h4-probe"
version = "0.0.0"
edition = "2024"

[dependencies]
patina-core = { path = "/path/to/patina/crates/patina-core" }
```

Use the Rust source below as `src/main.rs`, then run:

```bash
PATINA_AMBIGUITY_LOG= cargo +1.97.1 run --release --offline --manifest-path /path/to/probe/Cargo.toml
```

The offline command needs the crate dependencies cached. Neither a verifier
installation nor a repository test change is required.

```rust
use patina_core::{ScopeId, ScopeSet, scope_resolve::resolve_index};
use std::time::Instant;

// Ordered tables over four scopes, including the SmallVec 3-to-4 boundary.
const MASKS: usize = 16;
const MAX_CANDIDATES: u32 = 4;

fn expected(reference: u8, bindings: &[u8]) -> Result<Option<usize>, ()> {
    let eligible = |mask: u8| mask & reference == mask;
    if !bindings.iter().any(|&mask| eligible(mask)) {
        return Ok(None);
    }
    // The first eligible set containing every eligible set wins. This oracle
    // uses no production subset, length, maximum-selection or sorting code.
    bindings.iter().enumerate().find_map(|(i, &mask)| {
        (eligible(mask)
            && bindings.iter().all(|&other| !eligible(other) || other & mask == other))
        .then_some(i)
    }).map(Some).ok_or(())
}

fn main() {
    let started = Instant::now();
    let sets: Vec<_> = (0..MASKS).map(|mask| {
        let mut set = ScopeSet::new();
        for bit in 0..4 {
            if mask & (1 << bit) != 0 { set.add_scope(ScopeId(bit)); }
        }
        set
    }).collect();
    let mut counts = [0usize; 3]; // unbound, selected index, ambiguous
    let mut repeated_winner = 0usize;
    for len in 0..=MAX_CANDIDATES {
        for mut code in 0..MASKS.pow(len) {
            let bindings: Vec<_> = (0..len).map(|_| {
                let mask = (code % MASKS) as u8;
                code /= MASKS;
                mask
            }).collect();
            let table: Vec<_> = bindings.iter()
                .map(|&mask| (sets[mask as usize].clone(), ())).collect();
            for reference in 0..MASKS {
                let want = expected(reference as u8, &bindings);
                let actual = resolve_index("x", &sets[reference], &table).map_err(|_| ());
                assert_eq!(actual, want, "reference={reference} table={bindings:?}");
                assert_eq!(resolve_index("x", &sets[reference], &table).map_err(|_| ()), actual);
                counts[match actual { Ok(None) => 0, Ok(Some(_)) => 1, Err(()) => 2 }] += 1;
                if let Ok(Some(i)) = actual {
                    if bindings.iter().filter(|&&mask| mask == bindings[i]).count() > 1 {
                        repeated_winner += 1;
                    }
                }
            }
        }
    }
    assert_eq!(counts.iter().sum::<usize>(), 1_118_480);
    assert!(counts.iter().all(|&n| n > 0) && repeated_winner > 0);
    println!("states={} unbound={} selected={} ambiguous={} repeated-winning-set={} selector-calls={} elapsed={:?}",
        counts.iter().sum::<usize>(), counts[0], counts[1], counts[2], repeated_winner,
        2 * counts.iter().sum::<usize>(), started.elapsed());
}
```

### H5 — transferred to the syntax-case design

Deferred mechanization is now owned by
[the syntax-case design](../../macro/SYNTAX_CASE_DESIGN.md#deferred-mechanization-at-the-syntax-case-boundary-h5).
The future decision and its entry conditions live there, not in this completed
assurance track.

## 6. Sequencing

The planned priority was H2 → H1 → H3; H1 and H2 could be developed
independently. H1/H2 are now bounded per-PR gates, H3 is a manual lane, and
H4's evaluation closes the current track. H5 moved to the syntax-case design;
it is not a prerequisite for archiving this PRD. The named runtime defects
remain in their live issues and triage entries.

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
