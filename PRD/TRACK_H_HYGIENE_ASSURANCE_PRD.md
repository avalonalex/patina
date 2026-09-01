# Track H — Hygiene Assurance PRD

**Created:** 2026-08-31
**Status:** Proposed — no work item started. Written the day the hygiene queue
(triage families 36 and 38) closed with the matrix at 28 of 28, because closing
it exposed the method's ceiling: every one of families 33–39 was found by
accident, and the fix for each was measured against a hand-enumerated grid.
This track exists so the *next* family is found by a machine.
**Scope decision:** **test the property, not the patches.** Hygiene has a
formal statement — expansion is invariant under α-renaming — and that statement
is checkable by metamorphic testing, property-based testing, and bounded
verification, none of which need a theorem prover or an external oracle for
their first and highest-value increments. Full mechanized verification of the
expander is explicitly deferred (§4 H5): no community artifact exists to adopt,
and a hand-built model would drift from the Rust.
**Dependencies:** none on open PRs. H1 and H2 build only on `main`; H3 needs
the chibi/Racket harness that scored the matrix on 2026-08-28 made
repeatable.

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
| `crates/patina-tests/tests/hygiene_matrix.rs` | 28 shapes (7 binders × site × read/write), scored against chibi 0.12 **and** Racket 9.3, pinned in both directions | Hand-enumerated; its own header lists six axes it does not cover |
| `PATINA_SCOPE_TRACE` | What scope set a binding actually carries, and how a reference resolved, per phase | Explains a failure; finds nothing on its own |
| Ambiguity as an error (Flatt's rule, PR #134–#137) | A reference two incomparable bindings could answer is refused, everywhere | Catches under-determination, not capture |
| Corpus + Larceny lanes | Real-world programs | Discovery by accident — the lesson of families 33–39 |

The gap: nothing *generates* programs, and nothing states hygiene as a
property quantified over all programs.

## 2. The property

Hygiene has a standard formal statement: **expansion commutes with
α-renaming** (Herman & Wand, *A Theory of Hygienic Macros*, ESOP 2008;
restated algorithm-independently in Adams, *Towards the Essence of Hygiene*,
POPL 2015). Rename a program's identifiers consistently and the observable
behavior must not change.

Two consequences make this directly testable:

1. **Every capture defect is spelling-sensitivity.** Family 36's silent shape
   is precisely "rename the use-site `x` and the answer changes from 5 to 1."
   A test for spelling-sensitivity is a test for the whole defect class,
   including members nobody has written down yet.
2. **Uniform whole-program renaming needs no oracle.** Renaming one spelling
   to a fresh one — every occurrence, its `define` included — preserves
   semantics under *any* correct implementation. Patina can be checked
   against itself: run the original and the renamed program, demand the same
   answer. No chibi, no Racket, no reference expander in the loop.

The SRFI 101 episode that surfaced families 33–35 was this experiment run by
accident: the library's shadowing names *were* an adversarial renaming. This
track runs it on purpose.

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

### H1 — α-renaming metamorphic harness *(first: oracle-free, days of work)*

A test pass that, given a Scheme test program:

1. picks an identifier spelling written in the program (binder or global),
2. renames every occurrence uniformly to a fresh spelling (its `define`
   included),
3. runs original and renamed on both backends, and
4. asserts the outputs agree — with the renaming applied to the expected
   output, so a printed symbol renames with the program.

Seed sources, in order: the matrix's 28 programs × every user identifier in
each; the `larceny_families.rs` hygiene repros; then `scheme_tests`
programs; eventually corpus files.

Known edge discipline (each is a skip-rule, not a blocker): programs that
construct symbols with `string->symbol` or compare against quoted symbols
whose spelling is load-bearing are skipped or have their expectations
renamed too; error outcomes compare by error *class*, never message text
(messages contain names); `PATINA_SCOPE_TRACE` stays available for
explaining any failure the harness finds.

**Acceptance:** harness lives in `patina-tests` and runs in the normal
`cargo test` gate. **Non-vacuity** (repo practice — the baseline is the
pre-fix commit, not a stash): pointed at `c18f1edf` (main before the family
36/38 fixes), the harness must fail on the matrix's silent-capture shape;
pointed at current main, it must pass everything it is seeded with.

### H2 — property tests on the resolution kernel *(a day; pure Rust)*

`proptest` suites in `patina-core` over randomly generated scope tables and
references, encoding the invariants that regressed as universally quantified
properties:

| Property | The family it encodes |
|---|---|
| On ⊆-chains, resolution is total and never ambiguous | 39 — nested binders accumulate, chains always decide |
| A reference writes the binding it reads: `set_with_scopes` and `get_with_scopes` land on the same binding for any table | 38 — closed as six separate repros over several PRs; here it is one line |
| The by-name fallback never returns a binding `is_candidate` rejected for the reference | 36 (step 1) |
| Resolution is deterministic; an identical-scope-set tie is decided by recency and reported, never raised | 39's TIE half |

**Acceptance:** each property names its family in a comment. Non-vacuity:
reverting `set_with_scopes` to exact-match (the pre-#137 rule) makes the
symmetry property fail within the default case budget.

### H3 — poison-shadow and differential generators *(a week; needs the external harness scripted)*

Two generators, one discipline:

- **Poison-shadow** (oracle-free): for each macro call site in a seed
  program and each identifier free in that macro's template, wrap the call
  site in a binder of that spelling (`(let ((list 'poison)) …)`) and assert
  invariance. This is family 36's shape as a generator — the matrix's
  `outside` rows quantified over real macros instead of one synthetic `m`.
- **Differential** (dual oracle): a small grammar over the matrix's axes
  plus the uncovered ones (macro-generated macros, nesting depth, ellipsis
  depth, derived binding forms, literals); run patina-VM, patina-tree-walker,
  chibi 0.12, Racket 9.3; any disagreement is shrunk and promoted to a
  matrix row. Requires scripting the chibi/Racket harness used to score the
  matrix on 2026-08-28 so the run is one command.

Generator discipline, from this repo's own recorded lessons: force
evaluation order through `(define r …)` (never `(list (f) x)` —
`unspecified-order-fakes-a-divergence`); generate only programs both oracles
*accept* (agreement on rejection is not agreement on an answer — the
matrix's own `do-var` lesson); stay out of R7RS "is an error" territory.

**Acceptance:** a scheduled or manually-run lane (like the Larceny lanes,
not the per-PR gate); its first sweep either finds a defect (which becomes a
matrix row + triage family) or establishes a clean baseline at a recorded
program count.

### H4 — bounded verification of the kernel *(optional; evaluate before committing)*

The resolution kernel (`patina_core::scope_resolve` plus the environment's
get/set pair) is small and nearly pure. Two routes, either sufficient:

- **Kani or Creusot on the real code**: bounded proof that for all tables up
  to size N, resolution is deterministic, total on chains, and the fallback
  respects rejection. No model-implementation gap. Evaluate first whether
  the `Rc<RefCell<…>>` environment plumbing needs the kernel extracted
  further (it is already mostly a pure module).
- **A reference expander**: ~200 lines of Clinger–Rees renaming over a
  mini-core, obviously correct by inspection, diffed against the desugarer
  on H3's generated programs. This breaks the shared-frontend blind spot the
  VM/tree-walker differential cannot: family 36 read identically wrong on
  both backends because both trusted the same frontend.

**Acceptance:** a written evaluation (which route, what N or what mini-core
subset, what it proved) even if the outcome is "not worth it" — the
evaluation is the deliverable; commitment to more is a separate decision.

### H5 — deferred: mechanization at the syntax-case boundary

If Phase 3 re-founds expansion on one specified algorithm (see
`PRD/phase2/SYNTAX_CASE_DESIGN.md`), that specification is the moment to
consider a mechanized model — Lean 4 is the natural host (its own expander
is the hygiene design built for a theorem-proving language: Ullrich & de
Moura, IJCAR 2020). Until then a model would shadow a moving implementation.
Parked deliberately; nothing in H1–H4 depends on it.

## 6. Sequencing

H1 → H2 are independent of everything and of each other; both are per-PR
gates once landed. H3 follows (its poison-shadow half could ship with H1's
plumbing; its differential half waits on the scripted harness). H4 is opt-in
after H2 shows where the kernel's edges are. No dependency on Track L; what
H3 finds feeds the triage queue like any defect.

## 7. Verification (track-wide)

Every harness proves non-vacuity against a recorded historical defect commit
before it counts as landed: check the harness out beside the defect (`git
checkout <commit> -- <files>`), watch it fail, then watch it pass on main —
a green gate that was never seen red proves nothing. The named anchor for
hygiene: `c18f1edf`, the last main on which the matrix read 10 cells
wrong.

## 8. References

- E. Kohlbecker, D. P. Friedman, M. Felleisen, B. Duba. *Hygienic Macro
  Expansion.* LFP 1986.
- W. Clinger, J. Rees. *Macros That Work.* POPL 1991.
- R. K. Dybvig, R. Hieb, C. Bruggeman. *Syntactic Abstraction in Scheme.*
  LSC 1992 (syntax-case).
- D. Herman, M. Wand. *A Theory of Hygienic Macros.* ESOP 2008 — hygiene as
  α-equivalence preservation; the property H1 tests.
- M. D. Adams. *Towards the Essence of Hygiene.* POPL 2015 —
  algorithm-independent restatement.
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
