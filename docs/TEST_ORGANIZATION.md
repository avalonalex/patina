# Test Organization Strategy

## Overview (Updated 2025-12-12)

The Patina test suite has grown significantly (~1,500 tests) and follows a clear organizational pattern:

1. **Unit Tests** - Component-level tests inline with source code
2. **Integration Tests** - Full-stack tests in dedicated `patina-tests` crate

## Test Structure

```
workspace/
├── crates/
│   ├── patina-core/
│   │   └── src/
│   │       └── *.rs          # ~138 unit tests inline
│   │
│   ├── patina-macros/
│   │   └── src/
│   │       ├── compiler/tests.rs    # Extracted test module
│   │       ├── expander/tests.rs    # Extracted test module
│   │       └── *.rs          # ~183 unit tests inline
│   │
│   ├── patina-frontend/
│   │   └── src/
│   │       └── *.rs          # ~100 unit tests inline
│   │
│   ├── patina-ir/
│   │   └── src/
│   │       └── *.rs          # ~22 unit tests inline
│   │
│   ├── patina-runtime/
│   │   └── src/
│   │       └── *.rs          # ~9 unit tests inline
│   │
│   ├── patina-tree-walker/
│   │   └── src/
│   │       └── *.rs          # ~15 unit tests inline
│   │
│   ├── patina-pipeline/
│   │   └── src/
│   │       └── *.rs          # ~13 unit tests inline
│   │
│   ├── patina-interpreter/
│   │   └── src/
│   │       └── *.rs          # ~3 unit tests inline
│   │
│   └── patina-tests/         # Integration test crate
│       ├── Cargo.toml
│       └── tests/
│           ├── common/       # Test utilities
│           ├── compliance/   # R7RS compliance tests
│           ├── integration/  # Chibi comparison tests
│           ├── scheme/       # *.scm test files, run by scheme_suite.rs
│           │   ├── control/  #   evaluation order, tail calls, wind, parameters
│           │   ├── reader/   #   identifier syntax
│           │   ├── expansion/#   macros, include, core syntactic bindings
│           │   ├── data/     #   values and conversions
│           │   ├── stdlib/   #   the (scheme …) libraries
│           │   ├── srfi/     #   bundled SRFI behaviour
│           │   └── libraries/#   import/export machinery visible from Scheme
│           └── *.rs          # Feature-specific tests
```

### Scheme test files (`tests/scheme/`, driven by `scheme_suite.rs`)

**Prefer these for new tests of the language.** They are ordinary, portable
SRFI 64 programs — `(import (scheme base) (srfi 64))`, `test-begin`,
`test-equal`, `test-end` — and one Rust driver runs every one of them on every
backend. Adding a backend touches the driver; adding a test touches neither.

**`scheme_suite.rs`'s `SUITE` table is the enumeration of these files** — the
list below covers `tests/*.rs` only, and does not carry notes about files that
have migrated out of it. `the_suite_table_and_the_directory_agree` keeps `SUITE`
and the directory in step, which no prose list can do.

Two reasons to reach for a `.scm` file first:

- **Cost.** Measured on a warm build, adding one `.scm` file rebuilds in
  **0.098 s**; adding one `.rs` test file costs **8.96 s**, because every `.rs`
  file directly in a `tests/` directory is its own crate and its own link
  against the whole workspace. That is the problem CLAUDE.md's build-cost table
  describes, and #193's reason for existing — 88 binaries when it was measured,
  72 as of 2026-09-09, Phase 1 complete and `hygiene.rs` migrated after it.
