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
| `srfi/133/test.sld` | 0 |
| `srfi/113/test.sld` | **2** |
| `srfi/158/test.sld` | **1** on the tree-walker, **2** on the VM |

The counts are asserted exactly, in both directions: a regression fails, and so
does a fix until the number is lowered. An expectations table, not a skip list.

## What running them found

Four defects in Patina, none of which the hand-written tests next to these
libraries had caught:

- **SRFI 113 sets** — 2 failing assertions.
- **SRFI 158 generators** — fails 2 assertions on the VM but only 1 on the
  tree-walker. A backend divergence is worth more attention than the count.
- **SRFI 132 sort** — `list-stable-sort` is **not stable**: equal elements under
  the comparator come back reordered, which is the one property that
  distinguishes it from `list-sort`. `list-merge!` drops elements from its first
  argument entirely — `(list-merge! > '(9 7 5 3 1) '(9 6 3 0))` returns
  `(9 9 6 3 0)` instead of nine elements. `vector-merge` errors on empty vectors
  with explicit ranges.

## Suites not included, and why

- **SRFI 132** — its suite uses `cadddr` without importing `(scheme cxr)`, so it
  aborts partway and cannot produce a trustworthy count. The defects above were
  observed before it aborted and are real regardless.
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
