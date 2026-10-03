# Test Organization Strategy

## Overview (Updated 2025-12-12)

The Patina test suite has grown significantly (~1,500 tests) and follows a clear organizational pattern:

1. **Unit Tests** - Component-level tests inline with source code
2. **Integration Tests** - Full-stack tests in dedicated `patina-tests` crate

## CI trigger scope

[`ci.yml`](../.github/workflows/ci.yml) skips the full workflow when all changed
paths are under `PRD/`, using `paths-ignore: ['PRD/**']` on both main-branch
pushes and the existing pull-request targets. A change outside that tree
triggers every job as before, including changes to other documentation,
fixtures, tests, scripts, or the workflow itself. PRD-only reviews still need
local link/path checks and `git diff --check`.

This uses [GitHub's native path filtering](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#git-diff-comparisons):
pull requests compare against the merge base; pushes compare before and after.
Diff timeouts or pushes of more than 1,000 commits run the workflow anyway;
filtering considers only the first 3,000 changed files, so split unusually
large mixed changes into smaller reviews.

Main has no required status checks as of 2026-09-30. Before requiring these
checks, revisit this opt-out: [GitHub leaves required checks pending when a
whole workflow is filtered out](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks#handling-skipped-but-required-checks).
Such a policy needs a check that runs for every PR and accounts for skipped jobs.

[`gc-zeal.yml`](../.github/workflows/gc-zeal.yml) is filtered the other way:
it runs when a change touches what its lane checks most directly (see "GC
lanes" below), weekly on `main` for whatever the filter misses, and by hand
(`workflow_dispatch`). [`nightly.yml`](../.github/workflows/nightly.yml)
runs daily on `main`, by hand, and on a pull request that changes one of its
lanes' own inputs (the scripts, the pinned baseline, the workflow). The same
caveat applies to both.

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

### Reader properties and fuzzing (#366)

`patina-frontend/tests/reader_properties.rs` generates arbitrary Unicode and
combinations of Scheme syntax, and checks every split in representative inputs.
Its shared `tests/support/reader_checks.rs` compares `Parser::parse_all`,
successive whole-text parses, chunk-fed `Reader`, and repeated port reads. It
compares the successful prefix **and** accept/reject outcome, exercises trial
EOF and consumed-buffer compaction, and compares cyclic graphs iteratively.
This is agreement between entry points, not an external conformance oracle.

`patina-primitives/src/primitives/io/datum_writer_properties.rs` constructs
data independently of the reader, writes it, reads it and checks the runtime's
`equal?`. It extends #368's symbol property to strings, characters, booleans,
fixnums/bignums/rationals/reals/complexes, bytevectors, proper/improper lists and
vectors. Trees use depth 5 and a target of 64 nodes; separate 1–8-node pair/vector
graphs exercise cycles and sharing. `write`, `write-shared`, and (for trees)
`write-simple` are covered. Generated NaNs have the canonical payload because
Scheme text does not encode NaN payloads; signed zero and exactness are checked.
Writer output also goes through the shared reader comparison.

These properties run in the normal Rust CI lane with seed 366, 256 cases per
property and a 4096-step shrink limit (1024 symbol cases, and 32 cyclic cases,
whose runtime `equal?` starts tracking cycles after a million visits). Increase the
ordinary sampling budget with `PROPTEST_CASES`; failures print minimized inputs.
Keep discovered defects as named regressions and fuzz seeds. #565 is the first:
`#0=#0#` must not expose an internal label placeholder.

```bash
cargo test -p patina-frontend --test reader_properties
cargo test -p patina-primitives datum_writer
cargo test -p patina-repl --test reader_robustness
PROPTEST_CASES=10000 cargo test -p patina-frontend --test reader_properties
```

`patina-repl/tests/diagnostics.rs` also checks source spans (#367) through both
backends: repeated identifiers, reordered macro arguments, Unicode and mixed
line endings, and deferred calls into included, loaded and library files.
Core tests check document identity across same-named inputs, retained excerpts,
stream compaction and GC slot reuse. Parser tests distinguish program identifier
occurrences from ordinary interned read data; quoted pair/vector cycles check
that annotations do not escape into Scheme values.

`patina-frontend/tests/number_literals.rs` checks the shared number scanner
(#369) through whole-text parsing, token replay, ports and `string->number`:
prefix ordering, radices, SRFI 169 separators, exact decimal conversion limits,
float rounding and error positions in the original spelling. The numeric Scheme
suite checks complex exponent signs, polar exactness, non-decimal infinity/NaN
and zero-denominator read errors on both backends; external differences are in
`DIVERGENCES.tsv`. CLI diagnostic snapshots pin the single-character carets.

`reader_robustness.rs` runs child processes with deadlines on both backends:
million-element lists, million-level nesting, million-directive sequences and
million-character strings with Unicode, escapes and an unterminated variant.
This keeps stack aborts and hangs observable without taking down the test runner.

The separate `fuzz/` workspace uses libFuzzer and the same reader comparison.
It does not format arbitrary-depth data. Invalid UTF-8 is discarded at the
`&str` API boundary; byte-decoding tests remain in `patina-core`. The target
checks both varying chunk lengths derived from the input and single-character
feeds. The checked-in seeds and dictionary cover directives, escapes, numbers,
comments, labels and incomplete input. Generated corpus and failure artifacts
are ignored, while `fuzz/Cargo.lock` pins the fuzz dependencies.

Run this short smoke test from the repository root (nightly is used only for
fuzz instrumentation; the normal toolchain stays pinned):

```bash
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
mkdir -p fuzz/corpus/reader
cargo +nightly fuzz run reader fuzz/corpus/reader fuzz/seeds/reader -- \
  -max_total_time=60 -max_len=4096 -timeout=5 -rss_limit_mb=1024 \
  -seed=366 -dict=fuzz/reader.dict
```

The first corpus directory receives discoveries; tracked seeds stay unchanged.
The timeout, input-length and RSS limits bound a run and expose pathological
inputs; a passing smoke run does not prove an asymptotic resource bound. For a
failure, use the artifact path reported by libFuzzer:

```bash
cargo +nightly fuzz tmin reader fuzz/artifacts/reader/<artifact> -- -max_total_time=30
cargo +nightly fuzz run reader fuzz/artifacts/reader/<artifact>
```

See the [cargo-fuzz tutorial](https://rust-fuzz.github.io/book/cargo-fuzz/tutorial.html)
for running and reducing targets. There is no scheduled fuzz CI job; ordinary
properties run in CI and this command is the bounded fuzz smoke lane.

### Scheme test files (`tests/scheme/`, driven by `scheme_suite.rs`)

**Prefer these for new tests of the language.** They are ordinary, portable
SRFI 64 programs — `(import (scheme base) (srfi 64))`, `test-begin`,
`test-equal`, `test-end` — and one Rust driver runs every one of them on every
backend. Adding a backend touches the driver, not the files; adding a row
touches neither, and adding a file adds one line to the driver's `SUITE`.

**`scheme_suite.rs`'s `SUITE` table is the enumeration of these files** — the
list below covers `tests/*.rs` only, and does not carry notes about files that
have migrated out of it. `the_suite_table_and_the_directory_agree` keeps `SUITE`
and the directory in step, which no prose list can do.

Two reasons to reach for a `.scm` file first:

- **Every backend runs it.** A file asks which backend it is on through
  `cond-expand` (see "A row where the two backends differ" below), so a new
  backend needs no change to the files. The driver would change: its
  comparison is written for exactly two backends today
  (`run_on_both_backends`, `backends_ran_the_same_rows`), so a third means
  generalizing those, not only adding an entry.
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
  the loss is real and that coverage belongs in `data/external-representation.scm`,
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

**Build cost is not a reason either way.** It used to head the list above —
adding a `.scm` file rebuilds in 0.098 s, a `.rs` file in 8.96 s, because
each `.rs` file is its own crate and link — and both numbers came from a
rotted `target/` (CLAUDE.md's build-cost section says how it rots and how
to tell). On a healthy target, measured 2026-09-11: a row in an existing
`.scm` file needs no rebuild (0.1 s); a new `.scm` file needs a `SUITE`
entry, which recompiles the driver (1.0 s); a new `.rs` file compiles and
links in 0.4 s; and each test binary adds about 0.05 s to a full rebuild.
#193 took the workspace from 87 test binaries to 51, which saves about 2 s
a rebuild.

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

**When an oracle hangs or crashes on one row, opt that row out of that oracle,
and say why beside it.** SRFI 64 stops a file where its interpreter dies, so
one troublesome row costs the oracle every row after it; a skip in front of the
row costs only the row:

```scheme
;; Gauche 0.9.15 allocates without bound compiling this template (measured
;; 2026-09-13 inside a procedure that is never called), so it is skipped there.
(cond-expand (gauche (test-skip 1)) (else))
(test-error "a circular operand list is refused" #t ...)
```

One skip per row, even for adjacent rows: `(test-skip 2)` binds to whichever
two rows follow it, and an edit between them moves the skip onto a row nobody
chose. The comment carries the measurement, as a register note would, so the
opt-out can be re-checked against a new oracle version instead of inherited.
`rg -B1 '\((gauche|chibi|\(or chibi gauche\)) \(test-skip' crates/patina-tests/tests/scheme`
lists every opt-out with its row.

The lane reports such a file as "did not complete", ending with the watchdog's
`killed after 60s` or the oracle's own out-of-memory message. Neither names the
row, and Gauche's output is buffered when it is not writing to a terminal, so
an abort loses even the `FAIL` lines before it. To find the row by hand, give
Gauche a line-buffered port and a runner that announces each test:

```sh
gosh -r7 -u srfi.64 \
  -e '(set! (port-buffering (current-output-port)) :line)' \
  -e '(test-runner-factory (let ((make (test-runner-factory))) (lambda () (let* ((r (make)) (begin! (test-runner-on-test-begin r))) (test-runner-on-test-begin! r (lambda (r) (display "%%%% begin ") (write (test-runner-test-name r)) (newline) (begin! r))) r))))' \
  crates/patina-tests/tests/scheme/<file>.scm
```

The last `%%%% begin` line before the failure is the row. chibi needs nothing
extra when it errors, and leaves nothing to read when it is killed.

Opt a row out only when the oracle cannot run it; a row whose answer merely
differs belongs in the register. And keep the `*` for a file where the
trouble precedes every row, as in `control/prompts.scm`, whose note records
why importing Gauche's own prompt procedures would not rescue it.

The lane trusts this because it checks its guards first. Beside the smoke test,
it runs a program that never returns under a two-second timeout and requires
it to be killed rather than scored, and it requires Gauche to stop at a 16M
heap ceiling. The first guards the failure #317 hid: an in-process alarm that
Gauche caught let a spinning `test-error` row pass.

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
`PRD/ARCHIVE/TRACK_L_SNOW_LIBRARIES_PRD.md` §6. It used to be the deleted file's
header, which is why it is restated here: seventeen suite files still say
"Moved from `larceny_families.rs`", and a licence claim must not depend on a
file that no longer exists or on a triage doc marked for deletion.

Sixty-four rows went, ten slices at a time, each carrying a `Larceny family N`
line so `scheme_tests/reports/larceny_triage.md` still maps — better than
before, since the triage doc names a file and a row rather than a 1497-line
haystack, and `every_triage_pointer_names_something_that_exists` now checks that
those pointers resolve. Six rows stayed in Rust at the time: family 40's three
backend divergences, which followed once a `.scm` row could tell the backends
apart (they are the last section of `expansion/hygiene.scm` now — see "A row
where the two backends differ" below), and the two families that need real
files on disk (`include_syntax.rs`, and `standard_ports.rs`, whose file row
later moved to `vfs_file_io.rs`), which stay.

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
it: a divergence whose wrong answer is not *delivered to the row* (see "A row
where the two backends differ" below); that an error escapes an *unguarded*
program (observable only from outside it); and anything asserting *which stage* rejected a program, or
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
answer between our two backends — that is a divergence, and it has a spelling
of its own.

**A row where the two backends differ** is a suite row too, since #193's
divergence slice. Each backend advertises its own feature identifier
(`patina-vm`, `patina-tree-walker` — `Heap::add_feature`, at construction),
so a file can say which one is known to get a row wrong:

```scheme
(cond-expand (patina-tree-walker (test-expect-fail 1)) (else))
(test-equal "the row's name" <the right answer> <the program>)
```

The row asserts the *right* answer, on every implementation — chibi and Gauche
take the `else` and arbitrate it like any other row, which the Rust
quarantines never had — and the line above says who is expected to fail it.
`scheme_suite.rs` holds the two backends to the same rows and to exactly the
expected-failure difference the file declares (`backends_ran_the_same_rows`),
and fails the run on the `xpass` the day the wrong backend starts passing:
delete the line, and the row is an ordinary assertion. That is what
`assert_divergence` did in Rust, with one thing traded away — the stage pin,
since a `test-expect-fail` says only "does not pass" and not "fails at run
time".

The test for whether a divergence can be a row is whether the wrong backend's
answer is **delivered to the row**: a wrong value, or an error a `guard` in
the row can catch. When it is not — the failure escapes every handler in the
program, or a continuation is invoked that runs the rest of the file from
where it was captured — the row takes every row after it down with it, and
the pin stays in Rust, as per-backend assertions. No such pin exists today:
the one family that needed it, the tree-walker's nested trampoline, closed on
2026-09-10, and the `assert_divergence` helper that spelled it went with the
last caller (`git log -S assert_divergence` has it). The inventory of what is
knowingly wrong is `rg 'patina-(vm|tree-walker) \(test-expect-fail'
crates/patina-tests/tests/scheme`, plus the two matrix files.

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

And when the oracle *hangs*, *aborts* or blows its stack rather than refusing to
compile, the cheapest shape of all works — `test-skip` prevents evaluation, so
the program never runs and the file completes. Measured 2026-09-09 across the
three files that carried a chibi `*`:

| file | skips needed | chibi now answers |
|---|---|---|
| `control/callability.scm` | 1 | 25 of 26 — 22 agreeing, 3 registered |
| `control/internal-escape-boundaries.scm` | 1 | 10 of 11, all agreeing |
| `control/wind-thunk-exceptions.scm` | 6+, still failing | none — `*` kept |

*Answers*, not passes: the three differences chibi is now registered for are
arbitration too, and the most useful kind — a difference the register explains
beats a row that agrees. Count rows with
`grep -cE '^\(test-(equal|assert|error)'`; the obvious `grep -c '^(test-'`
counts `test-begin` and `test-end` too, which is how the first draft of this
table said 28.

Two of the three were one row each, costing thirty-five rows of arbitration
between them for want of two lines. The third really is the file: chibi dies on
row 1, then 2, 4, 5, 6 and 7 in turn, and at that point the `*` is cheaper
than the skips and says more.

**That is the bisection worth doing before accepting a `*`** — instrument the
file so each row announces itself to `(current-error-port)`, see where the
oracle stops, scope that row, repeat. If it takes more than two or three, the
premise really is the file.

Use `write-string` for the marker, not `display`: these files import
`(scheme base)` and `(srfi 64)` only, so `display` is unbound and the
instrumented run dies before the first marker, which reads exactly like the
oracle failing at row 0.

That reasoning does **not** extend to a difference in an *answer*. Those stay
unscoped and classified in `DIVERGENCES.tsv`, because there the oracle is
telling you something about a claim your own rows make — and scoping them away
is how you would never learn it. Two Gauche bugs (shirok/Gauche#1326, #1327)
were filed because rows that disagreed were left to disagree in the open, and
both were fixed upstream within days (2026-09-13 and -14). Their register rows
stay until the lane's pinned Gauche is a release that carries the fixes.

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
| patina-pipeline | — | Compatibility facade; tested by the isolated embedding consumer |
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

#### **Compliance Tests** — migrated

`tests/compliance/` held 406 tests of R7RS behaviour, organized by report
section and compiled into one binary through `compliance.rs`. #193 moved all
of them to suite files — lists, strings, vectors, predicates and numbers to
`tests/scheme/data/`, control features to `tests/scheme/control/`, and core
forms, derived forms, quasiquote and macros to `tests/scheme/expansion/` —
and deleted the directory with `compliance.rs`. Each suite file's header
names the module its rows came from.

#### **Feature Tests** (top-level `tests/`)

Named by role, not by count: every per-file number this list used to carry had
rotted by the time anyone checked — `numeric_operations.rs` had migrated to
`data/numeric-operations.scm` and was still listed, `hygiene.rs` was down from
"~108" to 49 and is now deleted, `cps_features.rs` from 31 to 11 and then to 1. For a current count,
`grep -c '^#\[test\]'` the file; for the suite files, `SUITE` in
`scheme_suite.rs` carries a floor per file and a test keeps it honest.

- `hygiene_matrix.rs` — macro hygiene as a *scoreboard*: 28 use-site-binder
  shapes against chibi and Racket, and 111 generated / library-imported macro
  shapes against chibi and Gauche, read as a table when a fix moves a row.
  Its ignored `dump_programs` writes every program out for re-measuring.
  Stays Rust.
  `hygiene.rs` is gone, migrated into `tests/scheme/expansion/`
  (`hygiene.scm`, `syntax-rules-literals.scm`, `let-syntax.scm`,
  `ellipsis.scm`); add a portable hygiene row there
- `hygiene_metamorphic.rs` — Track H1's normal binding-aware generated gate.
  H3's larger four-implementation sweep is explicitly ignored in `cargo test`;
  run `./scripts/run_hygiene_differential.sh` manually with Chibi and Racket's
  `r7rs-lib` installed. It fails if a required oracle is unavailable and keeps
  sources, versions, per-axis counts and minimized findings under the printed
  `target/hygiene-h3/` directory. Use `--seed N --case INDEX` to replay one case
  or `--historical` for seed 285's capture witness. See the supported grammar,
  budgets and measured results in
  [Track H's archived PRD](../PRD/ARCHIVE/completed_planning/TRACK_H_HYGIENE_ASSURANCE_PRD.md#h3--differential-generation-and-shrinking-manual-or-scheduled-lane).
- `escape_from_primitive.rs` — escaping out of a Rust primitive's callback,
  on both backends. `backend_divergence.rs` is gone: its rows are in
  `control/cps-features.scm`, `control/callability.scm`, `control/prompts.scm`
  and `expansion/hygiene.scm`, the open ones as backend-scoped expectations
- `control_flow_matrix.rs` — the 24-shape transfer matrix behind
  `docs/VM_RUNTIME.md` §5.6; the prompt rows it does not cover are
  `control/prompts.scm`. `cps_features.rs` holds one test, which needs a
  thread with a sized stack

#### **Library Tests**
- `sld_file_loading.rs` — library loading from `.sld` files
- `r7rs_libraries.rs` — R7RS library compliance
- `scheme_base.rs` — `(scheme base)`
- `bundled_provenance.rs` — pins every third-party file claimed byte-identical
  to an upstream release, so an unrecorded edit fails

#### Import modifier policy and context coverage (#592)

`ImportSet::resolve_bindings` in `crates/patina-runtime/src/import_set.rs` is the shared
name-selection policy for program imports, library imports, `eval`/`load`, and
`environment` on both backends. It maps each importing name to its original
library export. Loading and evaluating the library stay with the backend (or
`ApplyContext`); installation uses `Library::import_into`, with the VM's
`import_export` wrapper preserving primitive-shadow invalidation. The selector
holds strings, not Scheme values or temporary environments.

| Import-set case | Policy |
|---|---|
| `only` | Select the named bindings; an unknown name is an error (#485). Repeated names are accepted; an empty list imports nothing. |
| `except` | Remove the named bindings; unknown or repeated names are harmless. An empty list changes nothing (#489, #592). |
| `prefix` | Prefix every name in the immediately enclosed set. Bare names are not additionally imported. |
| `rename` | Require each source name to exist (#489); rename simultaneously so swaps work. Keep unmentioned exports. An empty list changes nothing. |
| Nested modifiers | Work from the library outward. Validation sees the names produced by the enclosed set, not the library's original spellings. |
| Syntax exports | Macros and core syntax participate in selection and renaming exactly like variables. Macro references keep their definition-site bindings. |
| Exported binding identity | Every alias still denotes the original exported location (#406), including a library's renamed exports. Later assignments by the library remain visible. |
| Failure effects | Loading/initialization is not rolled back. Preserve the existing final-`only` behavior: names before the first missing name are installed; an inner modifier failure installs nothing in the outer destination. |

This preserves Patina's established policy, not a claim that every Scheme must
reject the same invalid import. In particular, Chibi 0.12 and Gauche 0.9.15
both accept an unknown `except` name in `environment`; measured on 2026-10-01 (UTC),
the corrected `stdlib/eval.scm` row returns 42 on both. The existing divergence
register describes the separate `rename` disagreement with Chibi. This work
does not introduce a new validation policy for conflicting rename destinations.

`import_modifiers.rs` runs the same 18 selection/validation cases through five
contexts on both backends, plus primitive-cache rebinding and partial-failure
regressions (190 backend executions). These are fixed seeds for
[#589](https://github.com/avalonalex/patina/issues/589) in testing master plan
[#584](https://github.com/avalonalex/patina/issues/584), not a new generator.
Existing `library-bindings.scm`, `spliced_imports.rs`, `sld_file_loading.rs`,
`hygiene_matrix.rs` and the control/rooting tests remain separate guardrails.

```bash
cargo test -p patina-tests --test import_modifiers
./scripts/run_suite_oracles.sh stdlib/eval.scm
```

#### **Integration Tests** (`tests/integration/`)
- Compare Patina output with chibi-scheme
- Test full program execution
- Verify compatibility

**Run with:**
```bash
# Run all integration tests
cargo test --package patina-tests

# Run specific test file
cargo test --package patina-tests --test hygiene_matrix
cargo test --package patina-tests --test cps_features

# Run every suite file (tests/scheme/*.scm) on both backends
cargo test --package patina-tests --test scheme_suite
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
| expansion/hygiene.scm | 41 | measured 2026-09-11 |
| scheme_base.rs | ~50 | |
| sld_file_loading.rs | 40 | measured 2026-09-09 |
| data/record-types.scm | 23 rows (41 Rust tests before #193 Phase 2) | measured 2026-09-11 |
| tail-recursion.scm | 38 | measured 2026-09-11 |
| control/prompts.scm | 58 rows | measured 2026-09-11 |

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

# Only the suite files (tests/scheme/*.scm)
cargo test --package patina-tests --test scheme_suite

# Only CPS features
cargo test --package patina-tests --test cps_features

# Only the macro hygiene scoreboard
cargo test --package patina-tests --test hygiene_matrix
```

### Embedding API and feature coverage (#595)

`Interpreter<B>` supplies the one program loop and source tracking used by the
VM/tree-walker aliases and the legacy adapters. The runnable examples and the
[migration guide](../README.md#embedding-from-rust) make backend selection
explicit. `patina-pipeline` only re-exports compatibility types from the
interpreter; the interpreter never depends on that facade.

```bash
cargo test -p patina-interpreter -p patina-pipeline --lib --tests
cargo test -p patina-tests --test interpreter_api
./scripts/check_embedding_features.sh
```

The script creates a consumer outside the workspace and tests no features
(host-supplied backend), VM, tree-walker, both, legacy-only, and actual defaults.
It checks the normal dependency graph as well as execution: a VM-only host must
not include the tree-walker or pipeline, and a tree-walker-only host must not
include the VM. Workspace dev-dependency feature unification cannot supply a
missing feature. It uses cached dependencies (`--offline`; run a workspace
build first) and `target/embedding-features`, separate from the CLI artifacts.
Both CI platform test jobs run it.

The consumer covers malformed input (#329), library/macro imports, macro and
runtime diagnostics with source positions, multiple/cyclic result formatting,
host command lines, and old pipeline module paths with a child environment.
`legacy_embedding.rs` pins the three corrected convenience-API discrepancies,
legacy error categories and effects before a read error. These are fixed
embedding cases for [#589](https://github.com/avalonalex/patina/issues/589),
not a separate equivalence generator. Both backend examples and the rustdoc
examples should also run when this API changes.

### GC lanes (`docs/GC_DESIGN.md` §11)

The collector is held to output equality: a program prints the same with
collection off as with it on, however often it collects. The lanes run against
a check build — debug, or release with `--features patina-core/gc-check` —
where a stale heap reference, a broken deferral rule or a register its
liveness map wrongly retired panics where it is used, rather than surfacing
later as a wrong answer or not at all (#621, #624, #625).

| Lane | Script | What it runs | Interval | Budget | In CI |
|---|---|---|---|---|---|
| Differential | `scripts/run_gc_differential.sh [binary]` | the chibi suite under GC off, the adaptive default and stress, on both backends, with the tally pinned; plus two reclamation proofs per backend, in bytes (below) | `PATINA_GC_STRESS_INTERVAL`, default 16; 1 in the release lane | the release lane's step took 5.9 min on the runner and the debug lane's 1.1 min (2026-10-03) | `ci.yml`, every change: release `gc-check` at 1, and debug at 16 |
| Zeal | `scripts/run_gc_zeal.sh [binary]` | `tests/scheme/control/*.scm` except `tail-recursion.scm` under GC off and zeal, on both backends; each file must match, exit 0, print its SRFI 64 summary and have collected, and the binary must first pass a probe that it honours zeal (below) | `PATINA_GC_ZEAL=entry`: every outermost safe point | 10 min on the runner (2026-10-03), within the job's 60 | `gc-zeal.yml`, release `gc-check`, when a change touches the VM's runtime, compiler or types, `patina-core`'s heap or `tagged_value.rs`, the library loader or registry, the tree-walker's evaluator, the toolchain, or the lane's own files, and weekly on `main`; it also runs `finished_forms_release_code` under zeal |
| Stress, per PR | `scripts/run_gc_stress_tests.sh` | 13 `cargo test` targets that drive the collector, control flow and library loading through the embedding API (`callability`, `control_flow_matrix`, `cps_features`, `ephemerons`, `escape_from_primitive`, `finished_forms_release_code`, `gc_tree_walker`, `gc_vm`, `hygiene_matrix`, `interpreter_api`, `library_loading`, `macro_definition_env`, `vm_callprimitive`), and `scheme_suite.rs`; each must run its pinned number of tests, none filtered out, pass, and report at least its pinned number of collections | `PATINA_GC_STRESS=16`; 4096 for `scheme_suite.rs` | about a minute: 62 s here (2026-10-02), 37 s of it the 13 targets, 28 s of those `gc_tree_walker`, nearly all one deep-recursion test; the step's limit is 15 min | `ci.yml`'s Test Suite, every change, on ubuntu and macOS, in the debug build that job has just tested |
| Stress, nightly | `scripts/run_larceny_gc_stress.sh [--tree-walker] [--r6rs]` | Larceny's R7RS and `(r6rs ...)` suites at the pinned commit, on both backends; each suite's tally must be its row in `scheme_tests/reports/larceny_gc_stress.tsv`, its exit status the one its tally implies, with no panic, no timeout, at least the row's pinned number of collections, and only the job's backend in its process | 16; 4096 for `char`, `flonum`, `lazy`, `sort` and `stream`; `ephemeron` left out until #609 | a job per backend, each limited to 75 min; on the runner (2026-10-03) the VM's R7RS lane took 434 s and its `(r6rs ...)` lane 24 s, a 9-minute job, and the tree-walker's 1774 s and 51 s, a 31-minute job, about 2.2 times the time on an M-series Mac (307 s, 20–22 s, 757–1008 s, 29–36 s); a suite may take 600 s on the VM and 1500 s on the tree-walker, whose slowest, `stream`, took 783 s | `nightly.yml`, release `gc-check`, daily on `main`, and on a pull request that changes the lane |

Zeal costs about 7× stress 1, so it never runs the whole suite: the chibi suite
under zeal took 882–920 s on the VM alone (2026-10-01), and
`tail-recursion.scm` alone 277 s on the VM and 627 s on the tree-walker.

Each check has a positive control, a test that must panic with the check's
message, and the release GC lane runs them in its `gc-check` build:
`stale_references.rs`, `gc_protocol.rs` and `retired_registers.rs` in
`patina-tests`, and `patina-core`'s `heap::check::` and
`heap::gc::tests::protocol::`. A build without the checks reports them
ignored. `crates/patina-repl/tests/gc_zeal.rs` is the control for zeal
itself, in every build: a loop that allocates nothing collects at every
iteration under zeal and hardly at all at stress 1. It runs in `ci.yml`'s Test
Suite, not against the zeal lane's binary, so the lane runs the same loop on
that binary before it starts, on both backends: at least 1000 collections
under zeal and fewer than 100 with no GC variable set, or it fails there. Its
per-file check that a run collected cannot stand in for this: a control file
can collect under the default GC too (`cps-features.scm` does, once; before
the byte trigger, #606, every one did, while it loaded SRFI 64).

**The reclamation proofs are in bytes, and none can pass without collecting
(#606).** `(gc-stats)` reports `bytes-reclaimed`, every byte a collection has
freed, which grows only when a collection frees something; each proof
requires it to grow across its workload. In `run_gc_differential.sh`, the
stress proof churns 20,000 conses at the lane's interval and requires more
than 1000 collections and at least 90% of the bytes the churn allocated
(`bytes-allocated`) reclaimed; the default-mode proof churns 20,000 vectors of
1000 elements, about 160 MB, with no GC variable, and requires a collection,
half of those bytes reclaimed and `committed-bytes` under a quarter of them
(an object-count trigger collected nothing there: 20,000 allocations were
under its floor of 65,536). In the shared GC tests (`gc_shared_tests!` in
`crates/patina-tests/tests/common/mod.rs`, run by `gc_vm` and
`gc_tree_walker`), the pair, cycle and arena-plateau proofs measure
`bytes-reclaimed` across their churn, and the arena comparison requires it
above 0. `crates/patina-repl/tests/gc_byte_trigger.rs`, in the Test Suite,
runs #606's shapes through the CLI with no GC variable: 200 garbage vectors
of 100,000 elements and 1,000 VM captures 1,000 frames deep each collect
about every 8 MiB and end with a few MB committed, and 600 kept captures
collect three times, because L counts their snapshots (with them left out of
L it collects eleven times, and the test fails).

The lanes see a missed trace edge only when no other path reaches the value,
so the trace code has checks of its own (#623, `docs/GC_DESIGN.md` §5.4).
`scripts/check_gc_trace_names.py`, in the Clippy job, requires every trace
function to take its struct apart by name. Sentinel tests, in the Test Suite,
put a fresh value in each traced field of each traced struct, root the struct
alone, collect, and check each value survived (`heap::sentinels`); each fails
with its field's trace line deleted. They live beside the code they test:
`patina-core`'s `heap::trace_sentinels`, `environment::gc_edge_tests` and
`library::gc_edge_tests`, `patina-vm`'s `trace_sentinel_tests`, the
tree-walker's `gc_roots::sentinel_tests` and `patina-runtime`'s
`gc_root_tests`.

**Every stress run must have collected (#626).** A lane that passes
without collecting has tested nothing, and four have (#5, #164, #200, #201):
the variable that sets the mode did not reach the process, or the program
allocated too little to cross the interval, and neither shows in the output.
`PATINA_GC_COUNT_DIR=<dir>` makes each process write `gc-count.<pid>` there:
one line with the mode its environment selects, the backends it has made and
the collections it has run, rewritten at every collection
(`crates/patina-core/src/heap/gc.rs`), and written with a count of zero as
soon as it makes a collector, so a process that never collects says so. Both
stress lanes require a record from every run, under the run's interval, with
at least the pinned minimum: half the count measured when it was pinned,
which is deterministic for a program at an interval, never below 1, and
above what the run makes without stress. The nightly lane also requires the
record to name its job's backend and no other: a tree-walker job that ran the
VM would pass nearly every row, since the two backends' tallies differ only
in `time`. One per-PR target pins a minimum near zero on purpose:
`library_loading` does nearly all its work inside library loads, and a load
defers collection for as long as its unevaluated body exists; it is in the
set for the day that deferral is lost. `macro_definition_env` works inside
library loads too, but one of its tests expands a library's macro after a
collection, so it collects.

**A change that moves a Larceny tally re-pins its stress rows in the same
pull request.** The nightly lane holds each suite on each lane to its row in
`scheme_tests/reports/larceny_gc_stress.tsv`, so a fix from the defect queue
(`scheme_tests/reports/larceny_triage.md`) that changes a suite's status,
passed or total count fails the nightly the morning after it merges unless
the rows move with it, on every lane it moves. Edit their status, passed and
total to the regenerated reports', or re-pin them with
`run_larceny_gc_stress.sh --update-baseline`; `run_larceny_tests.sh` warns
when a fresh tally differs from its row. Where the nightly finds a tally
that differs, it runs the suite again without stress and says which it is:
the same tally without stress is a stale baseline, a different one is the
collector changing what a program computes. The baseline was measured on
macOS and checked on the nightly's ubuntu x86_64 runner, whose tallies it
holds: `flonum` fails one more assertion on x86_64 than on arm64 (#634), so a
local macOS run reports that row as "not the collector". A row that differs
on the runner for the platform's sake (a libm result, the clock) is
re-pinned from it, with `nightly.yml`'s `update-baseline` input, which
uploads the rewritten rows rather than committing them.

The positive controls, run when the lanes landed (#626): with the stress
variable misspelled in the per-PR script, every target passed its tests and
failed the lane on its record's mode, and all but `macro_definition_env`,
whose minimum was then zero, on its count too; the per-PR lane run with a test
filter failed every target on its test count (#201's lesson, a run cut
short); a nightly suite run by a binary wrapped to drop the variable failed
on its mode and its count; one tally edited in the baseline failed its
suite; and a library load without its deferral, #6's shape, failed both.
Without `ParsedLibrary`'s `GcDeferGuard::holding`, nine of the 13 targets
failed with a use-after-free (`callability`, `control_flow_matrix`,
`ephemerons`, `escape_from_primitive`, `gc_tree_walker`, `hygiene_matrix`,
`interpreter_api`, `library_loading` and `macro_definition_env`), and so did
`scheme_suite.rs`; the VM's Larceny suites did not, because a VM library
body also runs inside `VmState::with_globals`' guard, and with that one gone
too every suite panicked at bootstrap. The tree-walker's bootstrap panics
without the first alone. Three more were run when review found holes, each
failing the lane where the lane before it passed: a tree-walker job whose
runner dropped `--tree-walker` failed every suite on its record's backend;
a binary that printed its whole tally and then died of SIGSEGV failed every
suite on its exit status; and with stress never firing,
`macro_definition_env` now fails on its count like the rest.
`scripts/tests/test_gc_stress_lanes.py`, in the Test Suite's offline script
tests, keeps each failure path of both scripts under test against fake
binaries.

`scheme_suite.rs` runs at 4096 because of one file. At 16 the whole target
had not finished after 25 minutes; run through the debug CLI, one file at a
time, `srfi/regex-graphemes.scm` had not finished after 300 s on either
backend, while every other file together took about 90 s on the VM and 130 s
on the tree-walker. It compiles SRFI 115's grapheme regex, a large live heap
of character sets, and matches it against 400 syllables: 9 s on the VM and
22 s on the tree-walker without stress, 12 s and 28 s at 4096, 58 s and 81 s
at 256. The whole target took 20 s without stress, 27 s at 4096, 47 s at
1024 and 125 s at 256 (2026-10-02).

Four tests compare collecting with not collecting, and under a stress lane's
variable a backend made from the environment collects on the side that must
not. They name their mode instead: `gc_shared_tests!`'s
`collecting_keeps_the_arena_smaller_than_not_collecting` and
`unreachable_cycles_are_reclaimed`, on each backend, use
`vm_interpreter_gc_off` and `tree_walker_interpreter_gc_off`, where only
`(gc)` collects. #626 named a fifth,
`a_form_whose_continuation_has_died_lets_its_code_go_at_a_collection`, which
since #629 keeps every continuation it captures and passes in every mode, so
it still reads the environment, and the zeal lane still runs it under zeal.

The nightly lane's `time` suite measures wall-clock time: one assertion
requires a one-second loop to stop within 100 ms of its deadline. It passes
on the VM and fails on the tree-walker here, with or without stress, as the
tracked reports also record, and a slower or busier runner could move it
either way, so its rows hold only its total and that it reached a tally
(`-` for its status and passed count).

```bash
cargo build --release -p patina-repl --bin patina --features patina-core/gc-check
PATINA_GC_STRESS_INTERVAL=1 ./scripts/run_gc_differential.sh target/release/patina
./scripts/run_gc_zeal.sh target/release/patina
./scripts/run_larceny_gc_stress.sh                 # and --tree-walker, --r6rs
cargo build && ./scripts/run_gc_differential.sh target/debug/patina
cargo test --all --lib --tests && ./scripts/run_gc_stress_tests.sh
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

For the chibi R7RS gate, run `cargo build --release` followed by
`./scripts/run_chibi_tests.sh` and `./scripts/run_chibi_tests_tree_walker.sh`.
Both scripts print the suite output, including durations, to the console.
The committed `scheme_tests/reports/results*.txt` omit only tally durations,
and `compatibility*.md` omit generation timestamps, so identical outcomes
produce identical reports. Counts, skipped tests and failure details remain
in the saved output.

### Third-party corpus measurements

Build the binary under test with `cargo build --release`, then run
`cargo run --release -p patina-compat -- run`. It writes the measured results
to `compat/reports/results.scm` and their rendering to
`compat/reports/report.md`. Use `--tree-walker` for that backend, with explicit
`--results` and `--report` paths to preserve the canonical VM artifacts.

The harness requests `patina --diagnostics-file <path>` for every child
(#382). This opt-in mode writes a separate JSON-lines file alongside ordinary
stderr, for scripts, stdin programs and `-p`; interactive and dump modes reject
it. The first line is `{"protocol":"patina-diagnostics","version":1}`, even
on success. Subsequent records have a `kind` and human-readable `message`,
with `path`, `library` (an array of name components), `identifier` or
`extension` where relevant. Names and paths are JSON strings, including
whitespace, quotes and Unicode; they are never recovered from printed prose.
Only uncaught, reported errors produce records. Under `-k` each reported error
gets one, and a later `(exit 0)` cannot erase the failed status.

Kinds are `parse`, `syntax`, `missing-library`, `unbound-identifier`, `load`,
`native-extension`, `runtime` and `io`. The corpus groups `parse` and `syntax`
into its existing `parse-error` bucket. An initialization or export failure
is `load-error`; a nested missing library or reader failure retains its more
specific cause. This makes SRFI 179's missing `u1vector-ref` a `load-error` on
both backends, without changing why it is excluded. Classification uses tags
and payloads; `message` supplies readable histogram detail only.

The empty-package self-check requires the protocol header, so an old binary
cannot silently fall back to prose classification. Missing, malformed or
unsupported streams fail classification. Timeout and suite-tally precedence
remain enforced. Third-party test frameworks still report their own tallies
as text, and the deliberately emitted Scheme FFI-stub marker
`requires FFI, unavailable in Patina:` remains an explicit protocol of
`test-lib/chibi/filesystem.sld`; it is not an interpreter error-message match.

`wrong-result` snapshots retain a `(failures ...)` list with suite tallies,
failure excerpts and, for SRFI 64, failed test records from the announced log
inside the run's scratch directory. Logs are read before scratch cleanup;
each log scan is limited to 1 MiB. `runtime-error` retains `(errors ...)` with
diagnostic messages and paths, exit status and fallback output when no typed
diagnostic is available. Smoke completion failures identify the missing, duplicate,
malformed or mismatched tally. Evidence is capped at 32 lines and 8 KiB per
package, with at most 512 bytes per line and an explicit truncation marker.
The report displays it for excluded packages too. Older snapshots without
these fields remain readable and show “Not recorded in this snapshot”.
Evidence collection does not change classification precedence or the score.

The pass headline splits successful packages into **upstream test suites**,
**maintained smoke checks**, and **import-only probes**. All three counts come
from the snapshot's per-package `mode` and `status`; failed packages do not
contribute. A smoke pass establishes only the assertions its driver makes;
a probe pass establishes loading without calling exported procedures.

`compat/smoke/manifest.scm` registers smoke drivers by package slug and expected
assertion count. The 106 drivers cover binary-record read/write round trips;
PFDS queues, heaps, deques, difference lists, fectors, lazy lists, sequences
and sets, HAMTs, bounded-balance trees, finger trees and priority search
queues, and their alist, bitwise, vector and list helpers; SLIB formatting,
string search, string casing, string ports, line I/O, generic writing,
printf, alists, queues, trees, common list functions and topological sorting;
SLIB integer and real math, modular arithmetic, rational approximation,
factorization, matrix operations, array iteration, interpolation and subarrays;
SLIB byte arrays, byte/number encodings, coercions, chapter ordering, filename
matching, soundex, formatted input, pretty printing and URI processing;
SLIB color spaces, palette dictionaries, daylight models, character plotting,
Fourier transforms, minimization and random sampling;
SLIB calendar arithmetic, TZ rules and binary timezone records, POSIX and
Common Lisp time APIs, common helpers, dynamic bindings and substring moves;
SRFI 63 arrays, SRFI 95 sorting, SRFI 43
vectors, SRFI 37 argument parsing; SRFI 2, 11, 16, 26, 31 and 227 syntax;
SRFI 28 formatting, SRFI 29 localization, SRFI 38 shared-structure I/O,
SRFI 39 parameters, SRFI 51 rest arguments and checks, SRFI 145 assumptions;
SRFI 25 shared arrays, SRFI 42 comprehensions and SRFI 78 lightweight testing;
SRFI 19 times and dates, all five SRFI 166 formatter modules;
and SRFI 180 and MacDuffie JSON.
The PFDS checks exercise branching updates that preserve older versions,
deque rebalancing, lazy-tail memoization, sequence splits and set operations.
All 16 PFDS packages now run smoke checks. The finger-tree driver checks
every split boundary of a 64-element tree, cumulative value measures,
order-sensitive measures, deep append and removal from both ends. The
priority-search-queue driver checks priority ties, reordering after updates,
inclusive range queries, custom orderings and deletion after 64 insertions.
The map checks cover hash collisions, sparse and deep branches, tree ranks
and indexed lookups, callback counts, ordered folds and map combinations.
The HAMT driver gates the upstream correction in `compat/patches/` (#508),
including 480 mixed operations checked against an alist model with five
hash functions. Its correctness overlay is recorded alongside the
SRFI corrections in `compat/patches/README.md`.
The text checks exercise search boundaries, mutation, callback results, port
state, formatting and bounded output.
The SRFI procedure checks cover stable keyed sorting and destructive operations,
indexed vector callbacks and slices, argument-parser callback seeds, JSON
round trips and error records. The SRFI 95 and 180 drivers also gate the
two-element sortedness and JSON number grammar corrections in
`compat/patches/`; their correctness overlays are recorded in
`compat/patches/README.md` (#504, #505).
The syntax drivers exercise short-circuit evaluation, parallel and sequential
multiple-value bindings, arity dispatch, `cut` versus `cute` evaluation timing,
recursive bindings, and optional-argument defaults and rest arguments. They
also check that generated bindings do not capture user variables. Prefixed
imports keep the corpus macros distinct from similarly named bundled forms.
The six utility drivers check formatting directives and errors, locale fallback
and bundle replacement, shared identity and cycles, parameter converters and
continuation reentry, rest-argument modes and short-circuit checks, and lazy
assumption diagnostics. The SRFI 145 failure checks measure the pinned
implementation's explicit error path; the SRFI permits implementations not to
signal false assumptions. The SRFI 38 driver gates the string-escape correction
in `compat/patches/srfi-38.patch` (#511): the portable reader delegates string
decoding to the host reader and copies the result to preserve mutable strings,
including through shared labels. Its correctness overlay is recorded in
`compat/patches/README.md`.
The SRFI 25 driver checks nonzero bounds, rank-zero and empty arrays, all index
forms, shape independence, and shared mutation through transposes, diagonals
and composed affine views. SRFI 42 covers nested and parallel generators,
indices, empty inputs, reductions, short-circuiting and evaluation counts.
Its indexed multi-vector regression gates `compat/patches/srfi-42.patch`
(#513), also exercised through SRFI 78's `check-ec`. SRFI 78 captures deliberate
failures and reports to assert evaluation order, accumulated counts, reporting
modes, disabled checks and the first counterexample. It tests the four exports
of the pinned library; `check-reset!` and `check-passed?` are not exported there.
The time and formatting batch adds 131 assertions across seven packages:
SRFI 19 (31), SRFI 166 (45), SLIB alist (14), queue (11), tree (11), common
list functions (10) and topological sort (9). Dates use fixed instants and
explicit zones; one clock check temporarily replaces and restores the shared
`current-second` binding. No timing thresholds or sleeps are used. These
checks gate the arithmetic, calendar and clock-adapter overlay (#515), but
do not establish process/thread CPU-clock accuracy: the pinned library still
uses a wall-clock surrogate for those sources.
Formatter checks cover state restoration, numeric output, joining, padding,
trimming, row/column tracking, shared and cyclic writing, Unicode display
width, colors, wrapping and columnar output. The #516 overlay repairs word
and character wrapping and cursor boundaries; two assertions temporarily
install checked cursor movement so both Patina backends detect the invalid
movement that Gauche rejects. All temporary bindings are restored with
`dynamic-wind`. SLIB checks cover supplied equality, mutations, callback
arguments, copying and sharing, fold direction and graph ordering constraints.
The alist overlay (#517) makes numeric `=` lookup honor exact/inexact
equivalence instead of substituting `eqv?`.
The numeric and array batch adds 153 assertions across nine SLIB packages:
math-integer (18), math-real (20), modular (17), rationalize (12), factor (16),
determinant (17), array-for-each (18), array-interpolate (16) and subarray (19).
Checks cover exact arithmetic and domain errors, signed modular operations,
integer numerator/denominator pairs, factor reconstruction, matrix inversion,
scalar and empty arrays, source-sized copying, interpolation boundaries and
mutation through shared views. Prime checks use deterministic small cases and
a known large prime; no composite is expected to pass a probabilistic test.
The numeric overlays (#519) repair negative unit powers, optional `atan`
arguments, signed gcds, zero powers, singleton ratios, zero prime counts and
singleton matrix inverses. The array overlays (#520) repair scalar traversal,
copy bounds, low-edge interpolation and empty trimmed views.
The byte/text batch adds 172 assertions across ten SLIB drivers: byte (25),
byte-number (23), coerce (17), chapter-order (12), filename (19), soundex (10),
scanf (24), pretty-print (10), pprint-file (8) and URI (24). Five more assertions
extend the existing SRFI 63 driver from 6 to 11, for 177 new assertions total.
Checks cover independent copies, byte order, short reads and consumed counts,
integer and IEEE encodings, numeric collation, type conversions, chapter
carries, glob captures, parsing, printed-data round trips and comments.
Byte I/O uses ASCII string ports because the pinned adapter calls `read-char`
and `write-char`; it does not establish binary-port or non-ASCII fidelity.
Filename callbacks request zero temporary names, so they exercise return
values without creating or deleting files. The file formatter is exercised
with caller-owned string ports. SLIB scanf's explicit-whitespace rule is
intentional: input whitespace is skipped only when the format contains it,
as documented in the [SLIB manual](https://people.csail.mit.edu/jaffer/slib/Standard-Formatted-Input.html).
The #522 overlays repair byte copies, offset counts, reverse-read EOF loops,
single-precision infinity encoding and SRFI 63's empty/deep list conversion.
The #523 overlays repair vector coercions, standalone and width-limited zero
fields, URI authority markers and absent ports, and callback return values.
The [patch policy](../compat/patches/README.md#correctness-patches-during-429)
authorized upstream correctness repairs during the now-completed #429 work
without further per-patch approval, with issue evidence, reference comparisons
and CI regression assertions required. It records the rules for changes
outside that scope as well.

The color/scientific batch adds 155 assertions across ten SLIB drivers:
color-space (43), color (27), daylight (15), nbs-iscc (8), resene (8),
saturate (8), charplot (10), fourier-transform (13), minimize (10) and
random-inexact (13). Checks cover color conversions and serialization,
whitepoints, weighted color differences, spectral boundaries and integration,
palette lookup and enumeration, fixed solar geometry and sky-model identities,
plotted dimensions/labels/markers, transform signs and multidimensional round
trips, minimum locations and evaluation counts, and sampling transformations.
Floating comparisons use explicit tolerances. Plot checks set dimensions and
capture string ports; callback sample sets are checked without imposing an
evaluation order. Random checks inject equal samples within each Box-Muller
pair to permit either `let` initializer order, restore the shared binding,
and compare state replay only within a host. No statistical thresholds or
cross-host PRNG sequences are asserted. File-backed illuminant readers and
full physical/colorimetric conformance are outside this smoke scope.
The #525 overlays repair folded parser tags, hexadecimal string lengths,
whitepoint construction, scalar metric factors, squared CIE94 scales, the
last spectral table row, descending spectrum traversal and the solid-sphere
sampler's documented squared-norm return value.

The SLIB time/helper batch adds 140 assertions across eight drivers:
time-core (31), tzfile (12), time-zone (23), posix-time (17), common-lisp-time
(14), common (18), dynamic (13) and rev2-procedures (12). Fixed Gregorian
dates cover pre-epoch leap years and century exceptions; zone cases exercise
northern/southern DST boundaries, Julian versus ordinal rules, exact binary
transition lookup, repeated local hours and historical offset changes.
Calendar seconds are compared numerically without requiring exactness.
The test-only `(patina compat time-fixtures)` helper creates and removes
small binary files in the runner's per-package scratch directory. They cover
signed TZif v1 fields, mode abbreviations, optional flags and leap records,
plus binary/text file opening. These checks do not claim TZif v2/v3 support,
leap-second application, or a policy for nonexistent spring-forward times.
Clock-dependent Common Lisp entry points temporarily replace and restore the
shared Scheme clock and zone-resolver bindings; no expected result depends
on the host clock, environment or timezone database.
The helper checks also exercise port closure after normal return, all callback
result arities, symbol uniqueness, dynamic binding restoration on exceptions
and continuation exit/reentry, overlapping string moves and bounded fills.
The six correctness overlays are tracked in #527 (calendar, timezone and
time conversion) and #528 (common helper uniqueness and value forwarding).

The text/parsing batch adds 97 assertions across six drivers: Chibi char-set
(19), char-set-boundary (11), html-parser (17), irregex (17), sxml (15), and
SLIB xml-parse (18). They cover set algebra and mutation, all 11,172 Hangul
syllables' LV/LVT partition, grapheme matching, HTML callback/recovery behavior,
SXML escaping/rendering, regex capture/replacement/folding, XML namespaces and
duplicate attributes. Four-host results are 88/97 before and 97/97 after the
#530/#531 corrections. Tests retain the pinned Unicode 6.3 data version.
#530 also repairs the bundled SRFI 115 copy, with six public-API regressions
in `tests/scheme/srfi/regex-graphemes.scm`. Native Chibi passes two and Gauche
five; the four missing-data/LV cases and Gauche's merged-LV case are classified
in `DIVERGENCES.tsv`. Both Patina backends pass all six.

The utility batch adds 126 assertions across Chibi config (30), environment
monad (10), generators (35), association unpacking (18), trivial tar writer
(15), Chris Oei test (8), and lightweight-testing (10). These cover config
precedence, persistence, schemas and file includes; monad binding/restoration;
generator construction, coroutine traversal, consumers and exhaustion;
unpack validation; tar headers, checksum and block alignment; and both
successful and deliberately failing test-helper calls. All 126 pass on both
backends, Chibi and Gauche. Three overlays fix the upstream defects in #534.
The tar writer also exposed #533: Patina binary I/O now honors the current
port when the optional port is omitted. Ten added public Scheme port rows
pass on both backends. Chibi's bytevector `u8-ready?` limitation is recorded
in the oracle register; the other new rows agree with both references.

The final wrapper batch adds 58 assertions: SLIB directory (11), Chibi
temporary directories (8), Chibi line editor (16), and rebottled PSTk (23).
They cover filesystem traversal/globs and cleanup, bounded history and text
editing with string ports, and Tcl command serialization/widget caching.
All pass on both Patina backends, Chibi and Gauche with the reference setup
below. Three overlays repair #536. The corpus now has **37 upstream suites,
106 smoke packages and zero passing import-only probes**. The Rust guard
`every_in_scope_package_has_execution_coverage` prevents an in-scope package
from losing both its driver and manifest entry and silently reverting to a
probe; the existing registration and execution-count guards remain active.

All 1522 assertions were compared against both Patina backends, Chibi 0.12
and Gauche 0.9.15 using the pinned corpus libraries with their patch overlays.
The six-driver PFDS maps batch adds 72 assertions: all pass on both Patina
backends and Gauche, while Chibi passes 71. Its one difference is the known
SRFI 151 defect returning 0 for `(arithmetic-shift -1 -100)`; the PFDS bitwise
driver keeps the expected -1. This is already classified as `oracle-defect`
in `crates/patina-tests/tests/scheme/DIVERGENCES.tsv`, under `srfi/bitwise.scm`
and "a negative operand fills to -1". It is not a passing comparison or a
reason to change Patina's answer.
The final two PFDS drivers add 44 assertions, all passing on both backends
and both references with the pinned libraries and no additional patches.
The six SRFI utility drivers add 76 assertions, all passing on both backends
and both references with the SRFI 38 overlay.
The SRFI 25/42/78 batch adds 63 assertions (20/28/15), all passing on both
backends and both references with the SRFI 42 overlay.
The seven-package time/formatting/collections batch passes all 131 assertions
on both Patina backends. Each reference passes 130/131; the topological-sort
case-insensitive-key assertion is classified here as an **oracle defect**.
Both references can select an incompatible default hash for the R7RS
`string-ci=?` predicate: inserting `"B"` into `(make-hash-table string-ci=?)`
and querying `"b"` can miss, while explicitly passing `string-ci-hash` succeeds.
Gauche's failures depend on hash collisions, so an occasional passing run is
not evidence of correctness. [SRFI 69](https://srfi.schemers.org/srfi-69/srfi-69.html#make-hash-table)
explicitly guarantees an appropriate default for `string-ci=?`; Patina passes
and the assertion is retained.
This compatibility driver is outside the `tests/scheme` oracle lane, so its
divergence is recorded here rather than adding an unexecuted register row.
The nine-package numeric/array batch passes 130/153 assertions on each of
the four implementations before repair, and 153/153 with the overlays.
The byte/text batch passes all 177 new assertions on both Patina backends and
Gauche. Chibi passes 176/177: its native `(magnitude (expt 2.0 -1074))` returns
zero although both the input and `abs` of the input are nonzero. The pinned
double encoder uses `magnitude`, so Chibi loses the smallest subnormal. This
is an **oracle defect**, not a passing comparison; the expected byte encoding
is retained. Like the other compatibility-driver differences above, this is
outside the `tests/scheme` oracle lane and is recorded here and in #522.
Before repair, byte reverse reads at EOF time out on all four implementations;
no complete before-repair assertion total is claimed. Gauche alone loses
zero/multiple callback results in `call-with-tmpnam`: the original captures
them in a single-value `let`, whose result arity is unspecified by R7RS.
The overlay uses `call-with-values` and preserves every result on all four.
The color/scientific batch passes 137/155 before repair and 155/155 afterward
on both Patina backends and Gauche. Chibi passes 136/155 before and 154/155
afterward. Its remaining automatic Fourier round-trip failure is an **oracle
defect**: evaluating a later real-minus-complex subtraction changes the
imaginary component of an earlier sum. The primitive-only reproduction is:

```scheme
(import (scheme base) (scheme inexact) (scheme write))
(let* ((t (* (exp (* 0-2i (atan 1))) -2))
       (u -2) (sum (+ u t)) (difference (- u t)))
  (write (list sum difference t)))
```

Chibi prints `sum` with imaginary part -2, although it was +2 before the
subtraction; both Patina backends and Gauche preserve +2. The correct Fourier
expectation remains in the driver. This difference is recorded here and in
#525, outside the `tests/scheme` oracle lane; it is not a passing comparison.
Drivers and their test-only `(patina compat smoke)` helper live outside `compat/vendor/`;
nothing is bundled or added to the upstream extractions.
For the SRFI comparisons, temporary library declarations and imports
were renamed into `(patina-corpus srfi ...)`, with implementation bodies
identical to the patched corpus: search paths alone let preloaded reference
libraries interfere.
For SRFI 38, the staged declarations also select the portable `38.scm` branch
on Chibi, so all four comparisons exercise the same implementation. The
separate `38.chibi.scm` branch is unchanged and was not tested by this batch.
SRFI 42's staged `.sld` also replaces its inert `(else #f)` library declaration
with `(else)` because Gauche rejects that declaration. This staging-only
adaptation also applies to the dependency used by SRFI 78; the `.scm`
implementation bodies remain identical to the patched corpus.
For SRFI 166, all five sublibraries and the included `(chibi show shared)`
helper are isolated as well. Staged declarations select the portable
optional-argument branches, and all four comparisons stage isolated copies
of `lib/srfi/165` and `test-lib/chibi/optional`. Gauche lacks SRFI 165 and cannot
initialize the formatter's 21 defaults with its multiple-value limit. The
staged SRFI 165 macro therefore collects defaults in a vector and binds each
temporary by index, preserving simultaneous evaluation; the SRFI 166
implementation bodies remain identical. The normal Patina corpus runs use
the existing dependencies without this reference adaptation.
Chibi lacks SRFI 60, so its JSON and PFDS runs used the portable `lib/srfi/60.sld`
facade over its native SRFI 151; Gauche used its native SRFI 60.
The SLIB numeric/array comparisons isolate the selected SLIB libraries and
`(slib common)`, plus the pinned SRFI 63 declaration and all its imports to
preserve array-record identity. All four comparisons use the same isolated
portable SRFI 60 facade over native SRFI 151. Only library declarations and
imports are renamed; the implementation bodies match the patched corpus.
The byte/text comparisons use the same SRFI isolation and also rename every
transitive SLIB dependency. The common/directory adapters retain their native
conditional branches; URI's absolute-path test performs no filesystem calls.
Patina receives `test-lib` and the corpus Chibi pathname dependency, while the
references use their native directory support. No implementation body is
adapted for these reference comparisons beyond the documented overlays.

The color/scientific comparisons also isolate pinned SRFI 95, including its
`.scm` implementation and SRFI 63 imports. They stage `lib/srfi/27.sld` and
`27.scm` unchanged apart from renaming the declaration/imports to
`(patina-corpus srfi 27)` on all four hosts. Chibi's native C aliases and state
behavior otherwise interfere with replacing `random-real`; these comparisons
measure the SLIB transformations over the same portable SRFI 27 dependency
used by Patina. All SLIB implementation bodies match the corpus with its
named overlays. Before-repair measurements retain previously merged dependency
overlays and omit only the three #525 overlays.

The SLIB time/helper comparisons isolate all transitive SLIB dependencies and
pinned SRFI 63, using the same portable SRFI 60 facade over native SRFI 151
as the earlier SLIB batches. Native common/directory conditional branches
remain selected without launching processes. Only declarations/imports are
renamed; implementation bodies match the corpus overlays. Before-repair runs
retain prior dependency repairs and omit only the six #527/#528 overlays.
The eight drivers pass 107/140 assertions before repair on both Patina
backends and Chibi, and 105/140 on Gauche; all four pass 140/140 afterward.
Gauche alone exposes the single-value binding in `call-with-open-ports`;
the other hosts tolerate it. The repaired wrapper forwards every value on
all four while retaining the upstream normal-return port closure contract.

The text/parsing reference runs rename all transitive pinned libraries into
`(patina-corpus ...)`, including off-path irregex and the Chibi iset/char-set
record types. Chibi's native full/ascii support tables are copied under the
same namespace with imports renamed so their records share that identity;
these support tables are unchanged. Native conditional branches remain
selected. Boundary consumers choose Chibi char-sets on Chibi and SRFI 14
elsewhere, preserving #431. Before-repair runs retain all previously merged
overlays, including #431 and the regexp suite's missing-data guard, and omit
only the new #530/#531 corrections. The pinned implementation bodies match
the normal corpus runs.

The utility reference runs use the same renamed corpus namespace. The
`chrisoei-test` and `lightweight-testing` drivers additionally stage the pinned
`test-lib/chibi` test framework with diff, optional and terminal-color support
under its ordinary names on all hosts; only reporter output is captured,
while real assertion evaluation and failure accounting remain active.
Before overlays, config reaches 22/26, generators 31/35, and tar 12/15 on
all four hosts; the other four drivers pass unchanged. The config cascade
raises on Patina/Gauche but loses outer precedence on Chibi. Tar was measured
on Patina after #533's runtime repair, independently of its padding overlay.
Four further config regression assertions bring its final count to 30.
No vendor file or reference interpreter was modified.

The final wrapper references retain native host branches. Directory's
initial 11-row driver passes nine rows on Patina/Chibi; Gauche reaches one
pass and three failures before failed directory creation prevents the rest.
The line editor's portable library declaration fails on Gauche and is warned
and ignored on Patina; Chibi passes its initial 15 rows. PSTk reproduces a
false-widget result and then cannot construct a second widget on all four
hosts. The repaired drivers pass 11/16/23 rows respectively. The final editor
row exercises the portable no-op terminal setup on Patina/Gauche and bypasses
native Chibi stty explicitly; no actual terminal mode is changed.

The temporary-directory driver passes eight rows unchanged on Patina and
Chibi. Gauche lacks `(chibi filesystem)`, so its separate reference staging
maps only directory creation, directory predicates and recursive removal to
Gauche's `sys-mkdir`, `file-is-directory?` and `remove-directory*`, and stages
the pinned `test-lib/chibi/string` dependency. The pinned temp-file body and
pathname dependency remain unchanged; descriptor entry points in that adapter
raise and are never called. All eight directory rows then pass on Gauche.
This adapter is a reference-only comparison of the directory wrapper, not a
claim that Gauche natively supplies the Chibi library.

**Coverage boundaries after #429:** `call-with-temp-file` still needs raw
POSIX descriptors unavailable in Patina; temporary-directory tests cover
normal-return cleanup and preservation, not cleanup on arbitrary escapes.
Filesystem mode arguments are accepted but Patina's VFS does not implement
permissions. The line editor runs with string ports and an explicit width;
live tty behavior remains unmeasured. PSTk's driver temporarily replaces its
exported `tk-eval` transport binding, restores it afterward, and measures real
command builders/cache behavior; it does not launch Tcl/Tk, exercise pipes,
or claim GUI integration. These packages' smoke passes are deliberately
narrower than full API support.

**Optional Chibi-suite audit (2026-09-28):** the pinned corpus already holds
29 Chibi `*-test.sld` modules: 22 run and pass as package suites; seven are
covered by the existing exclusions (assert, voting, mecab, DNS, SMTP, SSL and
XGBoost). Separately, four version-matched Chibi suites (string, optional,
diff and terminal ANSI) run in `upstream_srfi_suites.rs`; the fifth staged
suite, filesystem, is explicitly disabled because it opens a raw descriptor
before reaching its portable directory assertions. Its existing rationale
is in `scheme_tests/upstream/README.md`.

The external Chibi checkout at `bb9b3215e52bd29cecdfa3ce37cd97721f0c2cc0`
contains 43 `*-test.sld` modules (excluding `(chibi test)`, the framework),
rather than treating the old roughly-50 estimate as a current count. Nineteen
match corpus suite names, five match the separately staged suites, and 19
are additional: binary-record, csv, doc, generic, io, json, log, loop, memoize,
numeric, process, pty, shell, show/c, sxml, syntax-case, system, text and weak.
Those additional files are not version-matched tests from the pinned
snowballs. This audit does not import a moving external checkout into CI.
Future conformance expansion can pin and evaluate them separately, starting
with overlapping smoke subjects such as binary-record and sxml. #429 closes
the current corpus's import-only coverage gap; it does not assert complete
conformance or remove the existing FFI/licence/upstream exclusions.

CI runs all active smoke drivers on both backends, using the release build
from the R7RS compliance job. The same gates can be run locally:

```bash
./target/release/patina-compat check-smoke
./target/release/patina-compat check-smoke --tree-walker
```

`check-smoke` discovers its selection from the manifest, keeps the full
corpus available for dependency resolution, and requires every selected
package to finish in smoke mode with a passing result. It exits 0 on success,
1 for a failed or incomplete driver, and 2 for setup errors (including no
active drivers, invalid registrations, or failed artifact writes). It rejects
filters and exclusions. `run` remains a measurement command: package failures
are recorded in its results without making its exit status fail.

The smoke gate prints its report but writes artifacts only when `--results`
and/or `--report` explicitly names a destination. CI supplies separate paths
under `$RUNNER_TEMP` for each backend; these subset measurements never replace
the committed full-corpus reports by default.

Execution prefers the package's own test program, then a registered smoke
driver, then the import probe. Smoke mode retains the probe's imports of every
provided library under unused prefixes before evaluating the driver's own
forms. It uses the same patched/off-path package staging, dependency closure,
isolated library paths, scratch working directory and timeout as other modes.
Driver imports contribute test-only dependencies. The smoke directory is an
explicit library root for the assertion helper.

Each driver imports `(patina compat smoke)`, calls `check-equal` or
`check-error` with a label, and ends with `(smoke-finish)`. The helper emits
`(patina-compat-smoke PASSED FAILED)`. A run passes only if it exits cleanly,
reports no interpreter errors, and emits exactly one tally with zero failures
and the manifest's positive assertion count. Missing, duplicate or truncated
tallies fail, including an early successful exit. An assertion mismatch is
`wrong-result`; an incomplete run is `runtime-error`.

To add coverage, add `<slug>.scm` and its count to the manifest, compare the
assertions with the reference implementations against the same package
version, and run both Patina backends with `--filter <slug>` and explicit
output paths. Then remeasure the corpus. A custom `--vendor` directory uses
its sibling `smoke/` directory when present, just as patches live in its
sibling `patches/`; once present, the smoke manifest and registered drivers
must be valid. Missing drivers, duplicate or stale slugs, and nonpositive
counts stop discovery rather than silently dropping to probe mode.

New snapshots carry `(measured-at "2026-09-19T08:09:10Z")`: the UTC start time
of the corpus run, in RFC 3339 format at second precision. The report prints
that same value as **Measured**. `cargo run --release -p patina-compat -- report`
only re-renders saved results; it preserves their measurement time. Legacy
snapshots without this optional version-1 field remain readable and report
their measurement time as unknown. Re-run the corpus to date them; do not
stamp an old snapshot with today's date.

The timestamp identifies when the run happened, not the tested binary's
source revision or the corpus revision. Those are not recorded by the
snapshot format. A corpus number should carry its measurement date, and a
change to any `(scheme …)` export list is a reason to re-measure.

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
        ├── scheme/        # *.scm suite files, run against each backend
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
      ├─ patina-primitives (shared datum writer)
      ├─ patina-vm (optional)
      ├─ patina-tree-walker (optional)
      └─ patina-runtime

patina-pipeline → patina-interpreter (legacy-pipeline feature, no defaults)
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

## Local build performance (measured 2026-09-11)

Historical measurements from the agent instructions, retained for troubleshooting.
These timings describe one machine and checkout; diagnose current slow builds
before cleaning artifacts, and avoid cleaning a target directory another session uses.

**Locally, the whole Rust gate costs well under a minute — when `target/` is
healthy.** Measured 2026-09-11 on 10-core Apple silicon, 51 test binaries,
after touching `patina-vm/src/runtime/vm_state.rs`, which every test binary
links, so this is the worst realistic case:

| Command | Time |
|---|---|
| `cargo build --release` — the repro, the chibi lanes, the benchmarks | **3.2 s** |
| `cargo test -p patina-tests --test <one file> --no-run` | **0.5–5.5 s** |
| `cargo test --all --lib --tests` | **2.5 s** to build, **29 s** to run |
| `cargo clippy --all-targets --all-features` | **1.3 s** |
| any of them again with no edit in between | 0.1 s |
| from `cargo clean`: release, all tests, clippy | 11 s, 28 s, 6 s |

So run the full local gate whenever you want the answer before CI has it —
writing a PR description that states a result, bumping `rust-toolchain.toml`,
or a change whose blast radius you cannot bound. Clippy and `cargo test` do not
evict each other's artifacts, and the workspace's only non-default feature
(`patina-tree-walker/verbose-tracing`) gates no code, so neither is a reason
to rebuild (measured 2026-09-06).

**When those numbers are 100× worse, `target/` has rotted: run `cargo clean`.**
On macOS, cargo's default `split-debuginfo = "unpacked"` leaves each test
binary's debug info in its object files beside it in `target/debug/deps`, and
nothing deletes the old ones, so every full test build leaves about 1,100
`*.rcgu.o` files behind. By 2026-09-11 the main checkout held 1.9 million of
them (69 GB on disk). rustc scans that directory on every invocation, so a
full test rebuild took **220 s** and clippy **220 s** where a healthy tree
takes 2.5 s and 1.3 s. Deleting `target/debug/incremental` alone did not help.

That rot, not the number of test binaries, is what this section used to
measure — 493 s for `cargo test`, 580 s for clippy, explained as "87 binaries
× ~6 s". An A/B on one machine settled it: in fresh target directories the
tree before #193 rebuilt its 87 test binaries in 4 s and today's tree its 51 in
2 s, while today's tree took 220 s in the rotted directory. A test binary costs
about 0.05 s per full rebuild.

Check with `find target/debug/deps -name '*.o' | wc -l`: thousands are
normal, a million is the state above. `cargo clean` took 256 s to delete 1.9
million files, and everything rebuilds from scratch in about 45 s. The root
fix would be `split-debuginfo = "packed"` in `[profile.test]`, which leaves no
object files, but `dsymutil` then runs for every binary and a full test
rebuild takes 9.8 s instead of 2.5 s — measured, not adopted.

**#193's outcome, corrected.** It took the workspace from 87 test binaries to
**51** (`find crates -path '*/tests/*.rs' -not -path '*/tests/*/*' | wc -l`),
across 56 suite files and 1757 rows. Its build-time case was the rotted
directory, so the binaries it removed saved about 2 s a rebuild, not minutes.
On a healthy target a test is cheap to add in either form: a row in an
existing `.scm` file needs no rebuild (0.1 s), a new `.scm` file needs a
`SUITE` entry and so recompiles the driver (1.0 s), and a new `.rs` file
compiles and links in 0.4 s. What #193 bought is the rest of its case: one
suite that every backend runs, and chibi and Gauche arbitrating every row
through `crates/patina-tests/tests/scheme/DIVERGENCES.tsv`.
