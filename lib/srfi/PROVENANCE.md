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
| SRFI 162 | — | `128/162-impl.scm` | the SRFI's own sample implementation, `https://srfi.schemers.org/srfi-162/srfi/128/162-impl.scm` (John Cowan, MIT) | file sha256 `973b7a5e6557ecfaa5e218a2d51e63077572235973f27c3db484ab5bac9513f2` |
| `(srfi 41)` | 0.1.0 | `41.sld`, `41.scm` | snow-fort, Retropikzel's R7RS port of Philip Bewig's stream reference implementation (MIT) | `c6bbc9b5d856f1ebdb3d9ce75d9542cc32050b13b82eb3e53fd9a8b96b67fdc2` |
| `(srfi 117)` | chibi 0.12-134-gf2660362 | `117.sld`, `117/queue.scm` | chibi-scheme's `lib/srfi/117.sld` and `lib/srfi/117/queue.scm` (Alex Shinn, BSD 3-Clause) | file sha256 `5e8c71fb0ccf7a7f501dae8c721b5652a994dca99b3579a58b66240f226f76ee`, `9962ef42db9494d19eeb5e98db3babe0e7ede4b32b0c8d678068dd3a86aed5dd` |
| `(srfi 127)` | chibi 0.12-134-gf2660362 | `127.sld`, `127.scm` | chibi-scheme's `lib/srfi/127.sld` and `lib/srfi/127.scm` (Alex Shinn, BSD 3-Clause) | file sha256 `60f374f4a4ac4bb46780e780f8c4c6dae073f31068b58188ad12f2a697c922a1`, `32d8c7b8646d023ae07be9fdc8584f671ac0e69a3cb5f7796590676067006a05` |
| `(srfi 41)`'s `stream-match` | chibi 0.12-134-gf2660362 | `41-match.scm` | chibi-scheme's **own** `lib/srfi/41.scm` — not the file of that name here (Alex Shinn, BSD 3-Clause) | file sha256 `01d33bc8f17a6b9bea94e73f6534bcaa474a6f21eff42687f726cc9f7c5d6c12` |
| `(srfi 27)` | 2025.12.14 | `27.sld`, `27.scm` | snow-fort, Retropikzel's R7RS port of Sebastian Egner's 54-bit MRG32k3a reference implementation (MIT) | `b8d2322e40955ccc986e9b0b10c1c36044ff5c659d33698722f9b36ec77fdea5` |

Tarball URLs:
`http://snow-fort.org/s/iki.fi/retropikzel/srfi/14/0.1.0/srfi-14-0.1.0.tgz`,
`http://snow-fort.org/s/iki.fi/retropikzel/srfi/27/2025.12.14/srfi-27-2025.12.14.tgz` and
`http://snow-fort.org/s/iki.fi/retropikzel/srfi/41/0.1.0/srfi-41-0.1.0.tgz`
(the compat corpus vendored the same tarballs until Patina bundled them; the
corpus drops packages Patina provides). Each `.scm` sits beside its `.sld`
rather than in a numbered subdirectory so the `(include "…")` resolves
unchanged.

**`(srfi 41)`'s `.sld` deviates in two lines, and `41-match.scm` is a second
source.** The port comments `stream-match` out: the SRFI's reference
implementation writes it in `syntax-case`, which Patina does not have. The
macro is chibi-scheme's `syntax-rules` equivalent instead — from
**chibi's own** `lib/srfi/41.scm`, which is a different file from the
`lib/srfi/41.scm` in this repo (Alex Shinn, BSD 3-Clause) — in its own file,
`41-match.scm`, pinned post-edit, with chibi's `assert` replaced
by the error the SRFI specifies; the `.sld` exports `stream-match` and `_`
(the wildcard its patterns match as a literal) and includes that file. Taking
the *port's* body rather than chibi's whole library is deliberate: chibi's
`stream-filter` is not lazy enough to take a bounded prefix of an infinite
stream — `(stream->list 2 (stream-filter (lambda (n) (= n (* n n))) (stream-from 0)))`
does not terminate there — while the reference implementation, which is the
specification's own code, answers `(0 1)`.

**`(srfi 117)` and `(srfi 127)` are chibi's, byte-identical, and taken from
chibi rather than the SRFI because chibi's are plain R7RS over `(srfi 1)`** —
nothing chibi-only in either. That is not true of every SRFI in that tree:
chibi's `(srfi 116)` is built on its own `(srfi 1 immutable)`, which is why
immutable lists are not bundled alongside these two. Both libraries are
exercised far more by Larceny's suites (40 and 109 assertions) than by
chibi's own (16 and 3), so the Larceny lane is where a regression in them
would show first.

