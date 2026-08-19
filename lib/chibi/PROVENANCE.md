# Provenance of `lib/chibi/`

Every file in this tree is **byte-identical** to its pinned upstream snowball
package (verified 2026-08-12 by diffing against the sha256-checked tarballs
below), with one exception: `filesystem.sld` carries a Patina `cond-expand`
branch, described at the end of this file. All are by Alex Shinn under the BSD
3-Clause licence, reproduced in full under [Licence](#licence) below.

Only `test.scm`, `string.scm` and `term/ansi.scm` carry an in-file copyright
notice — upstream's own files for `diff`, `optional`, `filesystem` and every
`.sld` have none, and we have not removed any. For those files **this record is
the only notice**, which is why the licence text lives here rather than behind
a link.

| Package | Version | Files here | Tarball sha256 |
|---|---|---|---|
| `(chibi test)` | 0.9.0 | `test.scm`, `test.sld` | `86997714be7fb6ade1b094d91727f9c9becd9051a41d1703986643d1ed09865d` |
| `(chibi diff)` | 0.9.1.3 | `diff.scm`, `diff.sld` | `07b62a03d280924f0bd42ca6375c752884a480779984dd7e9889e150f892fbac` |
| `(chibi optional)` | 0.9.1.3 | `optional.scm`, `optional.sld` | `30b58c0bbecbe37560fc24086417d2ab908536b74d8775670da55f1eb6971e9c` |
| `(chibi string)` | 0.9.0 | `string.scm`, `string.sld` | `86a73c53b2e7a4e1201ff10115a5488890993c0020051b3abe0fc785a077ec11` |
| `(chibi filesystem)` | 0.9.0 | `filesystem.sld` (+ local branch) | `dad608a7fbc00fe8e9929ff6124edad13bedfdc58cc14042be29b33f64c13483` |
| `(chibi term ansi)` | 0.9.0 | `term/ansi.scm`, `term/ansi.sld` | `805e33d6b87c6d54337bf0c89002f13c323e6d836291d2e88a252140d1552599` |

Tarball URLs follow the pattern
`http://snow-fort.org/s/gmail.com/alexshinn/chibi/<name>/<version>/chibi-<name>-<version>.tgz`
(for `term ansi`: `chibi/term/ansi/…/chibi-term-ansi-0.9.0.tgz`). These are
snow-fort snowball releases, older than chibi-scheme's git head — e.g.
`test.scm`'s copyright runs 2010-2020 — and the simplified SRFI-1 `any` at the
top of `test.scm` is upstream's own portability shim, not a local edit.

## Licence

All files in this tree are covered by the following, from chibi-scheme's
`COPYING`. It is reproduced here, not linked, because BSD 3-Clause condition 1
requires a source redistribution to retain the conditions and disclaimer — and
because a clone taken offline, or a link that rots, must still carry them.

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

Patina's own MIT licence covers Patina; it does not relicense anything in this
tree, and vendoring changes no terms. Condition 3 is why nothing in this
repository presents Alex Shinn or chibi-scheme as endorsing Patina — the
README's references describe what we test against, which is attribution, not
endorsement.

`(chibi string)` was bundled 2026-08-14 for `(srfi 130)`, which is written
against it; it arrived via the compat corpus's vendored copy of the same
snowball (`compat/vendor/` recorded it unmodified, and it is byte-identical to
chibi-scheme's own tree at `f266036`), so the corpus no longer carries it. Its
`cond-expand` takes the non-chibi branch here, where string cursors are plain
integers — the fast-random-access path the library was written to support.

Two known upstream defects are inherited, not local: `test.scm`'s
`string-search` misses last-position matches (affects `TEST_FILTER`-family
matching only), and `(chibi optional)` does not desugar under Patina (see
`scheme_tests/upstream/README.md`). Fixes belong upstream or in a
deliberately-marked local edit, not in silent patches.

## `(chibi filesystem)` — the one local branch (2026-08-14)

Upstream's `cond-expand` has branches for `chibi`, `chicken` and `sagittarius`
and **no `else`**, so on any other implementation the library loads defining
nothing and every importer fails on its first export. That is why five corpus
packages sat in the load-error bucket: not a missing primitive anywhere, a
missing branch. `filesystem.sld` therefore carries a `(patina …)` branch, marked
`;; PATINA LOCAL EDIT:` at its head. The rest of the file is upstream.

The branch is deliberately half an implementation, along a line the library's
own shape draws. The portable directory API — `directory-files`,
`create-directory`, `current-directory`, `directory-fold-tree`,
`with-directory` and friends — is implemented on primitives that route through
Patina's VFS `FileSystem` trait, so it behaves the same against an in-memory
filesystem. The POSIX layer — file descriptors, `stat` fields, symlinks, pipes,
permissions — is stubbed with upstream's own `define-unimplemented` idiom,
lifted from its sagittarius branch, which stubs the same fd procedures for the
same reason. Those need the FFI layer (`PRD/FFI_DESIGN.md`), not more Scheme,
and raise a marker string that `crates/patina-compat` classifies as
out-of-scope rather than as our defect.

`delete-file-hierarchy` is the branch's one deviation from upstream *behaviour*
(audit 2026-08-17, B1/B2). Upstream refuses `""` and `"/"` before touching
anything and honours an `ignore-errors?` argument; the branch had neither, so a
computed-empty path began a depth-first walk of the root. Both are restored, but
the tolerance is spelled with `guard` rather than upstream's return-value test,
because Patina's `delete-file`/`delete-directory` raise where chibi's return
`#f` (the same deviation `F1` records for the directory primitives generally).

`filesystem.scm` and `filesystem.stub` are **not** vendored. Both are reachable
only from the `chibi` branch, which also needs `include-shared` and the C shim,
so shipping them would put unreachable C-dependent Scheme in the library tree.

## The rule (audit 2026-08-10, group E)

Bundled library files **match upstream**. If a change is unavoidable, mark the
edit site with `;; PATINA LOCAL EDIT:` and record the deviation in the tree's
provenance home — this file, `lib/srfi/PROVENANCE.md`, or the library's `.sld`
header (as `lib/srfi/132.sld` does); one home per tree. Files claimed
byte-identical are pinned by
`crates/patina-tests/tests/bundled_provenance.rs` (its `PINNED` table is the
authoritative scope), so an unrecorded edit fails the suite.
