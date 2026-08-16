# Provenance of byte-identical trees under `lib/srfi/`

Libraries whose bundled files are **all** byte-identical to upstream are
recorded here — with no local edit, there is no file to carry an in-file
note. Libraries that do deviate document themselves in their own `.sld`
header instead (`lib/srfi/132.sld` is the model), and `lib/chibi/` has its
own record. One home per tree.

| Package | Version | Files | Upstream | Tarball sha256 |
|---|---|---|---|---|
| `(srfi 14)` | 0.1.0 | `14.sld`, `14.scm` | snow-fort, Retropikzel's R7RS port of Olin Shivers' char-set reference implementation (MIT-Scheme-old) | `de94f90d7b032ea554ed51b4cbce942b22df6fefe68497d661b9c26e3c7e690e` |
| `(srfi 125)` | chibi 0.12.0 | `125/hash.scm` | chibi-scheme's `lib/srfi/125/hash.scm` (Alex Shinn, BSD 3-Clause) | file sha256 `d469201d00fa0b955ba23a01e02707034dad5ba2ef1a29cd7b12b880e76f1053` |
| `(srfi 27)` | 2025.12.14 | `27.sld`, `27.scm` | snow-fort, Retropikzel's R7RS port of Sebastian Egner's 54-bit MRG32k3a reference implementation (MIT) | `b8d2322e40955ccc986e9b0b10c1c36044ff5c659d33698722f9b36ec77fdea5` |

Tarball URLs:
`http://snow-fort.org/s/iki.fi/retropikzel/srfi/14/0.1.0/srfi-14-0.1.0.tgz` and
`http://snow-fort.org/s/iki.fi/retropikzel/srfi/27/2025.12.14/srfi-27-2025.12.14.tgz`
(the compat corpus vendored the same tarballs until Patina bundled them; the
corpus drops packages Patina provides). Each `.scm` sits beside its `.sld`
rather than in a numbered subdirectory so the `(include "…")` resolves
unchanged.

`(srfi 125)` is the one entry not from a snow-fort tarball: snow-fort has no
SRFI 125 package, so `125/hash.scm` is taken from the pinned chibi checkout and
recorded by file digest instead. Only that file is upstream's — `125.sld` is
Patina's own, because upstream's imports `(chibi ast)` and relies on chibi's
C-backed SRFI 69; the three resulting deviations are documented in its header,
which is where this tree records deviation.

`(srfi 69)` is **not** byte-identical and is deliberately absent from the table
above: `69/srfi-69-impl.scm` carries one marked local fix, `PATINA DEVIATION` at
the `real?` branch of `hash`. R7RS makes `numerator`/`denominator` return
inexact results for inexact arguments, so upstream's expression produced an
inexact hash, which `vector-ref` then rejected as an index — any float key
crashed the table. Upstream does not hit it because the implementations it
targets return exact hashes.

## Licences

`(srfi 14)` needs nothing added here: `14.scm` carries its own attribution
chain (MIT Scheme → Brian D. Carlstrom → Olin Shivers → Retropikzel) *and* the
full MIT Scheme 1988–1995 licence text at the end of the file, exactly as
upstream ships it.

`(srfi 125)`'s `hash.scm` carries no in-file notice, as chibi's own files
mostly do not; it is Alex Shinn's under the BSD 3-Clause licence reproduced in
full in `lib/chibi/PROVENANCE.md`, which is the same licence and the same
author as every file in that tree.

`(srfi 27)` does not. `27.scm` carries a single author line —
`Sebastian.Egner@philips.com, Mar-2002` — and no terms at all, upstream's own
state, not something removed here. Its licence requires that "the above
copyright notice and this permission notice shall be included in all copies",
so the text is reproduced below rather than linked. Taken verbatim from the
SRFI 27 document at <https://srfi.schemers.org/srfi-27/srfi-27.html>.

```
Copyright (C) Sebastian Egner (2002). All Rights Reserved.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

The adapted ports named under **The boundary** below carry their upstream
notices in-file where upstream has one; `lib/srfi/132.sld` is the model for
recording a tree whose licence lives in a per-file notice, including its note
that `select.scm` has none upstream either. `lib/srfi/130.sld` records the one
BSD-licensed port, whose text is in `lib/chibi/PROVENANCE.md` § Licence — same
author, same licence.

`(srfi 14)` was bundled 2026-08-14 as a dependency of `(chibi string)`, which
`(srfi 130)` is written against — it is imported for exactly two names,
`char-set?` and `char-set-contains?`. That is a thin use of a large library,
but the alternative was a hand-maintained subset, and SRFI 14 was already an
L1 bundling target on its own in-degree.

The package ships no test suite; conformance is covered by
`crates/patina-tests/tests/srfi_27.rs` (both backends must agree on exact
pseudo-randomized streams) and by the SRFI 132 suite, whose quickselect
draws its pivots from `random-integer`.

The rule and its enforcement: `lib/chibi/PROVENANCE.md` § The rule, and
`crates/patina-tests/tests/bundled_provenance.rs`, whose `PINNED` table is
the authoritative scope.

**The boundary:** the adapted ports elsewhere in this tree (SRFI 1, 69, 113,
128, 133, 158, …) are *not* byte-identical to any upstream and are
deliberately unpinned; their sources and known deviations are Track L
territory (`PRD/TRACK_L_SNOW_LIBRARIES_PRD.md`) until each is reconciled to
this standard or recorded here.
