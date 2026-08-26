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
| `(srfi 116)` | 1.5 | `116/ilists-base.scm`, `116/ilists-impl.scm` | the SRFI's own reference implementation, `https://srfi.schemers.org/srfi-116/srfi-116.tgz` (John Cowan, MIT) | tarball sha256 `9b97c816dd6151b8297e8b6d0ee65fa8daf12123d018e2f46cdce87d0c0fe283` |
| `(srfi 117)` | 1.5 | `117/list-queues-impl.scm` | the SRFI's own reference implementation, `https://srfi.schemers.org/srfi-117/srfi-117.tgz` (John Cowan, MIT) | tarball sha256 `ffc8349567a8169eb53e818dd15f73b0485c3db9aca79e432d7e6781db2b8e46` |
| `(srfi 127)` | — | `127/lseqs-impl.scm` | the SRFI's own reference implementation, `https://srfi.schemers.org/srfi-127/srfi-127.tgz` (John Cowan, MIT) | tarball sha256 `edff4ba12bcc5d4e11d48189a2db4bdbb86b8f424f3bdadbb0350eee095e3828` |
| `(srfi 134)` | — | `134/ideque-stream-impl.scm` | the SRFI's own reference implementation, `https://srfi.schemers.org/srfi-134/srfi-134.tgz` (Shiro Kawai and Wolfgang Corcoran-Mathe, MIT) | tarball sha256 `424f71e3ae9681e20c1c18a19985bd3a98f1c6bf7b34983ed8c611ebc0026c6b` |
| `(srfi 144)` | — | `144.sld`, `144/144.constants.scm`, `144/144.body0.scm`, `144/144.r6rs.scm`, `144/144.body.scm`, `144/144.special.scm` | the SRFI's own reference implementation, `https://srfi.schemers.org/srfi-144/srfi-144.tgz` (William D Clinger, MIT) | tarball sha256 `cb37d320088588aaf6a96c3c25addf6bec0db56a2ea10a907d8e43e16c950be1` |
| `(srfi 135)` | — | `135.sld`, `135.body.scm`, `135/kernel8.sld`, `135/kernel8.body.scm` | the SRFI's own reference implementation, `https://srfi.schemers.org/srfi-135/srfi-135.tgz` (William D Clinger, MIT) | tarball sha256 `f8e9cbcdfcd757ed5dc5835e152bedd621e0933dfed16c5cb815253900fb2735` |
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
the *port's* body rather than chibi's whole library is deliberate: under
chibi, `(stream->list 2 (stream-filter (lambda (n) (= n (* n n))) (stream-from 0)))`
does not terminate, while the reference implementation, which is the
specification's own code, answers `(0 1)`.

