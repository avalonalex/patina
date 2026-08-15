# Upstream SRFI test suites

The reference test suites for the SRFIs Patina bundles, run against Patina's
implementations by `crates/patina-tests/tests/upstream_srfi_suites.rs`.

These are the specification authors' own tests, not ours. That is the point:
a hand-written test only checks the cases its author thought of, and the author
here is the same person who wrote the implementation. `srfi/151/test.sld` alone
is 145 assertions against the 13 in
`crates/patina-tests/tests/srfi_151_bitwise.rs`.

All from chibi-scheme's `lib/`, BSD (Alex Shinn), copied unmodified except
`srfi/130/test.sld`'s import list — see the note under the table.

| Suite | Assertions | Failing |
|---|---|---|
| `srfi/151/test.sld` | 145 | 0 |
| `srfi/143/test.sld` | 141 | 0 |
| `srfi/132/test.sld` | 221 | 0 |
| `srfi/133/test.sld` | 93 | 0 |
| `srfi/113/test.sld` | 253 | 0 |
| `srfi/130/test.sld` | 219 | 0 |
| `srfi/158/test.sld` | 76 | 0 |

**`srfi/130/test.sld` is the one adapted suite.** Upstream imports
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

Seven defects, none of which the hand-written tests beside these libraries had
caught. All are now fixed; the last is recorded below.

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

## Suites not included, and why

- **SRFI 1, SRFI 69** — their suites import `(chibi)`, chibi's implementation
  core, which Patina does not provide.
- **SRFI 128** — its suite does not parse here; worth investigating whether that
  is our reader or chibi-specific syntax.
- **`(chibi optional)`** — its suite fails to desugar with "Parameter must be a
  symbol, got pair", which looks like a Patina defect in formals handling rather
  than a test problem. Also worth investigating.

Copied unmodified. They import `(chibi test)`, which Patina bundles verbatim
from the snow-fort 0.9.0 snowball — byte-identical to the sha256-pinned
tarball recorded in `lib/chibi/PROVENANCE.md`, and guarded by
`crates/patina-tests/tests/bundled_provenance.rs`. Running these suites at all
is a consequence of that adoption, since the hand-written subset it replaced
could not express `test-group` or report a failure count.

Kept here rather than under `lib/` so the shipped library tree stays free of
test code. The directory is a library search root: `(srfi 151 test)` resolves to
`srfi/151/test.sld` beneath it.

Add a suite when Patina bundles the library it tests.