- **Portability.** The same file runs under chibi and Gauche unchanged, which
  makes it an oracle and not only a suite. Differences are real findings — for
  `callability.scm`, Gauche's three disagreements are the deliberate
  divergences its own comments already document.

  Not every file gets *both* oracles, and which ones it gets belongs **in the
  file**, measured, not restated here where the two copies drift apart. The
  control-flow files are the case in point: chibi cannot survive some deep
  `dynamic-wind`/continuation shapes, so each says which rows it corroborates
  and which it dies on.

  **A `.scm` row can keep printed-form coverage, and should where the row is
  about it.** `assert_program_eval_to` compared the datum writer's *printed*
  output; `test-equal` compares with `equal?`, so a bare migration stops
  noticing a printing regression that leaves `equal?` intact — the class #187
  and #189 were about. Where that is the point of the row, put the writer back
  under test with this exact three-line helper, copied verbatim:

  ```scheme
  (define (written x)
    (let ((p (open-output-string))) (write x p) (get-output-string p)))
  ```

  Two siblings exist for the other procedures that share the writer's passes,
  recorded here for the same reason and subject to the same rule — a `shared`
  that quietly used `write` would change what a whole file asserts, and the
  three differ by exactly one procedure name:

  ```scheme
  (define (shared x)
    (let ((p (open-output-string))) (write-shared x p) (get-output-string p)))
  (define (displayed x)
    (let ((p (open-output-string))) (display x p) (get-output-string p)))
  ```

  It needs `(scheme write)` in the import set — which resolves without one on
  Patina (issue #211), so an omission is invisible here and fails on both
  oracles. `reader/vertical-bar-identifiers.scm` is the worked example: it could
  not migrate at all until this existed, and the round-trip row it enabled found
  a live writer bug. Copies must stay identical; one using `display` would
  change what a whole file asserts. Where printing is *incidental* to the row,
  the loss is real and that coverage belongs in `external_representation.rs`,
  which asserts on rendering deliberately. Where printing *is* the row, keep it
  rather than relocating it: `data/circular-data.scm` is the worked example at
  scale — 20 of its 27 rows assert an exact printed form, and it says in its own
  header why `equal?` cannot see any of them.

  A file that disclaims a property should say where the property is checked
  instead. `tail-recursion.scm` is the case in point: its rows pin that each
  special form evaluates correctly when its last expression recurses, not that
  the call is a *tail* call in constant space — that one is
  `vm_callprimitive.rs::tail_deopt_runs_deep_mutual_recursion` at 100 000 deep,
  with the space measurement itself recorded out-of-band in PRD TRACK_P §P8.2
  (5.49 MB against 109 MB before the fix).

  Two things follow for a new file. **Order a row an oracle cannot survive
  last**, because SRFI 64 stops the file where it dies — in
  `internal-escape-boundaries.scm` that one move took chibi from 6 rows to 10.
  And **do not generalise one bad row to the file**: the first draft of that
  file claimed chibi could not arbitrate it at all, which threw away ten rows of
  corroboration that were there for the asking.

#### The oracle lane (`scripts/run_suite_oracles.sh`, #193 Phase 3)

Running these files under chibi and Gauche is what makes them an oracle rather
than only a suite, and until Phase 3 it happened *by hand*: every file header
recorded tallies that nothing re-measured, and stale ones were caught only by
someone re-running four interpreters.

The lane checks **the classified set of differing rows**, not the tallies.
That distinction is the design, not a shortcut:

- Gating on an oracle's numbers makes its *bugfix* break our build, and the
  natural repair is to edit a number — which trains mechanical updating and
  says nothing about which side moved. It also promotes "chibi says X" from
  commentary to constraint.
- Gating on the divergence set asks the useful question: did a difference
  appear that nobody has explained, or did an explained one go away?

`crates/patina-tests/tests/scheme/DIVERGENCES.tsv` is the register: one row per
file, oracle and differing test, each with a class — `latitude` (R7RS permits
both), `spec-silent`, `oracle-defect`, `patina-defect`, `needs-investigation`,
or `incomplete` for a file an oracle cannot finish. **The class is the point.**
A count says "3 rows differ" and gives no signal about who should change; an
`oracle-defect` row is a record that someone investigated and concluded the
*oracle* is wrong, so nobody later "fixes" Patina to match it. Reach for that
class only with evidence — the register carries one, and its note states what
R7RS does *and does not* require, so anyone reporting it upstream argues the
accurate case.

**The rule the lane cannot enforce, and the one that matters most:** a row's
expected value comes from the specification or from intended behaviour, never
from asking an oracle what it prints. Oracles are consulted afterwards, to
explain a difference. That polarity is what keeps their bugs out of our tests,
and no check here can substitute for it.

`every_registered_divergence_names_a_real_row` in `scheme_suite.rs` keeps the
register honest without needing either interpreter installed: it pins the class
vocabulary, requires a note, and checks that every registered row still names a
test that exists — a rename would otherwise leave the lane reporting one edit
as two mismatches.

**Where a new `.scm` file goes: directory by kind, filename by concern.** The
directory is one of the seven above and says what *sort* of thing the file is
about; the filename keeps the concern name the test has always had
(`tail-recursion.scm`, `wind-thunk-exceptions.scm`), because several of these
files are named after defect classes rather than spec sections and the name is
the documentation. Deliberately *not* by R7RS section: a concern often spans several — `data/`
holds conversions from §6.2 today and would hold §6.6, 6.7 and 6.8's
(`char->integer`, `string->list`, `vector->list`) beside them — and a section
number would delete why the file exists.

#### A file organised by provenance gets redistributed, not migrated

`larceny_families.rs` was the one place the suite departed from the rule above,
and **it is gone** — the redistribution finished on 2026-09-09, and this section
is what it left behind. It held Patina's own MIT-licensed reproduction of every
defect family Larceny's R7RS suites surfaced (the suites are LGPL and are not
vendored), organised by **where a defect was found** rather than what it is
about. So `equal?` on circular structures sat in a Larceny file while
`data/circular-data.scm` argued about `equal?` on circular structures two
directories away.

