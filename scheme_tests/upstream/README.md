# Upstream test suites

The reference test suites for the libraries Patina bundles — SRFIs and
`(chibi …)` libraries — run against Patina's implementations by
`crates/patina-tests/tests/upstream_srfi_suites.rs`.

These are the specification authors' own tests, not ours. That is the point:
a hand-written test only checks the cases its author thought of, and the author
here is the same person who wrote the implementation. `srfi/151/test.sld` alone
is 145 assertions against the 13 in
`crates/patina-tests/tests/srfi_151_bitwise.rs`.

The SRFI suites are from chibi-scheme's `lib/`; the `chibi/` suites are from
the same sha256-pinned snowballs the bundled libraries themselves came from
(`lib/chibi/PROVENANCE.md`), so each suite is version-matched to the code it
tests. Copied unmodified except where the table's note column says otherwise —
every adaptation is described under the table. All of them report through
`(chibi test)`, which Patina bundles verbatim from the snow-fort 0.9.0
snowball (sha256-pinned in `lib/chibi/PROVENANCE.md`, guarded by
`bundled_provenance.rs`); running these suites at all is a consequence of
that adoption, since the hand-written subset it replaced could not express
`test-group` or report a failure count.

| Suite | Assertions | Failing | Adapted? |
|---|---|---|---|
| `srfi/151/test.sld` | 145 | 0 | verbatim |
| `srfi/143/test.sld` | 141 | 0 | verbatim |
| `srfi/132/test.sld` | 221 | 0 | verbatim |
| `srfi/133/test.sld` | 93 | 0 | verbatim |
| `srfi/113/test.sld` | 253 | 0 | verbatim |
| `srfi/128/test.sld` | 170 | 0 | verbatim |
| `srfi/130/test.sld` | 219 | 0 | imports |
| `srfi/158/test.sld` | 76 | 0 | verbatim |
| `srfi/125/test.sld` | 74 | 0 | imports |
| `srfi/14/test.sld` | 72 | 0 | verbatim |
| `srfi/41/test.sld` | 186 | 0 | verbatim |
| `srfi/117/test.sld` | 34 | 1 | verbatim — the one failure is the suite's, not ours; see below |
| `srfi/127/test.sld` | 109 | 0 | verbatim |
| `srfi/116/test.sld` | 196 | 0 | verbatim |
| `chibi/string-test.sld` | 52 | 0 | verbatim |
| `chibi/optional-test.sld` | 11 | 0 | imports |
| `chibi/diff-test.sld` | 7 | 0 | imports |
| `chibi/term/ansi-test.sld` | 234 | 0 | framework shim |

**The chibi rows exist because of a hole the 2026-08-19 audit found.** Each
snowball ships its suite, and while the packages were vendored in
`compat/vendor/` those suites ran in the compat harness. When the corpus
builder started excluding packages Patina bundles (Track L §L4), the suites
left with them — and nothing re-added them here, so the bundled libraries'
upstream tests ran *nowhere*. "Add a suite when Patina bundles the library"
was prose; it is now enforced by
`every_bundled_library_has_a_suite_or_a_recorded_reason` in the same test
file — how it decides is described under "Suites not included" below.

