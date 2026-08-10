# Upstream SRFI test suites

The reference test suites for the SRFIs Patina bundles, run against Patina's
implementations by `crates/patina-tests/tests/upstream_srfi_suites.rs`.

These are the specification authors' own tests, not ours. That is the point:
a hand-written test only checks the cases its author thought of, and the author
here is the same person who wrote the implementation. `srfi/151/test.sld` alone
is 145 assertions against the 13 in
`crates/patina-tests/tests/srfi_151_bitwise.rs`.

All copied unmodified from chibi-scheme's `lib/`, BSD (Alex Shinn).

| Suite | Failing assertions |
|---|---|
| `srfi/151/test.sld` | 0 — 145 assertions |
| `srfi/143/test.sld` | 0 — 141 assertions |
| `srfi/132/test.sld` | 0 |
| `srfi/133/test.sld` | 0 |
| `srfi/113/test.sld` | 0 |
| `srfi/158/test.sld` | 1 — the suite calls `with-input-from-string`, a chibi extension Patina does not provide |

The counts are asserted exactly, in both directions: a regression fails, and so
does a fix until the number is lowered. An expectations table, not a skip list.

## What running them found

Six defects, none of which the hand-written tests beside these libraries had
caught. All are fixed.

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

## Suites not included, and why

- **SRFI 1, SRFI 69** — their suites import `(chibi)`, chibi's implementation
  core, which Patina does not provide.
- **SRFI 128** — its suite does not parse here; worth investigating whether that
  is our reader or chibi-specific syntax.
- **`(chibi optional)`** — its suite fails to desugar with "Parameter must be a
  symbol, got pair", which looks like a Patina defect in formals handling rather
  than a test problem. Also worth investigating.

Copied unmodified. They import `(chibi test)`, which Patina bundles verbatim
from upstream — running them at all is a consequence of that adoption, since
the hand-written subset it replaced could not express `test-group` or report a
failure count.

Kept here rather than under `lib/` so the shipped library tree stays free of
test code. The directory is a library search root: `(srfi 151 test)` resolves to
`srfi/151/test.sld` beneath it.

Add a suite when Patina bundles the library it tests.