*Attribution corrected 2026-08-25, when this was reported upstream.* The
culprit is chibi's `stream->list`, not its `stream-filter`: the loop passes
`(stream-cdr strm)` to its next iteration, so the cdr is forced before that
iteration tests the count, and asking for *n* elements forces *n+1*. The
filter itself is lazy — `(stream-car (stream-cdr (stream-filter …)))` answers
`1` there. Reported as
[ashinn/chibi-scheme#1181](https://github.com/ashinn/chibi-scheme/issues/1181).

**`(srfi 116)` carries three `PATINA LOCAL EDIT`s and eight recorded
failures, and the two are different things.**

*Fixed, marked in place.* `ievery` built its argument list with `ipair` and
tested `heads` with `ipair?`, where `%cars+cdrs` walks with `pair?` and
returns ordinary lists — so the n-ary case returned `#t` for every input
**without ever calling the predicate**. `iany`, immediately below it, has the
correct `cons`/`pair?` form, which is what makes this a transcription slip.
Neither suite catches it: the only n-ary `ievery` assertion in each expects
`#t`. Two smaller ones in `make-improper-ilist-comparator`:
`improper-list-type` classified with `pair?`, so every ilist fell into the
"other" bucket and the type-ordering branch was dead; and the ordering
predicate returned `0` — true in Scheme — for two empty ilists, making
`x < x` hold.

*Still failing, and left alone.* Larceny's suite reports 8 assertions in the
comparator section, unchanged by those edits: five `comparator-test-type`
accepting an ipair whose elements are the wrong type, and three
`comparator-compare` answering `-1` where `+1` is expected. The third of
those traces further than the PR that bundled this claimed: probing
`make-improper-ilist-comparator` directly raises a type error, because it
compares an ipair's cdr with the *element* comparator rather than with
itself, so a two-element ilist reaches `=` with an ipair argument. That is a
fourth upstream bug in the same section, not a spec question — but repairing
a comparator design by hand is a different undertaking from correcting three
transcription slips, and the section is untested upstream (the SRFI's own
test file has no comparator tests, and chibi's 196 assertions do not reach
them), so it is recorded as triage family 29 rather than rewritten here.

**What passes is what the suites exercise**, which is not the same as "the
rest of the library is correct" — `ievery` is the standing proof of that,
having been green in both suites while doing nothing at all.

**`(srfi 117)` and `(srfi 127)` are the SRFIs' own reference**`(srfi 117)` and `(srfi 127)` are the SRFIs' own reference
implementations, and chibi's copies were tried first and rejected.** Both of
chibi's pass Larceny's suites and chibi's own, and both are wrong in ways
neither suite reaches — found by review, each reproduced before acting:
`list-queue-remove-back!` never re-points the queue's last pair, so
`list-queue-back` answers the removed element and the next
`list-queue-add-back!` is silently dropped; `list-queue-set-list!` raises on
the empty list; `lseq-append` uses `cdr` where it needs `lseq-cdr`, so a
generator-backed argument is truncated after one element; and `lseq-member`
passes its comparison arguments in the order opposite to SRFI 1's. The
reference implementations have none of these. This is the second time chibi's
copy has been the wrong source (SRFI 41 was the first) and the first time a
*suite* was wrong too — see the note on the SRFI 117 row in
`upstream_srfi_suites.rs`.

All four were reproduced against `chibi-scheme` itself, not merely against
its sources under Patina, and reported upstream:
[#1179](https://github.com/ashinn/chibi-scheme/issues/1179) (SRFI 117) and
[#1180](https://github.com/ashinn/chibi-scheme/issues/1180) (SRFI 127). Both
were confirmed present on chibi master (186e0659) before filing. If they are
fixed upstream, these bundles can go back to being a straight copy — the
reference implementations are here for the defects, not out of preference.

**One `PATINA LOCAL EDIT` in `117/list-queues-impl.scm`**, marked in place:
`list-queue-join!` did an unguarded `(set-cdr! (get-last queue1) …)`, which
raises when queue1 is empty — Larceny's suite hits it — and never re-pointed
queue1's last pair, so `(list-queue-append! a b)` followed by
`list-queue-add-back!` lost every element of `b`. Both are repaired without
changing the joined result.

**Two upstream properties, left as found** because they are the
specification's own reference implementation and neither suite nor Patina has
a stake in changing them: `lseq?` walks the whole chain, so it does not
terminate on a circular list (Gauche's is O(1)); and `lseq-map` uses the eof
object as its end-of-sequence sentinel, so a mapping procedure that
legitimately returns one truncates the sequence.

**The `.sld` files are Patina's.** Upstream names these libraries
`(srfi-117)` and `(lseqs)`, which is not what R7RS code imports; ours declare
`(srfi 117)` and `(srfi 127)` and include the byte-identical implementation
beside them. `(srfi 127)`'s takes its generator procedures from the bundled
`(srfi 158)` rather than the `(srfi 121)` of the SRFI's day.

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

**SRFI 134 is split, not edited.** Upstream ships the implementation inline in
its `srfi/134.sld` rather than as a separate include, so
`134/ideque-stream-impl.scm` is that library's `(begin …)` body lifted out
verbatim, and `134.sld` is ours. No `PATINA LOCAL EDIT` — nothing between the
two files was changed, and the import and export lists are upstream's.

The file is named `ideque-stream-impl.scm`, not the `ideque-impl.scm` every
sibling's naming would suggest, because upstream *has* an
`ideque-2list/ideque-impl.scm` and it is a different implementation. The
distribution carries two: the stream-based one at the canonical
`srfi/134.sld`, which is what is bundled, and an older two-list one. A diff
against the conventional name would land on the wrong file.

The suite in `scheme_tests/upstream/srfi/134/` is the distribution's top-level
`srfi-134-tests.scm`, the one matched to the bundled implementation; 119 of 119
pass. Only its import block is Patina's — upstream `cond-expand`s between
Chicken's `test` and SRFI 64, and this harness runs `(chibi test)` — with
`(scheme char)` and `(srfi 8)` added for the `char-ci=?` and `receive` the test
bodies use. `(srfi 158)` is upstream's own choice, not a substitution.

**What running two suites does and does not buy.** Larceny's `ideque` suite
passes 114 of 114 against this bundle and the SRFI's own passes 119 of 119, and
an earlier version of this note read that difference as complementary coverage
— "neither is a superset of the other". Measured rather than inferred, that is
wrong: the two exercise the same 55 procedures, with nothing unique to either,
and `ideque=` in particular is covered by both. Different assertion counts do
not imply different reach. The second suite is worth having because it *runs*
in CI, where the Larceny lane does not, and because the bundling guard requires
an upstream suite per bundled library — not because it tests more.

**SRFI 144 carries two marked local edits, and no others.**

`144.sld` is upstream's own library declaration — export list, imports,
`cond-expand`s and includes — with the *include paths* rewritten: Patina
resolves a relative `include` against the directory of the file containing it,
and the bodies live in `144/` beside the `.sld` rather than next to it. Nothing
else in that file differs. Upstream's Larceny-FFI branch is kept and is simply
never taken; its `else` is what supplies the definitions here.

`144/144.r6rs.scm` is the fallback for hosts without `(rnrs arithmetic
flonums)`, and it defines `r6rs:flnumerator` and `r6rs:fldenominator` as R7RS
`numerator` and `denominator`. Those are not the same procedures at the
infinities: R6RS gives `(numerator +inf.0)` as `+inf.0` and
`(denominator +inf.0)` as `1.0`, while R7RS leaves a non-rational argument an
error, and Patina and chibi both raise. SRFI 144 requires the R6RS answers, so
the delegation supplies them.

**No upstream suite is registered for it, and that is recorded rather than
quiet** — see the `srfi 144` entry in `upstream_srfi_suites.rs`'s `NO_SUITE`.
The SRFI's own suite is 1473 lines against a Larceny-family harness, and
chibi's tests chibi's API rather than the SRFI's (`sign-bit` for `flsign-bit`;
an exact `1` where `flloggamma`'s second value is `1.0`). What does exercise it
is Larceny's lane, at 1279 of 1280 on both backends.

The one failure there is not ours: `(fl* x x x)` for x = 1/3 is
`0.037037037037037035` in Patina and in chibi, in either association order,
against the suite's expected `0.03703703703703703`. Two of the four failures it
started with were the local edit above; the third was Patina's own — `(/ 1.0
-0.0)` answered `+inf.0`, because the sign of an infinite quotient was taken
from the numerator alone and `-0.0` is negative without being *less than* zero.
That fix is in `patina-core`, not here, and it applies to `/` generally.

**SRFI 135's `.sld` and kernel are upstream's byte for byte; its body carries
four marked local edits.** Upstream ships `srfi/135.sld` beside
`srfi/135.body.scm`, with the kernel under `srfi/135/`, and the four files sit
here in exactly that arrangement, so every `(include "…")` resolves without
being touched. That is worth doing — SRFI 144's `.sld` needed its include
paths rewritten because its bodies went into a subdirectory — but it is not
unique: `27.sld`, `41.sld` and most of `lib/chibi/` are upstream's verbatim
too, and an earlier version of this note claimed a uniqueness that does not
hold.

The library selects a kernel; `135.sld` imports `(srfi 135 kernel8)`, and that
three-element name maps to `lib/srfi/135/kernel8.sld` under the ordinary
loader rules. `kernel0` and `kernel16` are alternative representations
upstream also ships and are not bundled.

**The four edits are all one defect class: a procedure given a *text* where
the code assumed a string.** Each is upstream's — chibi ships the same body
and still has all four — and each is invisible to both suites, which is how
they survived a 1071-of-1071 and a 1069-of-1069 run.

- `%text-upcase` and `%text-downcase` pass `(subtext txt i n)`, a text, to
  `string-upcase` / `string-caser`, which take strings. Raised a type error
  for any text with an ASCII cased character *before* a character above
  U+007F — the scanner starts on an all-ASCII fast path and switches to the
  slow, broken copy at the cased character. The fast path converts; the slow
  one did not.
- `%text-downcase`'s fast path hardcoded `textual-downcase` instead of
  applying the `string-caser` it was handed, so `textual-foldcase` on a text
  returned the *downcased* text: folding `ß` gave `ß` where the string form
  gives `ss`, and a medial sigma folded to a final one.
- `textual-replicate` returned the string `""` for a zero-width slice where
  SRFI 135 says text. The only literal-string return in the file.

**Its `cond-expand`s do not take the fallbacks.** Patina ships
`lib/rnrs/unicode.sld` and `lib/rnrs/base.sld`, and `cond-expand`'s `library`
requirement finds them, so `(srfi 135)` imports `string-titlecase` from
`(rnrs unicode)` and `div`/`mod` from `(rnrs base)`; the body's own
`%string-titlecase` is guarded on those libraries being *absent* and is never
defined. That is a real load-time dependency of `(scheme text)` on the R6RS
lane, and it is why `textual-titlecase` answers `"Hello-World Foo"` rather
than the whitespace-only `"Hello-world Foo"` the fallback would give. An
earlier version of this note said the opposite.

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
