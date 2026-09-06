# Provenance of `lib/chibi/`

This tree holds **one library**, and it is here for exactly one reason: it is
the implementation `lib/srfi/130.sld` is written against. Everything else that
used to live here was a test-lane dependency with no `lib/` importer, and moved
to `test-lib/chibi/` in #196 (`filesystem`) and #197 (`test`, `diff`,
`optional`, `term ansi`); those records moved with the files.

`string.scm` and `string.sld` are **byte-identical** to their pinned upstream
snowball package (verified 2026-08-12 by diffing against the sha256-checked
tarball below), by Alex Shinn under the BSD 3-Clause licence, reproduced in
full under [Licence](#licence) below. `string.scm` carries its own in-file
copyright notice; `string.sld` does not, so for that file **this record is the
only notice**, which is why the licence text lives here rather than behind a
link.

| Package | Version | Files here | Tarball sha256 |
|---|---|---|---|
| `(chibi string)` | 0.9.0 | `string.scm`, `string.sld` | `86a73c53b2e7a4e1201ff10115a5488890993c0020051b3abe0fc785a077ec11` |

The tarball URL follows the pattern
`http://snow-fort.org/s/gmail.com/alexshinn/chibi/<name>/<version>/chibi-<name>-<version>.tgz`.
These are snow-fort snowball releases, older than chibi-scheme's git head.

**This tree is scheduled to disappear.** #198 inlines the `else`-branch
definitions `(srfi 130)` actually uses into `lib/srfi/130.scm` and deletes
`lib/chibi/`, folding what remains of this record into
`lib/srfi/PROVENANCE.md`. Until then the single importer is real and
`(chibi string)` is genuinely runtime-forced, which is why it did not move
with the others.

**What #198 must relocate, not just delete.** § The rule and § Licence below
are the canonical copies, and eight references point at this file. They all
dangle the moment it goes, so relocating those two sections is part of the
deletion, not a follow-up:

- `test-lib/chibi/PROVENANCE.md` § The rule — defers here rather than
  restating the wording, deliberately
- `crates/patina-tests/tests/bundled_provenance.rs` — module doc and the
  assertion failure message
- `lib/srfi/PROVENANCE.md` ×3 — the one-copy-per-tree note, the BSD text for
  its own port, and § The rule's enforcement pointer
- `lib/srfi/130.sld` ×2 — its header cites the BSD text here as the licence
  for a file carrying no in-file notice, so this one is a licence obligation
  rather than a broken link

`grep -rn 'lib/chibi/PROVENANCE' .` is the check.

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

## The rule (audit 2026-08-10, group E)

Vendored library files **match upstream** — the ones Patina bundles and the
ones it only supplies. If a change is unavoidable, mark the edit site with
`;; PATINA LOCAL EDIT:` and record the deviation in the tree's provenance
home — this file, `lib/srfi/PROVENANCE.md`, `test-lib/chibi/PROVENANCE.md`, or
the library's `.sld` header (as `lib/srfi/132.sld` does); one home per tree.
Files claimed byte-identical are pinned by
`crates/patina-tests/tests/bundled_provenance.rs` (its `PINNED` table is the
authoritative scope), so an unrecorded edit fails the suite.