**Those rows are Patina's own work, and that has to keep being findable.**
Larceny's suites are LGPL and are not vendored — nothing from them is quoted
anywhere in this repo. Every program is written from scratch to exhibit the
same *family* of defect, which is what makes it MIT like the rest of the
codebase, and the durable statement of that is
`PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6. It used to be the deleted file's
header, which is why it is restated here: seventeen suite files still say
"Moved from `larceny_families.rs`", and a licence claim must not depend on a
file that no longer exists or on a triage doc marked for deletion.

Sixty-four rows went, ten slices at a time, each carrying a `Larceny family N`
line so `scheme_tests/reports/larceny_triage.md` still maps — better than
before, since the triage doc names a file and a row rather than a 1497-line
haystack, and `every_triage_pointer_names_something_that_exists` now checks that
those pointers resolve. Six rows stayed in Rust because a `.scm` file cannot
express them: family 40's three backend divergences (`backend_divergence.rs`),
and the two families that need real files on disk (`include_syntax.rs`,
`standard_ports.rs`).

**What the move was actually worth** is not the binary it saved. Rows that had
only ever run on Patina were suddenly arbitrated by two other implementations,
and that found things: a Gauche defect (a template-generated `let-syntax`
capturing its own sibling), a chibi one (`only` resolving against a library's
internal names), a row that had been *relying* on our own family-40 defect to
pass, and a quarantine whose note had outlived its own fix by two weeks. None of
those was reachable from a file only Patina ran.

Lessons that came out of doing it, worth knowing before the next one:

- **Putting a row beside its subject is what makes it worth reading.**
  `circular-data.scm`'s header already argued that `equal?` must terminate on
  cycles, measured on the *easy* shape. Family 2 is the hard one — two distinct
  cycles with the same unrolling — and it now sits under the claim it
  substantiates. Neither file said anything new; the adjacency did.
- **The move is where import bugs surface.** Family 2's program used `cdddr`,
  which is `(scheme cxr)` and not `(scheme base)`. Patina resolves it anyway
  (issue #211), so in Rust it was invisible; as a `.scm` file it failed on both
  oracles at once.
- **A row can be too expensive for the lane.** `(string->number "#e1e1000000")`
  is `#f` here in no time, and chibi *computes* it — 189 s and a million-digit
  integer, which timed the oracle lane out and cost chibi the arbitration of
  every other row in that file. Scoping it to Patina returned them, because
  `test-skip` prevents evaluation rather than discarding a result.
- **One top level is not the same as many empty ones, and that is a gain.**
  Each Rust test ran in a fresh interpreter, so its program had the top level
  to itself; a suite file gives fifteen rows one shared top level, and names
  can collide. Renaming to avoid that is usually right — but not always, and
  `expansion/let-syntax.scm` is the case. Two of its rows are there because
  Larceny's `base` suite defines its own `f`: they passed in isolation and
  failed inside a real program, and the Rust file could only note it in a
  comment. In one file with a top-level `f`, the condition that found the bug
  is *restored*, not lost. Read the comments before renaming a colliding
  identifier — sometimes the collision is the test.

Two migration rules learned the hard way in #193 Phase 1, both from rows that
passed while asserting something else:

