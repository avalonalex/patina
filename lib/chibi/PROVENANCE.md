# Provenance of `lib/chibi/`

Every file in this tree is **byte-identical** to its pinned upstream snowball
package (verified 2026-08-12 by diffing against the sha256-checked tarballs
below). All are by Alex Shinn, BSD-licensed
(<http://synthcode.com/license.txt>); `test.scm` and `term/ansi.scm` carry
in-file notices, `diff` and `optional` rely on this record.

| Package | Version | Files here | Tarball sha256 |
|---|---|---|---|
| `(chibi test)` | 0.9.0 | `test.scm`, `test.sld` | `86997714be7fb6ade1b094d91727f9c9becd9051a41d1703986643d1ed09865d` |
| `(chibi diff)` | 0.9.1.3 | `diff.scm`, `diff.sld` | `07b62a03d280924f0bd42ca6375c752884a480779984dd7e9889e150f892fbac` |
| `(chibi optional)` | 0.9.1.3 | `optional.scm`, `optional.sld` | `30b58c0bbecbe37560fc24086417d2ab908536b74d8775670da55f1eb6971e9c` |
| `(chibi term ansi)` | 0.9.0 | `term/ansi.scm`, `term/ansi.sld` | `805e33d6b87c6d54337bf0c89002f13c323e6d836291d2e88a252140d1552599` |

Tarball URLs follow the pattern
`http://snow-fort.org/s/gmail.com/alexshinn/chibi/<name>/<version>/chibi-<name>-<version>.tgz`
(for `term ansi`: `chibi/term/ansi/…/chibi-term-ansi-0.9.0.tgz`). These are
snow-fort snowball releases, older than chibi-scheme's git head — e.g.
`test.scm`'s copyright runs 2010-2020 — and the simplified SRFI-1 `any` at the
top of `test.scm` is upstream's own portability shim, not a local edit.

Two known upstream defects are inherited, not local: `test.scm`'s
`string-search` misses last-position matches (affects `TEST_FILTER`-family
matching only), and `(chibi optional)` does not desugar under Patina (see
`scheme_tests/upstream/README.md`). Fixes belong upstream or in a
deliberately-marked local edit, not in silent patches.

## The rule (audit 2026-08-10, group E)

Bundled library files **match upstream**. If a change is unavoidable, mark the
edit site with `;; PATINA LOCAL EDIT:` and record the deviation here (or in
the library's `.sld` header, as `lib/srfi/132.sld` does) — one home per tree.
`crates/patina-tests/tests/bundled_provenance.rs` pins every file in this
tree and in `lib/srfi/132/`, so an unrecorded edit fails the suite.