**SRFI 162 has no library of its own, deliberately.** Its bindings are exported
from `(srfi 128)` because SRFI 162 says to: *"Implementers are urged to add them
to their SRFI 128 libraries, for which reason they are not packaged as a
separate library."* So there is no `lib/srfi/162.sld`, and adding one would name
a library the SRFI declined to define. Two things fall out of following that
rather than inventing a name: chibi's SRFI 128 suite runs here **verbatim**, and
the five constants `scheme_tests/upstream/srfi/125/test.sld` used to define for
itself now come from the library it is testing against.

The file is upstream's, not chibi's, and the difference is one line: chibi
comments out `default-comparator` because its own `comparators.scm` defines it,
while the SRFI 128 reference implementation Patina uses does not. Taking the
SRFI's copy means the import is byte-identical to the specification's own
sample implementation, and `bundled_provenance.rs` pins it — unlike the rest of
`128/`, which is the adapted port.

`(srfi 125)` is the one entry not from a snow-fort tarball: snow-fort has no
SRFI 125 package, so `125/hash.scm` is taken from the pinned chibi checkout and
recorded by file digest instead. Only that file is upstream's — `125.sld` is
Patina's own, because upstream's imports `(chibi ast)` and relies on chibi's
C-backed SRFI 69; the resulting deviations are documented in its header,
which is where this tree records deviation.

`(srfi 14)`'s `14.scm` carries **one marked local fix** (2026-08-19, `PATINA
LOCAL EDIT` at the site, pinned post-edit): upstream's `ucs-range->char-set`
passed its extracted base char-set to `%default-base`, which expects the
maybe-base *rest list* — its `pair?` test read the record as "no base given"
and silently defaulted it to empty. Caught by chibi's `(srfi 14 test)` (72
assertions, `scheme_tests/upstream/`) the day that suite was restored; the
write-up is in `PRD/ARCHIVE/TRACK_L_FIXED_DEFECTS.md`.

`(srfi 69)` is **not** byte-identical and is deliberately absent from the table
above: `69/srfi-69-impl.scm` carries three marked local fixes, each `PATINA
DEVIATION` at its site, and the reasoning stays in the file, where anyone
diffing against upstream will be standing.

1. `hash`'s result is coerced to an exact integer, so that an inexact key
   cannot crash the table.
2. `hash`'s `real?` branch is split, because `+inf.0`, `-inf.0` and `+nan.0`
   are real but not rational and so reached `numerator`, which raises.
3. `hash-by-identity` is a real identity hash rather than an alias for the
   structural `hash`. Upstream can define it as an alias because chibi's SRFI
   69 is C-backed and never runs this file; here it meant `(make-hash-table
   eq?)` — and, since SRFI 125's `make-eq-comparator` routes through it, every
   eq-comparator table — lost a key that was mutated after insertion, errored
   on a procedure key, and did not terminate on a circular one.

## Licences

`(srfi 14)` needs nothing added here: `14.scm` carries its own attribution
chain (MIT Scheme → Brian D. Carlstrom → Olin Shivers → Retropikzel) *and* the
full MIT Scheme 1988–1995 licence text at the end of the file, exactly as
upstream ships it.

`(srfi 125)`'s `hash.scm` carries no in-file notice, as chibi's own files
mostly do not — upstream's own state, not something removed here. Two things
establish whose it is, since the file itself says nothing: chibi's `AUTHORS`
opens "Alex Shinn wrote the initial version of chibi-scheme and all distributed
modules", and the list of SRFIs it *does* attribute to their reference
implementations (101, 134, 135, 139, 146, 154, 165) does not include 125 —
consistent with the file being a thin layer over SRFI 69 rather than SRFI 125's
sample implementation, which is a hash table in its own right. So it is Shinn's
under chibi's `COPYING`, BSD 3-Clause, whose first condition requires that
redistributions "retain the above copyright notice, this list of conditions and
the following disclaimer". The file cannot, so the text is reproduced here
rather than linked, verbatim from `COPYING` at chibi 0.12.0. The same text
appears in `lib/chibi/PROVENANCE.md` for that tree; one copy per tree, as
elsewhere in this file.

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