- **Keep top-level `define`s at top level.** A top-level self-reference
  resolves through the global environment at call time; an internal `define` is
  `letrec*` and compiles to a local slot. Wrapping a recursive row in
  `(let () (define …) …)` silently moves it off the path it was written for.
- **Sequence side effects with `let*`, never argument positions.** R7RS leaves
  argument evaluation order unspecified and chibi evaluates right to left, so
  `(list (c) (c 5) (c 3) (c))` answers differently there — reporting a
  difference between implementations that is not one.
- **Write `test-error` with three arguments: `(test-error "name" #t expr)`.**
  SRFI 64's specifiers are `(test-error [[name] error-type] expr)`, so the
  two-argument `(test-error "name" expr)` puts the string in the *error-type*
  position and leaves the row's name `#f`. Measured 2026-09-09: chibi and
  Gauche then print a bare `FAIL` with no name, which no `DIVERGENCES.tsv` line
  can match and no reader can trace. Every row in the suite passes `#t`.

  That `#t` is not a placeholder to be improved on later. SRFI 64's reference
  implementation ignores the error type entirely on an R7RS host — its
  `(or srfi-34 r7rs)` branch is `(guard (ex (else #t)) expr #f)` — so a
  `test-error` row asserts only that *something* was raised, on every
  implementation the lane runs. A row that needs to say *which* error belongs
  in Rust, where `ErrorClass` can say it. (Patina's bundled copy calls
  `error-matches?` and discards the result, which is the same behaviour and is
  deliberate: matching upstream is what keeps a file's answer the same here and
  on the oracles. The call earns its keep by warning on a type that is neither
  `#t` nor a predicate — which is how the two-argument slip above announces
  itself in the log.)

**What still belongs in a `.rs` file**, because a `.scm` file cannot express
it: a row whose backends give *different values*; a row deliberately asserted
on one backend; that an error escapes an *unguarded* program (observable only
from outside it); and anything asserting *which stage* rejected a program, or
touching `Heap`, `VmState`, `Instruction`, `SourceMap`, GC counters or the
library registry. The rule of thumb #193 uses: what is about the **language**
goes to Scheme, what is about the **implementation** stays in Rust.

The unguarded rows have **one** home: `callability.rs`, whatever feature they
came from. A file of their own would split the class across two places and,
since `callability.rs` exists regardless, would give back the binary the
migration just saved.

**A row whose *premise* is Patina-specific** — as opposed to one whose answer
differs between our two backends — can stay in Scheme, scoped with the
`cond-expand` identifier #208 added:

```scheme
(cond-expand (patina) (else (test-skip 1)))
(test-equal "the row's name" ...)
```

The `test-skip` is not decoration. `cond-expand` alone deletes the row on other
implementations with nothing anywhere saying so, and a row that can vanish
quietly is what the skip and floor checks exist to prevent; with it, chibi and
Gauche *report* a skip. Use this only where the premise genuinely is ours (a
Patina-specific validation, say), never to paper over a difference in an
answer — that is a divergence, and it belongs in Rust where it can be named.

**Where an oracle refuses to *compile* a row, neither form works.**
`test-skip` suppresses evaluation, so a row it guards is still read and
compiled, which is enough to lose the whole file when an implementation rejects
the program while compiling the form around it. Gauche does exactly that to
three rows of `expansion/ellipsis.scm`: R7RS §4.3.2 makes a `syntax-rules`
written where `...` is bound an ordinary three-variable pattern, and Gauche
reports `Pattern variable b is used in wrong level` and stops.

A `cond-expand` clause that is not selected is never compiled, so the row goes
inside one:

```scheme
(cond-expand
  (gauche)   ; cannot compile the rows below — see the header
  (else (test-equal "the row's name" ...)))
```

**Which leaves a real choice, and it turns on ownership.** The alternative is to
let the file die on that oracle and register it `*` / `incomplete`, so the lane
holds the claim and reports it if the oracle ever starts completing. Use that
when the oracle's behaviour is a finding you intend to act on. Use the omission
when it is not — and for a *compile-time refusal* it usually is not, because
there is no Patina behaviour in question and nothing of ours that could drift
to match it. Whether Gauche compiles a program is Gauche's conformance; file it
upstream if it is worth filing, and do not pay for the record with that
oracle's arbitration of every other row in the file.

