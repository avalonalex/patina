# Upstream SRFI test suites

The reference test suites for the SRFIs Patina bundles, run against Patina's
implementations by `crates/patina-tests/tests/upstream_srfi_suites.rs`.

These are the specification authors' own tests, not ours. That is the point:
a hand-written test only checks the cases its author thought of, and the author
here is the same person who wrote the implementation. `srfi/151/test.sld` alone
is 145 assertions against the 13 in
`crates/patina-tests/tests/srfi_151_bitwise.rs`.

| Suite | Source | Licence |
|---|---|---|
| `srfi/151/test.sld` | chibi-scheme `lib/srfi/151/test.sld` | BSD (Alex Shinn) |
| `srfi/143/test.sld` | chibi-scheme `lib/srfi/143/test.sld` | BSD (Alex Shinn) |

Copied unmodified. They import `(chibi test)`, which Patina bundles verbatim
from upstream — running them at all is a consequence of that adoption, since
the hand-written subset it replaced could not express `test-group` or report a
failure count.

Kept here rather than under `lib/` so the shipped library tree stays free of
test code. The directory is a library search root: `(srfi 151 test)` resolves to
`srfi/151/test.sld` beneath it.

Add a suite when Patina bundles the library it tests.
