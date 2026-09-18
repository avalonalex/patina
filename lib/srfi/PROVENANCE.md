# Provenance of byte-identical trees under `lib/srfi/`

Libraries whose bundled files are **all** byte-identical to upstream are
recorded here — with no local edit, there is no file to carry an in-file
note. Libraries that do deviate document themselves in their own `.sld`
header instead (`lib/srfi/132.sld` is the model), and `test-lib/chibi/` has
its own record. One home per tree.

| Package | Version | Files | Upstream | Tarball sha256 |
|---|---|---|---|---|
| `(srfi 125)` | chibi 0.12.0 | `125/hash.scm` | chibi-scheme's `lib/srfi/125/hash.scm` (Alex Shinn, BSD 3-Clause) | file sha256 `d469201d00fa0b955ba23a01e02707034dad5ba2ef1a29cd7b12b880e76f1053` |
| SRFI 162 | — | `128/162-impl.scm` | the SRFI's own sample implementation, `https://srfi.schemers.org/srfi-162/srfi/128/162-impl.scm` (John Cowan, MIT) | file sha256 `973b7a5e6557ecfaa5e218a2d51e63077572235973f27c3db484ab5bac9513f2` |
| `(srfi 64)` | 0.2.1 | `64.sld`, `64.scm` | snow-fort `http://snow-fort.org/s/iki.fi/retropikzel/srfi/64/0.2.1/srfi-64-0.2.1.tgz`, Retropikzel's R7RS packaging of the SRFI 64 reference implementation (Per Bothner, MIT) | `f3ec28b4ce2b6f2fe432d81ac9afbf96c0359cbc6e0f2f3b935793994329f887` |
| `(srfi 41)` | 0.1.0 | `41.sld`, `41.scm` | snow-fort, Retropikzel's R7RS port of Philip Bewig's stream reference implementation (MIT) | `c6bbc9b5d856f1ebdb3d9ce75d9542cc32050b13b82eb3e53fd9a8b96b67fdc2` |
| `(srfi 116)` | 1.5 | `116/ilists-base.scm`, `116/ilists-impl.scm` | the SRFI's own reference implementation, `https://srfi.schemers.org/srfi-116/srfi-116.tgz` (John Cowan, MIT) | tarball sha256 `9b97c816dd6151b8297e8b6d0ee65fa8daf12123d018e2f46cdce87d0c0fe283` |
| `(srfi 117)` | 1.5 | `117/list-queues-impl.scm` | the SRFI's own reference implementation, `https://srfi.schemers.org/srfi-117/srfi-117.tgz` (John Cowan, MIT) | tarball sha256 `ffc8349567a8169eb53e818dd15f73b0485c3db9aca79e432d7e6781db2b8e46` |
| `(srfi 127)` | — | `127/lseqs-impl.scm` | the SRFI's own reference implementation, `https://srfi.schemers.org/srfi-127/srfi-127.tgz` (John Cowan, MIT) | tarball sha256 `edff4ba12bcc5d4e11d48189a2db4bdbb86b8f424f3bdadbb0350eee095e3828` |
| `(srfi 134)` | — | `134/ideque-stream-impl.scm` | the SRFI's own reference implementation, `https://srfi.schemers.org/srfi-134/srfi-134.tgz` (Shiro Kawai and Wolfgang Corcoran-Mathe, MIT) | tarball sha256 `424f71e3ae9681e20c1c18a19985bd3a98f1c6bf7b34983ed8c611ebc0026c6b` |
| `(srfi 144)` | — | `144.sld`, `144/144.constants.scm`, `144/144.body0.scm`, `144/144.r6rs.scm`, `144/144.body.scm`, `144/144.special.scm` | the SRFI's own reference implementation, `https://srfi.schemers.org/srfi-144/srfi-144.tgz` (William D Clinger, MIT) | tarball sha256 `cb37d320088588aaf6a96c3c25addf6bec0db56a2ea10a907d8e43e16c950be1` |
| `(srfi 115)` | — | `115.sld`, `115.scm`, `115/boundary.sld`, `115/boundary.scm` | the SRFI's own distribution, `https://srfi.schemers.org/srfi-115/srfi-115.tgz`, `contrib/duy-nguyen/` (Alex Shinn; BSD-3-Clause, the boundary data CC0-1.0) | tarball sha256 `e8e7294adfb695518ef5ac6d59989048e2143dafbb3d87588458ab76bfe715c2` |
| `(srfi 160)` | — | `160/base.sld`, `160/base/*.scm`, and `160/<type>.sld` + `160/<type>-impl.scm` for twelve types | the SRFI's own reference implementation, `https://srfi.schemers.org/srfi-160/srfi-160.tgz` (John Cowan, MIT); the per-type files are its `atexpander.sh` output, see below | tarball sha256 `5e86da759a2b2060d38480813af5f9d5333c3c7df4b5cdefdc96762103f63796` |
| `(srfi 146)` | — | `146.sld`, `146.scm`, `146/hash.sld`, `146/hash.scm`, and its own supporting libraries at `lib/nieper/rbtree.{sld,scm}` and `lib/gleckler/{hamt,hamt-map,hamt-misc,vector-edit}.{sld,scm}` | the SRFI's own reference implementation, `https://srfi.schemers.org/srfi-146/srfi-146.tgz` (Marc Nieper-Wißkirchen, with Arthur A. Gleckler's HAMT, MIT) | tarball sha256 `52b10ba6f113407b095c582f98dd55947a7e984f7e629bae64467fb474ae28ad` |
| `(srfi 135)` | — | `135.sld`, `135.body.scm`, `135/kernel8.sld`, `135/kernel8.body.scm` | the SRFI's own reference implementation, `https://srfi.schemers.org/srfi-135/srfi-135.tgz` (William D Clinger, MIT) | tarball sha256 `f8e9cbcdfcd757ed5dc5835e152bedd621e0933dfed16c5cb815253900fb2735` |
| `(srfi 101)` | — | `101.sld`, `101.scm` | chibi-scheme's R7RS adaptation (Alex Shinn, 2018) of the SRFI's own reference implementation (David Van Horn, MIT), byte-identical | source `~/Project/reference/chibi-scheme/lib/srfi/101.{sld,scm}` |
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
[ashinn/chibi-scheme#1181](https://github.com/ashinn/chibi-scheme/issues/1181),
and **fixed upstream 2026-08-27** in
[`f15b0814`](https://github.com/ashinn/chibi-scheme/commit/f15b0814), which
takes the count test before forcing the cdr and adds the bounded-prefix
assertion (chibi also removed a stray `not` in `stream-filter`'s `else` arm in
the same commit — a dead-code typo, not the fault, exactly as the corrected
attribution above says). The refreshed suite is in
`scheme_tests/upstream/srfi/41/test.sld`.

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
were confirmed present on chibi master (186e0659) before filing.

**Both were fixed upstream 2026-08-27** —
[`32ed54b0`](https://github.com/ashinn/chibi-scheme/commit/32ed54b0) and
[`c00200ec`](https://github.com/ashinn/chibi-scheme/commit/c00200ec) — taking
the reported semantics for all four, and each shipping regression tests close
to the reported repros. That condition having fired, the paragraph this
replaces offered to make these bundles a straight copy of chibi's again.
**They stay the SRFIs' own reference implementations**, for reasons the
defects were never the whole of:

- chibi's `list-queue-append!` is `(make-list-queue (append-map list-queue-list …))`,
  a fresh copy. That is a permitted reading — SRFI 117 says it is an error to
  assume anything about the arguments afterwards — but it is chibi's choice,
  not the specification's, and adopting it would silently change what
  `list-queue-append!` costs.
- The `PATINA LOCAL EDIT` below repairs `list-queue-join!`, which is the
  reference implementation's own procedure; chibi has no counterpart, so the
  swap would trade a recorded one-line edit for an unrecorded behavioural
  change.
- Nothing is now wrong with either source, so the swap would buy no
  correctness — only churn in a tree whose value is that it is version-matched
  and pinned.

What the fixes *did* buy is upstream test coverage: the suites in
`scheme_tests/upstream/srfi/{117,127}/` were re-vendored at those commits
(2026-08-30) and all six new assertions passed on arrival on both backends.

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

`(srfi 14)` is **no longer third-party, as of #372 (2026-09-17)**, and is
absent from the table above for that reason. It was Retropikzel's R7RS port of
Olin Shivers' reference implementation (snow-fort 0.1.0, MIT-Scheme-old,
tarball sha256
`de94f90d7b032ea554ed51b4cbce942b22df6fefe68497d661b9c26e3c7e690e`), pinned
post-edit over one marked local fix. It is now Patina-authored.

**Why it was replaced rather than edited.** That implementation stores a
char-set as a 256-character string indexed by code point — its own header says
it "is Latin-1 specific. Would certainly have to be rewritten for Unicode."
The representation is the defect: a character above U+00FF indexed past the
end of the string and raised, and `ucs-range->char-set` clipped a range to the
first 256 code points without a word, so `char-set:full` had 256 members and a
Hangul range was empty. Every one of its ~60 procedures was written against
that string, so there was no edit site smaller than the file. Larceny triage
family 45 and issue #372 carry the measurements.

The replacement keeps a char-set as a normalized list of inclusive code-point
ranges; `14.scm`'s header documents the invariant and why the `char-set:*`
class constants come from a Rust primitive rather than a Scheme scan. Two
behaviours changed that the SRFI leaves open, both recorded here because they
are choices rather than consequences: iteration and the cursors now walk
**ascending** rather than descending (both references do, and Larceny's suite
pins the accumulation), and `char-set-hash` folds over ranges rather than over
256 string indices, so hashing `char-set:full` is two steps instead of
1112064.

What the change is measured by: upstream's own `(srfi 14 test)` still passes
72 of 72 (`upstream_srfi_suites.rs`), Larceny's `charset` suite went from 91 to
**93 of 93 on both backends**, and
`crates/patina-tests/tests/scheme/srfi/char-sets.scm` is this tree's own
87-assertion suite for the Unicode property neither of those reaches. The
superseded local fix — upstream's `ucs-range->char-set` handing its extracted
base char-set to a `%default-base` that expects the *rest list*, so the base
silently defaulted to empty — is written up in
`PRD/ARCHIVE/TRACK_L_FIXED_DEFECTS.md`; the rewrite has no such shape, since
its `%default-base` is reached only from a rest list.

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

**SRFI 115 is byte-identical, and is the clean case the two beside it are
not.**

`lib/srfi/115.*` and `lib/srfi/115/boundary.*` are `contrib/duy-nguyen/` from
the SRFI's own distribution, unedited. It needed no adaptation at all: its own
`cond-expand` already reaches for `(chibi test)` on anything that is not
Larceny, and its non-chibi branch asks only for libraries Patina already
ships. Upstream's suite passes **85 of 85 on both backends**.

**Every file carries an explicit `SPDX-License-Identifier`** — BSD-3-Clause for
the implementation, CC0-1.0 for `boundary.*`, which is generated Unicode
word-boundary data — and the distribution ships a `LICENSES/` directory with
both texts. That is worth stating plainly next to its two neighbours in this
file: SRFI 4 had no notice on any file and was reimplemented, and SRFI 159 (not
bundled) has none on ten of seventeen. Here nothing is inferred.

**One dependency is worth recording**, because it is the reverse of the usual
direction: SRFI 115 needs `(srfi 14)`, and it is viable here only since #372
gave char-sets the whole Unicode range. The Latin-1 implementation it replaced
is exactly what the chibi-regexp corpus failure runs aground on — so Patina
ships the standard-track regex library while the chibi one it is descended from
still cannot load, for a reason recorded in `compat/EXCLUSIONS.scm`.

**SRFI 4 is Patina's own, and the reason is a licence question rather than a
technical one.**

`lib/srfi/4.*` is not in the table above because it is not third-party. The
SRFI's own distribution carries John Cowan's portable R7RS port in
`contrib/cowan/`, and it was measured working here first — 240 of 240 on its
own suite, both backends, no adaptation — before being set aside.

**What the audit found.** None of the four files that port consists of
carries a licence notice of any kind. That alone would follow the `(srfi 125)`
precedent below: establish the licence from the surrounding distribution and
reproduce the text here. What stops that reasoning is a sibling: the same
`contrib/cowan/` directory contains `r6rs/bytevectors-impl.scm` under
*William D Clinger's* terms — "Permission to copy this software … subject to
the restriction that all copies made of this software must include this
copyright and permission notice in full" — which is not the MIT the
distribution's `README.org` carries. So that directory is demonstrably **not**
covered uniformly by the repository header, and inferring MIT for the
unmarked files from it would be inferring from a premise a neighbouring file
disproves.

Compare SRFI 160 and SRFI 146, where every single file carries an explicit
SPDX header. Cowan marks his work when he intends to.

**So the interface was implemented instead.** That is the same move the SRFI
14 rewrite made and for the same reason, stated in § Licences below: the
SRFI's *interface* is what an implementation implements, and an API is not the
licensed artifact. `4.scm` is about 400 lines over `(r6rs bytevectors)`, which
Patina already shipped — `R7RS_LARGE_STATUS.md` used to call SRFI 4
"plausibly Rust work rather than a port", which #383 corrected; the primitives
were all already there.

It is held to `crates/patina-tests/tests/scheme/srfi/homogeneous-vectors.scm`
(46 rows) and, more searchingly, to SRFI 160's registered `s16` suite, which
drives these types from the layer above: 110 of 110, on both backends. The
complex vectors are the sharpest case, since `(srfi 160 base)` builds a
c64vector by wrapping an `f32vector` and driving it through `make-f32vector`,
`f32vector-set!` and `f32vector-length`; `homogeneous-vectors.scm` is where
that is asserted, because SRFI 160's own `base` suite is print-only (see
below) and so cannot be registered.

**SRFI 160 is the one bundled library whose files are generated.**

Upstream ships *templates*, not sources. `atexpander.sh` runs
`sed "s/@/$at/g"` over three files — `srfi/160/at.sld`, `at-impl.scm` and
`base/at-vector2list.scm` — once per type, for twelve types: the ten SRFI 4
has plus `c64` and `c128`, which `(srfi 160 base)` exports and SRFI 4 has no
equivalent of. What is bundled here is that script's output, run unmodified
against the pinned tarball.

**So the pinned artifact is the generator's output rather than the tarball's
content**, which is a real departure from the rule the rest of this file
states, and is recorded rather than hidden. The alternative — running the
expander at build time — would put a shell script on the critical path of
every build to save committing 36 files that change only when the tarball
does. Regenerating is one command against the recorded sha256, and
`bundled_provenance.rs` pins every generated file, so an edit to one is still
a deliberate act.

SRFI 160's `s16` suite passes with no adaptation, 110 of 110 on both
backends, and is registered. Upstream tests `s16` alone and says why — "if one
vector type works, they all work", since the twelve are one template — and the
per-type parameters that argument does not cover are in
`homogeneous-vectors.scm`. Upstream's *other* suite, for `(srfi 160 base)`,
is **not** registered and is not vendored: like SRFI 4's, it is a print-only
harness whose own `test-assert`/`test-not` macros `display` "OK" or "FAIL" and
report nothing a driver can read, so a row for it would be vacuously green.
`homogeneous-vectors.scm` covers that layer instead.

**What neither closes.** `(scheme vector @)`'s in-scope corpus requester is
srfi-179, whose *library itself* imports `(chibi assert)`, which needs
`(chibi)` — implementation-specific, and external by policy. So srfi-179
cannot pass whatever is bundled here, and the corpus number does not move.
These two ship on eligibility and edition completeness, which the bundling
policy makes sufficient.

**SRFI 146 is byte-identical and brings two namespaces with it.**

Every file is upstream's, unedited: `146.{sld,scm}` and `146/hash.{sld,scm}`
from the tarball above, plus the supporting libraries it ships —
`(nieper rbtree)`, the red-black tree `(srfi 146)` is built on, and Arthur A.
Gleckler's `(gleckler hamt)`, `(gleckler hamt-map)`, `(gleckler hamt-misc)`
and `(gleckler vector-edit)`, the HAMT under `(srfi 146 hash)`.

**Those two namespaces are bundled under their own names rather than renamed**,
which is a deliberate choice and the only one here worth arguing about. Moving
them under `(patina …)` or `(srfi 146 …)` would make every file a local edit
and put them outside the byte-identical rule that `bundled_provenance.rs`
enforces, to hide two directories nobody imports: nothing outside `(srfi 146)`
and `(srfi 146 hash)` refers to them, and `upstream_srfi_suites.rs`'s
`NO_SUITE_TREES` records that they are covered by those two libraries' own
suites. Keeping them verbatim is what lets the pin mean something.

**It needed no adaptation at all**, which is rare enough in this file to note:
both the SRFI's own suites pass on the first run, 97 of 97 for `(srfi 146)` and
77 of 77 for `(srfi 146 hash)`, on both backends. Compare `(srfi 125)` and
`(srfi 130)`, whose suites needed their import lists adapted.

**Three shims were bundled for it**, each because SRFI 146's dependency closure
reaches a library R7RS or Patina had made redundant:

- `(srfi 145)` — `assume`, used by `146.scm` and `(nieper rbtree)`. It is the
  SRFI's own first sample implementation with its `cond-expand` resolved: the
  document offers a reporting version and one whose whole body is
  `((assume obj . _) obj)`, which discards the check. The reporting one is
  taken, since a library that silently drops its assertions is worse than no
  library. chibi takes the other, which is a registered divergence rather than
  a defect in either direction.
- `(srfi 2)` — `and-let*`, used by `(nieper rbtree)`. Patina-authored, because
  the SRFI has no portable reference implementation to bundle: Oleg Kiselyov's
  original is a low-level macro and the document carries no `syntax-rules`
  version. `crates/patina-tests/tests/scheme/srfi/and-let.scm` checks each
  clause form against the SRFI's text.
- `(srfi 16)` — `case-lambda`, used by the Gleckler libraries. A re-export of
  `(scheme case-lambda)`, which is *already* SRFI 16's reference
  implementation, so a second copy would be two definitions to keep in step.

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
unique: `27.sld`, `41.sld` and most of `test-lib/chibi/` are upstream's
verbatim too, and an earlier version of this note claimed a uniqueness that does not
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

**SRFI 101 comes from chibi, not from the SRFI, and that was a correction.**
The SRFI's own distribution ships R6RS `.sls` libraries, so the first attempt
here was a hand port of them. That was the wrong source: chibi's
`lib/srfi/101.scm` is Alex Shinn's 2018 R7RS adaptation of the same Van Horn
reference implementation, it sits in the directory this repo already takes
suites from, and taking it means the usual byte-identical vendoring instead of
a port with hand-marked edits — and, not incidentally, it keeps Van Horn's
copyright and permission notice, which the hand port had dropped.

Both files are byte-identical to chibi's and laid out flat, so the `.sld`'s
`(include "101.scm")` resolves unchanged. No `PATINA LOCAL EDIT`. Its imports
— `(srfi 1)`, `(srfi 125)`, `(srfi 151)` renamed for
`bitwise-arithmetic-shift` — are all bundled here, and it carries its own
`assert`, so it needs none of the `(rnrs …)` shims the R6RS original wanted.

It exports one name R7RS-large does not have, `length<=?`, which is chibi's
addition.

**`(scheme rlist)` is not a plain re-export**, unlike every other alias here.
SRFI 101 deliberately shadows the list operations it replaces — it exports
`cons`, `car`, `list?` — so a program using it imports `(scheme base)` with
those excluded. R7RS-large wants both importable together, so its
`(scheme rlist)` renames all 48: `rcons`, `rcar`, `rlist?`, with `make-rlist`,
`rlist->list` and `list->rlist` for the three that read badly with a bare
prefix. Getting that wrong is not subtle — exporting the shadowing names under
the alias makes Larceny's suite overflow the stack, because the suite imports
`(scheme base)` alongside it.

## Licences

`(srfi 14)` needed nothing added here while it was upstream's: `14.scm`
carried its own attribution chain (MIT Scheme → Brian D. Carlstrom → Olin
Shivers → Retropikzel) *and* the full MIT Scheme 1988–1995 licence text at the
end of the file, exactly as upstream shipped it. Since #372 replaced that file
outright (§ above), no third-party text remains in it and there is nothing
left to carry: the SRFI's *interface* is what the rewrite implements, and an
API is not the licensed artifact.

`(srfi 146)` needs nothing added here, and that was checked file by file
rather than inferred from the tarball: all fourteen bundled files carry an
explicit `SPDX-License-Identifier: MIT` header *and* the full MIT permission
text inline — the four `(srfi 146)` files and `(nieper rbtree)` under
Marc Nieper-Wißkirchen's copyright (2016, 2018), and the four Gleckler
libraries under Arthur A. Gleckler's (2004, 2015, 2021). MIT's condition is
that the notice travel with the software, and since each file carries its own,
bundling them verbatim satisfies it without anything being reproduced here.
This is also the second reason not to rename those two namespaces: an edited
file is one whose notice someone has to re-establish.

`(srfi 115)`'s four files carry an SPDX *identifier* but not the licence text,
which BSD-3-Clause's first condition asks to be retained. The condition is met
the way `(srfi 125)`'s is below — by reproducing the text here, one copy per
tree — and the notice each file does carry supplies the `<year> <owner>` the
distribution's own `LICENSES/BSD-3-Clause.txt` leaves as placeholders:
"Copyright (c) 2013 - 2016 Alex Shinn" for `115.sld` and `115.scm`, and
"Copyright (c) 2015 Alex Shinn" for `115/boundary.*`, which is additionally
CC0-1.0 and so carries no condition at all. The BSD-3-Clause text is the one
already reproduced below for `(srfi 125)`; it is the standard three-clause
text, identical in both distributions, so it is not repeated a second time.

`(srfi 4)` needs nothing here: it is Patina-authored (§ above), so there is no
third-party text in it to carry. The audit that led to that decision is worth
keeping, because it is the one case in this tree where the usual reasoning
failed: the SRFI's `contrib/cowan/` port has no notice on any of its four
files, and a sibling in the same directory
(`r6rs/bytevectors-impl.scm`) is under William D Clinger's terms rather than
the distribution's MIT — so the repository header could not be taken to cover
the unmarked files, and the licence would have been inferred rather than
established. Reimplementing removed the question instead of documenting it.

`(srfi 160)` needs nothing added either: all 40 of its bundled files carry an
explicit `SPDX-License-Identifier: MIT` over John Cowan's 2018 copyright,
including the generated ones, since `atexpander.sh` copies the template's
header into each expansion.

**It also corrupts the address while copying it, in 36 of the 40.** The
expander is a global `sed "s/@/$at/g"`, and the template's
`SPDX-FileCopyrightText: 2018 John Cowan <cowan@ccil.org>` contains an `@`, so
each expansion carries `<cowanu8ccil.org>`, `<cowans16ccil.org>`,
`<cowanc128ccil.org>` and so on. Only `base.sld`, `base/complex.scm`,
`base/r7rec.scm` and `base/valid.scm` — the four files the expander does not
touch — keep the real address.

This is upstream's own behaviour, reproduced here deliberately: re-running
`atexpander.sh` against the tarball whose sha256 is recorded above yields
bytes identical to what is bundled, mangled address included, and changing it
would make 36 files diverge from the generator for a cosmetic repair. The
licence grant is unaffected — the identifier, the year and the name are
intact, and only the contact address is damaged — but a REUSE or SPDX linter
reads `SPDX-FileCopyrightText` and will flag those 36, which is worth knowing
before someone treats it as our error.

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
appears in `test-lib/chibi/PROVENANCE.md` for that tree; one copy per tree, as
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
BSD-licensed port, whose text is in § Licences above — same author, same
licence. Its `130.chibi-string.scm` is covered by the same notice.

`(srfi 14)` was bundled 2026-08-14 as a dependency of `(chibi string)`, which
`(srfi 130)` was written against. Since #198 inlined the subset of that
library which `(srfi 130)` uses (`130.chibi-string.scm`, after which
`(chibi string)` itself moved to `test-lib/`), SRFI 14 is **a dependency of
`(srfi 130)` directly** —
still for exactly two names, `char-set?` and `char-set-contains?`, both used
by the inlined `make-char-predicate`, and `lib/srfi/130.sld` now imports
`(srfi 14)` itself rather than inheriting it. That is a thin use of a large
library, but the alternative was a hand-maintained subset, and SRFI 14 was
already an L1 bundling target on its own in-degree — so it stays bundled
either way; only the reason moved. Those two names are also why #372's rewrite
fixed `(srfi 130)`'s `string-index` and `string-skip` — which raised on a
char-set predicate that met a character above U+00FF — without touching
`(srfi 130)` at all.

The package ships no test suite; conformance is covered by
`crates/patina-tests/tests/scheme/stdlib/random.scm` (both backends must
give the recorded pseudo-randomized stream) and by the SRFI 132 suite, whose quickselect
draws its pivots from `random-integer`.

The rule and its enforcement: § The rule below, and
`crates/patina-tests/tests/bundled_provenance.rs`, whose `PINNED` table is
the authoritative scope.

**The boundary:** the adapted ports elsewhere in this tree (SRFI 1, 69, 113,
128, 133, 158, …) are *not* byte-identical to any upstream and are
deliberately unpinned; their sources and known deviations are Track L
territory (`PRD/TRACK_L_SNOW_LIBRARIES_PRD.md`) until each is reconciled to
this standard or recorded here.

## The rule (audit 2026-08-10, group E)

*Canonical home. This section lived in `lib/chibi/PROVENANCE.md` until #198
emptied that tree — its last library moved to `test-lib/chibi/`, whose record
defers here rather than restating the wording, so there is one to edit.*

Vendored library files **match upstream** — the ones Patina bundles and the
ones it only supplies from `test-lib/`. If a change is unavoidable, mark the
edit site with `;; PATINA LOCAL EDIT:` and record the deviation in the tree's
provenance home — this file, `test-lib/chibi/PROVENANCE.md`, or the library's
`.sld` header (as `lib/srfi/132.sld` does); one home per tree. Files claimed
byte-identical are pinned by
`crates/patina-tests/tests/bundled_provenance.rs` (its `PINNED` table is the
authoritative scope), so an unrecorded edit fails the suite.

`130.chibi-string.scm` is derived from upstream: an inlined subset with
renames, rather than a byte-identical copy. Its header records that derivation,
and `bundled_provenance.rs` pins the result so later changes remain deliberate.
The two `PATINA LOCAL EDIT` sites added for #204 fix inherited predicate defects:
`string-any` starts at the requested cursor, and `string-every` uses a bounded
scan that preserves the final successful predicate result instead of converting
it to a boolean. SRFI 130's predicate specification requires both behaviors;
`tests/scheme/srfi/string-cursors.scm` covers them on both backends, with Chibi's
contrary results recorded in `DIVERGENCES.tsv`.