**`chibi/diff-test.sld` and `chibi/optional-test.sld` are adapted in their
imports only**: upstream wraps its `(chibi test)` import in
`(cond-expand (chibi …) (else …))` where the else branch inlines a minimal
framework shim "to avoid circular dependencies in snow installations". Patina
bundles `(chibi test)` but does not advertise the `chibi` feature, so the
else branch would win — and the shim neither counts failures nor reports
through `current-test-reporter`, which the counting harness requires (in
optional-test the shim's own `test-error` also lacks the two-argument form
the suite's body uses, so it cannot even expand). The cond-expand is resolved
by hand to upstream's own chibi branch: `(import (chibi test))`. Test bodies
untouched.

**`chibi/term/ansi-test.sld` goes one step further**: it has no chibi branch
at all — the framework shim is unconditional. Its five framework definitions
(`test`, `test-assert`, `test-error`, `test-begin`, `test-end`) are replaced
by `(import (chibi test))`; the suite's two domain-specific helper macros and
every test body are untouched.

**`srfi/125/test.sld` is adapted too**, in two places, test bodies untouched.
Its imports exclude `string-hash` and `string-ci-hash` from `(srfi 128)` so
they resolve to SRFI 125's — the tests call them with a bound, which is SRFI
69's convention and what SRFI 125 re-exports, while SRFI 128's take one
argument. Upstream needs no exclusion because chibi binds a single native
procedure under both names, so the ambiguity never arises there; without the
exclusion two assertions error.

It used to carry a second adaptation as well: five comparator constants
(`default-comparator`, `eq-comparator`, `eqv-comparator`, `string-comparator`,
`string-ci-comparator`) defined in the test file because they belong to SRFI
162, which was not bundled. **Retired 2026-08-23** — Patina exports SRFI 162
from `(srfi 128)`, where SRFI 162 asks implementers to put it, so the suite
imports them like upstream does. Worth stating why that is better and not
merely shorter: a constant re-derived inside the test file is not the one the
library builds, so those five assertions were testing a local copy rather than
the code under test. The suite still reports 74 of 74.

**`srfi/130/test.sld` is the other adapted suite.** Upstream imports
`(chibi char-set)` and `(chibi char-set full)`; the latter imports `(chibi)`,
chibi's implementation core, which Patina does not provide — the same wall that
keeps the SRFI 1 and SRFI 69 suites out below. But the suite uses only six
char-set names (`char-set`, `char-set-complement`, `char-set-contains?`,
`char-set:lower-case`, `char-set:letter`, `char-set:digit`), all of them
standard SRFI 14, which Patina now bundles. So the two imports are replaced by
`(srfi 14)` and nothing else changes: the diff against upstream is one line, and
no test body is touched. Recorded here rather than done silently, because the
value of these suites is that they are not ours to edit.

Which exposes something worth fixing: **nothing mechanically guards this tree.**
`lib/` has `crates/patina-tests/tests/bundled_provenance.rs` hashing every
bundled file against a recorded pin, so an unrecorded edit fails the suite. The
claim above is prose only, and always has been — true for the six suites here
before this one. It mattered less while "copied unmodified" was exceptionless;
now that there is one adaptation, a second could arrive without disclosure and
nothing would notice. The fix is the mechanism `lib/` already has: pin these
`test.sld` files by hash too, with `srfi/130/test.sld` pinned at its adapted
hash, exactly as `lib/srfi/130.scm` is pinned post-edit.

Both columns are enforced by `upstream_srfi_suites.rs`, not prose: the failure
count is asserted exactly, in both directions — a regression fails, and so does
a fix until the number is lowered — and the assertion count is a floor, so a
run that `TEST_FILTER`-skips its way to zero failures also fails. An
expectations table, not a skip list. Every suite runs on both backends.

## What running them found

Eight defects, none of which the hand-written tests beside these libraries had
caught. All are now fixed; the newest is first.

- **SRFI 14 `ucs-range->char-set` discarded its base set** — fixed 2026-08-19,
  found by `srfi/14/test.sld` on its first run. The port handed
  `%default-base` the extracted char-set where every other caller hands it
  the maybe-base *rest list*; the helper's `pair?` test — its way of telling
  "given" from "defaulted" — is false for a char-set record, so the base
  silently defaulted to empty:

  ```scheme
  (ucs-range->char-set 97 103 #t (string->char-set "12345"))
  ;; was  => chars a-f only
  ;; SRFI => chars a-f plus 1-5
  ```

  The `!` variant was unaffected (it takes the base positionally and mutates
  it). One more instance of the pattern this file exists for: the defect sat
  in the one procedure whose optional-argument handling was rewritten rather
  than kept in the reference's shape.

- **SRFI 113 `set-unfold` / `bag-unfold` took their arguments in the wrong
  order** — fixed. The SRFI orders them `(comparator stop? mapper successor
  seed)`; Patina had the comparator last and inverted the predicate's sense,
  naming it `continue?`. Any caller written against the spec got an
  argument-order error.

- **Higher-order Rust primitives were not continuation-safe** — fixed.
  `string-for-each`, `string-map`, `vector-for-each` and `vector-map` called
  back into Scheme from inside a Rust frame, and a continuation captured there
  did not survive: `(make-for-each-generator string-for-each "abc")` silently
  dropped its first element. `map` and `for-each` over lists were already
  defined in Scheme and unaffected, which is why the bug hid — the obvious
  spot-check works. All four now live in `lib/scheme/base/higher_order.scm`
  alongside them. This also removed a VM/tree-walker divergence in SRFI 158.

- **SRFI 132 sort — three defects, all in a hand-written port.** Patina's
  `(srfi 132)` was a ~100-line port rather than the reference implementation,
  and each defect was ours alone — no spec ambiguity, nothing another R7RS
  implementation would hit:

  1. `list-stable-sort` was **not stable** — the one property distinguishing it
     from `list-sort`. The merge took a cell from `ls2` and then recursed as
     `(%list-merge less (cdr ls2) ls1)`, putting `ls2`'s remainder in the `ls1`
     position. Ties are broken in favour of `ls1`, so swapping the arguments
     swapped which input wins them, and equal elements came back reordered.
  2. `list-merge!` dropped elements: `(list-merge! > '(9 7 5 3 1) '(9 6 3 0))`
     returned five instead of nine, because its splice loop discarded the
     result of one recursive call.
  3. `vector-merge` raised `unbound variable: cadddr` — its optional-argument
     parsing used `caddr`/`cadddr`, which `(scheme base)` does not export. This
     is the defect that kept the suite from finishing, so it was previously
     recorded here as the *suite's* missing `(scheme cxr)` import. It was ours.

  Rather than patch a port that had three bugs in a hundred lines, `(srfi 132)`
  is now Olin Shivers' reference implementation with John Cowan's SRFI 132
  modifications, unmodified from
  [srfi-132](https://github.com/scheme-requests-for-implementation/srfi-132).
  Patina already bundles his SRFI-1 reference the same way. It needed one
  two-line `assert` shim and nothing else, and passed this suite on the first
  run — which is the argument for using it: the sort algorithms are not where
  Patina should be spending its own correctness budget.

  Both were checked against the suite. Reintroducing defect 1 into the old port
  turned the suite red with 2 failures, so the 0 in the table is not vacuous.

- **The standard port procedures were not parameter objects** — fixed 2026-08-15, and the reason
  this table has no non-zero row left. R7RS §6.13.1 requires
  `current-input-port`, `current-output-port` and `current-error-port` to be
  parameter objects overridable with `parameterize`; Patina implemented all three
  as plain 0-argument procedures, so `parameterize` failed on them:

  ```scheme
  (parameterize ((current-input-port (open-input-string "a b c"))) (read))
  ;; was => Invalid syntax: current-input-port expects exactly 0 arguments, got 1
  ```

  `make-parameter` and `parameterize` were fine for user-defined parameters; only
  the three built-in ports sat outside the machinery. This was the SRFI 158 suite's one
  failure — it defines its own `with-input-from-string` in exactly these terms,
  so it was a recorded Patina defect and not, as this file once said, a chibi
  extension we declined to provide.

**`srfi/117/test.sld`'s one failure is the suite's own.** It calls
`(list-queue-append! x …)` and then asserts `x` is unchanged, where SRFI 117
says of that procedure: "it is an error to assume anything about the contents
of the list-queues after the procedure returns". Patina ships the SRFI's
reference implementation, which reuses the storage the specification frees it
to reuse; Larceny's suite makes no such assumption and passes 40 of 40. This
is the only row whose non-zero failure count is not a defect in our port, and
`upstream_srfi_suites.rs` says so at the row.

## Suites not included, and why

The authoritative lists are the `NO_SUITE` (per library) and
`NO_SUITE_TREES` (per `lib/` tree) tables in `upstream_srfi_suites.rs` — the
guard test fails if they and the suite table together do not account for
every `.sld` under `lib/`, so a newly bundled library or tree is in scope by
default. The non-obvious entries:

- **SRFI 1, SRFI 69** — their suites import `(chibi)`, chibi's implementation
  core, which Patina does not provide.
- **SRFI 27** — its suite imports `(scheme flonum)`, i.e. SRFI 144, which
  Patina does not bundle yet. Add the suite when SRFI 144 lands.
- **`(chibi filesystem)`** — its suite opens a raw file descriptor
  (`(open tmp-file open/write)`) before its directory tests, hitting the
  bundled library's FFI stub *outside any test form*, which aborts the run:
  4 of its assertions pass and the directory half — the part Patina actually
  implements — is never reached. `chibi/filesystem-test.sld` is staged here
  verbatim (it is in no table row and nothing runs it) so that when FFI
  lands, enabling it is one `suite_tests!` row.

(An earlier note here said `(chibi optional)`'s suite "fails to desugar with
'Parameter must be a symbol, got pair'". Re-run 2026-08-19: the actual
blocker was upstream's inline framework shim, whose `test-error` lacks the
two-argument form the suite uses — with the imports adapted to the real
`(chibi test)`, the suite runs clean. The suite is now included.)

Kept here rather than under `lib/` so the shipped library tree stays free of
test code. The directory is a library search root: `(srfi 151 test)` resolves to
`srfi/151/test.sld` beneath it.

## Licence

Every suite here is by Alex Shinn — the SRFI suites from chibi-scheme's
`lib/`, the `chibi/` suites from his snow-fort snowballs — under the BSD
3-Clause licence in chibi's `COPYING`. None of the files carries an in-file
notice — upstream's own state — and the licence's first condition requires that
redistributions "retain the above copyright notice, this list of conditions and
the following disclaimer". Reproduced here rather than named, verbatim from
`COPYING` at chibi 0.12.0, because naming a licence is not retaining its notice.

```
Copyright (c) 2009-2021 Alex Shinn
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:
1. Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.
3. The name of the author may not be used to endorse or promote products
   derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```