**The omission is not always available**, and when it is not, the `*` row is
right. `expansion/template-references.scm` is the case: chibi cannot define a
library in a script and every row in that file needs one, so there is no subset
to scope past. Ask first whether the oracle fails on *some* rows or on the
file's whole premise.

And when the oracle *hangs* rather than refusing to compile, the cheapest shape
of all works — `test-skip` prevents evaluation, so the program never runs and
the file completes. Measured 2026-09-09: one such skip takes
`control/callability.scm` from nothing at all on chibi to 22 arbitrated rows.
Three files carry a `*` for chibi today and at least one of them should not;
`DIVERGENCES.tsv` records the measurement.

That reasoning does **not** extend to a difference in an *answer*. Those stay
unscoped and classified in `DIVERGENCES.tsv`, because there the oracle is
telling you something about a claim your own rows make — and scoping them away
is how you would never learn it. Two Gauche bugs (shirok/Gauche#1326, #1327)
were filed because rows that disagreed were left to disagree in the open.

The omission costs nothing on our side: the driver's floor for the file fails if
a row stops running on Patina, which is the direction that matters, and the
`test-skip` rules below exist for the same reason. It costs the day the oracle
changes — nothing notices — so the file's header must say which rows are
omitted and why. That header is then the only record.

**Skip the count, not the name, and put it immediately above its row.** SRFI
64's `test-skip` takes a count, a name or a predicate; in this suite the count
is **required**, not merely preferred, because it is the one form a check can
verify. `(test-skip "the row's name")` keeps a second copy of the title that
has to stay character-identical to the `test-equal` beneath it; `(test-skip 1)`
has no second copy to keep in step. The same goes for `test-expect-fail`, which
takes the same specifiers through the same code in `lib/srfi/64.scm`.

Zero is barred too, and for a sharper reason than the others: `(test-skip 0)`
is `(test-match-nth 1 0)`, a predicate that is never true, so it reads as a
count while guarding nothing at all.

Neither form is immune, and they rot in opposite directions. A desynced *name*
matches nothing, so the row runs on chibi and Gauche after all — the guard is
gone, but any real disagreement still surfaces there. A desynced *count* skips
a **different** row, which can turn a genuine oracle failure green; that is the
worse outcome, and it is why the count has to sit directly above what it
guards. "The next row" is also loose: the count binds to the next test the
runner *reaches*, and a `test-group` counts as one, so anything test-shaped in
between takes the skip instead.

Both ways of rotting are invisible from a normal run for the same reason —
Patina takes the `(patina)` branch and never evaluates the `else` at all. Two
tests in `scheme_suite.rs` take back as much of that as they can, and it is
worth knowing which half is airtight:

- `every_scoped_row_skips_by_count_and_sits_above_its_row` reads each file's
  text, for both `test-skip` and `test-expect-fail`. It **pins** the positive
  count form, because the text says which form was written. Adjacency it can
  only approximate: it requires a test form directly beneath the specifier, but
  cannot tell *which* one, so slipping another assertion in still passes there.
  Treat adjacency as a rule you keep, not one you are caught breaking.
- `the_count_form_skips_exactly_the_next_row` pins that `(test-skip 1)` still
  means what this paragraph says on our own SRFI 64. Nothing else covers it: no
  file in `tests/scheme/` ever evaluates a `test-skip` on Patina, and upstream's
  suite uses the integer shorthand nowhere.

What would close the gap properly is running each file a second time with the
`patina` feature absent, so the `else` branches actually execute and the skip
count can be checked against the number of scoped rows. That needs a mutation
path through `Heap`'s feature registry, which is closed on first read by design.

Every file is listed in `scheme_suite.rs`'s `SUITE` table with a minimum
assertion count. That floor is not bookkeeping — a file that stops running
reports no failures, so without it a truncated or skipped file passes. Skips
are rejected outright for the same reason.

## Test Categories

### 1. Unit Tests (In Component Crates)

**Location:** Inline with `#[cfg(test)]` in source files

**Purpose:** Test component internals and edge cases in isolation

**Current Distribution:**

| Crate | Unit Tests | Notes |
|-------|-----------|-------|
| patina-core | 138 | Value operations, CoreExpr |
| patina-macros | 183 | Pattern matching, hygiene |
| patina-frontend | 100 | Lexer, parser, desugarer |
| patina-ir | 22 | CPS transformation |
| patina-tree-walker | 15 | Evaluator internals |
| patina-pipeline | 13 | Pipeline orchestration |
| patina-runtime | 9 | Environment, library system |
| patina-interpreter | 3 | High-level API |
| **Total** | **~483** | |

**Run with:**
```bash
# Run unit tests for a specific crate
cargo test --package patina-frontend
cargo test --package patina-macros
cargo test --package patina-core
```

### 2. Integration Tests (patina-tests Crate)

**Location:** `crates/patina-tests/tests/`

**Purpose:** Test the complete interpreter working end-to-end

**Categories:**

#### **Compliance Tests** (`tests/compliance/`)
R7RS specification compliance organized by category:
- `numbers.rs` - Numeric operations (~30 tests)
- `strings.rs` - String operations (~25 tests)
- `lists.rs` - List operations (~20 tests)
- `vectors.rs` - Vector operations
- `predicates.rs` - Type predicates
- `derived.rs` - Derived forms (let, cond, case)
- `control.rs` - Control flow
- `quasiquote.rs` - Quasiquote expansion
- `macros_advanced.rs` - Advanced macro patterns (~60 tests)
- `rationals.rs` - Rational numbers
- `numeric_edge_cases.rs` - Edge cases

#### **Feature Tests** (top-level `tests/`)

Named by role, not by count: every per-file number this list used to carry had
rotted by the time anyone checked — `numeric_operations.rs` had migrated to
`data/numeric-operations.scm` and was still listed, `hygiene.rs` was down from
"~108" to 49 and is now 18, `cps_features.rs` from 31 to 11. For a current count,
`grep -c '^#\[test\]'` the file; for the suite files, `SUITE` in
`scheme_suite.rs` carries a floor per file and a test keeps it honest.

- `hygiene.rs`, `hygiene_matrix.rs` — macro hygiene; the matrix is a
  scoreboard of 28 shapes against chibi and Racket and stays Rust, while
  `hygiene.rs` is being migrated row by row into `tests/scheme/expansion/`
  (`hygiene.scm`, `syntax-rules-literals.scm`). Add a portable hygiene row
  there, not here
- `backend_divergence.rs` — the registry of behaviours where the two backends
  differ, `assert_divergence` being the only way onto it
- `cps_features.rs`, `control_flow_matrix.rs` — continuations, prompts, and the
  24-shape transfer matrix behind `docs/VM_RUNTIME.md` §5.6
- `complex_numbers.rs`, `record_types.rs`, `scheme_eval.rs` — feature areas
  whose rows are about the implementation rather than the language

#### **Library Tests**
- `sld_file_loading.rs` — library loading from `.sld` files
- `r7rs_libraries.rs` — R7RS library compliance
- `scheme_base.rs` — `(scheme base)`
- `bundled_provenance.rs` — pins every third-party file claimed byte-identical
  to an upstream release, so an unrecorded edit fails

#### **Integration Tests** (`tests/integration/`)
- Compare Patina output with chibi-scheme
- Test full program execution
- Verify compatibility

**Run with:**
```bash
# Run all integration tests
cargo test --package patina-tests

# Run specific test file
cargo test --package patina-tests --test hygiene
cargo test --package patina-tests --test cps_features

# Run compliance tests
cargo test --package patina-tests --test compliance

# Run specific category
cargo test --package patina-tests numbers::
cargo test --package patina-tests primitives::
```

## Test Utilities

**Common Helpers** (`crates/patina-tests/tests/common/mod.rs`):
```rust
// Primary assertion helpers
assert_eval_to(expr, expected)           // Evaluate and compare result
assert_eval_error(expr)                  // Verify error is raised
assert_program_eval_to(code, expected)   // Multi-expression programs
assert_eval_type(expr, check, name)      // Verify result type
```

## Test Counts (as of 2025-12-12)

| Category | Tests | Notes |
|----------|-------|-------|
| Unit tests (all crates) | ~483 | Inline with production code |
| Integration tests | ~1,000+ | In patina-tests crate |
| **Total** | **~1,500** | 3 ignored, measured 2026-09-09 |

**By test file (largest).** These are the numbers as last written down, and
several are known stale — the counts above them say why a prose list of this
kind does not survive contact with a migration. Re-measure before quoting:
`grep -c '^#\[test\]' crates/patina-tests/tests/<file>.rs`.

| File | Tests | Lines |
|------|-------|-------|
| compliance.rs | ~380 | via sub-modules |
| hygiene.rs | 18 | measured 2026-09-09, and shrinking |
| scheme_base.rs | ~50 | |
| sld_file_loading.rs | 40 | measured 2026-09-09 |
| record_types.rs | 41 | measured 2026-09-09 |
| tail-recursion.scm | 36 | 301 |
| cps_features.rs | 31 | 580 |

## Running Tests

### Run Everything
```bash
# All tests (unit + integration) - ~1,500 tests
cargo test --workspace

# Only integration tests (most comprehensive)
cargo test --package patina-tests
```

### Run Specific Categories
```bash
# Only unit tests for a crate
cargo test --package patina-frontend
cargo test --package patina-macros

# Only R7RS compliance
cargo test --package patina-tests --test compliance

# Only CPS features
cargo test --package patina-tests --test cps_features

# Only macro hygiene
cargo test --package patina-tests --test hygiene
```

### Development Workflow
```bash
# During development: test the component you're working on
cargo test --package patina-frontend  # If working on parser
cargo test --package patina-macros    # If working on macros

# Before commit: run all tests
cargo test --workspace

# Quick sanity check
cargo test --package patina-tests --test interpreter_api
```

## Inline Test Guidelines

### When to Use Inline Tests

**Good for inline tests:**
- Testing private functions not accessible from outside
- Testing internal invariants
- Unit testing helper functions
- Tests tightly coupled to implementation details

**Move to patina-tests when:**
- Testing public API behavior
- Testing feature integration across modules
- Tests become >100 lines in a single module
- Tests could apply to multiple backends

### Current Inline Test Distribution

The macro system has the highest inline test density due to the complexity of pattern matching and hygiene:

| Module | Prod Lines | Test Lines | Ratio |
|--------|-----------|-----------|-------|
| patina-macros/interface.rs | 207 | 500 | 2.4x |
| patina-macros/matcher/mod.rs | 244 | 316 | 1.3x |
| patina-frontend/library_parser.rs | 577 | 683 | 1.2x |
| patina-frontend/parser/mod.rs | 941 | 885 | 0.9x |
| patina-frontend/desugarer/mod.rs | 1,340 | 727 | 0.5x |

**Note:** High test ratios in macro code are acceptable given the domain complexity.

## Benefits of This Organization

### Clear Separation
- Unit tests live with their components
- Integration tests in dedicated crate
- Feature tests grouped by functionality

### Scalable for Multi-Backend
Backend crates:
```
crates/
├── patina-vm/             # Register-based bytecode VM (default)
├── patina-tree-walker/    # CPS tree-walking backend (--tree-walker)
├── patina-jit/            # JIT compiler backend (future)
└── patina-tests/          # Tests ALL backends
    └── tests/
        ├── compliance/    # Run against each backend
        └── integration/   # Compare all backends vs chibi
```

### Fast Iteration
- Test only what you're working on
- Unit tests run in milliseconds
- Integration tests run when needed

### Clear Dependencies
```
patina-tests depends on:
  └─ patina-interpreter
      ├─ patina-frontend
      ├─ patina-tree-walker
      ├─ patina-pipeline
      └─ patina-runtime
```

## Known Issues and Future Work

### Documented Bugs (via ignored tests)
The test suite documents known bugs by marking tests as `#[ignore]`:
- Some tests are marked ignored to document implementation differences with chibi-scheme

### Future Enhancements

**When Adding VM Backend:**
1. Create `patina-vm` crate
2. Integration tests will automatically test it via `Backend` trait
3. Add VM-specific unit tests in `patina-vm/src/`

**When Adding Benchmarks:**
```bash
# Future: crates/patina-benchmarks/
cargo bench --package patina-benchmarks
```

## Summary

**Current Organization:**
- ~483 unit tests inline with component crates
- ~1,000+ integration tests in `patina-tests` crate
- Clear separation by purpose and scope
- Scales well for multiple backends
- Fast, targeted testing during development

**Key Principle:**
> Unit tests verify component correctness in isolation.
> Integration tests verify the full interpreter works end-to-end.
> Compliance tests verify R7RS spec adherence across all backends.
